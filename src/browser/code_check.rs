//! Live acceptance of the Code place (ac-22) in the production window, against
//! the host it embeds, with a second client attached over the host's
//! authenticated loopback routes the way the web client attaches. Enabled only
//! by `snapshot`.
//!
//! `FRAME_CODE_CHECK=<output>`. The check makes a fresh checkout and host
//! state under `<output>`, opens the checkout in the Code place, and drives
//! the place with keystrokes dispatched to its window, an IME composition
//! through the editor's input handler, and its own actions. The second client
//! (`web` below) is the host's Rust client over `Connection::remote`, or with
//! `FRAME_CODE_PEER=ide-cm` the ide-cm web client itself, served by its dev
//! server and driven in headless Chrome (`ide_cm`); either reads the loopback
//! credential from the host's file and never prints it. With
//! `FRAME_CODE_REMOTE_URL` the check instead runs the remote clause against a
//! separately started host (`Check::remote`). Every
//! step lands in `report.json` with a screenshot, whether it passed or not;
//! the process exits nonzero if any step failed.
use super::workspace_check::capture;
use super::*;
use crate::ide::{Code, Link, embed};

#[cfg(target_os = "macos")]
mod ax;
mod ide_cm;
use gpui_kit::component::Root;
use gpui_kit::{
    AsyncApp, EntityInputHandler as _, Keystroke, Modifiers, WindowBounds, WindowOptions,
};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use theorem_ide_host::client::document::{DocumentSession, Keep, Output};
use theorem_ide_host::client::terminal::TerminalAttachment;
use theorem_ide_host::client::{Connection, Live};
use tokio::sync::mpsc::UnboundedReceiver;

const SOURCE: &str = "fn main() { old(1); }\nfn later() {  }\n";
const LIMIT: Duration = Duration::from_secs(30);

pub fn run(output: PathBuf) -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    // The check waits on tenth-of-a-second timers while its window is often
    // behind others; App Nap would coalesce them into minutes.
    #[cfg(target_os = "macos")]
    let _awake = {
        use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
        NSProcessInfo::processInfo().beginActivityWithOptions_reason(
            NSActivityOptions::UserInitiatedAllowingIdleSystemSleep,
            &NSString::from_str("Code check"),
        )
    };
    // With `FRAME_CODE_REMOTE_URL` the window opens the Code place itself and
    // attaches to that separately started host; `FRAME_CODE_REMOTE_CHECKOUT`
    // is the checkout it serves, written here before the window opens.
    let remote = std::env::var_os("FRAME_CODE_REMOTE_URL").is_some();
    let checkout = match std::env::var_os("FRAME_CODE_REMOTE_CHECKOUT") {
        Some(path) if remote => PathBuf::from(path),
        _ => output.join("checkout"),
    };
    let host = output.join("host");
    for dir in [&checkout, &host] {
        let _ = std::fs::remove_dir_all(dir);
    }
    std::fs::create_dir_all(checkout.join("src"))?;
    std::fs::write(checkout.join("src/lib.rs"), SOURCE)?;
    std::fs::write(checkout.join("README.md"), "A fixture checkout.\n")?;
    let profile = output.join("temporary-servo-profile");
    std::fs::create_dir_all(&profile)?;
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::init(cx);
            crate::install_commands(cx);
            Engine::start(&profile, cx);
            recents::init(None, cx);
            let mut view = None;
            let handle = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::centered(size(px(1280.), px(820.)), cx)),
                        ..TitleBar::window_options()
                    },
                    |window, cx| {
                        glass::install(window, cx);
                        let browser = cx.new(|cx| Browser::new(window, cx));
                        view = Some(browser.clone());
                        window.activate_window();
                        cx.new(|cx| {
                            Root::new(browser, window, cx).bg(gpui_kit::transparent_black())
                        })
                    },
                )
                .expect("code check window");
            let handle: gpui_kit::AnyWindowHandle = handle.into();
            let browser = view.unwrap();
            cx.activate(true);
            let cleanup = profile.clone();
            cx.spawn(async move |cx| {
                let mut check = Check {
                    output,
                    checkout,
                    host,
                    handle,
                    steps: Vec::new(),
                    peer: String::new(),
                    accessibility: Value::Null,
                };
                let ran = if remote {
                    check.remote(&browser, cx).await
                } else {
                    check.scenarios(&browser, cx).await
                };
                if let Err(error) = ran {
                    check
                        .step("aborted", false, json!({"error": format!("{error:#}")}), cx)
                        .await;
                }
                let _ = std::fs::remove_dir_all(cleanup);
                if check.steps.iter().any(|step| step["passed"] != true) {
                    eprintln!("a code step failed; see report.json");
                    std::process::exit(1);
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
    Ok(())
}

struct Check {
    output: PathBuf,
    checkout: PathBuf,
    host: PathBuf,
    handle: gpui_kit::AnyWindowHandle,
    steps: Vec<Value>,
    /// The second client, once it attaches.
    peer: String,
    /// What the window's accessibility tree showed (`Check::accessibility`).
    accessibility: Value,
}

/// The second client: the host's own Rust client over its authenticated
/// HTTP routes, the transport and contract the web client uses.
struct Web {
    connection: Connection,
    live: UnboundedReceiver<Live>,
    document: Option<DocumentSession>,
    terminal: Option<TerminalAttachment>,
    conflicts: Vec<Value>,
    refusals: Vec<String>,
}

impl Web {
    async fn connect(link: &Link, host: &Path) -> anyhow::Result<Self> {
        use anyhow::Context as _;
        let origin = link
            .served
            .clone()
            .context("the embedded host serves no origin")?;
        Self::attach(
            &origin,
            "local",
            &host.join("token"),
            link.connection.workspace(),
        )
        .await
    }

    /// Attach over HTTP to `workspace` on the host at `origin`.
    async fn attach(
        origin: &str,
        tenant: &str,
        token_file: &Path,
        workspace: &str,
    ) -> anyhow::Result<Self> {
        use anyhow::Context as _;
        let token = std::fs::read_to_string(token_file).context("reading the host credential")?;
        let connection = Connection::remote(
            reqwest::Client::new(),
            origin,
            tenant,
            token.trim(),
            workspace,
            "web-check",
            crate::ide::runtime()?.handle().clone(),
        );
        connection
            .open()
            .await
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let live = connection.live(None);
        Ok(Self {
            connection,
            live,
            document: None,
            terminal: None,
            conflicts: Vec::new(),
            refusals: Vec::new(),
        })
    }

    async fn open(&mut self, resource: &str) -> anyhow::Result<()> {
        let (operation, arguments) = DocumentSession::open_call(resource, None);
        let head = self
            .connection
            .call(operation, arguments)
            .await
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        self.document = DocumentSession::from_head(resource, &head, Some("web-check"));
        anyhow::ensure!(self.document.is_some(), "unreadable head {head}");
        Ok(())
    }

    fn text(&self) -> String {
        self.document
            .as_ref()
            .map(|d| d.text().to_owned())
            .unwrap_or_default()
    }

    fn revision(&self) -> u64 {
        self.document.as_ref().map_or(0, DocumentSession::revision)
    }

    fn settled(&self) -> bool {
        self.document.as_ref().is_some_and(|d| !d.pending())
    }

    async fn type_text(&mut self, text: &str) {
        let outputs = match self.document.as_mut() {
            Some(document) => document.edit_to(text, false),
            None => Vec::new(),
        };
        self.take(outputs).await;
    }

    async fn call(&self, operation: &str, arguments: Value) -> Result<Value, String> {
        self.connection
            .call(operation, arguments)
            .await
            .map_err(|error| error.to_string())
    }

    /// Carry out the session's calls in order, feeding each reply back.
    async fn take(&mut self, outputs: Vec<Output>) {
        let mut queue: VecDeque<Output> = outputs.into();
        while let Some(output) = queue.pop_front() {
            match output {
                Output::Call(call) => {
                    let reply = self.connection.perform(call).await;
                    if let Err(error) = &reply.result {
                        self.refusals.push(error.to_string());
                    }
                    if let Some(document) = self.document.as_mut() {
                        queue.extend(document.complete(reply));
                    }
                }
                Output::Conflict(conflict) => self.conflicts.push(conflict),
                Output::Dropped(error) | Output::Failed { error, .. } => {
                    self.refusals.push(error.to_string())
                }
                Output::Apply(_) | Output::Retired => {}
            }
        }
    }

    /// Handle what the live stream has delivered.
    async fn pump(&mut self) {
        while let Ok(event) = self.live.try_recv() {
            let outputs = match &event {
                Live::Event(frame) => {
                    if let Some(terminal) = self.terminal.as_mut()
                        && let (_, Some(call)) = terminal.on_live(frame)
                    {
                        let reply = self.connection.perform(call).await;
                        if let Some(terminal) = self.terminal.as_mut() {
                            let _ = terminal.complete(&reply);
                        }
                    }
                    self.document
                        .as_mut()
                        .map(|d| d.on_live(frame))
                        .unwrap_or_default()
                }
                Live::Resync(_) => self
                    .document
                    .as_mut()
                    .map(DocumentSession::resync)
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            self.take(outputs).await;
        }
    }
}

/// The second client of the check: the host's Rust client, or ide-cm's page
/// (`FRAME_CODE_PEER=ide-cm`).
enum Peer {
    Rust(Web),
    Page(ide_cm::Page),
}

impl Peer {
    async fn connect(link: &Link, host: &Path, output: &Path) -> anyhow::Result<Self> {
        use anyhow::Context as _;
        if std::env::var("FRAME_CODE_PEER").as_deref() == Ok("ide-cm") {
            let origin = link
                .served
                .clone()
                .context("the embedded host serves no origin")?;
            let page = ide_cm::Page::start(
                &origin,
                &host.join("token"),
                link.connection.workspace(),
                "src/lib.rs",
                output,
            )
            .await?;
            return Ok(Self::Page(page));
        }
        let mut web = Web::connect(link, host).await?;
        web.open("src/lib.rs").await?;
        Ok(Self::Rust(web))
    }

    fn name(&self) -> String {
        match self {
            Self::Rust(_) => "rust-client".into(),
            Self::Page(page) => format!("ide-cm@{}", page.revision),
        }
    }

    fn transport(&self) -> &'static str {
        match self {
            Self::Rust(web) => web.connection.transport(),
            Self::Page(_) => "ide-cm page through its dev server's /api/environment/v1 stand-in",
        }
    }

    fn text(&self) -> String {
        match self {
            Self::Rust(web) => web.text(),
            Self::Page(page) => page.text(),
        }
    }

    fn revision(&self) -> u64 {
        match self {
            Self::Rust(web) => web.revision(),
            Self::Page(page) => page.revision(),
        }
    }

    fn settled(&self) -> bool {
        match self {
            Self::Rust(web) => web.settled(),
            Self::Page(page) => page.settled(),
        }
    }

    fn dirty(&self) -> bool {
        match self {
            Self::Rust(web) => web.document.as_ref().is_some_and(DocumentSession::dirty),
            Self::Page(page) => page.dirty(),
        }
    }

    async fn pump(&mut self) {
        match self {
            Self::Rust(web) => web.pump().await,
            Self::Page(page) => page.pump().await,
        }
    }

    async fn type_text(&mut self, text: &str) {
        match self {
            Self::Rust(web) => web.type_text(text).await,
            Self::Page(page) => {
                if let Err(error) = page.type_text(text).await {
                    page.failures.push(format!("typing: {error:#}"));
                }
            }
        }
    }

    async fn undo(&mut self) {
        match self {
            Self::Rust(web) => {
                let outputs = web
                    .document
                    .as_mut()
                    .map(DocumentSession::undo)
                    .unwrap_or_default();
                web.take(outputs).await;
            }
            Self::Page(page) => {
                if let Err(error) = page.undo().await {
                    page.failures.push(format!("undo: {error:#}"));
                }
            }
        }
    }

    async fn call(&self, operation: &str, arguments: Value) -> Result<Value, String> {
        match self {
            Self::Rust(web) => web.call(operation, arguments).await,
            Self::Page(page) => page.call(operation, arguments).await,
        }
    }

    /// Open a shell on the host and attach to it.
    async fn open_terminal(&mut self) -> Option<String> {
        match self {
            Self::Rust(web) => {
                let (operation, arguments) = TerminalAttachment::open_call(100, 24);
                let opened = web.call(operation, arguments).await.ok()?;
                let session = opened["session"].as_str()?.to_owned();
                let mut attachment = TerminalAttachment::new(&session, 100, 24);
                if let Some(call) = attachment.attach() {
                    let reply = web.connection.perform(call).await;
                    let _ = attachment.complete(&reply);
                }
                web.terminal = Some(attachment);
                Some(session)
            }
            Self::Page(page) => match page.open_terminal().await {
                Ok(session) => session,
                Err(error) => {
                    page.failures.push(format!("terminal: {error:#}"));
                    None
                }
            },
        }
    }

    async fn terminal_input(&mut self, text: &str) {
        match self {
            Self::Rust(web) => {
                if let Some(call) = web.terminal.as_ref().map(|t| t.input(text)) {
                    let _ = web.connection.perform(call).await;
                }
            }
            Self::Page(page) => {
                if let Err(error) = page.terminal_type(text).await {
                    page.failures.push(format!("terminal input: {error:#}"));
                }
            }
        }
    }

    async fn terminal_text(&self) -> String {
        match self {
            Self::Rust(web) => web
                .terminal
                .as_ref()
                .map(TerminalAttachment::contents)
                .unwrap_or_default(),
            Self::Page(page) => page.terminal_text().await,
        }
    }

    fn conflicts_seen(&self) -> usize {
        match self {
            Self::Rust(web) => web.conflicts.len(),
            Self::Page(page) => page.conflicts_seen(),
        }
    }

    /// The page's conflict banners; the Rust client draws none.
    fn conflict_banners(&self) -> Value {
        match self {
            Self::Rust(_) => Value::Null,
            Self::Page(page) => page.banners(),
        }
    }

    /// What went wrong on the second client's side during the check.
    async fn refusals(&self) -> Vec<String> {
        match self {
            Self::Rust(web) => web.refusals.clone(),
            Self::Page(page) => {
                let mut all = page.failures.clone();
                all.extend(page.errors().await);
                all
            }
        }
    }

    /// The page as the user would see it now, beside the native capture.
    async fn shot(&self, output: &Path, name: &str) {
        if let Self::Page(page) = self {
            page.shot(&output.join(format!("{name}-ide-cm.png"))).await;
        }
    }
}

/// What the native place holds of its open document.
#[derive(Clone, Debug, Default)]
struct Native {
    resource: String,
    revision: u64,
    session: String,
    pending: bool,
    editor: String,
}

impl Native {
    fn settled_on(&self, text: &str) -> bool {
        !self.pending && self.session == text && self.editor == text
    }

    fn json(&self) -> Value {
        json!({
            "resource": self.resource, "revision": self.revision,
            "pending": self.pending, "text": self.session,
            "editor_matches_session": self.editor == self.session,
        })
    }
}

fn native(code: &Entity<Code>, cx: &mut AsyncApp) -> Native {
    code.read_with(cx, |code, cx| {
        let editor = code.editor().read(cx).value().to_string();
        match code.document() {
            Some((resource, revision, session, pending)) => Native {
                resource,
                revision,
                session,
                pending,
                editor,
            },
            None => Native {
                editor,
                ..Native::default()
            },
        }
    })
}

fn log_since(code: &Entity<Code>, from: usize, cx: &mut AsyncApp) -> Vec<Value> {
    code.read_with(cx, |code, _| {
        code.log().get(from..).unwrap_or_default().to_vec()
    })
}

fn log_len(code: &Entity<Code>, cx: &mut AsyncApp) -> usize {
    code.read_with(cx, |code, _| code.log().len())
}

/// A key as the platform delivers it: its name and the text it types.
fn key(text: &str) -> Keystroke {
    Keystroke {
        modifiers: Modifiers::default(),
        key: text.to_owned(),
        key_char: Some(text.to_owned()),
    }
}

impl Check {
    async fn scenarios(
        &mut self,
        browser: &Entity<Browser>,
        cx: &mut AsyncApp,
    ) -> anyhow::Result<()> {
        // 1. The folder opens in the place, served by the embedded host.
        self.handle
            .update(cx, |_, window, cx| {
                browser.update(cx, |browser, cx| {
                    browser.open_place(Place::Code, window, cx)
                })
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let code = browser
            .read_with(cx, |browser, _| browser.code.clone())
            .ok_or_else(|| anyhow::anyhow!("the Code place mounts"))?;
        let (checkout, host) = (self.checkout.clone(), self.host.clone());
        let link = cx
            .background_spawn(async move { embed(&checkout, &host) })
            .await?;
        self.handle
            .update(cx, |_, window, cx| {
                let link = link.clone();
                code.update(cx, |code, cx| code.attach(link, window, cx))
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let listed = self
            .until(cx, None, |cx| {
                code.read_with(cx, |code, cx| code.file_count(cx) >= 2)
            })
            .await;
        let transport = code.read_with(cx, |code, _| code.link().map(|l| l.connection.transport()));
        self.step(
            "open-workspace",
            listed && transport == Some("direct"),
            json!({
                "transport": transport,
                "files": code.read_with(cx, |code, cx| code.file_count(cx)),
                "workspace": link.connection.workspace(),
                "served": link.served.is_some(),
            }),
            cx,
        )
        .await;

        // 2. A file opens in the editor as a shared document.
        self.handle
            .update(cx, |_, window, cx| {
                code.update(cx, |code, cx| code.open_file("src/lib.rs", window, cx))
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let opened = self
            .until(cx, None, |cx| native(&code, cx).settled_on(SOURCE))
            .await;
        // The native session against the checkout file; the host's head is
        // compared at step 3, through the second client.
        let on_disk = std::fs::read_to_string(self.checkout.join("src/lib.rs")).ok();
        let mut opened_detail = native(&code, cx).json();
        opened_detail["checkout_matches"] = json!(on_disk.as_deref() == Some(SOURCE));
        self.step(
            "open-document",
            opened && on_disk.as_deref() == Some(SOURCE),
            opened_detail,
            cx,
        )
        .await;
        self.accessibility(cx).await;

        // 3. The second client attaches over the loopback routes to the same
        //    workspace and document.
        let mut web = Peer::connect(&link, &self.host, &self.output).await?;
        self.peer = web.name();
        let at = native(&code, cx);
        web.shot(&self.output, "second-client-opens").await;
        self.step(
            "second-client-opens",
            web.text() == at.session && web.revision() == at.revision,
            json!({"peer": web.name(), "web_revision": web.revision(), "native": at.json(), "web_transport": web.transport()}),
            cx,
        )
        .await;

        // 4. Native keystrokes reach the host as edits; the other client sees them.
        self.caret_at_end(&code, cx);
        self.type_keys("// native", cx).await;
        let expected = format!("{SOURCE}// native");
        let converged = self
            .until(cx, Some(&mut web), |cx| {
                native(&code, cx).settled_on(&expected)
            })
            .await
            && self.web_on(&mut web, &expected, cx).await;
        web.shot(&self.output, "native-typing").await;
        self.step(
            "native-typing",
            converged,
            json!({"native": native(&code, cx).json(), "web": web.text(), "web_revision": web.revision()}),
            cx,
        )
        .await;

        // 5. The other client's edit arrives in the native editor. The target
        //    is the text the other client's editor holds once it has typed:
        //    an editor may add to typed keys (a newline continues a `//`
        //    comment in ide-cm's CodeMirror), so the prefix avoids a newline.
        let before_theirs = web.text();
        web.type_text(&format!("/* web */ {before_theirs}")).await;
        let web_settled = self.web_settled(&mut web, cx).await;
        let theirs = web.text();
        let typed = theirs.starts_with("/* web */") && theirs.ends_with(before_theirs.as_str());
        let arrived = typed
            && self
                .until(cx, Some(&mut web), |cx| {
                    native(&code, cx).settled_on(&theirs)
                })
                .await;
        web.shot(&self.output, "remote-edit-arrives").await;
        self.step(
            "remote-edit-arrives",
            web_settled && arrived && web.settled() && web.text() == theirs,
            json!({"native": native(&code, cx).json(), "web": theirs, "web_revision": web.revision()}),
            cx,
        )
        .await;

        // 6. IME: composed text stays in the editor; only the commit is an edit.
        self.caret_at_end(&code, cx);
        let before = native(&code, cx);
        self.handle
            .update(cx, |_, window, cx| {
                code.read(cx).editor().clone().update(cx, |editor, cx| {
                    editor.replace_and_mark_text_in_range(None, "ka", None, window, cx)
                })
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        self.pause(cx, 400).await;
        let composing = native(&code, cx);
        let head = web
            .call("documents.head", json!({"resource_id": "src/lib.rs"}))
            .await
            .unwrap_or_default();
        let held_back = composing.editor.ends_with("ka")
            && composing.session == before.session
            && head["text"].as_str() == Some(before.session.as_str());
        self.shot("ime-composing", cx).await;
        self.handle
            .update(cx, |_, window, cx| {
                code.read(cx).editor().clone().update(cx, |editor, cx| {
                    editor.replace_text_in_range(None, "か", window, cx)
                })
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let committed = format!("{}か", before.session);
        let reached = self
            .until(cx, Some(&mut web), |cx| {
                native(&code, cx).settled_on(&committed)
            })
            .await
            && self.web_on(&mut web, &committed, cx).await;
        web.shot(&self.output, "ime-composition").await;
        self.step(
            "ime-composition",
            held_back && reached,
            json!({
                "composing_editor_tail": composing.editor.chars().rev().take(4).collect::<String>().chars().rev().collect::<String>(),
                "composing_sent": composing.session != before.session,
                "host_during_composition_unchanged": head["text"].as_str() == Some(before.session.as_str()),
                "committed": native(&code, cx).json(),
                "path": "EntityInputHandler::replace_and_mark_text_in_range then replace_text_in_range on the editor, the calls the platform input method makes",
            }),
            cx,
        )
        .await;

        // 7. A stale edit: another writer commits between the native client's
        //    view and its keystroke; the host refuses the stale base, the
        //    session catches up, rebases and resends.
        self.caret_at_end(&code, cx);
        let from = log_len(&code, cx);
        let raced = self
            .handle
            .update(cx, |_, window, cx| {
                let (state, workspace) = {
                    let link = code.read(cx).link().expect("linked");
                    (link.host.clone().expect("embedded"), link.connection.workspace().to_owned())
                };
                let revision = code.read(cx).document().map_or(0, |d| d.1);
                let raced = {
                    let _entered = crate::ide::runtime().map(|runtime| runtime.enter());
                    state.call(
                        &workspace,
                        Some("web-check"),
                        "documents.edit",
                        json!({"resource_id": "src/lib.rs", "operation_id": "check-race-1", "base_revision": revision,
                               "edits": [{"start": 0, "end": 0, "insert": "/*w*/"}]}),
                    )
                };
                window.dispatch_keystroke(key("z"), cx);
                raced.map(|_| revision).map_err(|refusal| refusal.to_string())
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let expected = format!("/*w*/{}z", committed);
        let converged = self
            .until(cx, Some(&mut web), |cx| {
                native(&code, cx).settled_on(&expected)
            })
            .await
            && self.web_on(&mut web, &expected, cx).await;
        let log = log_since(&code, from, cx);
        let refused_stale = log.iter().any(|entry| {
            entry["event"] == "refused"
                && matches!(
                    entry["code"].as_str(),
                    Some("stale_revision" | "revision_conflict")
                )
        });
        web.shot(&self.output, "stale-edit").await;
        self.step(
            "stale-edit",
            raced.is_ok() && refused_stale && converged,
            json!({"raced_at_revision": raced, "log": log, "native": native(&code, cx).json()}),
            cx,
        )
        .await;

        // 8. Save from the native window (cmd-s) while the other client is
        //    attached; the checkout holds the text and the other client
        //    learns it is saved.
        // The control: before the save the checkout lacks the text and the
        // other client reports unsaved edits.
        let disk = self.checkout.join("src/lib.rs");
        let disk_before = std::fs::read_to_string(&disk).ok();
        let web_dirty_before = self.until(cx, Some(&mut web), |_| true).await && web.dirty();
        let unsaved_before = disk_before.as_deref() != Some(expected.as_str()) && web_dirty_before;
        let from = log_len(&code, cx);
        self.handle
            .update(cx, |_, window, cx| {
                window.dispatch_keystroke(Keystroke::parse("cmd-s").expect("cmd-s"), cx)
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let saved = self
            .until(cx, Some(&mut web), |_| {
                std::fs::read_to_string(&disk).ok().as_deref() == Some(expected.as_str())
            })
            .await;
        let web_clean =
            self.until(cx, Some(&mut web), |_| true).await && self.web_clean(&mut web, cx).await;
        web.shot(&self.output, "save-concurrent").await;
        self.step(
            "save-concurrent",
            unsaved_before && saved && web_clean,
            json!({
                "before_save": {"checkout_differs": disk_before.as_deref() != Some(expected.as_str()), "web_dirty": web_dirty_before},
                "log": log_since(&code, from, cx),
                "disk_matches": saved,
                "web_sees_saved": web_clean,
            }),
            cx,
        )
        .await;

        // 9. Diagnostics: a syntax error typed natively shows for the revision
        //    it was computed at, and the other client reads the same rows there.
        self.caret_after(&code, "fn later() { ", cx);
        let from = log_len(&code, cx);
        self.type_keys("@@", cx).await;
        let typed = native(&code, cx).session.contains("{ @@ }");
        let shown = self
            .until(cx, Some(&mut web), |cx| {
                let at = native(&code, cx);
                !at.pending
                    && code.read_with(cx, |code, _| code.diagnostics_at()) == Some(at.revision)
                    && at.session.contains("{ @@ }")
            })
            .await;
        let revision = native(&code, cx).revision;
        let native_rows = log_since(&code, from, cx)
            .into_iter()
            .rev()
            .find(|entry| entry["event"] == "diagnostics" && entry["revision"] == revision)
            .map(|entry| entry["rows"].clone())
            .unwrap_or(Value::Null);
        let web_rows = web
            .call(
                "diagnostics.subscribe",
                json!({"resource_id": "src/lib.rs", "revision": revision}),
            )
            .await
            .map(|found| found["diagnostics"].clone())
            .unwrap_or(Value::Null);
        let has_error = native_rows
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["provider"] == "theorem.syntax"));
        web.shot(&self.output, "diagnostics-consistent").await;
        self.step(
            "diagnostics-consistent",
            typed && shown && has_error && native_rows == web_rows,
            json!({"revision": revision, "native_rows": native_rows, "web_rows": web_rows}),
            cx,
        )
        .await;

        // 10. A refactor prepared at a recorded revision is refused once the
        //     document leaves it.
        self.web_on(&mut web, &native(&code, cx).session, cx).await;
        let from = log_len(&code, cx);
        let prepared_at = native(&code, cx).revision;
        self.handle
            .update(cx, |_, window, cx| {
                code.update(cx, |code, cx| code.prepare_refactor(window, cx))
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let prepared = self
            .until(cx, Some(&mut web), |cx| {
                code.read_with(cx, |code, _| code.plan_revision()) == Some(prepared_at)
            })
            .await;
        let moved = format!("{} ", web.text());
        web.type_text(&moved).await;
        let caught_up = self
            .until(cx, Some(&mut web), |cx| {
                native(&code, cx).settled_on(&moved)
            })
            .await;
        self.handle
            .update(cx, |_, window, cx| {
                code.update(cx, |code, cx| code.apply_refactor(window, cx))
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let refused = self
            .until(cx, Some(&mut web), |cx| {
                log_since(&code, from, cx).iter().any(|entry| {
                    entry["action"] == "refactor.apply" && entry["code"] == "stale_revision"
                })
            })
            .await;
        web.shot(&self.output, "refactor-stale-plan-refused").await;
        self.step(
            "refactor-stale-plan-refused",
            prepared && caught_up && refused,
            json!({
                "prepared_at": prepared_at,
                "document_now_at": native(&code, cx).revision,
                "notice": code.read_with(cx, |code, _| code.notice().map(str::to_owned)),
                "log": log_since(&code, from, cx),
            }),
            cx,
        )
        .await;

        // 11. Prepared again at the current revision, it applies.
        let from = log_len(&code, cx);
        let prepared_at = native(&code, cx).revision;
        self.handle
            .update(cx, |_, window, cx| {
                code.update(cx, |code, cx| code.prepare_refactor(window, cx))
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let prepared = self
            .until(cx, Some(&mut web), |cx| {
                code.read_with(cx, |code, _| code.plan_revision()) == Some(prepared_at)
            })
            .await;
        self.handle
            .update(cx, |_, window, cx| {
                code.update(cx, |code, cx| code.apply_refactor(window, cx))
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let applied = self
            .until(cx, Some(&mut web), |cx| {
                let at = native(&code, cx);
                !at.pending && at.session.contains("new(1)") && at.editor == at.session
            })
            .await
            && self.web_on(&mut web, &native(&code, cx).session, cx).await;
        web.shot(&self.output, "refactor-from-recorded-revision")
            .await;
        self.step(
            "refactor-from-recorded-revision",
            prepared && applied,
            json!({
                "prepared_at": prepared_at,
                "notice": code.read_with(cx, |code, _| code.notice().map(str::to_owned)),
                "log": log_since(&code, from, cx),
                "native": native(&code, cx).json(),
            }),
            cx,
        )
        .await;

        // 12. The file is rewritten on disk under unsaved native edits.
        self.disk_conflict("external-disk-modification", &code, &disk, &mut web, cx)
            .await?;

        // 13. Per-client undo after a reconnect: the native stream drops, the
        //     other client writes, the native client follows again from its
        //     last cursor and undoes only its own edit.
        self.web_on(&mut web, &native(&code, cx).session, cx).await;
        self.pause(cx, 1000).await;
        self.caret_at_end(&code, cx);
        let before_mine = native(&code, cx).session.clone();
        self.type_keys(" mine", cx).await;
        let mine = format!("{before_mine} mine");
        self.until(cx, Some(&mut web), |cx| native(&code, cx).settled_on(&mine))
            .await;
        self.web_on(&mut web, &mine, cx).await;
        code.update(cx, |code, _| code.disconnect());
        let theirs = format!("theirs {mine}");
        web.type_text(&theirs).await;
        let web_committed = self.until(cx, Some(&mut web), |_| true).await && web.settled();
        self.pause(cx, 500).await;
        let unseen = !native(&code, cx).session.starts_with("theirs");
        self.handle
            .update(cx, |_, window, cx| {
                code.update(cx, |code, cx| code.reconnect(window, cx))
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let caught_up = self
            .until(cx, Some(&mut web), |cx| {
                native(&code, cx).settled_on(&theirs)
            })
            .await;
        self.handle
            .update(cx, |_, window, cx| {
                window.dispatch_keystroke(Keystroke::parse("cmd-z").expect("cmd-z"), cx)
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let undone = format!("theirs {before_mine}");
        let own_undone = self
            .until(cx, Some(&mut web), |cx| {
                native(&code, cx).settled_on(&undone)
            })
            .await
            && self.web_on(&mut web, &undone, cx).await;
        // The other client's undo then reverts only its own edit too. Its
        // editor groups typed keys into undo steps by its own timing, so it
        // undoes until its edit is gone, one key per step at most, and no
        // step may touch the native client's text.
        let mut web_undo_steps = Vec::new();
        for _ in theirs
            .strip_suffix(mine.as_str())
            .unwrap_or_default()
            .chars()
        {
            if web.text() == before_mine {
                break;
            }
            web.undo().await;
            self.web_settled(&mut web, cx).await;
            web_undo_steps.push(web.text());
        }
        let web_undid_its_own = web_undo_steps
            .iter()
            .all(|text| text.ends_with(before_mine.as_str()))
            && self.web_on(&mut web, &before_mine, cx).await
            && self
                .until(cx, Some(&mut web), |cx| {
                    native(&code, cx).settled_on(&before_mine)
                })
                .await;
        web.shot(&self.output, "undo-after-reconnect").await;
        self.step(
            "undo-after-reconnect",
            web_committed && unseen && caught_up && own_undone && web_undid_its_own,
            json!({
                "native_missed_while_disconnected": unseen,
                "caught_up_after_reconnect": caught_up,
                "undid_only_its_own_edit": own_undone,
                "web_undo_reverted_only_its_own_edit": web_undid_its_own,
                "web_undo_steps": web_undo_steps,
                "native": native(&code, cx).json(),
            }),
            cx,
        )
        .await;

        // 14. One terminal: the other client opens a shell, the native place
        //     attaches to it from the live announcement, and both type into it.
        let session = web.open_terminal().await;
        let native_attached = self
            .until(cx, Some(&mut web), |cx| {
                code.read_with(cx, |code, _| code.terminal_text().is_some())
            })
            .await;
        web.terminal_input("echo from-$((20+2))-web\n").await;
        self.handle
            .update(cx, |_, window, cx| {
                code.update(cx, |code, cx| {
                    code.send_terminal("echo from-$((20+1))-native\n", window, cx)
                })
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let both = |text: &str| text.contains("from-21-native") && text.contains("from-22-web");
        let shown = self
            .until(cx, Some(&mut web), |cx| {
                code.read_with(cx, |code, _| code.terminal_text())
                    .is_some_and(|text| both(&text))
            })
            .await;
        let mut web_shows = false;
        let deadline = Instant::now() + LIMIT;
        while !web_shows && Instant::now() < deadline {
            web.pump().await;
            web_shows = both(&web.terminal_text().await);
            self.pause(cx, 100).await;
        }
        web.shot(&self.output, "terminal-attach").await;
        // The screen's accessible label, read from the tree GPUI sent; the
        // accessibility capture at step 2 ran before any terminal existed.
        let label = session
            .as_ref()
            .map(|session| format!("Terminal {session} output"));
        let labelled = self
            .handle
            .update(cx, |_, window, _| window.debug_a11y_tree_json())
            .ok()
            .flatten()
            .zip(label.as_ref())
            .is_some_and(|(tree, label)| tree.contains(label.as_str()));
        self.step(
            "terminal-attach",
            session.is_some() && native_attached && shown && web_shows,
            json!({
                "session": session,
                "opened_by": web.name(),
                "native_attached_from_live_announcement": native_attached,
                "native_shows_both": shown,
                "screen_label": label,
                "screen_label_in_gpui_tree": labelled,
                "web_shows_both": web_shows,
                "presentation": "the host's own screen text; the adapted terminal composition is X01's",
                "web_client_refusals_during_the_check": web.refusals().await,
            }),
            cx,
        )
        .await;
        Ok(())
    }

    /// The remote clause, live: the window opened on the Code place by itself
    /// and attached to a host in another process; a stale edit and a disk
    /// conflict there, with the host's Rust client as the other writer.
    async fn remote(&mut self, browser: &Entity<Browser>, cx: &mut AsyncApp) -> anyhow::Result<()> {
        use anyhow::Context as _;
        let origin = std::env::var("FRAME_CODE_REMOTE_URL")?;
        let tenant = std::env::var("FRAME_CODE_REMOTE_TENANT")?;
        let token_file = PathBuf::from(std::env::var("FRAME_CODE_REMOTE_TOKEN_FILE")?);
        // 1. The launch hook shows the place, and the place attaches remotely.
        let shown = self
            .until(cx, None, |cx| {
                browser.read_with(cx, |browser, _| {
                    browser.place == Some(Place::Code) && browser.code.is_some()
                })
            })
            .await;
        let code = browser
            .read_with(cx, |browser, _| browser.code.clone())
            .context("the Code place mounts")?;
        let listed = self
            .until(cx, None, |cx| {
                code.read_with(cx, |code, cx| code.file_count(cx) >= 2)
            })
            .await;
        let (transport, workspace) = code
            .read_with(cx, |code, _| {
                code.link().map(|link| {
                    (
                        link.connection.transport(),
                        link.connection.workspace().to_owned(),
                    )
                })
            })
            .context("the place is attached")?;
        self.step(
            "remote-open-workspace",
            shown && listed && transport == "remote",
            json!({
                "shown_at_launch": shown,
                "transport": transport,
                "origin": origin,
                "workspace": workspace,
                "host_process": "separate: theorem-ide-host --http",
                "files": code.read_with(cx, |code, cx| code.file_count(cx)),
            }),
            cx,
        )
        .await;

        // 2. The file opens, and the host's Rust client attaches beside it.
        self.handle
            .update(cx, |_, window, cx| {
                code.update(cx, |code, cx| code.open_file("src/lib.rs", window, cx))
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let opened = self
            .until(cx, None, |cx| native(&code, cx).settled_on(SOURCE))
            .await;
        let mut other = Web::attach(&origin, &tenant, &token_file, &workspace).await?;
        other.open("src/lib.rs").await?;
        let mut web = Peer::Rust(other);
        self.peer = format!("{} (remote)", web.name());
        self.step(
            "remote-open-document",
            opened && web.text() == SOURCE,
            json!({"native": native(&code, cx).json(), "web_revision": web.revision()}),
            cx,
        )
        .await;

        // 3. A stale edit: the other client commits while the window is
        //    blocked between its view and its keystroke, so the keystroke's
        //    edit names a revision the host has left.
        self.caret_at_end(&code, cx);
        let from = log_len(&code, cx);
        let Peer::Rust(other) = &web else {
            unreachable!("the remote check's peer is the Rust client")
        };
        let racer = other.connection.clone();
        let raced = self
            .handle
            .update(cx, |_, window, cx| {
                let revision = code.read(cx).document().map_or(0, |d| d.1);
                let raced = crate::ide::runtime()
                    .map_err(|e| e.to_string())
                    .and_then(|runtime| {
                        runtime
                            .block_on(racer.call(
                                "documents.edit",
                                json!({"resource_id": "src/lib.rs", "operation_id": "remote-race-1",
                                   "base_revision": revision,
                                   "edits": [{"start": 0, "end": 0, "insert": "/*w*/"}]}),
                            ))
                            .map_err(|refusal| refusal.to_string())
                    });
                window.dispatch_keystroke(key("z"), cx);
                raced.map(|_| revision)
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let expected = format!("/*w*/{SOURCE}z");
        let converged = self
            .until(cx, Some(&mut web), |cx| {
                native(&code, cx).settled_on(&expected)
            })
            .await
            && self.web_on(&mut web, &expected, cx).await;
        let log = log_since(&code, from, cx);
        let refused_stale = log.iter().any(|entry| {
            entry["event"] == "refused"
                && matches!(
                    entry["code"].as_str(),
                    Some("stale_revision" | "revision_conflict")
                )
        });
        self.step(
            "remote-stale-edit",
            raced.is_ok() && refused_stale && converged,
            json!({"raced_at_revision": raced, "log": log, "native": native(&code, cx).json()}),
            cx,
        )
        .await;

        // 4. The checkout changes on the host's disk under unsaved edits.
        let disk = self.checkout.join("src/lib.rs");
        self.disk_conflict(
            "remote-external-disk-modification",
            &code,
            &disk,
            &mut web,
            cx,
        )
        .await?;
        Ok(())
    }

    /// The file is rewritten on disk under unsaved native edits: the
    /// conflict bar shows. Keeping the disk's text replaces the editor's
    /// with the checkout's (I01's `keep: "disk"`); a second rewrite under
    /// new unsaved edits is answered by keeping the editor's text, which
    /// the save writes back. The other client is told of both conflicts.
    async fn disk_conflict(
        &mut self,
        name: &str,
        code: &Entity<Code>,
        disk: &Path,
        web: &mut Peer,
        cx: &mut AsyncApp,
    ) -> anyhow::Result<()> {
        let save_from = log_len(code, cx);
        self.handle
            .update(cx, |_, window, cx| {
                code.update(cx, |code, cx| code.save(window, cx))
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        self.until(cx, Some(&mut *web), |cx| {
            log_since(code, save_from, cx)
                .iter()
                .any(|entry| entry["event"] == "saved")
        })
        .await;
        self.caret_at_end(code, cx);
        self.type_keys("!", cx).await;
        let unsaved = native(code, cx).session.clone();
        self.until(cx, Some(&mut *web), |cx| {
            native(code, cx).settled_on(&unsaved)
        })
        .await;
        std::fs::write(disk, "written by the shell\n")?;
        let flagged = self
            .until(cx, Some(&mut *web), |cx| {
                code.read_with(cx, |code, _| code.conflict().is_some())
            })
            .await;
        let kept_unsaved = native(code, cx).session == unsaved;
        self.shot("disk-conflict", cx).await;
        let conflict = code.read_with(cx, |code, _| code.conflict().cloned());
        self.handle
            .update(cx, |_, window, cx| {
                code.update(cx, |code, cx| code.resolve(Keep::Disk, window, cx))
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let took_disk = self
            .until(cx, Some(&mut *web), |cx| {
                let at = native(code, cx);
                code.read_with(cx, |code, _| code.conflict().is_none())
                    && !at.pending
                    && at.session == "written by the shell\n"
                    && at.editor == at.session
            })
            .await;
        let disk_after_keep_disk = std::fs::read_to_string(disk).ok();
        self.shot("disk-keep-disk", cx).await;
        self.caret_at_end(code, cx);
        self.type_keys("?", cx).await;
        let unsaved_again = native(code, cx).session.clone();
        self.until(cx, Some(&mut *web), |cx| {
            native(code, cx).settled_on(&unsaved_again)
        })
        .await;
        std::fs::write(disk, "rewritten by the shell\n")?;
        let flagged_again = self
            .until(cx, Some(&mut *web), |cx| {
                code.read_with(cx, |code, _| code.conflict().is_some())
            })
            .await;
        self.handle
            .update(cx, |_, window, cx| {
                code.update(cx, |code, cx| code.resolve(Keep::Authority, window, cx))
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        self.until(cx, Some(&mut *web), |cx| {
            code.read_with(cx, |code, _| code.conflict().is_none()) && !native(code, cx).pending
        })
        .await;
        self.handle
            .update(cx, |_, window, cx| {
                code.update(cx, |code, cx| code.save(window, cx))
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let written_back = self
            .until(cx, Some(&mut *web), |cx| {
                let at = native(code, cx);
                std::fs::read_to_string(disk).ok().as_deref() == Some(at.session.as_str())
                    && at.session == unsaved_again
            })
            .await;
        // The other client's session is told of both conflicts.
        let web_told = self
            .web_until(web, cx, |web| web.conflicts_seen() >= 2)
            .await;
        // The banners are read once the other client has the save, so what is
        // still showing is what stays after both conflicts were answered.
        let web_saw_the_save = self.web_clean(web, cx).await;
        web.shot(&self.output, name).await;
        self.step(
            name,
            flagged
                && kept_unsaved
                && took_disk
                && disk_after_keep_disk.as_deref() == Some("written by the shell\n")
                && flagged_again
                && written_back
                && web_told,
            json!({
                "conflict": conflict,
                "unsaved_edits_kept_until_answered": kept_unsaved,
                "keep_disk_took_the_checkout_text": took_disk,
                "disk_after_keep_disk": disk_after_keep_disk,
                "second_conflict": flagged_again,
                "kept": ["disk", "authority"],
                "disk_matches_editor": written_back,
                "web_conflicts_seen": web.conflicts_seen(),
                "web_saw_the_save": web_saw_the_save,
                "web_conflict_banners": web.conflict_banners(),
            }),
            cx,
        )
        .await;
        Ok(())
    }

    /// Put the editor's caret at the end of its text.
    fn caret_at_end(&self, code: &Entity<Code>, cx: &mut AsyncApp) {
        let _ = self.handle.update(cx, |_, window, cx| {
            let editor = code.read(cx).editor().clone();
            editor.update(cx, |editor, cx| {
                let end = editor.value().len();
                editor.set_selected_range(end..end, cx);
                editor.focus(window, cx);
            });
        });
    }

    /// Put the editor's caret just after the first `marker`.
    fn caret_after(&self, code: &Entity<Code>, marker: &str, cx: &mut AsyncApp) {
        let _ = self.handle.update(cx, |_, window, cx| {
            let editor = code.read(cx).editor().clone();
            editor.update(cx, |editor, cx| {
                let at = editor
                    .value()
                    .find(marker)
                    .map_or(0, |at| at + marker.len());
                editor.set_selected_range(at..at, cx);
                editor.focus(window, cx);
            });
        });
    }

    /// The window's accessibility tree as macOS presents it, kept beside the
    /// report; it is evidence, not a step. First asked from outside the
    /// process through `AXUIElement` (`script/code-check-ax.swift`), which macOS
    /// refuses unless the asking process is trusted for accessibility; the
    /// answer, refusal included, is kept as it came. Then asked inside the
    /// process through the view's `NSAccessibility` answers, which is what an
    /// assistive client receives and which starts GPUI's AccessKit tree; GPUI's
    /// own record of the tree it sent is saved too.
    async fn accessibility(&mut self, cx: &mut AsyncApp) {
        let script = std::env::var_os("FRAME_CODE_AX_SCRIPT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR")).join("script/code-check-ax.swift")
            });
        let outside = self.outside_tree(&script, cx).await;
        let write = |name: &str, value: &Value| {
            let _ = std::fs::write(
                self.output.join(name),
                serde_json::to_vec_pretty(value).unwrap_or_default(),
            );
        };
        write("ax-tree-outside.json", &outside);
        #[cfg(target_os = "macos")]
        let inside = {
            // The first ask starts the tree; the next frame fills it.
            let _ = self.handle.update(cx, |_, window, _| ax::walk(window));
            self.pause(cx, 600).await;
            self.handle
                .update(cx, |_, window, _| ax::walk(window))
                .ok()
                .flatten()
                .unwrap_or(Value::Null)
        };
        #[cfg(not(target_os = "macos"))]
        let inside = Value::Null;
        write("ax-tree-inside.json", &inside);
        let gpui = self
            .handle
            .update(cx, |_, window, _| window.debug_a11y_tree_json())
            .ok()
            .flatten();
        let gpui: Value = gpui
            .as_deref()
            .map(|text| serde_json::from_str(text).unwrap_or_else(|_| json!(text)))
            .unwrap_or(Value::Null);
        write("gpui-a11y-tree.json", &gpui);
        let labels = [
            "Open a folder",
            "Save the file",
            "Open a terminal in the workspace",
            "Prepare the replacement at this revision",
            "Apply the prepared replacement",
            "Workspace files",
            "src/lib.rs",
        ];
        let (inside_text, gpui_text) = (inside.to_string(), gpui.to_string());
        self.accessibility = json!({
            "outside": {
                "trusted": outside["trusted"],
                "error": outside["error"],
                "error_name": outside["error_name"],
                "nodes": outside["nodes"],
                "file": "ax-tree-outside.json",
            },
            "inside": {
                "labels_found": labels.iter().filter(|l| inside_text.contains(*l)).collect::<Vec<_>>(),
                "file": "ax-tree-inside.json",
            },
            "gpui": {
                "labels_found": labels.iter().filter(|l| gpui_text.contains(*l)).collect::<Vec<_>>(),
                "file": "gpui-a11y-tree.json",
            },
            "labels_sought": labels,
        });
    }

    /// Run the `AXUIElement` dump against this process without blocking the
    /// main thread, which has to answer it.
    async fn outside_tree(&self, script: &Path, cx: &mut AsyncApp) -> Value {
        // Files, not pipes: a large tree would fill a pipe nobody reads yet.
        let (out, err) = (
            self.output.join("ax-tree-outside.out"),
            self.output.join("ax-tree-outside.err"),
        );
        let child = std::fs::File::create(&out)
            .and_then(|stdout| Ok((stdout, std::fs::File::create(&err)?)))
            .and_then(|(stdout, stderr)| {
                std::process::Command::new("swift")
                    .arg(script)
                    .arg(std::process::id().to_string())
                    .stdout(stdout)
                    .stderr(stderr)
                    .spawn()
            });
        let mut child = match child {
            Ok(child) => child,
            Err(error) => return json!({"error_name": format!("could not run swift: {error}")}),
        };
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => self.pause(cx, 200).await,
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return json!({"error_name": "the AXUIElement dump did not finish in 120 s"});
                }
            }
        }
        let stdout = std::fs::read(&out).unwrap_or_default();
        let value = serde_json::from_slice(&stdout).unwrap_or_else(|_| {
            json!({
                "error_name": "unreadable output",
                "stdout": String::from_utf8_lossy(&stdout),
                "stderr": std::fs::read_to_string(&err).unwrap_or_default(),
            })
        });
        let _ = std::fs::remove_file(&out);
        value
    }

    /// Type `text` a key at a time into the focused element, a frame apart.
    async fn type_keys(&self, text: &str, cx: &mut AsyncApp) {
        self.pause(cx, 150).await;
        for c in text.chars() {
            let _ = self.handle.update(cx, |_, window, cx| {
                window.dispatch_keystroke(key(&c.to_string()), cx);
            });
            self.pause(cx, 30).await;
        }
    }

    async fn pause(&self, cx: &mut AsyncApp, ms: u64) {
        let _ = self.handle.update(cx, |_, window, _| window.refresh());
        cx.background_executor()
            .timer(Duration::from_millis(ms))
            .await;
    }

    /// Poll `ready` every tenth of a second for up to `LIMIT`, handling the
    /// other client's live events between polls.
    async fn until(
        &self,
        cx: &mut AsyncApp,
        mut web: Option<&mut Peer>,
        mut ready: impl FnMut(&mut AsyncApp) -> bool,
    ) -> bool {
        let deadline = Instant::now() + LIMIT;
        loop {
            if let Some(web) = web.as_deref_mut() {
                web.pump().await;
            }
            if ready(cx) {
                return true;
            }
            if Instant::now() > deadline {
                return false;
            }
            self.pause(cx, 100).await;
        }
    }

    /// The other client settled on `text`.
    async fn web_on(&self, web: &mut Peer, text: &str, cx: &mut AsyncApp) -> bool {
        let deadline = Instant::now() + LIMIT;
        loop {
            web.pump().await;
            if web.settled() && web.text() == text {
                return true;
            }
            if Instant::now() > deadline {
                return false;
            }
            self.pause(cx, 100).await;
        }
    }

    /// Wait for the other client to have nothing pending.
    async fn web_settled(&self, web: &mut Peer, cx: &mut AsyncApp) -> bool {
        self.web_until(web, cx, Peer::settled).await
    }

    /// The other client learned that the document is saved.
    async fn web_clean(&self, web: &mut Peer, cx: &mut AsyncApp) -> bool {
        self.web_until(web, cx, |web| !web.dirty()).await
    }

    /// Pump the other client until `done` holds, or until [`LIMIT`] passes.
    async fn web_until(
        &self,
        web: &mut Peer,
        cx: &mut AsyncApp,
        done: impl Fn(&Peer) -> bool,
    ) -> bool {
        let deadline = Instant::now() + LIMIT;
        loop {
            web.pump().await;
            if done(web) {
                return true;
            }
            if Instant::now() > deadline {
                return false;
            }
            self.pause(cx, 100).await;
        }
    }

    /// Record a step with its capture, and rewrite the report.
    async fn step(&mut self, name: &str, passed: bool, detail: Value, cx: &mut AsyncApp) {
        self.shot(name, cx).await;
        self.steps
            .push(json!({"step": name, "passed": passed, "detail": detail}));
        let report = json!({
            "checkout": self.checkout,
            "peer": self.peer,
            "accessibility": self.accessibility,
            "steps": self.steps,
        });
        let _ = std::fs::write(
            self.output.join("report.json"),
            serde_json::to_vec_pretty(&report).unwrap_or_default(),
        );
    }

    async fn shot(&self, name: &str, cx: &mut AsyncApp) {
        let path = self.output.join(format!("{name}.png"));
        capture(self.handle, Appearance::Light, &path, cx).await;
    }
}
