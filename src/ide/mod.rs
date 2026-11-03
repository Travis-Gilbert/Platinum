//! The Code place: one workspace of Theorem's IDE host (`theorem-ide-host`),
//! edited natively on the contract its web client speaks.
//!
//! A local folder is served by a host this process embeds. The place calls
//! it in process (`Connection::direct`): no HTTP and no TypeScript on the
//! keystroke path. The same host serves its authenticated routes on loopback,
//! with a credential file under the app's data folder, so a web client can
//! open the very workspace. A remote host is reached over those routes
//! (`Connection::remote`), with the same meaning and refusals.
//!
//! Each open file is one `DocumentSession`: edits apply at once, one is in
//! flight, another writer's revisions are transformed in, and undo and redo
//! are this client's own, across reconnects. A file changed on disk under
//! unsaved edits raises a conflict bar; diagnostics are shown only for the
//! revision they were computed at; a refactor is prepared at a recorded
//! revision and refused once the document has left it. The terminal pane
//! attaches to a host shell, one this window or the web client opened, and
//! shows the host's screen (the adapted terminal composition is X01's).
//!
//! `FRAME_CODE_CHECKOUT` names a folder to open at launch;
//! `FRAME_CODE_PORT` the loopback port (any free one by default);
//! `FRAME_CODE_REMOTE_URL`, `FRAME_CODE_REMOTE_TENANT`,
//! `FRAME_CODE_REMOTE_TOKEN_FILE` and `FRAME_CODE_WORKSPACE` a remote host.
//!
//! SOURCING: GPUI Kit's `Editor`, `Input`, `List`, `Button`, `Alert` and
//! `NativeMenu`; the session, the transports and the terminal attachment are
//! `theorem_ide_host::client`.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::input::{Diagnostic, DiagnosticSeverity, RopeExt as _};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IndexPath, Sizable as _,
    alert::Alert,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{
        Copy, Cut, Editor, EditorState, Input, InputEvent, InputState, Paste, Redo, SelectAll, Undo,
    },
    list::{List, ListDelegate, ListItem, ListState},
    native_menu::NativeMenu,
    scroll::ScrollableElement as _,
    v_flex,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, EntityInputHandler as _, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, PathPromptOptions,
    Render, SharedString, Styled as _, Subscription, Task, WeakEntity, Window, actions, div,
    prelude::FluentBuilder as _, px,
};
use serde_json::{Value, json};
use theorem_ide_host::client::document::{DocumentSession, Keep, Output, Purpose, TextChange};
use theorem_ide_host::client::terminal::{TerminalAttachment, TerminalPurpose};
use theorem_ide_host::client::{CallError, Connection, Live, Reply};
use theorem_ide_host::http::HostState;

pub const CONTEXT: &str = "Code";

actions!(
    code,
    [
        /// Write the open file to the checkout.
        Save,
        /// Prepare the structural replacement at the open revision.
        PrepareRefactor,
        /// Apply the prepared plan.
        ApplyRefactor,
    ]
);

pub fn bindings() -> Vec<KeyBinding> {
    vec![KeyBinding::new("cmd-s", Save, Some(CONTEXT))]
}

/// Typing within this long of the last edit joins its undo step.
const MERGE_WINDOW: Duration = Duration::from_millis(800);
const TERMINAL_COLS: u16 = 100;
const TERMINAL_ROWS: u16 = 24;

/// The host's runtime: the embedded host's checkout watch and loopback
/// listener, its blocking calls, and a remote host's requests.
static RUNTIME: LazyLock<std::io::Result<tokio::runtime::Runtime>> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("frame-code")
        .enable_all()
        .build()
});

pub(crate) fn runtime() -> anyhow::Result<&'static tokio::runtime::Runtime> {
    RUNTIME
        .as_ref()
        .map_err(|error| anyhow::anyhow!("the code runtime did not start: {error}"))
}

/// Where the place's workspace lives.
#[derive(Clone)]
pub struct Link {
    pub connection: Connection,
    /// The embedded host itself, which the native check writes through
    /// synchronously to race the place's own edits.
    pub host: Option<HostState>,
    /// The loopback origin a web client attaches to, for an embedded host.
    pub served: Option<String>,
    /// The folder, or the remote origin.
    pub label: String,
}

/// Where the embedded host keeps its state and loopback credential.
fn host_dir() -> anyhow::Result<PathBuf> {
    let base = dirs::data_dir().ok_or_else(|| anyhow::anyhow!("no application data directory"))?;
    Ok(crate::brand::app_dir(&base).join("code-host"))
}

/// The embedded host for `checkout`, keeping its state under `data`: opened,
/// bound, and serving on loopback.
pub fn embed(checkout: &Path, data: &Path) -> anyhow::Result<Link> {
    let runtime = runtime()?;
    std::fs::create_dir_all(data)?;
    let token = credential(&data.join("token"))?;
    let state = HostState::new("local".into(), token.into_bytes(), data.join("root"))?;
    let workspace = workspace_id(checkout);
    {
        // The checkout watch spawns on the current runtime.
        let _entered = runtime.enter();
        state
            .open_workspace(&workspace)
            .map_err(|r| anyhow::anyhow!("{r}"))?;
        state
            .bind_checkout(&workspace, checkout.to_path_buf())
            .map_err(|r| anyhow::anyhow!("{r}"))?;
    }
    let port = std::env::var("FRAME_CODE_PORT")
        .ok()
        .and_then(|port| port.parse::<u16>().ok())
        .unwrap_or(0);
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))?;
    listener.set_nonblocking(true)?;
    let origin = format!("http://{}", listener.local_addr()?);
    std::fs::write(
        data.join("origin"),
        format!("{origin}\nworkspace={workspace}\ntenant=local\n"),
    )?;
    let served = state.clone();
    runtime.spawn(async move {
        match tokio::net::TcpListener::from_std(listener) {
            Ok(listener) => {
                if let Err(error) = theorem_ide_host::http::serve_on(listener, served).await {
                    log::error!("code host: {error}");
                }
            }
            Err(error) => log::error!("code host listener: {error}"),
        }
    });
    Ok(Link {
        connection: Connection::direct(
            state.clone(),
            &workspace,
            "platinum",
            runtime.handle().clone(),
        ),
        host: Some(state),
        served: Some(origin),
        label: checkout.display().to_string(),
    })
}

/// A host elsewhere, named by the `FRAME_CODE_REMOTE_*` variables.
pub fn remote_from_env() -> Option<anyhow::Result<Link>> {
    let origin = std::env::var("FRAME_CODE_REMOTE_URL").ok()?;
    Some((|| {
        use anyhow::Context as _;
        let tenant =
            std::env::var("FRAME_CODE_REMOTE_TENANT").context("FRAME_CODE_REMOTE_TENANT")?;
        let file = std::env::var("FRAME_CODE_REMOTE_TOKEN_FILE")
            .context("FRAME_CODE_REMOTE_TOKEN_FILE")?;
        let token = std::fs::read_to_string(&file).with_context(|| format!("reading {file}"))?;
        let workspace = std::env::var("FRAME_CODE_WORKSPACE").unwrap_or_else(|_| "local".into());
        Ok(Link {
            connection: Connection::remote(
                reqwest::Client::new(),
                &origin,
                &tenant,
                token.trim(),
                &workspace,
                "platinum",
                runtime()?.handle().clone(),
            ),
            host: None,
            served: None,
            label: origin,
        })
    })())
}

/// The loopback credential: 32 random bytes as hex, kept in a file only its
/// owner reads, made once and reused so a web client's setting survives a
/// relaunch. Never printed.
fn credential(path: &Path) -> anyhow::Result<String> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    if let Ok(token) = std::fs::read_to_string(path)
        && token.trim().len() >= 64
    {
        return Ok(token.trim().to_owned());
    }
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| anyhow::anyhow!("no randomness: {error}"))?;
    let token: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(token.as_bytes())?;
    Ok(token)
}

/// A stable workspace id for a folder: its name, and a digest of its path so
/// two folders of one name stay apart.
fn workspace_id(checkout: &Path) -> String {
    let name: String = checkout
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(48)
        .collect();
    let digest: String = theorem_ide_runtime::authority::content_hash(&checkout.to_string_lossy())
        .chars()
        .rev()
        .filter(char::is_ascii_alphanumeric)
        .take(12)
        .collect();
    format!(
        "{}-{digest}",
        if name.is_empty() { "folder" } else { &name }
    )
}

/// The notice when keeping the disk's text would overwrite lines the editor
/// also changed.
pub const UNMERGED: &str = "The disk and the editor changed the same lines, so nothing was merged.";

/// One open document and what the place shows of it.
struct Open {
    generation: u64,
    session: DocumentSession,
    last_edit: Option<Instant>,
    conflict: Option<Value>,
    /// The revision the editor's diagnostics were computed for.
    diagnostics_at: Option<u64>,
    asked_diagnostics_at: Option<u64>,
    plan: Option<Plan>,
}

struct Plan {
    id: String,
    revision: u64,
    targets: usize,
}

pub struct Code {
    focus_handle: FocusHandle,
    link: Option<Link>,
    files: Entity<ListState<Files>>,
    editor: Entity<EditorState>,
    pattern: Entity<InputState>,
    replacement: Entity<InputState>,
    command: Entity<InputState>,
    open: Option<Open>,
    generation: u64,
    /// Set while the place writes into the editor, so its Change is not
    /// taken for typing.
    applying: bool,
    terminal: Option<TerminalAttachment>,
    notice: Option<SharedString>,
    /// Refusals and outcomes, newest last, for the status line and checks.
    log: Vec<Value>,
    live: Option<Task<()>>,
    /// The last live event's cursor, to continue from after a dropped stream.
    cursor: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl Code {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| EditorState::new(window, cx).line_number(true));
        let pattern = cx.new(|cx| InputState::new(window, cx).default_value("old($a$)"));
        let replacement = cx.new(|cx| InputState::new(window, cx).default_value("new($a$)"));
        let command =
            cx.new(|cx| InputState::new(window, cx).placeholder("Type a command, Return sends it"));
        let weak = cx.entity().downgrade();
        let files = cx.new(|cx| ListState::new(Files::new(weak), window, cx).searchable(true));
        let subscriptions = vec![
            cx.subscribe_in(
                &editor,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.typed(window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &command,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.send_command(window, cx);
                    }
                },
            ),
        ];
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            link: None,
            files,
            editor,
            pattern,
            replacement,
            command,
            open: None,
            generation: 0,
            applying: false,
            terminal: None,
            notice: None,
            log: Vec::new(),
            live: None,
            cursor: None,
            _subscriptions: subscriptions,
        };
        match remote_from_env() {
            Some(Ok(link)) => this.attach(link, window, cx),
            Some(Err(error)) => this.notice = Some(format!("{error:#}").into()),
            None => {
                if let Some(folder) = std::env::var_os("FRAME_CODE_CHECKOUT") {
                    this.open_folder(PathBuf::from(folder), window, cx);
                }
            }
        }
        this
    }

    pub fn link(&self) -> Option<&Link> {
        self.link.as_ref()
    }

    pub fn editor(&self) -> &Entity<EditorState> {
        &self.editor
    }

    /// The open document's resource, revision and session text.
    pub fn document(&self) -> Option<(String, u64, String, bool)> {
        self.open.as_ref().map(|open| {
            (
                open.session.resource_id().to_owned(),
                open.session.revision(),
                open.session.text().to_owned(),
                open.session.pending(),
            )
        })
    }

    pub fn conflict(&self) -> Option<&Value> {
        self.open.as_ref()?.conflict.as_ref()
    }

    pub fn diagnostics_at(&self) -> Option<u64> {
        self.open.as_ref()?.diagnostics_at
    }

    pub fn plan_revision(&self) -> Option<u64> {
        Some(self.open.as_ref()?.plan.as_ref()?.revision)
    }

    pub fn terminal_text(&self) -> Option<String> {
        self.terminal.as_ref().map(TerminalAttachment::contents)
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn log(&self) -> &[Value] {
        &self.log
    }

    pub fn file_count(&self, cx: &App) -> usize {
        self.files.read(cx).delegate().paths.len()
    }

    fn choose_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(folder) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |this, window, cx| this.open_folder(folder, window, cx));
            }
        })
        .detach();
    }

    /// Serve `folder` from an embedded host and open it.
    pub fn open_folder(&mut self, folder: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.notice = Some(format!("Opening {}…", folder.display()).into());
        cx.notify();
        let embedding = cx
            .background_executor()
            .spawn(async move { embed(&folder, &host_dir()?) });
        cx.spawn_in(window, async move |this, cx| {
            let link = embedding.await;
            let _ = this.update_in(cx, |this, window, cx| match link {
                Ok(link) => this.attach(link, window, cx),
                Err(error) => {
                    this.notice = Some(format!("{error:#}").into());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Open the link's workspace, follow its live events and list its files.
    pub(crate) fn attach(&mut self, link: Link, window: &mut Window, cx: &mut Context<Self>) {
        self.close_document(cx);
        self.terminal = None;
        let connection = link.connection.clone();
        self.notice = None;
        self.link = Some(link);
        self.cursor = None;
        cx.spawn_in(window, async move |this, cx| {
            match connection.open().await {
                Ok(_) => {
                    // Follow before any document opens, so none of its
                    // revisions falls between its head and the stream.
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.follow(window, cx);
                        this.list_files(window, cx);
                    });
                }
                Err(error) => {
                    let _ = this.update(cx, |this, cx| this.refused("workspace.open", &error, cx));
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// Follow the workspace's live events from the last one seen.
    fn follow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(connection) = self.connection() else {
            return;
        };
        let mut live = connection.live(self.cursor.clone());
        self.live = Some(cx.spawn_in(window, async move |this, cx| {
            while let Some(event) = live.recv().await {
                if this
                    .update_in(cx, |this, window, cx| this.on_live(event, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    /// Drop the live stream, as a lost connection does; the document and its
    /// undo history stay.
    #[cfg(feature = "snapshot")]
    pub(crate) fn disconnect(&mut self) {
        self.live = None;
    }

    /// Follow again from the last event seen, catching up on what was missed.
    #[cfg(feature = "snapshot")]
    pub(crate) fn reconnect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.follow(window, cx);
    }

    fn list_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(connection) = self.connection() else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let listed = connection.call("workspace.files.list", json!({})).await;
            let _ = this.update_in(cx, |this, window, cx| match listed {
                Ok(listed) => {
                    let mut paths: Vec<SharedString> = listed["files"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|file| {
                            file["path"]
                                .as_str()
                                .map(|p| SharedString::from(p.to_owned()))
                        })
                        .collect();
                    paths.sort();
                    this.files.update(cx, |list, cx| {
                        list.delegate_mut().paths = paths;
                        cx.notify();
                    });
                    let _ = window;
                    cx.notify();
                }
                Err(error) => this.refused("workspace.files.list", &error, cx),
            });
        })
        .detach();
    }

    fn connection(&self) -> Option<Connection> {
        self.link.as_ref().map(|link| link.connection.clone())
    }

    fn close_document(&mut self, cx: &mut Context<Self>) {
        self.open = None;
        self.generation += 1;
        cx.notify();
    }

    /// Open `path` in the editor as a shared document.
    pub fn open_file(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(connection) = self.connection() else {
            return;
        };
        self.close_document(cx);
        let generation = self.generation;
        let (operation, arguments) = DocumentSession::open_call(path, None);
        let path = path.to_owned();
        cx.spawn_in(window, async move |this, cx| {
            let opened = connection.call(operation, arguments).await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.generation != generation {
                    return;
                }
                let session = opened
                    .as_ref()
                    .ok()
                    .and_then(|head| DocumentSession::from_head(&path, head, Some("platinum")));
                match (opened, session) {
                    (_, Some(session)) => {
                        let text = session.text().to_owned();
                        this.open = Some(Open {
                            generation,
                            session,
                            last_edit: None,
                            conflict: None,
                            diagnostics_at: None,
                            asked_diagnostics_at: None,
                            plan: None,
                        });
                        this.applying = true;
                        this.editor
                            .update(cx, |editor, cx| editor.set_value(text, window, cx));
                        this.applying = false;
                        this.editor
                            .update(cx, |editor, cx| editor.focus(window, cx));
                        this.ask_diagnostics(window, cx);
                    }
                    (Err(error), None) => this.refused("documents.open", &error, cx),
                    (Ok(head), None) => {
                        this.notice = Some(format!("Unreadable document head: {head}").into());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The editor's text changed: typing, a paste, an IME commit. Text still
    /// being composed is not an edit; its commit is.
    fn typed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.applying {
            return;
        }
        let composing = self
            .editor
            .update(cx, |editor, cx| editor.marked_text_range(window, cx))
            .is_some();
        if composing {
            return;
        }
        let value = self.editor.read(cx).value();
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let now = Instant::now();
        let merge = open
            .last_edit
            .is_some_and(|at| now.duration_since(at) < MERGE_WINDOW);
        open.last_edit = Some(now);
        let outputs = open.session.edit_to(&value, merge);
        self.handle(outputs, window, cx);
    }

    fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(open) = self.open.as_mut() {
            open.last_edit = None;
            let outputs = open.session.undo();
            self.handle(outputs, window, cx);
        }
    }

    fn redo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(open) = self.open.as_mut() {
            open.last_edit = None;
            let outputs = open.session.redo();
            self.handle(outputs, window, cx);
        }
    }

    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(open) = self.open.as_mut() {
            let outputs = open.session.save();
            self.handle(outputs, window, cx);
        }
    }

    pub fn resolve(&mut self, keep: Keep, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(open) = self.open.as_mut() {
            open.conflict = None;
            let outputs = open.session.resolve_conflict(keep);
            self.handle(outputs, window, cx);
        }
    }

    /// Carry out what the session asked for.
    fn handle(&mut self, outputs: Vec<Output>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(generation) = self.open.as_ref().map(|open| open.generation) else {
            return;
        };
        for output in outputs {
            match output {
                Output::Call(call) => {
                    let Some(connection) = self.connection() else {
                        continue;
                    };
                    cx.spawn_in(window, async move |this, cx| {
                        let reply = connection.perform(call).await;
                        let _ = this.update_in(cx, |this, window, cx| {
                            this.completed(generation, reply, window, cx)
                        });
                    })
                    .detach();
                }
                Output::Apply(changes) => self.apply(&changes, window, cx),
                Output::Conflict(conflict) => {
                    if let Some(open) = self.open.as_mut() {
                        open.conflict = Some(conflict);
                    }
                }
                Output::Unreconciled(mut conflict) => {
                    // The host merges the disk change from the saved base and
                    // refuses lines both sides changed: keep the bar up.
                    self.notice = Some(UNMERGED.into());
                    if let Some(open) = self.open.as_mut() {
                        conflict["resource_id"] = json!(open.session.resource_id());
                        open.conflict = Some(conflict);
                    }
                }
                Output::Dropped(error) => {
                    self.refused("documents.edit", &error, cx);
                    self.notice =
                        Some(format!("Your unsent edits were refused and dropped: {error}").into());
                }
                Output::Failed { action, error } => self.refused(action, &error, cx),
                Output::Retired => {
                    self.notice = Some("The file was deleted.".into());
                    self.close_document(cx);
                }
            }
        }
        self.ask_diagnostics(window, cx);
        cx.notify();
    }

    fn completed(
        &mut self,
        generation: u64,
        reply: Reply<Purpose>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .open
            .as_ref()
            .is_none_or(|open| open.generation != generation)
        {
            return;
        }
        if let (Purpose::Save { revision }, Ok(_)) = (&reply.purpose, &reply.result) {
            self.log
                .push(json!({"event": "saved", "revision": revision}));
        }
        if let Err(error) = &reply.result {
            self.log.push(json!({"event": "refused", "purpose": format!("{:?}", reply.purpose), "error": error.to_string(), "code": error.code()}));
        }
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let outputs = open.session.complete(reply);
        self.handle(outputs, window, cx);
    }

    /// Apply changes from the session to the editor, keeping the reader's
    /// selection where its text went.
    fn apply(&mut self, changes: &[TextChange], window: &mut Window, cx: &mut Context<Self>) {
        let Some(expected) = self
            .open
            .as_ref()
            .map(|open| open.session.text().to_owned())
        else {
            return;
        };
        self.applying = true;
        self.editor.update(cx, |editor, cx| {
            let selection = editor.selected_range();
            for change in changes.iter().rev() {
                editor.set_selected_range(change.start..change.end, cx);
                editor.replace(change.insert.clone(), window, cx);
            }
            if editor.value().as_ref() != expected.as_str() {
                // Never leave the editor showing other text than the session.
                editor.set_value(expected, window, cx);
            }
            let mapped = map_offset(selection.start, changes)..map_offset(selection.end, changes);
            editor.set_selected_range(mapped, cx);
        });
        self.applying = false;
    }

    /// Ask for diagnostics at the head, once the session holds no unsent
    /// edits; they are shown only if they still describe the editor's text.
    fn ask_diagnostics(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(connection) = self.connection() else {
            return;
        };
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let revision = open.session.revision();
        if open.session.pending() || open.asked_diagnostics_at == Some(revision) {
            return;
        }
        open.asked_diagnostics_at = Some(revision);
        let generation = open.generation;
        let arguments = json!({"resource_id": open.session.resource_id(), "revision": revision});
        cx.spawn_in(window, async move |this, cx| {
            let found = connection.call("diagnostics.subscribe", arguments).await;
            let _ = this.update_in(cx, |this, _, cx| {
                this.show_diagnostics(generation, revision, found, cx)
            });
        })
        .detach();
    }

    fn show_diagnostics(
        &mut self,
        generation: u64,
        revision: u64,
        found: Result<Value, CallError>,
        cx: &mut Context<Self>,
    ) {
        let rows = match found {
            Ok(found) => found["diagnostics"].as_array().cloned().unwrap_or_default(),
            Err(error) => return self.refused("diagnostics.subscribe", &error, cx),
        };
        let Some(open) = self
            .open
            .as_mut()
            .filter(|open| open.generation == generation)
        else {
            return;
        };
        if open.session.revision() != revision || open.session.pending() {
            return;
        }
        open.diagnostics_at = Some(revision);
        self.log
            .push(json!({"event": "diagnostics", "revision": revision, "rows": rows}));
        self.editor.update(cx, |editor, cx| {
            let text = editor.text().clone();
            if let Some(set) = editor.diagnostics_mut() {
                set.reset(&text);
                for row in &rows {
                    let start = row["start"].as_u64().unwrap_or(0) as usize;
                    let end = row["end"].as_u64().unwrap_or(start as u64) as usize;
                    let range = text.offset_to_position(start.min(text.len()))
                        ..text.offset_to_position(end.min(text.len()));
                    set.push(
                        Diagnostic::new(
                            range,
                            row["message"].as_str().unwrap_or_default().to_owned(),
                        )
                        .with_severity(severity(row["severity"].as_str()))
                        .with_code(row["code"].as_str().unwrap_or_default().to_owned())
                        .with_source(row["provider"].as_str().unwrap_or_default().to_owned()),
                    );
                }
            }
            cx.notify();
        });
    }

    pub fn prepare_refactor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(connection), Some(open)) = (self.connection(), self.open.as_ref()) else {
            return;
        };
        let revision = open.session.revision();
        let generation = open.generation;
        let arguments = json!({
            "resource_id": open.session.resource_id(),
            "revision": revision,
            "operation": "structural_replace",
            "pattern": self.pattern.read(cx).value().to_string(),
            "replacement": self.replacement.read(cx).value().to_string(),
        });
        cx.spawn_in(window, async move |this, cx| {
            let prepared = connection.call("refactor.prepare", arguments).await;
            let _ = this.update(cx, |this, cx| {
                match prepared {
                    Ok(plan) => {
                        this.log.push(json!({"event": "refactor.prepared", "revision": revision, "plan": plan}));
                        if let Some(open) = this.open.as_mut().filter(|open| open.generation == generation) {
                            open.plan = plan["plan_id"].as_str().map(|id| Plan {
                                id: id.to_owned(),
                                revision,
                                targets: plan["targets"].as_array().map_or(0, Vec::len),
                            });
                        }
                    }
                    Err(error) => this.refused("refactor.prepare", &error, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn apply_refactor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(connection), Some(plan)) = (
            self.connection(),
            self.open.as_ref().and_then(|open| open.plan.as_ref()),
        ) else {
            return;
        };
        let arguments = json!({"plan_id": plan.id});
        let revision = plan.revision;
        cx.spawn_in(window, async move |this, cx| {
            let applied = connection.call("refactor.apply", arguments).await;
            let _ = this.update(cx, |this, cx| {
                match applied {
                    Ok(applied) => {
                        this.log.push(json!({"event": "refactor.applied", "from_revision": revision, "result": applied}));
                        this.notice = Some(format!("Refactor applied from revision {revision}.").into());
                        if let Some(open) = this.open.as_mut() {
                            open.plan = None;
                        }
                    }
                    Err(error) => this.refused("refactor.apply", &error, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn refused(&mut self, action: &str, error: &CallError, cx: &mut Context<Self>) {
        self.log.push(json!({"event": "refused", "action": action, "code": error.code(), "error": error.to_string()}));
        self.notice = Some(match error {
            CallError::Refused(refusal) if refusal.code == "stale_revision" => format!(
                "{action} refused: the document moved from revision {} to {}.",
                refusal.detail["base_revision"], refusal.detail["current_revision"]
            )
            .into(),
            _ => format!("{action}: {error}").into(),
        });
        cx.notify();
    }

    fn on_live(&mut self, event: Live, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            Live::Open => {
                if self.notice.as_deref() == Some("Reconnecting…") {
                    self.notice = None;
                }
            }
            Live::Reconnecting => self.notice = Some("Reconnecting…".into()),
            Live::Ended(refusal) => {
                self.notice = Some(match refusal {
                    Some(refusal) => {
                        format!("The workspace refused its live stream: {refusal}").into()
                    }
                    None => "The workspace closed.".into(),
                });
            }
            Live::Resync(_) => {
                if let Some(open) = self.open.as_mut() {
                    let outputs = open.session.resync();
                    self.handle(outputs, window, cx);
                }
                if let Some(call) = self.terminal.as_mut().and_then(TerminalAttachment::attach) {
                    self.terminal_call(call, window, cx);
                }
            }
            Live::Event(frame) => {
                self.cursor = Some(frame.cursor.clone());
                if frame.kind == "terminal.opened"
                    && self.terminal.is_none()
                    && let Some(session) = frame.data["session"].as_str()
                {
                    self.attach_terminal(session, window, cx);
                }
                if let Some(terminal) = self.terminal.as_mut() {
                    let (_, call) = terminal.on_live(&frame);
                    if let Some(call) = call {
                        self.terminal_call(call, window, cx);
                    }
                }
                if let Some(open) = self.open.as_mut() {
                    let outputs = open.session.on_live(&frame);
                    self.handle(outputs, window, cx);
                }
                if frame.kind.starts_with("workspace.file") {
                    self.list_files(window, cx);
                }
            }
        }
        cx.notify();
    }

    pub fn open_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(connection) = self.connection() else {
            return;
        };
        let (operation, arguments) = TerminalAttachment::open_call(TERMINAL_COLS, TERMINAL_ROWS);
        cx.spawn_in(window, async move |this, cx| {
            let opened = connection.call(operation, arguments).await;
            let _ = this.update_in(cx, |this, window, cx| match opened {
                Ok(opened) => {
                    if let Some(session) = opened["session"].as_str() {
                        this.attach_terminal(session, window, cx);
                    }
                }
                Err(error) => this.refused("terminal.open", &error, cx),
            });
        })
        .detach();
    }

    fn attach_terminal(&mut self, session: &str, window: &mut Window, cx: &mut Context<Self>) {
        let mut terminal = TerminalAttachment::new(session, TERMINAL_COLS, TERMINAL_ROWS);
        let call = terminal.attach();
        self.terminal = Some(terminal);
        self.log
            .push(json!({"event": "terminal.attached", "session": session}));
        if let Some(call) = call {
            self.terminal_call(call, window, cx);
        }
    }

    fn terminal_call(
        &mut self,
        call: theorem_ide_host::client::Call<TerminalPurpose>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(connection) = self.connection() else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let reply = connection.perform(call).await;
            let _ = this.update(cx, |this, cx| {
                if let Some(terminal) = this.terminal.as_mut()
                    && let Err(error) = terminal.complete(&reply)
                {
                    this.refused("terminal", &error, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn send_terminal(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(call) = self.terminal.as_ref().map(|terminal| terminal.input(text)) {
            self.terminal_call(call, window, cx);
        }
    }

    fn send_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let line = self.command.read(cx).value().to_string();
        self.command
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.send_terminal(&format!("{line}\n"), window, cx);
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let open = self.open.as_ref();
        let status: SharedString = match (self.link.as_ref(), open) {
            (None, _) => "No workspace".into(),
            (Some(link), None) => {
                format!("{} · {}", link.label, link.connection.transport()).into()
            }
            (Some(link), Some(open)) => format!(
                "{} · revision {}{}{} · {}",
                open.session.resource_id(),
                open.session.revision(),
                if open.session.pending() {
                    " · sending"
                } else {
                    ""
                },
                if open.session.dirty() {
                    " · unsaved"
                } else {
                    ""
                },
                link.connection.transport(),
            )
            .into(),
        };
        h_flex()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("code-open-folder")
                    .debug_selector(|| "code-open-folder".into())
                    .small()
                    .ghost()
                    .icon(Lucide::FolderOpen)
                    .label("Open Folder")
                    .accessibility_label("Open a folder")
                    .on_click(cx.listener(|this, _, window, cx| this.choose_folder(window, cx))),
            )
            .child(
                Button::new("code-save")
                    .debug_selector(|| "code-save".into())
                    .small()
                    .ghost()
                    .icon(Lucide::Save)
                    .label("Save")
                    .disabled(open.is_none_or(|open| !open.session.dirty()))
                    .accessibility_label("Save the file")
                    .tooltip_with_action("Save", &Save, Some(CONTEXT))
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
            )
            .child(
                Button::new("code-terminal")
                    .debug_selector(|| "code-terminal".into())
                    .small()
                    .ghost()
                    .icon(Lucide::SquareTerminal)
                    .label("Terminal")
                    .disabled(self.link.is_none() || self.terminal.is_some())
                    .accessibility_label("Open a terminal in the workspace")
                    .on_click(cx.listener(|this, _, window, cx| this.open_terminal(window, cx))),
            )
            .child(
                div()
                    .id("code-status")
                    .debug_selector(|| "code-status".into())
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(status),
            )
            .when_some(
                self.link.as_ref().and_then(|link| link.served.clone()),
                |row, origin| {
                    row.child(
                        div()
                            .id("code-served")
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("Web clients: {origin}")),
                    )
                },
            )
    }

    fn render_refactor(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let plan = self.open.as_ref().and_then(|open| open.plan.as_ref());
        h_flex()
            .gap_2()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                div().w(px(220.)).child(
                    Input::new(&self.pattern)
                        .small()
                        .aria_label("Refactor pattern"),
                ),
            )
            .child(
                div().w(px(220.)).child(
                    Input::new(&self.replacement)
                        .small()
                        .aria_label("Refactor replacement"),
                ),
            )
            .child(
                Button::new("code-refactor-prepare")
                    .debug_selector(|| "code-refactor-prepare".into())
                    .small()
                    .outline()
                    .icon(Lucide::WandSparkles)
                    .label("Prepare")
                    .disabled(self.open.is_none())
                    .accessibility_label("Prepare the replacement at this revision")
                    .on_click(cx.listener(|this, _, window, cx| this.prepare_refactor(window, cx))),
            )
            .child(
                Button::new("code-refactor-apply")
                    .debug_selector(|| "code-refactor-apply".into())
                    .small()
                    .outline()
                    .label("Apply")
                    .disabled(plan.is_none())
                    .accessibility_label("Apply the prepared replacement")
                    .on_click(cx.listener(|this, _, window, cx| this.apply_refactor(window, cx))),
            )
            .when_some(plan, |row, plan| {
                row.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!(
                            "Prepared at revision {}, {} target(s)",
                            plan.revision, plan.targets
                        )),
                )
            })
    }

    fn render_terminal(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let terminal = self.terminal.as_ref()?;
        let screen = terminal.contents();
        Some(
            v_flex()
                .h(px(220.))
                .border_t_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .id("code-terminal-screen")
                        .debug_selector(|| "code-terminal-screen".into())
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .px_3()
                        .py_2()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_xs()
                        .whitespace_nowrap()
                        .children(
                            screen
                                .lines()
                                .map(|line| div().child(line.to_owned()))
                                .collect::<Vec<_>>(),
                        ),
                )
                .child(
                    div().px_3().py_1().child(
                        Input::new(&self.command)
                            .small()
                            .aria_label(format!("Terminal {} input", terminal.session())),
                    ),
                ),
        )
    }
}

/// Where a byte offset lands once `changes` (ascending, in the old text's
/// offsets) are applied. An insertion exactly at the offset lands after it;
/// an offset inside removed text moves to the end of that change's insert.
fn map_offset(offset: usize, changes: &[TextChange]) -> usize {
    let mut shift: isize = 0;
    for change in changes {
        if change.end <= offset && change.start < offset || change.end < offset {
            shift += change.insert.len() as isize - (change.end - change.start) as isize;
        } else if change.start < offset {
            return (change.start as isize + shift) as usize + change.insert.len();
        } else {
            break;
        }
    }
    (offset as isize + shift).max(0) as usize
}

fn severity(name: Option<&str>) -> DiagnosticSeverity {
    match name {
        Some("error") => DiagnosticSeverity::Error,
        Some("warning") => DiagnosticSeverity::Warning,
        Some("info") => DiagnosticSeverity::Info,
        _ => DiagnosticSeverity::Hint,
    }
}

impl Focusable for Code {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Code {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let conflict = self.conflict().cloned();
        let has_document = self.open.is_some();
        v_flex()
            .id("code-place")
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .size_full()
            .text_color(cx.theme().foreground)
            // Undo and redo are this client's own, not the editor's.
            .capture_action(cx.listener(|this, _: &Undo, window, cx| {
                if this.open.is_some() {
                    cx.stop_propagation();
                    this.undo(window, cx);
                }
            }))
            .capture_action(cx.listener(|this, _: &Redo, window, cx| {
                if this.open.is_some() {
                    cx.stop_propagation();
                    this.redo(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &Save, window, cx| this.save(window, cx)))
            .on_action(cx.listener(|this, _: &PrepareRefactor, window, cx| {
                this.prepare_refactor(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ApplyRefactor, window, cx| this.apply_refactor(window, cx)),
            )
            .child(self.render_toolbar(cx))
            .when_some(self.notice.clone(), |this, notice| {
                this.child(
                    div()
                        .px_3()
                        .pt_2()
                        .child(Alert::info("code-notice", notice).small()),
                )
            })
            .when_some(conflict, |this, conflict| {
                let path = conflict["resource_id"]
                    .as_str()
                    .unwrap_or("This file")
                    .to_owned();
                this.child(
                    h_flex()
                        .id("code-conflict")
                        .debug_selector(|| "code-conflict".into())
                        .gap_2()
                        .px_3()
                        .pt_2()
                        .child(
                            div().flex_1().child(
                                Alert::warning(
                                    "code-conflict-alert",
                                    format!("{path} changed on disk while you had unsaved edits."),
                                )
                                .small(),
                            ),
                        )
                        .child(
                            Button::new("code-keep-disk")
                                .debug_selector(|| "code-keep-disk".into())
                                .small()
                                .outline()
                                .label("Keep Disk")
                                .accessibility_label("Keep the version on disk")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.resolve(Keep::Disk, window, cx)
                                })),
                        )
                        .child(
                            Button::new("code-keep-editor")
                                .debug_selector(|| "code-keep-editor".into())
                                .small()
                                .primary()
                                .label("Keep Editor")
                                .accessibility_label("Keep the editor's version")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.resolve(Keep::Authority, window, cx)
                                })),
                        ),
                )
            })
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(
                        div()
                            .id("code-files")
                            .w(px(240.))
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .child(List::new(&self.files).size_full()),
                    )
                    .child(
                        div().flex_1().min_w_0().p_2().child(
                            Editor::new(&self.editor)
                                .h_full()
                                .readonly(!has_document)
                                .aria_label(match &self.open {
                                    Some(open) => format!("Editor, {}", open.session.resource_id()),
                                    None => "Editor, no file open".into(),
                                })
                                .context_menu(|menu: NativeMenu, _, _| {
                                    menu.menu("Undo", Box::new(Undo))
                                        .menu("Redo", Box::new(Redo))
                                        .separator()
                                        .menu("Cut", Box::new(Cut))
                                        .menu("Copy", Box::new(Copy))
                                        .menu("Paste", Box::new(Paste))
                                        .menu("Select All", Box::new(SelectAll))
                                        .separator()
                                        .menu("Save", Box::new(Save))
                                        .menu("Prepare Refactor", Box::new(PrepareRefactor))
                                        .menu("Apply Refactor", Box::new(ApplyRefactor))
                                }),
                        ),
                    ),
            )
            .child(self.render_refactor(cx))
            .children(self.render_terminal(cx))
    }
}

/// The workspace's files, as GPUI Kit's searchable list; confirming one opens
/// it.
pub struct Files {
    code: WeakEntity<Code>,
    paths: Vec<SharedString>,
    selected: Option<usize>,
}

impl Files {
    fn new(code: WeakEntity<Code>) -> Self {
        Self {
            code,
            paths: Vec::new(),
            selected: None,
        }
    }
}

impl ListDelegate for Files {
    type Item = ListItem;

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.paths.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let path = self.paths.get(ix.row)?.clone();
        let code = self.code.clone();
        let opened = path.clone();
        Some(
            ListItem::new(SharedString::from(format!("code-file:{path}")))
                .selected(self.selected == Some(ix.row))
                .accessibility_label(path.clone())
                .px_3()
                .py_1()
                .on_click(move |_, window, cx| {
                    let _ = code.update(cx, |code, cx| code.open_file(&opened, window, cx));
                })
                .child(div().text_sm().truncate().child(path)),
        )
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.selected = ix.map(|ix| ix.row);
        cx.notify();
    }

    fn confirm(
        &mut self,
        _secondary: bool,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        let Some(path) = self.selected.and_then(|row| self.paths.get(row)).cloned() else {
            return;
        };
        let _ = self
            .code
            .update(cx, |code, cx| code.open_file(&path, window, cx));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(start: usize, end: usize, insert: &str) -> TextChange {
        TextChange {
            start,
            end,
            insert: insert.into(),
        }
    }

    #[test]
    fn a_selection_follows_its_text_through_another_writers_changes() {
        let changes = [change(0, 0, "ab"), change(4, 6, "")];
        assert_eq!(
            map_offset(0, &changes),
            0,
            "another writer's insertion at the caret lands after it"
        );
        assert_eq!(map_offset(3, &changes), 5);
        assert_eq!(
            map_offset(5, &changes),
            6,
            "inside removed text lands at the removal"
        );
        assert_eq!(map_offset(8, &changes), 8);
    }
}
