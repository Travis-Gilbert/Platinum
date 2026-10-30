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
use theorem_surface_contracts::thread::ThreadRunState;
use theorem_surface_contracts::workspace::{WorkspaceActor, WorkspaceReceipt, WorkspaceRefusal};
use theoremweb_agent_runtime::chat_host::SIGN_IN_ROUTE;
use theoremweb_agent_runtime::client::ClientError;
use theoremweb_agent_runtime::sse::RunEventEnvelope;
use theoremweb_agent_runtime::wire::{Project, ThreadSnapshot};
use theoremweb_agent_runtime::{
    ApprovalAnswer, AttachmentUpload, BootstrapTarget, ChatAuthorityKind, ChatHostController,
    ChatHostEffect, ChatHostPhase, ChatHostProjection, ChatTransport, DeterministicDemoAuthority,
    Following, HttpChatTransport, Hydration, RunState, SubmitTurn, TransportFuture, TurnBinding,
    TurnFollow, TurnFollower, WorkspaceAnswer, live_run_id, reported_run_state,
};
use url::Url;

/// The explicitly selected authority, or the absence of native configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Connection {
    NotConfigured,
    Native,
    Demo,
}

const UNCLOSED_RUN_NOTICE: &str = "Theorem never closed this conversation's last run, so it \
    refuses new messages here. Start a new conversation to continue.";

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
    /// The generation whose turn the authority has not settled yet.
    ///
    /// A run's last event arrives before the authority has written its answer
    /// into the conversation, and until the settled conversation is adopted
    /// the controller holds the revision from before the turn. A message sent
    /// in that gap names a revision the authority has moved past, so the
    /// composer reads as sending until then.
    settling: Option<u64>,
    /// The run the current turn's observer was handed.
    followed_run: Option<String>,
    /// A run this window saw end that the authority still names as live.
    ///
    /// An authority that restarts mid-run keeps the run linked to its
    /// conversation with nothing left to settle it, and refuses every later
    /// turn there. Following it again only waits out a settlement that never
    /// comes, so a conversation naming it is shown as interrupted instead.
    unclosed_run: Option<String>,
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
            settling: None,
            followed_run: None,
            unclosed_run: None,
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
        if self.settling == Some(self.generation.get())
            && !self.projection.thread.run_state.is_live()
        {
            self.projection.composer.is_sending = true;
        }
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
                            this.follow_named_run(snapshot, cx);
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
                Ok(snapshot) => this.present_loaded(snapshot, cx),
                Err(error) => this.report_notice(
                    format!("Theorem could not load that conversation: {error}"),
                    cx,
                ),
            },
            cx,
        );
    }

    /// Show a conversation read from the authority and follow a run it names.
    fn present_loaded(&mut self, snapshot: ThreadSnapshot, cx: &mut Context<Self>) {
        let recovery = live_run_id(&snapshot).is_some().then(|| snapshot.clone());
        let generation = self.generation.get();
        self.adopt_snapshot(snapshot, cx);
        if self.generation.get() == generation {
            if let Some(snapshot) = recovery {
                self.follow_named_run(snapshot, cx);
            }
        }
    }

    /// Follow the run a loaded conversation names, unless it is the unclosed
    /// one.
    fn follow_named_run(&mut self, snapshot: ThreadSnapshot, cx: &mut Context<Self>) {
        if self.unclosed_run.is_some() && live_run_id(&snapshot) == self.unclosed_run {
            let owed = self
                .controller
                .adopt_report(snapshot, RunState::Interrupted);
            self.controller.report_notice(UNCLOSED_RUN_NOTICE);
            self.publish_and_fulfill(owed, cx);
        } else {
            self.resume_turn(snapshot, cx);
        }
    }

    /// Settling `run` failed. If this window had seen it end, remember it as
    /// unclosed and say whether it did.
    fn note_unclosed(&mut self, run: Option<String>) -> bool {
        if run.is_none() || self.projection.thread.run_state.is_live() {
            return false;
        }
        self.unclosed_run = run;
        true
    }

    /// Read the session and the conversation again after the authority
    /// refused a turn in it.
    ///
    /// Two refusals leave this window holding something the authority has
    /// moved past. It can keep the message and then refuse its run, while the
    /// controller restores the conversation as it stood before the send; and
    /// it forgets its CSRF sessions when it restarts or their lease ends.
    /// Left there, every later send is refused as a revision conflict or an
    /// expired token. The refusal stays on screen unless the conversation
    /// turns out to name a run: one running after all, or the unclosed one,
    /// which explains the refusal itself.
    fn reread_after_refusal(&mut self, thread_id: String, cx: &mut Context<Self>) {
        let refusal = self.projection.notice.clone();
        let target = BootstrapTarget {
            project_id: self.projection.selected_project_id.clone(),
            thread_id: Some(thread_id.clone()),
        };
        self.authority_call(
            Observer::Current,
            move |transport, _| transport.bootstrap(target),
            move |this, hydrated, cx| {
                this.settling = None;
                this.present_reread(&thread_id, refusal, hydrated, cx);
            },
            cx,
        );
        self.settling = Some(self.generation.get());
        self.publish(cx);
    }

    /// Show what a read after a refusal found, keeping `refusal` on screen
    /// as `reread_after_refusal` describes.
    fn present_reread(
        &mut self,
        thread_id: &str,
        refusal: Option<String>,
        hydrated: Result<Hydration, ClientError>,
        cx: &mut Context<Self>,
    ) {
        if self.projection.selected_thread_id.as_deref() != Some(thread_id) {
            self.publish(cx);
            return;
        }
        let hydration = match hydrated {
            Ok(hydration) => hydration,
            Err(error) => {
                return self.report_notice(
                    format!("Theorem could not reload that conversation: {error}"),
                    cx,
                );
            }
        };
        if !self.controller.refresh_csrf_token(&hydration.bootstrap) {
            return self.report_notice(
                "The Theorem session now belongs to another account. Reconnect before sending.",
                cx,
            );
        }
        let Some(snapshot) = hydration.snapshot else {
            return self.report_notice("Theorem no longer has that conversation.", cx);
        };
        let names_run = live_run_id(&snapshot).is_some();
        self.present_loaded(snapshot, cx);
        if let Some(refusal) = refusal.filter(|_| !names_run) {
            self.report_notice(refusal, cx);
        }
    }

    fn resume_turn(&mut self, snapshot: ThreadSnapshot, cx: &mut Context<Self>) {
        let run = live_run_id(&snapshot);
        self.authority_call(
            Observer::Current,
            move |transport, follower| transport.resume_turn(snapshot, follower),
            move |this, settled, cx| {
                this.settling = None;
                if settled.is_err() && this.note_unclosed(run) {
                    return this.report_notice(UNCLOSED_RUN_NOTICE, cx);
                }
                this.settle_snapshot(settled, "recover that conversation's run", cx)
            },
            cx,
        );
        self.settling = Some(self.generation.get());
    }

    fn submit(&mut self, turn: SubmitTurn, cx: &mut Context<Self>) {
        self.send_turn(turn, true, cx);
    }

    /// Send `turn`. When `renew` allows it, a refusal before the turn's run
    /// started goes through `renew_and_resend` first.
    fn send_turn(&mut self, turn: SubmitTurn, renew: bool, cx: &mut Context<Self>) {
        let sent = turn.clone();
        self.followed_run = None;
        self.authority_call(
            Observer::Supersede,
            move |transport, follower| transport.submit_turn(turn, follower),
            move |this, settled, cx| {
                this.settling = None;
                match settled {
                    Ok(snapshot) => {
                        this.controller.settle_turn_success(&sent);
                        this.adopt_written_turn(snapshot, cx);
                    }
                    Err(error) => {
                        let followed = this.followed_run.take();
                        if renew && followed.is_none() {
                            return this.renew_and_resend(sent, error, cx);
                        }
                        this.note_unclosed(followed);
                        this.refuse_turn(&sent, &error, cx);
                    }
                }
            },
            cx,
        );
        self.settling = Some(self.generation.get());
        self.publish(cx);
    }

    fn refuse_turn(&mut self, sent: &SubmitTurn, error: &ClientError, cx: &mut Context<Self>) {
        self.controller.reject_turn(sent, error.message());
        self.publish(cx);
        if let Some(snapshot) = &sent.snapshot {
            self.reread_after_refusal(snapshot.thread.id.clone(), cx);
        }
    }

    /// Renew the CSRF session after the authority refused `sent` before its
    /// run started, and send it once more when nothing of it was written.
    ///
    /// The authority keeps one CSRF session per account until its lease ends
    /// and forgets them all when it restarts, so the first send after either
    /// is refused at that gate. A session token other than the one `sent`
    /// carried, with the conversation still at the revision `sent` was
    /// written against, is that refusal: the same message goes again with the
    /// new token. Anything else is the refusal it looks like, and this read
    /// stands in for the one `reread_after_refusal` would make.
    fn renew_and_resend(&mut self, sent: SubmitTurn, refusal: ClientError, cx: &mut Context<Self>) {
        let Some((thread_id, revision)) = sent
            .snapshot
            .as_ref()
            .map(|snapshot| (snapshot.thread.id.clone(), snapshot.thread.revision))
        else {
            return self.refuse_turn(&sent, &refusal, cx);
        };
        let target = BootstrapTarget {
            project_id: self.projection.selected_project_id.clone(),
            thread_id: Some(thread_id.clone()),
        };
        self.authority_call(
            Observer::Current,
            move |transport, _| transport.bootstrap(target),
            move |this, hydrated, cx| {
                this.settling = None;
                if let Ok(hydration) = &hydrated
                    && hydration.bootstrap.csrf_token != sent.csrf_token
                    && hydration.snapshot.as_ref().map(|s| s.thread.revision) == Some(revision)
                    && this.controller.refresh_csrf_token(&hydration.bootstrap)
                {
                    let mut turn = sent;
                    turn.csrf_token.clone_from(&hydration.bootstrap.csrf_token);
                    return this.send_turn(turn, false, cx);
                }
                this.controller.reject_turn(&sent, refusal.message());
                this.publish(cx);
                let notice = this.projection.notice.clone();
                this.present_reread(&thread_id, notice, hydrated, cx);
            },
            cx,
        );
        self.settling = Some(self.generation.get());
        self.publish(cx);
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

    /// Adopt the conversation the authority wrote after the run of a turn
    /// this window followed.
    ///
    /// The authority writes a failed run's turn without its failure: the
    /// answer holds only the first progress part, still running, and the
    /// conversation names no run, which reads as a turn that finished. When
    /// the run's stream ended it unfinished, that ending is kept.
    fn adopt_written_turn(&mut self, snapshot: ThreadSnapshot, cx: &mut Context<Self>) {
        let streamed = match self.projection.thread.run_state {
            ThreadRunState::Failed => Some(RunState::Failed),
            ThreadRunState::Aborted => Some(RunState::Cancelled),
            ThreadRunState::Interrupted => Some(RunState::Interrupted),
            _ => None,
        };
        let reported = match (reported_run_state(&snapshot), streamed) {
            (RunState::Idle, Some(ended)) => ended,
            (reported, _) => reported,
        };
        let owed = self.controller.adopt_report(snapshot, reported);
        self.publish_and_fulfill(owed, cx);
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
                    self.create_conversation(project, csrf_token, true, cx);
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

    /// Create a conversation in `project`. When `renew` allows it, a refusal
    /// goes through `renew_and_create` first.
    fn create_conversation(
        &mut self,
        project: Option<Project>,
        csrf_token: String,
        renew: bool,
        cx: &mut Context<Self>,
    ) {
        let sent = (project.clone(), csrf_token.clone());
        self.authority_call(
            Observer::Supersede,
            move |transport, _| transport.create_conversation(project, csrf_token),
            move |this, settled, cx| match settled {
                Ok(snapshot) => {
                    this.creating_conversation = None;
                    this.adopt_snapshot(snapshot, cx);
                }
                Err(error) if renew => this.renew_and_create(sent, error, cx),
                Err(error) => this.refuse_new_conversation(&error, cx),
            },
            cx,
        );
        // The spawn cannot settle inside this entity update.
        // Keep its reservation while Loading is published.
        self.creating_conversation = Some(self.generation.get());
    }

    fn refuse_new_conversation(&mut self, error: &ClientError, cx: &mut Context<Self>) {
        self.creating_conversation = None;
        self.controller.reject_new_conversation(error.to_string());
        self.publish(cx);
    }

    /// Renew the CSRF session after the authority refused a new
    /// conversation, and create it once more if the session had lapsed.
    ///
    /// As with a send (`renew_and_resend`), a session token other than the
    /// one the request carried says it was refused at the CSRF gate. A
    /// create that reached an authority which then restarted before
    /// answering can leave one empty conversation behind; it holds nothing
    /// the reader wrote.
    fn renew_and_create(
        &mut self,
        (project, refused_token): (Option<Project>, String),
        refusal: ClientError,
        cx: &mut Context<Self>,
    ) {
        let target = BootstrapTarget {
            project_id: self.projection.selected_project_id.clone(),
            thread_id: None,
        };
        self.authority_call(
            Observer::Current,
            move |transport, _| transport.bootstrap(target),
            move |this, hydrated, cx| {
                if let Ok(hydration) = hydrated
                    && hydration.bootstrap.csrf_token != refused_token
                    && this.controller.refresh_csrf_token(&hydration.bootstrap)
                {
                    return this.create_conversation(
                        project,
                        hydration.bootstrap.csrf_token,
                        false,
                        cx,
                    );
                }
                this.refuse_new_conversation(&refusal, cx);
            },
            cx,
        );
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
    fn deliver(&self, apply: impl FnOnce(&mut TheoremDriver) -> TurnFollow) -> Following {
        if self.abandoned() {
            return Following::Detached;
        }
        let mut following = Following::Detached;
        let _ = self.driver.update(&mut *self.cx.borrow_mut(), |this, cx| {
            let followed = apply(this);
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
        self.deliver(|driver| {
            driver.followed_run = Some(binding.run_id.clone());
            driver.controller.bind_turn(binding)
        })
    }

    fn commit(
        &self,
        thread_id: &str,
        run_id: &str,
        events: Vec<RunEventEnvelope>,
    ) -> Result<Following, String> {
        let mut refused = None;
        let following = self.deliver(|driver| {
            let controller = &mut driver.controller;
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
    use theorem_surface_contracts::thread::{ThreadActor, ThreadPart};
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

    fn times_sent(projection: &ChatHostProjection, prompt: &str) -> usize {
        projection
            .thread
            .messages
            .iter()
            .filter(|message| {
                message.actor == ThreadActor::User
                    && message
                        .parts
                        .iter()
                        .any(|part| matches!(part, ThreadPart::Text { text, .. } if text == prompt))
            })
            .count()
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

    /// The demo authority with the real one's revision check: a send naming
    /// a revision the conversation has moved past is refused as a conflict,
    /// which the demo itself never does.
    ///
    /// It can also refuse a run after keeping its message, as a session
    /// without `models:invoke` is refused (the kept message only moves the
    /// revision, which is all the check reads); hold a turn's settlement back
    /// after its run's last event, as the authority writes the answer into
    /// the conversation after the stream ends; forget the CSRF session it
    /// issued, as it does when it restarts; fail a run and write its turn
    /// without the failure, as it does when the model provider refuses its
    /// key; and lose a run across a restart, as it does today: the
    /// conversation stays linked to the run, nothing settles it, and every
    /// later turn there is refused as a conflict.
    #[derive(Default)]
    struct StrictAuthority {
        inner: DeterministicDemoAuthority,
        csrf_token: RefCell<String>,
        kept: Cell<u64>,
        refuse_next_run: Cell<bool>,
        fail_next_run: Cell<bool>,
        settlement_held: Cell<bool>,
        settlement: RefCell<Option<std::task::Waker>>,
        orphaned: Cell<bool>,
        resumed: Cell<usize>,
    }

    impl StrictAuthority {
        fn held(&self, mut snapshot: ThreadSnapshot) -> ThreadSnapshot {
            snapshot.thread.revision += self.kept.get();
            snapshot
        }

        fn release_settlement(&self) {
            self.settlement_held.set(false);
            self.give_up_settling();
        }

        /// Restart with the run in flight forgotten.
        fn restart_mid_run(&self) {
            self.orphaned.set(true);
        }

        /// End the wait a turn's settlement loop gives a run, which for a
        /// lost run ends in failure.
        fn give_up_settling(&self) {
            if let Some(waker) = self.settlement.take() {
                waker.wake();
            }
        }

        fn unsettled(&self, thread_id: &str) -> ClientError {
            let run = self
                .inner
                .snapshot(thread_id)
                .as_ref()
                .and_then(live_run_id)
                .unwrap_or_default();
            ClientError::from_runtime(format!(
                "Theorem run {run} ended before its durable conversation settlement became visible"
            ))
        }
    }

    impl ChatTransport for StrictAuthority {
        fn bootstrap(
            &self,
            target: BootstrapTarget,
        ) -> TransportFuture<'_, theoremweb_agent_runtime::Hydration> {
            Box::pin(async move {
                let mut hydration = self.inner.bootstrap(target).await?;
                hydration.bootstrap.csrf_token = self.csrf_token.borrow().clone();
                hydration.snapshot = hydration.snapshot.map(|s| self.held(s));
                Ok(hydration)
            })
        }

        fn load_project_threads(
            &self,
            project_id: String,
        ) -> TransportFuture<
            '_,
            theoremweb_agent_runtime::wire::Page<theoremweb_agent_runtime::wire::Thread>,
        > {
            self.inner.load_project_threads(project_id)
        }

        fn load_thread(&self, thread_id: String) -> TransportFuture<'_, ThreadSnapshot> {
            Box::pin(async move {
                self.inner
                    .load_thread(thread_id)
                    .await
                    .map(|s| self.held(s))
            })
        }

        fn submit_turn(
            &self,
            turn: SubmitTurn,
            follower: Rc<dyn TurnFollower>,
        ) -> TransportFuture<'_, ThreadSnapshot> {
            Box::pin(async move {
                if turn.csrf_token != *self.csrf_token.borrow() {
                    return Err(ClientError::from_runtime(
                        "The chat CSRF token is invalid or expired",
                    ));
                }
                let sent = turn.snapshot.as_ref().expect("sent from a conversation");
                let thread_id = sent.thread.id.clone();
                let current = self.held(self.inner.snapshot(&thread_id).expect("held"));
                let lost_run = self.orphaned.get() && live_run_id(&current).is_some();
                if lost_run || sent.thread.revision != current.thread.revision {
                    return Err(ClientError::from_runtime(format!(
                        "chat revision conflict: expected {}, current {}",
                        sent.thread.revision, current.thread.revision
                    )));
                }
                if self.refuse_next_run.replace(false) {
                    self.kept.set(self.kept.get() + 1);
                    return Err(ClientError::from_runtime(
                        "The authenticated identity lacks a required chat scope",
                    ));
                }
                let follower: Rc<dyn TurnFollower> = if self.fail_next_run.replace(false) {
                    Rc::new(Failing(follower))
                } else {
                    follower
                };
                let mut run = self.inner.submit_turn(turn, follower);
                let settled = std::future::poll_fn(|task| {
                    if self.orphaned.get() {
                        return std::task::Poll::Ready(Err(self.unsettled(&thread_id)));
                    }
                    *self.settlement.borrow_mut() = Some(task.waker().clone());
                    run.as_mut().poll(task)
                })
                .await;
                std::future::poll_fn(|task| {
                    if self.settlement_held.get() {
                        *self.settlement.borrow_mut() = Some(task.waker().clone());
                        std::task::Poll::Pending
                    } else {
                        std::task::Poll::Ready(())
                    }
                })
                .await;
                settled.map(|s| self.held(s))
            })
        }

        fn resume_turn(
            &self,
            snapshot: ThreadSnapshot,
            follower: Rc<dyn TurnFollower>,
        ) -> TransportFuture<'_, ThreadSnapshot> {
            if self.orphaned.get() {
                self.resumed.set(self.resumed.get() + 1);
                let lost = self.unsettled(&snapshot.thread.id);
                return Box::pin(std::future::ready(Err(lost)));
            }
            self.inner.resume_turn(snapshot, follower)
        }

        fn cancel_run(
            &self,
            run_id: String,
            thread_id: String,
            csrf_token: String,
        ) -> TransportFuture<'_, ThreadSnapshot> {
            if self.orphaned.get() {
                // The cancel is recorded, and the conversation stays linked.
                let linked = self.inner.snapshot(&thread_id).map(|s| self.held(s));
                return Box::pin(std::future::ready(
                    linked.ok_or_else(|| ClientError::from_runtime("no such conversation")),
                ));
            }
            self.inner.cancel_run(run_id, thread_id, csrf_token)
        }

        fn decide_approval(&self, decision: ApprovalAnswer) -> TransportFuture<'_, ThreadSnapshot> {
            self.inner.decide_approval(decision)
        }

        fn answer_workspace_request(&self, answer: WorkspaceAnswer) -> TransportFuture<'_, ()> {
            self.inner.answer_workspace_request(answer)
        }

        fn open_conversation(
            &self,
            thread_id: String,
            current_project_id: Option<String>,
        ) -> TransportFuture<
            '_,
            (
                Option<
                    theoremweb_agent_runtime::wire::Page<theoremweb_agent_runtime::wire::Thread>,
                >,
                ThreadSnapshot,
            ),
        > {
            self.inner.open_conversation(thread_id, current_project_id)
        }

        fn create_conversation(
            &self,
            project: Option<theoremweb_agent_runtime::wire::Project>,
            csrf_token: String,
        ) -> TransportFuture<'_, ThreadSnapshot> {
            if csrf_token != *self.csrf_token.borrow() {
                return Box::pin(std::future::ready(Err(ClientError::from_runtime(
                    "The chat CSRF token is invalid or expired",
                ))));
            }
            self.inner.create_conversation(project, csrf_token)
        }

        fn delete_conversations(
            &self,
            deletion: theoremweb_agent_runtime::ConversationDeletion,
        ) -> TransportFuture<'_, theoremweb_agent_runtime::ConversationDeletionReport> {
            self.inner.delete_conversations(deletion)
        }

        fn upload_attachment(
            &self,
            attachment: AttachmentUpload,
        ) -> TransportFuture<'_, theoremweb_agent_runtime::wire::Upload> {
            self.inner.upload_attachment(attachment)
        }

        fn remove_attachment(
            &self,
            upload_id: String,
            revision: u64,
            csrf_token: String,
        ) -> TransportFuture<'_, ()> {
            self.inner
                .remove_attachment(upload_id, revision, csrf_token)
        }
    }

    /// Passes a run's events on, ending it failed where the demo closes it.
    struct Failing(Rc<dyn TurnFollower>);

    impl TurnFollower for Failing {
        fn bind(&self, binding: TurnBinding) -> Following {
            self.0.bind(binding)
        }

        fn commit(
            &self,
            thread_id: &str,
            run_id: &str,
            mut events: Vec<RunEventEnvelope>,
        ) -> Result<Following, String> {
            for event in &mut events {
                if event.source_event_type == "RUN.CLOSED" {
                    "RUN.FAILED".clone_into(&mut event.source_event_type);
                    event.payload = serde_json::json!({
                        "reason": "The model provider refused the authority's key",
                    });
                }
            }
            self.0.commit(thread_id, run_id, events)
        }

        fn abandoned(&self) -> bool {
            self.0.abandoned()
        }
    }

    fn strict(authority: Rc<StrictAuthority>, cx: &mut TestAppContext) -> Entity<TheoremDriver> {
        let driver = cx.new(|cx| {
            TheoremDriver::with_transport(ChatAuthorityKind::DeterministicDemo, authority, cx)
        });
        cx.run_until_parked();
        driver
    }

    #[gpui_kit::test]
    fn the_next_message_waits_until_the_authority_settles_the_last(cx: &mut TestAppContext) {
        let authority = Rc::new(StrictAuthority::default());
        authority.settlement_held.set(true);
        let driver = strict(authority.clone(), cx);
        send(&driver, "Answered, not yet settled", cx);
        cx.update(|cx| {
            let composer = &driver.read(cx).projection().composer;
            assert_eq!(composer.run_state, ThreadRunState::Idle);
            assert!(has_reply(
                driver.read(cx).projection(),
                "Answered, not yet settled"
            ));
            assert!(composer.is_sending);
            assert!(!composer.affordance("Sent after settlement").is_send());
        });
        authority.release_settlement();
        cx.run_until_parked();
        cx.update(|cx| {
            let composer = &driver.read(cx).projection().composer;
            assert!(composer.affordance("Sent after settlement").is_send());
        });
        send(&driver, "Sent after settlement", cx);
        cx.update(|cx| {
            let projection = driver.read(cx).projection();
            let notice = projection.notice.as_deref().unwrap_or_default();
            assert!(!notice.contains("refused"), "{notice}");
            assert!(has_reply(projection, "Sent after settlement"));
        });
    }

    #[gpui_kit::test]
    fn a_refused_turn_leaves_the_conversation_at_the_authority_revision(cx: &mut TestAppContext) {
        let authority = Rc::new(StrictAuthority::default());
        authority.refuse_next_run.set(true);
        let driver = strict(authority, cx);
        send(&driver, "Refused at the run", cx);
        cx.update(|cx| {
            let projection = driver.read(cx).projection();
            let notice = projection.notice.as_deref().unwrap_or_default();
            assert!(notice.contains("refused that turn"), "{notice}");
            assert_eq!(projection.composer.draft, "Refused at the run");
        });
        send(&driver, "Sent after the refusal", cx);
        cx.update(|cx| {
            let projection = driver.read(cx).projection();
            let notice = projection.notice.as_deref().unwrap_or_default();
            assert!(!notice.contains("refused"), "{notice}");
            assert!(has_reply(projection, "Sent after the refusal"));
        });
    }

    #[gpui_kit::test]
    fn a_send_after_the_authority_restarts_uses_its_new_session(cx: &mut TestAppContext) {
        let authority = Rc::new(StrictAuthority::default());
        let driver = strict(authority.clone(), cx);
        authority
            .csrf_token
            .replace("issued after the restart".to_owned());
        send(&driver, "Sent across the restart", cx);
        cx.update(|cx| {
            let projection = driver.read(cx).projection();
            let notice = projection.notice.as_deref().unwrap_or_default();
            assert!(!notice.contains("refused"), "{notice}");
            assert!(has_reply(projection, "Sent across the restart"));
            assert_eq!(times_sent(projection, "Sent across the restart"), 1);
            assert!(projection.composer.draft.is_empty());
        });
    }

    #[gpui_kit::test]
    fn a_send_is_renewed_once_and_then_refused_as_it_is(cx: &mut TestAppContext) {
        let authority = Rc::new(StrictAuthority::default());
        let driver = strict(authority.clone(), cx);
        authority
            .csrf_token
            .replace("issued after the restart".to_owned());
        authority.refuse_next_run.set(true);
        send(&driver, "Refused after the renewal", cx);
        cx.update(|cx| {
            let projection = driver.read(cx).projection();
            let notice = projection.notice.as_deref().unwrap_or_default();
            assert!(notice.contains("required chat scope"), "{notice}");
            assert_eq!(projection.composer.draft, "Refused after the renewal");
            assert!(!projection.composer.is_sending);
        });
        send(&driver, "Sent after the renewal", cx);
        cx.update(|cx| {
            let projection = driver.read(cx).projection();
            let notice = projection.notice.as_deref().unwrap_or_default();
            assert!(!notice.contains("refused"), "{notice}");
            assert!(has_reply(projection, "Sent after the renewal"));
        });
    }

    #[gpui_kit::test]
    fn a_new_conversation_after_the_authority_restarts_uses_its_new_session(
        cx: &mut TestAppContext,
    ) {
        let authority = Rc::new(StrictAuthority::default());
        let driver = cx.new(|cx| {
            TheoremDriver::with_transport(ChatAuthorityKind::NativeTransport, authority.clone(), cx)
        });
        cx.run_until_parked();
        authority
            .csrf_token
            .replace("issued after the restart".to_owned());
        driver.update(cx, |this, cx| this.new_conversation(cx));
        cx.run_until_parked();
        cx.update(|cx| {
            let this = driver.read(cx);
            assert_eq!(this.projection().notice, None);
            assert_eq!(this.projection().phase, ChatHostPhase::Ready);
            let thread = this.projection().selected_thread_id.as_deref().unwrap();
            assert!(thread.starts_with("demo-thread-new-"), "{thread}");
            assert!(this.creating_conversation.is_none());
        });
    }

    #[gpui_kit::test]
    fn a_failed_run_stays_failed_once_the_authority_writes_its_turn(cx: &mut TestAppContext) {
        let authority = Rc::new(StrictAuthority::default());
        authority.fail_next_run.set(true);
        let driver = strict(authority, cx);
        send(&driver, "Failed at the provider", cx);
        cx.update(|cx| {
            let projection = driver.read(cx).projection();
            let notice = projection.notice.as_deref().unwrap_or_default();
            assert!(!notice.contains("refused"), "{notice}");
            assert_eq!(projection.thread.run_state, ThreadRunState::Failed);
            assert!(!projection.composer.is_sending);
            assert_eq!(times_sent(projection, "Failed at the provider"), 1);
        });
    }

    #[gpui_kit::test]
    fn a_run_the_authority_lost_is_not_followed_again(cx: &mut TestAppContext) {
        let authority = Rc::new(StrictAuthority {
            inner: DeterministicDemoAuthority::new().holding_runs(),
            ..StrictAuthority::default()
        });
        let driver = strict(authority.clone(), cx);
        send(&driver, "Lost across the restart", cx);
        authority.restart_mid_run();
        driver.update(cx, |this, cx| this.stop(cx));
        cx.run_until_parked();
        authority.give_up_settling();
        cx.run_until_parked();
        let stuck = |driver: &Entity<TheoremDriver>, cx: &mut TestAppContext| {
            cx.update(|cx| {
                let projection = driver.read(cx).projection();
                let notice = projection.notice.as_deref().unwrap_or_default();
                assert!(notice.contains("new conversation"), "{notice}");
                assert_eq!(projection.thread.run_state, ThreadRunState::Interrupted);
                assert!(!projection.composer.can_cancel());
                assert!(!projection.composer.is_sending);
            });
            assert_eq!(
                authority.resumed.get(),
                0,
                "the lost run was followed again"
            );
        };
        stuck(&driver, cx);
        send(&driver, "Refused in the stuck conversation", cx);
        stuck(&driver, cx);
        cx.update(|cx| {
            let draft = &driver.read(cx).projection().composer.draft;
            assert_eq!(draft, "Refused in the stuck conversation");
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
