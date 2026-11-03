//! Live acceptance of the Code place (ac-22) in the production window, against
//! the host it embeds, with a second client attached over the host's
//! authenticated loopback routes the way the web client attaches. Enabled only
//! by `snapshot`.
//!
//! `FRAME_CODE_CHECK=<output>`. The check makes a fresh checkout and host
//! state under `<output>`, opens the checkout in the Code place, and drives
//! the place with keystrokes dispatched to its window, an IME composition
//! through the editor's input handler, and its own actions. The second client
//! (`web` below) is the host's Rust client over `Connection::remote`; it reads
//! the loopback credential from the host's file and never prints it. Every
//! step lands in `report.json` with a screenshot, whether it passed or not;
//! the process exits nonzero if any step failed.
use super::workspace_check::capture;
use super::*;
use crate::ide::{Code, Link, embed};
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
    let checkout = output.join("checkout");
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
                };
                if let Err(error) = check.scenarios(&browser, cx).await {
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
        let token =
            std::fs::read_to_string(host.join("token")).context("reading the host credential")?;
        let connection = Connection::remote(
            reqwest::Client::new(),
            &origin,
            "local",
            token.trim(),
            link.connection.workspace(),
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
                Output::Unreconciled(result) => self
                    .refusals
                    .push(format!("reconcile merged nothing: {result}")),
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
        self.step("open-document", opened, native(&code, cx).json(), cx)
            .await;

        // 3. The second client attaches over the loopback routes to the same
        //    workspace and document.
        let mut web = Web::connect(&link, &self.host).await?;
        web.open("src/lib.rs").await?;
        let at = native(&code, cx);
        self.step(
            "second-client-opens",
            web.text() == at.session && web.revision() == at.revision,
            json!({"web_revision": web.revision(), "native": at.json(), "web_transport": web.connection.transport()}),
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
        self.step(
            "native-typing",
            converged,
            json!({"native": native(&code, cx).json(), "web": web.text(), "web_revision": web.revision()}),
            cx,
        )
        .await;

        // 5. The other client's edit arrives in the native editor.
        let theirs = format!("// web\n{}", web.text());
        web.type_text(&theirs).await;
        let arrived = self
            .until(cx, Some(&mut web), |cx| {
                native(&code, cx).settled_on(&theirs)
            })
            .await;
        self.step(
            "remote-edit-arrives",
            arrived && web.settled(),
            json!({"native": native(&code, cx).json(), "web_revision": web.revision()}),
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
        let from = log_len(&code, cx);
        self.handle
            .update(cx, |_, window, cx| {
                window.dispatch_keystroke(Keystroke::parse("cmd-s").expect("cmd-s"), cx)
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let disk = self.checkout.join("src/lib.rs");
        let saved = self
            .until(cx, Some(&mut web), |_| {
                std::fs::read_to_string(&disk).ok().as_deref() == Some(expected.as_str())
            })
            .await;
        let web_clean =
            self.until(cx, Some(&mut web), |_| true).await && self.web_clean(&mut web, cx).await;
        self.step(
            "save-concurrent",
            saved && web_clean,
            json!({"log": log_since(&code, from, cx), "disk_matches": saved, "web_sees_saved": web_clean}),
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

        // 12. The file is rewritten on disk under unsaved native edits: the
        //     conflict bar shows. Keeping the disk's text merges from the saved
        //     base, so over lines both sides changed it merges nothing and the
        //     bar stays; keeping the editor's text writes it back.
        let save_from = log_len(&code, cx);
        self.handle
            .update(cx, |_, window, cx| {
                code.update(cx, |code, cx| code.save(window, cx))
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        self.until(cx, Some(&mut web), |cx| {
            log_since(&code, save_from, cx)
                .iter()
                .any(|entry| entry["event"] == "saved")
        })
        .await;
        self.caret_at_end(&code, cx);
        self.type_keys("!", cx).await;
        let unsaved = native(&code, cx).session.clone();
        self.until(cx, Some(&mut web), |cx| {
            native(&code, cx).settled_on(&unsaved)
        })
        .await;
        std::fs::write(&disk, "written by the shell\n")?;
        let flagged = self
            .until(cx, Some(&mut web), |cx| {
                code.read_with(cx, |code, _| code.conflict().is_some())
            })
            .await;
        let kept_unsaved = native(&code, cx).session == unsaved;
        self.shot("disk-conflict", cx).await;
        let conflict = code.read_with(cx, |code, _| code.conflict().cloned());
        self.handle
            .update(cx, |_, window, cx| {
                code.update(cx, |code, cx| code.resolve(Keep::Disk, window, cx))
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let disk_merged_nothing = self
            .until(cx, Some(&mut web), |cx| {
                code.read_with(cx, |code, _| {
                    code.notice() == Some(crate::ide::UNMERGED) && code.conflict().is_some()
                }) && !native(&code, cx).pending
            })
            .await;
        let still_unsaved = native(&code, cx).session == unsaved;
        self.shot("disk-keep-merged-nothing", cx).await;
        self.handle
            .update(cx, |_, window, cx| {
                code.update(cx, |code, cx| code.resolve(Keep::Authority, window, cx))
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        self.until(cx, Some(&mut web), |cx| {
            code.read_with(cx, |code, _| code.conflict().is_none()) && !native(&code, cx).pending
        })
        .await;
        self.handle
            .update(cx, |_, window, cx| {
                code.update(cx, |code, cx| code.save(window, cx))
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let written_back = self
            .until(cx, Some(&mut web), |cx| {
                let at = native(&code, cx);
                std::fs::read_to_string(&disk).ok().as_deref() == Some(at.session.as_str())
                    && at.session == unsaved
            })
            .await;
        self.step(
            "external-disk-modification",
            flagged && kept_unsaved && disk_merged_nothing && still_unsaved && written_back,
            json!({
                "conflict": conflict,
                "unsaved_edits_kept": kept_unsaved,
                "keep_disk_over_overlapping_lines_merged_nothing": disk_merged_nothing,
                "unsaved_edits_kept_after_keep_disk": still_unsaved,
                "kept": "editor",
                "disk_matches_editor": written_back,
                "web_conflicts_seen": web.conflicts.len(),
            }),
            cx,
        )
        .await;

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
        let web_keeps_its_step = web.document.as_ref().is_some_and(DocumentSession::can_undo);
        self.step(
            "undo-after-reconnect",
            web_committed && unseen && caught_up && own_undone && web_keeps_its_step,
            json!({
                "native_missed_while_disconnected": unseen,
                "caught_up_after_reconnect": caught_up,
                "undid_only_its_own_edit": own_undone,
                "web_keeps_its_undo_step": web_keeps_its_step,
                "native": native(&code, cx).json(),
            }),
            cx,
        )
        .await;

        // 14. One terminal: the other client opens a shell, the native place
        //     attaches to it from the live announcement, and both type into it.
        let (operation, arguments) = TerminalAttachment::open_call(100, 24);
        let opened = web.call(operation, arguments).await;
        let session = opened
            .as_ref()
            .ok()
            .and_then(|opened| opened["session"].as_str())
            .map(str::to_owned);
        if let Some(session) = &session {
            let mut attachment = TerminalAttachment::new(session, 100, 24);
            if let Some(call) = attachment.attach() {
                let reply = web.connection.perform(call).await;
                let _ = attachment.complete(&reply);
            }
            web.terminal = Some(attachment);
        }
        let native_attached = self
            .until(cx, Some(&mut web), |cx| {
                code.read_with(cx, |code, _| code.terminal_text().is_some())
            })
            .await;
        if let Some(call) = web
            .terminal
            .as_ref()
            .map(|t| t.input("echo from-$((20+2))-web\n"))
        {
            let _ = web.connection.perform(call).await;
        }
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
        let web_shows = web.terminal.as_ref().is_some_and(|t| both(&t.contents()));
        self.step(
            "terminal-attach",
            session.is_some() && native_attached && shown && web_shows,
            json!({
                "session": session,
                "opened_by": "web-check",
                "native_attached_from_live_announcement": native_attached,
                "native_shows_both": shown,
                "web_shows_both": web_shows,
                "presentation": "the host's own screen text; the adapted terminal composition is X01's",
                "web_client_refusals_during_the_check": web.refusals,
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
        mut web: Option<&mut Web>,
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
    async fn web_on(&self, web: &mut Web, text: &str, cx: &mut AsyncApp) -> bool {
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

    /// The other client learned that the document is saved.
    async fn web_clean(&self, web: &mut Web, cx: &mut AsyncApp) -> bool {
        let deadline = Instant::now() + LIMIT;
        loop {
            web.pump().await;
            if web.document.as_ref().is_some_and(|d| !d.dirty()) {
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
        let report = json!({"checkout": self.checkout, "steps": self.steps});
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
