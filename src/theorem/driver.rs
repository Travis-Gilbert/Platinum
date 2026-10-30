//! Owns the Agent place's controller, transport and asynchronous observers.
//!
//! The authority owns durable conversations; the controller owns their live
//! presentation. This entity caches one projection for readers and keeps retry
//! bytes outside it. Retry bytes survive only while their attachment is staged;
//! workspace answer identities survive for the authority's lifetime so a
//! revisited conversation cannot answer the same request twice. That set grows
//! by one small identity per encountered request and resets with the authority.
//! Replacing an observer never cancels the durable run it was watching.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;

use async_compat::Compat;
use gpui_kit::{AppContext as _, AsyncApp, Context, EventEmitter, PathPromptOptions, WeakEntity};
use theorem_surface_contracts::intent::{ComposerIntent, ThreadIntent};
use theorem_surface_contracts::workspace::{WorkspaceActor, WorkspaceReceipt, WorkspaceRefusal};
use theoremweb_agent_runtime::chat_host::SIGN_IN_ROUTE;
use theoremweb_agent_runtime::client::ClientError;
use theoremweb_agent_runtime::sse::RunEventEnvelope;
use theoremweb_agent_runtime::wire::ThreadSnapshot;
use theoremweb_agent_runtime::{
    ApprovalAnswer, AttachmentUpload, BootstrapTarget, ChatAuthorityKind, ChatHostController,
    ChatHostEffect, ChatHostPhase, ChatHostProjection, ChatTransport, DeterministicDemoAuthority,
    Following, HttpChatTransport, SubmitTurn, TransportFuture, TurnBinding, TurnFollow,
    TurnFollower, WorkspaceAnswer, live_run_id, reported_run_state,
};
use url::Url;

/// The explicitly selected authority, or the absence of native configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Connection {
    NotConfigured,
    Native,
    Demo,
}

/// Browser operations the composition root fulfills outside this entity.
pub enum DriverEvent {
    OpenUrl(Url),
}

/// One window's chat authority and its cached presentation.
pub struct TheoremDriver {
    controller: ChatHostController,
    transport: Rc<dyn ChatTransport>,
    connection: Connection,
    projection: ChatHostProjection,
    generation: Rc<Cell<u64>>,
    pending_new_conversation: bool,
    pending_thread_selection: Option<String>,
    creating_conversation: Option<u64>,
    attachment_sources: BTreeMap<String, AttachmentSource>,
    workspace_requests_answered: BTreeSet<String>,
    last_saved: (Option<String>, Option<String>),
    // Capture before publishing Loading, which can clear the saved selection.
    hydration_target: BootstrapTarget,
}

impl EventEmitter<DriverEvent> for TheoremDriver {}

#[derive(Clone)]
struct AttachmentSource {
    name: String,
    media_type: String,
    bytes: Vec<u8>,
}

impl TheoremDriver {
    /// Start native hydration when configured; otherwise remain unhydrated.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self::from_configuration(native_configured(), cx)
    }

    pub(super) fn from_configuration(configured: bool, cx: &mut Context<Self>) -> Self {
        let connection = if configured {
            Connection::Native
        } else {
            Connection::NotConfigured
        };
        let mut driver = Self::unstarted(
            ChatAuthorityKind::NativeTransport,
            Rc::new(HttpChatTransport),
            connection,
            cx,
        );
        if configured {
            driver.begin(cx);
        } else {
            driver.publish(cx);
        }
        driver
    }

    /// Hydrate through an injected authority, including the deterministic
    /// demo: the tests' seam, since the browser only ever starts from
    /// configuration and switches with `use_demo`.
    #[cfg(test)]
    pub(super) fn with_transport(
        authority: ChatAuthorityKind,
        transport: Rc<dyn ChatTransport>,
        cx: &mut Context<Self>,
    ) -> Self {
        let connection = if authority == ChatAuthorityKind::DeterministicDemo {
            Connection::Demo
        } else {
            Connection::Native
        };
        let mut driver = Self::unstarted(authority, transport, connection, cx);
        driver.begin(cx);
        driver
    }

    fn unstarted(
        authority: ChatAuthorityKind,
        transport: Rc<dyn ChatTransport>,
        connection: Connection,
        cx: &mut Context<Self>,
    ) -> Self {
        let controller = ChatHostController::new(authority, false);
        let projection = controller.projection();
        Self {
            controller,
            transport,
            connection,
            projection,
            generation: Rc::new(Cell::new(0)),
            pending_new_conversation: false,
            pending_thread_selection: None,
            creating_conversation: None,
            attachment_sources: BTreeMap::new(),
            workspace_requests_answered: BTreeSet::new(),
            last_saved: super::saved::selection(cx),
            hydration_target: BootstrapTarget::default(),
        }
    }

    /// The latest complete projection, borrowed without a per-frame clone.
    pub fn projection(&self) -> &ChatHostProjection {
        &self.projection
    }

    /// Which authority this driver currently uses.
    pub fn connection(&self) -> Connection {
        self.connection
    }

    /// Replace the authority with a fresh demo and supersede its old observers.
    pub fn use_demo(&mut self, cx: &mut Context<Self>) {
        self.advance_generation();
        self.transport = Rc::new(DeterministicDemoAuthority::new());
        self.controller = ChatHostController::new(ChatAuthorityKind::DeterministicDemo, false);
        self.connection = Connection::Demo;
        self.pending_new_conversation = false;
        self.pending_thread_selection = None;
        self.creating_conversation = None;
        self.attachment_sources.clear();
        self.workspace_requests_answered.clear();
        self.begin(cx);
    }

    /// Recheck missing configuration, then repeat hydration on this transport.
    pub fn retry(&mut self, cx: &mut Context<Self>) {
        if self.connection == Connection::NotConfigured {
            if !native_configured() {
                self.publish(cx);
                return;
            }
            self.connection = Connection::Native;
        }
        self.begin(cx);
    }

    /// The native authority's sign-in page, when its address is usable.
    pub fn sign_in_url(&self) -> Option<Url> {
        if self.connection != Connection::Native {
            return None;
        }
        let base = Url::parse(&std::env::var("THEOREMWEB_AUTHORITY_URL").ok()?).ok()?;
        if !matches!(base.scheme(), "http" | "https") {
            return None;
        }
        base.join(SIGN_IN_ROUTE).ok()
    }

    /// Ask the browser to open the configured sign-in page.
    pub fn open_sign_in(&mut self, cx: &mut Context<Self>) {
        if let Some(url) = self.sign_in_url() {
            cx.emit(DriverEvent::OpenUrl(url));
        } else {
            self.report_notice("No native Theorem sign-in address is configured.", cx);
        }
    }

    /// Load an authenticated project's conversations, publishing any refusal.
    pub fn select_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
        self.pending_thread_selection = None;
        let effect = match self.controller.select_project(project_id) {
            Ok(effect) => effect,
            Err(reason) => {
                self.controller.report_notice(reason);
                None
            }
        };
        self.publish_and_fulfill(effect, cx);
    }

    /// Load a conversation through the selected project's membership gate.
    pub fn select_thread(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        // The project page must arrive before its membership gate can judge it.
        if self.controller.phase() == ChatHostPhase::Loading && self.projection.threads.is_empty() {
            self.pending_thread_selection = Some(thread_id.to_owned());
            self.publish(cx);
            return;
        }
        self.pending_thread_selection = None;
        let effect = match self.controller.select_thread(thread_id) {
            Ok(None) if self.controller.phase() == ChatHostPhase::Loading => {
                // A different conversation may still be loading. Returning to
                // the currently projected one must supersede that older read.
                Some(ChatHostEffect::LoadThread {
                    thread_id: thread_id.to_owned(),
                })
            }
            Ok(effect) => effect,
            Err(reason) => {
                self.controller.report_notice(reason);
                None
            }
        };
        self.publish_and_fulfill(effect, cx);
    }

    /// Open a fresh conversation, waiting for hydration when necessary.
    pub fn new_conversation(&mut self, cx: &mut Context<Self>) {
        self.begin_new_conversation(cx);
        self.publish(cx);
    }

    /// Apply a transcript gesture and fulfill its controller effect.
    pub fn thread_intent(&mut self, intent: ThreadIntent, cx: &mut Context<Self>) {
        let effect = self.controller.handle_thread(intent);
        self.publish_and_fulfill(effect, cx);
    }

    /// Apply a composer gesture, publishing even an effect-free refusal.
    pub fn composer_intent(&mut self, intent: ComposerIntent, cx: &mut Context<Self>) {
        // These are shell transitions, so the controller intentionally returns
        // no effect for them. Platinum still owes the reader a visible answer.
        match intent {
            ComposerIntent::OpenSettings => {
                self.report_notice("Agent settings aren't available in Platinum yet.", cx);
            }
            ComposerIntent::CustomizeCapabilities => {
                self.report_notice(
                    "Capability customization isn't available in Platinum yet.",
                    cx,
                );
            }
            intent => {
                let effect = self.controller.handle_composer(intent);
                self.publish_and_fulfill(effect, cx);
            }
        }
    }

    /// Cancel the live run, including a stop owed before it acquires an id.
    pub fn stop(&mut self, cx: &mut Context<Self>) {
        self.composer_intent(ComposerIntent::Cancel, cx);
    }

    fn begin(&mut self, cx: &mut Context<Self>) {
        let (project_id, thread_id) = if self.connection == Connection::Native {
            super::saved::selection(cx)
        } else {
            (None, None)
        };
        self.hydration_target = BootstrapTarget {
            project_id,
            thread_id,
        };
        let effect = self.controller.begin();
        self.publish_and_fulfill(Some(effect), cx);
    }

    fn publish_and_fulfill(&mut self, effect: Option<ChatHostEffect>, cx: &mut Context<Self>) {
        self.publish(cx);
        if let Some(effect) = effect {
            self.fulfill(effect, cx);
        }
    }

    fn publish(&mut self, cx: &mut Context<Self>) {
        if self.pending_new_conversation && self.controller.phase() == ChatHostPhase::Ready {
            self.pending_new_conversation = false;
            self.pending_thread_selection = None;
            self.begin_new_conversation(cx);
        }
        self.projection = self.controller.projection();
        self.attachment_sources.retain(|id, _| {
            self.projection
                .composer
                .attachments
                .iter()
                .any(|a| &a.attachment_id == id)
        });
        if self.connection == Connection::Native {
            let project = self.projection.selected_project_id.as_deref();
            let thread = self.projection.selected_thread_id.as_deref();
            if (project, thread) != (self.last_saved.0.as_deref(), self.last_saved.1.as_deref()) {
                super::saved::set_selection(project, thread, cx);
                self.last_saved = (project.map(str::to_owned), thread.map(str::to_owned));
            }
        }
        cx.notify();
        self.answer_workspace_requests(cx);
    }

    fn report_notice(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.controller.report_notice(message);
        self.publish(cx);
    }

    fn fulfill(&mut self, effect: ChatHostEffect, cx: &mut Context<Self>) {
        match effect {
            ChatHostEffect::Bootstrap => self.bootstrap(cx),
            ChatHostEffect::LoadProjectThreads { project_id } => {
                self.load_project_threads(project_id, cx)
            }
            ChatHostEffect::LoadThread { thread_id } => self.load_thread(thread_id, cx),
            ChatHostEffect::SubmitTurn(turn) => self.submit(turn, cx),
            ChatHostEffect::CancelRun {
                run_id,
                thread_id,
                csrf_token,
            } => {
                self.authority_call(
                    Observer::Current,
                    move |transport, _| transport.cancel_run(run_id, thread_id, csrf_token),
                    |this, settled, cx| this.settle_snapshot(settled, "cancel that run", cx),
                    cx,
                );
            }
            ChatHostEffect::DecideApproval {
                run_id,
                thread_id,
                csrf_token,
                tool_call_id,
                approved,
            } => {
                let answer = ApprovalAnswer {
                    run_id,
                    thread_id,
                    csrf_token,
                    tool_call_id,
                    approved,
                };
                self.authority_call(
                    Observer::Current,
                    move |transport, _| transport.decide_approval(answer),
                    |this, settled, cx| this.settle_snapshot(settled, "send that decision", cx),
                    cx,
                );
            }
            ChatHostEffect::PickAttachment => self.fulfill_pick_attachment(cx),
            ChatHostEffect::UploadAttachment {
                attachment_id,
                name,
                media_type,
                bytes,
            } => {
                self.start_attachment_upload(
                    attachment_id,
                    AttachmentSource {
                        name,
                        media_type,
                        bytes,
                    },
                    cx,
                );
            }
            ChatHostEffect::RetryAttachment { attachment_id } => {
                if let Some(source) = self.attachment_sources.get(&attachment_id).cloned() {
                    self.start_attachment_upload(attachment_id, source, cx);
                } else {
                    self.controller.settle_attachment(&attachment_id, Err(
                        "The original bytes are no longer available; remove and pick the file again".into(),
                    ));
                    self.publish(cx);
                }
            }
            ChatHostEffect::RemoveAttachment {
                attachment_id,
                upload_id,
                revision,
                csrf_token,
            } => {
                self.authority_call(
                    Observer::Unbound,
                    move |transport, _| {
                        transport.remove_attachment(upload_id, revision, csrf_token)
                    },
                    move |this, removed, cx| {
                        match removed {
                            Ok(()) => {
                                this.attachment_sources.remove(&attachment_id);
                                this.controller.settle_attachment_removal(&attachment_id);
                            }
                            Err(error) => this.controller.report_notice(format!(
                                "The attachment reference could not be removed: {error}"
                            )),
                        }
                        this.publish(cx);
                    },
                    cx,
                );
            }
            ChatHostEffect::Dictate => {
                self.report_notice("Dictation isn't available in Platinum yet.", cx)
            }
            ChatHostEffect::Voice(_) => {
                self.report_notice("Voice isn't available in Platinum yet.", cx)
            }
            ChatHostEffect::ReadAloud(_) => {
                self.report_notice("Read aloud isn't available in Platinum yet.", cx)
            }
            ChatHostEffect::Capability {
                capability,
                selected,
            } => {
                let reason = "Capability changes aren't available in Platinum yet.";
                // A replaced or unknown capability keeps its existing state;
                // the independent notice still explains this host's refusal.
                let _ = self
                    .controller
                    .reject_capability(&capability, selected, reason);
                self.report_notice(reason, cx);
            }
            ChatHostEffect::ShowObject(_) => {
                self.report_notice("Opening this object isn't available in Platinum yet.", cx)
            }
            ChatHostEffect::OpenSource { url, .. } => match Url::parse(&url) {
                Ok(url) if matches!(url.scheme(), "http" | "https") => {
                    cx.emit(DriverEvent::OpenUrl(url));
                }
                _ => self.report_notice(
                    "This source can't be opened in Platinum; it needs an HTTP or HTTPS address.",
                    cx,
                ),
            },
        }
    }

    fn bootstrap(&mut self, cx: &mut Context<Self>) {
        let target = self.hydration_target.clone();
        self.authority_call(
            Observer::Supersede,
            move |transport, _| transport.bootstrap(target),
            |this, hydration, cx| match hydration {
                Ok(hydration) => {
                    let recovery = hydration
                        .snapshot
                        .as_ref()
                        .filter(|snapshot| live_run_id(snapshot).is_some())
                        .cloned();
                    let mut next = this.controller.adopt_hydration(hydration);
                    if !this.pending_new_conversation
                        && this.controller.phase() == ChatHostPhase::Ready
                    {
                        if let Some(thread_id) = this.pending_thread_selection.take() {
                            next = match this.controller.select_thread(&thread_id) {
                                Ok(effect) => effect,
                                Err(reason) => {
                                    this.controller.report_notice(reason);
                                    None
                                }
                            };
                        }
                    }
                    let generation = this.generation.get();
                    this.publish_and_fulfill(next, cx);
                    // A queued New Conversation may have superseded this read
                    // during publication. Do not rebind its former live run.
                    if this.generation.get() == generation {
                        if let Some(snapshot) = recovery {
                            this.resume_turn(snapshot, cx);
                        }
                    }
                }
                Err(error) => {
                    this.pending_new_conversation = false;
                    this.pending_thread_selection = None;
                    if error.is_unauthenticated() {
                        this.controller.refuse_bootstrap_unauthenticated();
                    } else {
                        this.controller.refuse_bootstrap(error.message());
                    }
                    this.publish(cx);
                }
            },
            cx,
        );
    }

    fn load_project_threads(&mut self, project_id: String, cx: &mut Context<Self>) {
        let requested = project_id.clone();
        self.authority_call(
            Observer::Supersede,
            move |transport, _| transport.load_project_threads(requested),
            move |this, loaded, cx| match loaded {
                Ok(threads) => {
                    let fallback = this.controller.hydrate_threads(&project_id, threads);
                    let next = match this.pending_thread_selection.take() {
                        Some(thread_id) => match this.controller.select_thread(&thread_id) {
                            Ok(effect) => effect,
                            Err(reason) => {
                                this.controller.report_notice(reason);
                                None
                            }
                        },
                        None => fallback,
                    };
                    this.publish_and_fulfill(next, cx);
                }
                Err(error) => {
                    this.pending_thread_selection = None;
                    this.report_notice(format!("Theorem could not load that project: {error}"), cx);
                }
            },
            cx,
        );
    }

    fn load_thread(&mut self, thread_id: String, cx: &mut Context<Self>) {
        self.authority_call(
            Observer::Supersede,
            move |transport, _| transport.load_thread(thread_id),
            |this, loaded, cx| match loaded {
                Ok(snapshot) => {
                    let recovery = live_run_id(&snapshot).is_some().then(|| snapshot.clone());
                    let generation = this.generation.get();
                    this.adopt_snapshot(snapshot, cx);
                    if this.generation.get() == generation {
                        if let Some(snapshot) = recovery {
                            this.resume_turn(snapshot, cx);
                        }
                    }
                }
                Err(error) => this.report_notice(
                    format!("Theorem could not load that conversation: {error}"),
                    cx,
                ),
            },
            cx,
        );
    }

    fn resume_turn(&mut self, snapshot: ThreadSnapshot, cx: &mut Context<Self>) {
        self.authority_call(
            Observer::Current,
            move |transport, follower| transport.resume_turn(snapshot, follower),
            |this, settled, cx| {
                this.settle_snapshot(settled, "recover that conversation's run", cx)
            },
            cx,
        );
    }

    fn submit(&mut self, turn: SubmitTurn, cx: &mut Context<Self>) {
        let sent = turn.clone();
        self.authority_call(
            Observer::Supersede,
            move |transport, follower| transport.submit_turn(turn, follower),
            move |this, settled, cx| match settled {
                Ok(snapshot) => {
                    this.controller.settle_turn_success(&sent);
                    this.adopt_snapshot(snapshot, cx);
                }
                Err(error) => {
                    this.controller.reject_turn(&sent, error.message());
                    this.publish(cx);
                }
            },
            cx,
        );
    }

    fn settle_snapshot(
        &mut self,
        settled: Result<ThreadSnapshot, ClientError>,
        operation: &str,
        cx: &mut Context<Self>,
    ) {
        match settled {
            Ok(snapshot) => self.adopt_snapshot(snapshot, cx),
            Err(error) => self.report_notice(format!("Theorem could not {operation}: {error}"), cx),
        }
    }

    fn adopt_snapshot(&mut self, snapshot: ThreadSnapshot, cx: &mut Context<Self>) {
        let reported = reported_run_state(&snapshot);
        let owed = self.controller.adopt_report(snapshot, reported);
        self.publish_and_fulfill(owed, cx);
    }

    fn begin_new_conversation(&mut self, cx: &mut Context<Self>) {
        match self.controller.phase() {
            ChatHostPhase::Ready => {
                self.pending_thread_selection = None;
                if matches!(
                    self.controller.authority_kind(),
                    ChatAuthorityKind::NativeTransport | ChatAuthorityKind::BrowserTransport
                ) {
                    let Some((project, csrf_token)) = self.controller.begin_durable_conversation()
                    else {
                        self.publish(cx);
                        return;
                    };
                    self.authority_call(
                        Observer::Supersede,
                        move |transport, _| transport.create_conversation(project, csrf_token),
                        |this, settled, cx| {
                            this.creating_conversation = None;
                            match settled {
                                Ok(snapshot) => this.adopt_snapshot(snapshot, cx),
                                Err(error) => {
                                    this.controller.reject_new_conversation(error.to_string());
                                    this.publish(cx);
                                }
                            }
                        },
                        cx,
                    );
                    // The spawn cannot settle inside this entity update.
                    // Keep its reservation while Loading is published.
                    self.creating_conversation = Some(self.generation.get());
                } else {
                    self.advance_generation();
                    self.controller.begin_new_conversation();
                }
            }
            ChatHostPhase::Unhydrated | ChatHostPhase::Loading => {
                if self.creating_conversation.is_none() {
                    self.pending_new_conversation = true;
                }
            }
            ChatHostPhase::Refused | ChatHostPhase::Unauthenticated => {
                self.controller
                    .report_notice("Connect to Theorem before starting a new conversation.");
            }
        }
    }

    fn answer_workspace_requests(&mut self, cx: &mut Context<Self>) {
        let due = self.controller.workspace_requests_due();
        for due in due {
            if !self
                .workspace_requests_answered
                .insert(due.request_ref.clone())
            {
                continue;
            }
            let receipt = WorkspaceReceipt::refused(
                due.operation,
                WorkspaceActor::Theorem,
                WorkspaceRefusal::InvalidTarget {
                    detail: "Workspace operations aren't available in Platinum yet.".into(),
                },
            );
            let answer = WorkspaceAnswer {
                run_id: due.run_id,
                csrf_token: due.csrf_token,
                request_ref: due.request_ref,
                receipt,
            };
            self.authority_call(
                Observer::Unbound,
                move |transport, _| transport.answer_workspace_request(answer),
                |this, settled, cx| {
                    if let Err(error) = settled {
                        this.report_notice(
                            format!("Theorem could not report that workspace refusal: {error}"),
                            cx,
                        );
                    }
                },
                cx,
            );
        }
    }

    fn fulfill_pick_attachment(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        let transport = Rc::clone(&self.transport);
        cx.spawn(
            async move |this: WeakEntity<TheoremDriver>, cx: &mut AsyncApp| {
                let selected = match paths.await {
                    Ok(Ok(Some(paths))) => paths,
                    Ok(Ok(None)) => return,
                    failure => {
                        let reason = match failure {
                            Ok(Err(error)) => format!("The attachment picker failed: {error}"),
                            _ => "The attachment picker closed before returning its selection."
                                .into(),
                        };
                        let _ = this.update(cx, |this, cx| {
                            if Rc::ptr_eq(&this.transport, &transport) {
                                this.report_notice(reason, cx);
                            }
                        });
                        return;
                    }
                };
                // One file at a time bounds the bytes waiting to enter the entity.
                for path in selected {
                    let source = cx
                        .background_spawn(async move { read_native_attachment(path) })
                        .await;
                    let accepted = this.update(cx, |this, cx| {
                        if !Rc::ptr_eq(&this.transport, &transport) {
                            return false;
                        }
                        match source {
                            Ok(source) => this.admit_attachment_bytes(source, cx),
                            Err(reason) => this.report_notice(reason, cx),
                        }
                        true
                    });
                    if !matches!(accepted, Ok(true)) {
                        break;
                    }
                }
            },
        )
        .detach();
    }

    fn admit_attachment_bytes(&mut self, source: AttachmentSource, cx: &mut Context<Self>) {
        let attachment_id = match theoremweb_agent_runtime::client::operation_key("native-file") {
            Ok(id) => id,
            Err(error) => {
                self.report_notice(error.to_string(), cx);
                return;
            }
        };
        match self.controller.begin_attachment(
            attachment_id.clone(),
            source.name.clone(),
            source.media_type.clone(),
        ) {
            Ok(()) => self.start_attachment_upload(attachment_id, source, cx),
            Err(reason) => self.report_notice(reason, cx),
        }
    }

    fn start_attachment_upload(
        &mut self,
        attachment_id: String,
        source: AttachmentSource,
        cx: &mut Context<Self>,
    ) {
        let (project_id, csrf_token) = match self.controller.attachment_target() {
            Ok(target) => target,
            Err(reason) => {
                self.controller
                    .settle_attachment(&attachment_id, Err(reason));
                self.publish(cx);
                return;
            }
        };
        self.attachment_sources
            .insert(attachment_id.clone(), source.clone());
        self.publish(cx);
        let upload = AttachmentUpload {
            project_id,
            name: source.name,
            media_type: source.media_type,
            bytes: source.bytes,
            csrf_token,
        };
        self.authority_call(
            Observer::Unbound,
            move |transport, _| transport.upload_attachment(upload),
            move |this, uploaded, cx| {
                let succeeded = uploaded.is_ok();
                this.controller
                    .settle_attachment(&attachment_id, uploaded.map_err(|error| error.to_string()));
                if succeeded {
                    this.attachment_sources.remove(&attachment_id);
                }
                this.publish(cx);
            },
            cx,
        );
    }

    fn advance_generation(&self) -> u64 {
        let next = self.generation.get().saturating_add(1);
        self.generation.set(next);
        next
    }

    fn authority_call<T: 'static>(
        &mut self,
        observer: Observer,
        call: impl for<'a> FnOnce(&'a dyn ChatTransport, Rc<dyn TurnFollower>) -> TransportFuture<'a, T>
        + 'static,
        settle: impl FnOnce(&mut Self, Result<T, ClientError>, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let generation = match observer {
            Observer::Supersede => {
                self.creating_conversation = None;
                self.advance_generation()
            }
            Observer::Current | Observer::Unbound => self.generation.get(),
        };
        let current = Rc::clone(&self.generation);
        let transport = Rc::clone(&self.transport);
        cx.spawn(
            async move |this: WeakEntity<TheoremDriver>, cx: &mut AsyncApp| {
                let follower: Rc<dyn TurnFollower> = Rc::new(DriverFollower {
                    driver: this.clone(),
                    cx: RefCell::new(cx.clone()),
                    generation,
                    current: Rc::clone(&current),
                });
                let answered = Compat::new(call(transport.as_ref(), follower)).await;
                if observer != Observer::Unbound && current.get() != generation {
                    return;
                }
                // Unbound replies may cross navigation, but never an explicit
                // authority replacement such as Use demo.
                let _ = this.update(cx, |this, cx| {
                    if Rc::ptr_eq(&this.transport, &transport) {
                        settle(this, answered, cx);
                    }
                });
            },
        )
        .detach();
    }
}

fn native_configured() -> bool {
    ["THEOREMWEB_AUTHORITY_URL", "THEOREMWEB_AUTH_TOKEN_FILE"]
        .iter()
        .all(|name| std::env::var(name).is_ok_and(|value| !value.trim().is_empty()))
}

fn read_native_attachment(path: PathBuf) -> Result<AttachmentSource, String> {
    use std::io::Read as _;

    const MAX_BYTES: u64 = 16 * 1024 * 1024;
    let name = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "The selected attachment has no usable file name.".to_owned())?
        .to_owned();
    let media_type = mime_guess::from_path(&path)
        .first_or_octet_stream()
        .essence_str()
        .to_owned();
    let file = std::fs::File::open(&path)
        .map_err(|_| format!("The selected attachment {name} could not be opened."))?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| format!("The selected attachment {name} could not be read."))?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_BYTES {
        return Err(format!(
            "The selected attachment {name} must contain 1 to {MAX_BYTES} bytes."
        ));
    }
    Ok(AttachmentSource {
        name,
        media_type,
        bytes,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Observer {
    Supersede,
    Current,
    Unbound,
}

struct DriverFollower {
    driver: WeakEntity<TheoremDriver>,
    cx: RefCell<AsyncApp>,
    generation: u64,
    current: Rc<Cell<u64>>,
}

impl DriverFollower {
    fn deliver(&self, apply: impl FnOnce(&mut ChatHostController) -> TurnFollow) -> Following {
        if self.abandoned() {
            return Following::Detached;
        }
        let mut following = Following::Detached;
        let _ = self.driver.update(&mut *self.cx.borrow_mut(), |this, cx| {
            let followed = apply(&mut this.controller);
            // Even a refusal with no follow-up effect must reach the reader.
            this.publish(cx);
            if let TurnFollow::Followed(effect) = followed {
                following = Following::Followed;
                if let Some(effect) = effect {
                    this.fulfill(effect, cx);
                }
            }
        });
        following
    }
}

impl TurnFollower for DriverFollower {
    fn bind(&self, binding: TurnBinding) -> Following {
        self.deliver(|controller| controller.bind_turn(binding))
    }

    fn commit(
        &self,
        thread_id: &str,
        run_id: &str,
        events: Vec<RunEventEnvelope>,
    ) -> Result<Following, String> {
        let mut refused = None;
        let following = self.deliver(|controller| {
            controller
                .follow_run_events(thread_id, run_id, events)
                .unwrap_or_else(|reason| {
                    controller.report_notice(reason.clone());
                    refused = Some(reason);
                    TurnFollow::Detached
                })
        });
        refused.map_or(Ok(following), Err)
    }

    fn abandoned(&self) -> bool {
        self.current.get() != self.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Entity, TestAppContext};
    use theorem_surface_contracts::thread::{ThreadActor, ThreadPart, ThreadRunState};
    use theorem_surface_contracts::workspace::{WorkspaceOperation, WorkspaceOutcome};

    fn demo(
        authority: Rc<DeterministicDemoAuthority>,
        cx: &mut TestAppContext,
    ) -> Entity<TheoremDriver> {
        let driver = cx.new(|cx| {
            TheoremDriver::with_transport(ChatAuthorityKind::DeterministicDemo, authority, cx)
        });
        cx.run_until_parked();
        driver
    }

    fn send(driver: &Entity<TheoremDriver>, text: &str, cx: &mut TestAppContext) {
        driver.update(cx, |this, cx| {
            this.composer_intent(
                ComposerIntent::Submit {
                    text: text.to_owned(),
                    quote: None,
                    steer: false,
                    context: Vec::new(),
                },
                cx,
            );
        });
        cx.run_until_parked();
    }

    fn has_reply(projection: &ChatHostProjection, prompt: &str) -> bool {
        projection.thread.messages.iter().any(|message| {
            message.actor == ThreadActor::Theorem && message.parts.iter().any(|part| {
                matches!(part, ThreadPart::Text { text, .. } if text.contains("Demo turn") && text.contains(prompt))
            })
        })
    }

    #[gpui_kit::test]
    fn hydration_loads_projects_and_conversations(cx: &mut TestAppContext) {
        let driver = demo(Rc::new(DeterministicDemoAuthority::new()), cx);
        cx.update(|cx| {
            let this = driver.read(cx);
            assert_eq!(this.connection(), Connection::Demo);
            assert_eq!(this.projection().phase, ChatHostPhase::Ready);
            assert!(!this.projection().projects.is_empty());
            assert!(!this.projection().threads.is_empty());
            assert!(this.projection().selected_thread_id.is_some());
        });
    }

    #[gpui_kit::test]
    fn selecting_a_conversation_loads_its_transcript(cx: &mut TestAppContext) {
        let driver = demo(Rc::new(DeterministicDemoAuthority::new()), cx);
        let (populated, empty) = cx.update(|cx| {
            let projection = driver.read(cx).projection();
            (
                projection.threads[0].thread_id.clone(),
                projection.threads[1].thread_id.clone(),
            )
        });
        driver.update(cx, |this, cx| this.select_thread(&empty, cx));
        cx.run_until_parked();
        cx.update(|cx| assert!(driver.read(cx).projection().thread.messages.is_empty()));
        driver.update(cx, |this, cx| this.select_thread(&populated, cx));
        cx.run_until_parked();
        cx.update(|cx| {
            let projection = driver.read(cx).projection();
            assert_eq!(
                projection.selected_thread_id.as_deref(),
                Some(populated.as_str())
            );
            assert!(!projection.thread.messages.is_empty());
            assert!(
                projection
                    .thread
                    .messages
                    .iter()
                    .any(|message| message.actor == ThreadActor::Theorem)
            );
        });
    }

    #[gpui_kit::test]
    fn submit_streams_a_reply_and_settles_to_idle(cx: &mut TestAppContext) {
        let authority = Rc::new(DeterministicDemoAuthority::new().holding_runs());
        let driver = demo(authority.clone(), cx);
        send(&driver, "Platinum streaming probe", cx);
        cx.update(|cx| {
            let projection = driver.read(cx).projection();
            assert_eq!(projection.thread.run_state, ThreadRunState::Streaming);
            assert!(has_reply(projection, "Platinum streaming probe"));
        });
        authority.release_runs();
        cx.run_until_parked();
        cx.update(|cx| {
            let projection = driver.read(cx).projection();
            assert_eq!(projection.thread.run_state, ThreadRunState::Idle);
            assert!(has_reply(projection, "Platinum streaming probe"));
        });
    }

    #[gpui_kit::test]
    fn stop_settles_a_held_run_without_replacing_its_observer(cx: &mut TestAppContext) {
        let driver = demo(
            Rc::new(DeterministicDemoAuthority::new().holding_runs()),
            cx,
        );
        send(&driver, "Hold this run for Stop", cx);
        let generation = cx.update(|cx| {
            let this = driver.read(cx);
            assert_eq!(
                this.projection().thread.run_state,
                ThreadRunState::Streaming
            );
            this.generation.get()
        });
        // The local half lands before the cancel command leaves.
        driver.update(cx, |this, cx| {
            this.stop(cx);
            assert_eq!(this.projection().thread.run_state, ThreadRunState::Aborted);
        });
        cx.run_until_parked();
        // Then the authority's settled conversation is adopted, as Theorem's
        // own host does (`a_stop_mid_run_lands_locally_and_settles_at_the_authority`).
        cx.update(|cx| {
            let this = driver.read(cx);
            assert_eq!(this.generation.get(), generation);
            assert_ne!(
                this.projection().thread.run_state,
                ThreadRunState::Streaming
            );
            assert!(!this.projection().composer.can_cancel());
            assert!(
                this.projection()
                    .notice
                    .as_deref()
                    .is_none_or(|notice| !notice.contains("could not")),
                "the stop was answered, not refused: {:?}",
                this.projection().notice
            );
        });
    }

    #[gpui_kit::test]
    fn missing_configuration_never_bootstraps(cx: &mut TestAppContext) {
        let driver = cx.new(|cx| TheoremDriver::from_configuration(false, cx));
        cx.run_until_parked();
        cx.update(|cx| {
            let this = driver.read(cx);
            assert_eq!(this.connection(), Connection::NotConfigured);
            assert_eq!(this.projection().phase, ChatHostPhase::Unhydrated);
            assert!(this.projection().projects.is_empty());
            assert_eq!(this.generation.get(), 0);
            assert!(this.sign_in_url().is_none());
        });
    }

    #[gpui_kit::test]
    fn unsupported_effect_publishes_a_notice(cx: &mut TestAppContext) {
        let driver = demo(Rc::new(DeterministicDemoAuthority::new()), cx);
        driver.update(cx, |this, cx| {
            this.composer_intent(ComposerIntent::Dictate, cx)
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(
                driver.read(cx).projection().notice.as_deref(),
                Some("Dictation isn't available in Platinum yet.")
            );
        });
        driver.update(cx, |this, cx| {
            this.fulfill(
                ChatHostEffect::OpenSource {
                    source_id: "unsafe-source".into(),
                    url: "file:///private/example".into(),
                },
                cx,
            )
        });
        cx.update(|cx| {
            assert!(
                driver
                    .read(cx)
                    .projection()
                    .notice
                    .as_deref()
                    .unwrap()
                    .contains("can't be opened")
            );
        });
    }

    #[gpui_kit::test]
    fn selection_supersedes_the_observer_without_cancelling_the_run(cx: &mut TestAppContext) {
        let authority = Rc::new(DeterministicDemoAuthority::new().holding_runs());
        let driver = demo(authority.clone(), cx);
        let (original, other) = cx.update(|cx| {
            let projection = driver.read(cx).projection();
            (
                projection.threads[0].thread_id.clone(),
                projection.threads[1].thread_id.clone(),
            )
        });
        send(&driver, "Keep running after selection", cx);
        driver.update(cx, |this, cx| this.select_thread(&other, cx));
        cx.run_until_parked();
        // Returning before release proves the run was left live and resumes
        // observation through the authoritative snapshot's live_run_id.
        driver.update(cx, |this, cx| this.select_thread(&original, cx));
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(
                driver.read(cx).projection().thread.run_state,
                ThreadRunState::Streaming
            )
        });
        authority.release_runs();
        cx.run_until_parked();
        cx.update(|cx| {
            let projection = driver.read(cx).projection();
            assert_eq!(
                projection.selected_thread_id.as_deref(),
                Some(original.as_str())
            );
            assert_eq!(projection.thread.run_state, ThreadRunState::Idle);
            assert!(has_reply(projection, "Keep running after selection"));
        });
    }

    #[gpui_kit::test]
    fn saved_native_target_is_restored_and_demo_does_not_overwrite_it(cx: &mut TestAppContext) {
        cx.update(|cx| {
            super::super::saved::init(None, cx);
            super::super::saved::set_selection(
                Some("demo-project-theoremweb"),
                Some("demo-thread-parity"),
                cx,
            );
        });
        let driver = cx.new(|cx| {
            TheoremDriver::with_transport(
                ChatAuthorityKind::NativeTransport,
                Rc::new(DeterministicDemoAuthority::new()),
                cx,
            )
        });
        cx.run_until_parked();
        let saved = cx.update(|cx| {
            assert_eq!(
                driver.read(cx).projection().selected_thread_id.as_deref(),
                Some("demo-thread-parity")
            );
            super::super::saved::selection(cx)
        });
        driver.update(cx, |this, cx| this.use_demo(cx));
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(driver.read(cx).connection(), Connection::Demo);
            assert_eq!(super::super::saved::selection(cx), saved);
        });
    }

    #[gpui_kit::test]
    fn a_live_run_is_followed_again_when_the_window_reopens(cx: &mut TestAppContext) {
        cx.update(|cx| super::super::saved::init(None, cx));
        let authority = Rc::new(DeterministicDemoAuthority::new().holding_runs());
        // A durable authority, so the selection is kept for the next window.
        let open = |cx: &mut TestAppContext| {
            let transport = authority.clone();
            let driver = cx.new(|cx| {
                TheoremDriver::with_transport(ChatAuthorityKind::NativeTransport, transport, cx)
            });
            cx.run_until_parked();
            driver
        };
        let first = open(cx);
        first.update(cx, |this, cx| {
            this.select_thread("demo-thread-controller", cx)
        });
        cx.run_until_parked();
        send(&first, "Keep going while Platinum restarts", cx);
        cx.update(|cx| {
            assert_eq!(
                first.read(cx).projection().thread.run_state,
                ThreadRunState::Streaming
            )
        });
        // The window closes mid-run; the run goes on at the authority.
        drop(first);
        cx.run_until_parked();

        let second = open(cx);
        cx.update(|cx| {
            let projection = second.read(cx).projection();
            assert_eq!(
                projection.selected_thread_id.as_deref(),
                Some("demo-thread-controller")
            );
            assert_eq!(projection.thread.run_state, ThreadRunState::Streaming);
            assert!(
                projection.composer.can_cancel(),
                "the resumed run can be stopped"
            );
        });
        authority.release_runs();
        cx.run_until_parked();
        cx.update(|cx| {
            let projection = second.read(cx).projection();
            assert_eq!(projection.thread.run_state, ThreadRunState::Idle);
            assert!(has_reply(projection, "Keep going while Platinum restarts"));
        });
    }

    #[gpui_kit::test]
    fn workspace_requests_receive_an_unavailable_receipt(cx: &mut TestAppContext) {
        let authority = Rc::new(DeterministicDemoAuthority::new());
        let driver = demo(authority.clone(), cx);
        authority.ask_workspace(WorkspaceOperation::ShowContext, Some("show the context"));
        send(&driver, "show the context", cx);
        let requests = authority.workspace_requests();
        assert_eq!(requests.len(), 1);
        let receipt = requests[0]
            .receipt
            .as_ref()
            .expect("workspace refusal reached the run");
        assert_eq!(receipt.outcome, WorkspaceOutcome::Refused);
        assert!(
            matches!(receipt.refusal.as_ref(), Some(WorkspaceRefusal::InvalidTarget { detail }) if detail.contains("Platinum"))
        );
        cx.update(|cx| {
            let this = driver.read(cx);
            assert_eq!(this.projection().thread.run_state, ThreadRunState::Idle);
            assert_eq!(this.workspace_requests_answered.len(), 1);
        });
    }

    #[gpui_kit::test]
    fn pending_new_conversation_waits_for_hydration(cx: &mut TestAppContext) {
        let driver = cx.new(|cx| {
            TheoremDriver::with_transport(
                ChatAuthorityKind::NativeTransport,
                Rc::new(DeterministicDemoAuthority::new()),
                cx,
            )
        });
        driver.update(cx, |this, cx| {
            assert_eq!(this.projection().phase, ChatHostPhase::Loading);
            this.new_conversation(cx);
        });
        cx.run_until_parked();
        cx.update(|cx| {
            let this = driver.read(cx);
            assert_eq!(this.projection().phase, ChatHostPhase::Ready);
            assert!(
                this.projection()
                    .selected_thread_id
                    .as_deref()
                    .unwrap()
                    .starts_with("demo-thread-new-")
            );
            assert!(this.projection().thread.messages.is_empty());
            assert!(!this.pending_new_conversation);
            assert!(this.creating_conversation.is_none());
        });
    }
}
