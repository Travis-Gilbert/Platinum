//! Live acceptance of the Agent place against a running Theorem authority, in
//! the production window: connecting, a run's work arriving as it happens,
//! Stop, and the breaks
//! a reader meets (the authority down at launch, lost mid-run and back, a run
//! followed from a second window, a failing model provider, a refused
//! credential). Enabled only by `snapshot`.
//!
//! `FRAME_THEOREM_LIVE_CHECK=<output>`, with `THEOREMWEB_AUTHORITY_URL` and
//! `THEOREMWEB_AUTH_TOKEN_FILE` naming the authority and
//! `FRAME_THEOREM_LIVE_STORE` naming a script that takes `start`, `stop`,
//! `start-refusing` (up, refusing this credential) or `start-bad-provider` (up,
//! with a model key the provider rejects). It sends messages and kills the
//! authority, so never point it at a shared one. Every step lands in
//! `report.json` whether it passed or not; the process exits nonzero if any
//! step failed.
//!
//! The authority streams whole parts (progress, programs, tool calls, and the
//! answer text once it is written), not text deltas, so a live run shows as
//! parts arriving before the run settles. Its agent may reach for host tools
//! on any question and can run out of iterations doing so; the messages that
//! need an answer ask it not to.
use super::workspace_check::{capture, until_within};
use super::*;
use crate::theorem::TheoremDriver;
use gpui_kit::component::Root;
use gpui_kit::{AsyncApp, WindowBounds, WindowOptions};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use theorem_surface_contracts::intent::ComposerIntent;
use theorem_surface_contracts::thread::{ThreadActor, ThreadPart, ThreadRunState};
use theoremweb_agent_runtime::{ChatHostPhase, ChatHostProjection};

pub fn run(output: PathBuf) -> anyhow::Result<()> {
    use anyhow::Context as _;
    let store = std::env::var_os("FRAME_THEOREM_LIVE_STORE")
        .context("FRAME_THEOREM_LIVE_STORE names the authority's control script")?;
    for name in ["THEOREMWEB_AUTHORITY_URL", "THEOREMWEB_AUTH_TOKEN_FILE"] {
        anyhow::ensure!(std::env::var_os(name).is_some(), "{name} is not set");
    }
    let _ = rustls::crypto::ring::default_provider().install_default();
    // The check waits on tenth-of-a-second timers while its window is often
    // behind others; App Nap would coalesce them into minutes.
    #[cfg(target_os = "macos")]
    let _awake = {
        use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
        NSProcessInfo::processInfo().beginActivityWithOptions_reason(
            NSActivityOptions::UserInitiatedAllowingIdleSystemSleep,
            &NSString::from_str("Theorem live check"),
        )
    };
    std::fs::create_dir_all(&output)?;
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
            crate::theorem::init(None, cx);
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
                .expect("live check window");
            let handle: gpui_kit::AnyWindowHandle = handle.into();
            let browser = view.unwrap();
            cx.activate(true);
            let cleanup = profile.clone();
            cx.spawn(async move |cx| {
                let mut check = Check {
                    output,
                    store,
                    handle,
                    steps: Vec::new(),
                };
                check.scenarios(&browser, cx).await;
                let _ = std::fs::remove_dir_all(cleanup);
                // Quitting ends the process, with success, before `run` returns.
                if check.steps.iter().any(|step| step["passed"] != true) {
                    eprintln!("a live step failed; see report.json");
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
    store: OsString,
    handle: gpui_kit::AnyWindowHandle,
    steps: Vec<serde_json::Value>,
}

/// What the card shows of the turn in progress: the last Theorem message.
#[derive(Clone, Debug, PartialEq)]
struct Turn {
    messages: usize,
    /// Parts of the last Theorem message, of any kind.
    parts: usize,
    answer: String,
    run_state: ThreadRunState,
    can_cancel: bool,
    /// The composer still reads as sending: the run may have ended, but the
    /// authority has not yet written the turn into the conversation.
    sending: bool,
    phase: ChatHostPhase,
    notice: Option<String>,
}

impl Turn {
    fn of(projection: &ChatHostProjection) -> Self {
        let last = projection
            .thread
            .messages
            .iter()
            .rev()
            .find(|message| message.actor == ThreadActor::Theorem);
        let answer = last
            .map(|message| {
                message
                    .parts
                    .iter()
                    .filter_map(|part| match part {
                        ThreadPart::Text { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<String>()
            })
            .unwrap_or_default();
        Self {
            messages: projection.thread.messages.len(),
            parts: last.map_or(0, |message| message.parts.len()),
            answer,
            run_state: projection.thread.run_state,
            can_cancel: projection.composer.can_cancel(),
            sending: projection.composer.is_sending,
            phase: projection.phase,
            notice: projection.notice.clone(),
        }
    }

    fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "messages": self.messages,
            "parts": self.parts,
            "answer_chars": self.answer.chars().count(),
            "answer_start": self.answer.chars().take(160).collect::<String>(),
            "run_state": format!("{:?}", self.run_state),
            "can_cancel": self.can_cancel,
            "sending": self.sending,
            "phase": format!("{:?}", self.phase),
            "notice": self.notice,
        })
    }
}

/// How one sent message went, sampled every tenth of a second.
struct Followed {
    /// When the first part of the reply showed while the run was live.
    first_part: Option<Duration>,
    /// Distinct part counts seen while the run was live: more than one means
    /// the reader watched the work arrive, not only the settled reply.
    live_part_counts: usize,
    settled: Option<Duration>,
    /// When the authority had written the settled turn into the
    /// conversation. Stopping it before then leaves the conversation linked
    /// to the run.
    written: Option<Duration>,
    last: Turn,
}

impl Followed {
    fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "first_part_ms": self.first_part.map(|at| at.as_millis()),
            "live_part_counts": self.live_part_counts,
            "settled_ms": self.settled.map(|at| at.as_millis()),
            "written_ms": self.written.map(|at| at.as_millis()),
            "last": self.last.json(),
        })
    }
}

impl Check {
    async fn scenarios(&mut self, browser: &Entity<Browser>, cx: &mut AsyncApp) {
        // 1. The authority is down when the place opens.
        self.store("stop", cx).await;
        self.handle
            .update(cx, |_, window, cx| {
                browser.update(cx, |browser, cx| {
                    browser.open_place(Place::Agent, window, cx)
                })
            })
            .unwrap();
        let driver = browser.read_with(cx, |browser, cx| {
            let (workbench, _) = browser.theorem.as_ref().expect("the Agent place mounts");
            workbench.read(cx).driver().clone()
        });
        let settled = until_within(Duration::from_secs(30), cx, |cx| {
            !matches!(
                phase(&driver, cx),
                ChatHostPhase::Loading | ChatHostPhase::Unhydrated
            )
        })
        .await;
        let card = turn(&driver, cx);
        self.step(
            "down-at-launch",
            settled && card.phase == ChatHostPhase::Refused && card.notice.is_some(),
            serde_json::json!({
                "connection": format!("{:?}", driver.read_with(cx, |driver, _| driver.connection())),
                "settled_within_30s": settled,
                "card": card.json(),
            }),
            cx,
        )
        .await;

        // 2. Retry once it is up.
        self.store("start", cx).await;
        let connected = self.connect(&driver, cx).await;
        let detail = driver.read_with(cx, |driver, _| {
            let projection = driver.projection();
            serde_json::json!({
                "projects": projection.projects.len(),
                "threads": projection.threads.len(),
                "selected_project": projection.selected_project_id,
                "card": Turn::of(projection).json(),
            })
        });
        self.step("retry-connects", connected, detail, cx).await;
        if !connected {
            return;
        }

        // 3. A new conversation answers a message, its work showing as it runs.
        let opened = new_conversation(&driver, cx).await;
        // A new account has no project; the first conversation makes "General".
        let projects = driver.read_with(cx, |driver, _| {
            let projection = driver.projection();
            (
                projection.projects.len(),
                projection.selected_project_id.clone(),
            )
        });
        self.step(
            "new-conversation",
            opened && projects.0 >= 1 && projects.1.is_some(),
            serde_json::json!({ "projects": projects.0, "selected_project": projects.1, "card": turn(&driver, cx).json() }),
            cx,
        )
        .await;
        let followed = self
            .send(
                &driver,
                "Without using tools, in three short sentences, explain why the sky looks blue.",
                Duration::from_secs(120),
                Some("live-streaming"),
                cx,
            )
            .await;
        let answered = followed.written.is_some()
            && followed.last.run_state == ThreadRunState::Idle
            && !followed.last.answer.is_empty();
        self.step(
            "live-answer",
            answered && followed.live_part_counts >= 2,
            followed.json(),
            cx,
        )
        .await;

        // 4. Stop while the run works.
        let before = self
            .submit(&driver, "Count from 1 to 400, one number per line.", cx)
            .await;
        let streaming = until_within(Duration::from_secs(90), cx, |cx| {
            working(&turn(&driver, cx), before)
        })
        .await;
        let at_stop = turn(&driver, cx);
        driver.update(cx, |driver, cx| driver.stop(cx));
        let stopped = until_within(Duration::from_secs(30), cx, |cx| {
            !turn(&driver, cx).can_cancel
        })
        .await;
        cx.background_executor().timer(Duration::from_secs(2)).await;
        let after = turn(&driver, cx);
        self.step(
            "stop-mid-answer",
            streaming && stopped && after.run_state == ThreadRunState::Aborted,
            serde_json::json!({
                "streaming_before_stop": streaming,
                "settled_within_30s": stopped,
                "at_stop": at_stop.json(),
                "after": after.json(),
            }),
            cx,
        )
        .await;

        // 5. The authority vanishes mid-run and comes back.
        let before = self
            .submit(
                &driver,
                "Write a detailed 800-word history of cartography.",
                cx,
            )
            .await;
        let streaming = until_within(Duration::from_secs(90), cx, |cx| {
            working(&turn(&driver, cx), before)
        })
        .await;
        self.store("stop", cx).await;
        cx.background_executor().timer(Duration::from_secs(5)).await;
        let during = turn(&driver, cx);
        self.shot("live-outage", cx).await;
        self.store("start", cx).await;
        let started = Instant::now();
        let settled = until_within(Duration::from_secs(60), cx, |cx| {
            !turn(&driver, cx).can_cancel
        })
        .await;
        let after = turn(&driver, cx);
        self.step(
            "outage-mid-answer",
            streaming && settled && !after.run_state.is_live(),
            serde_json::json!({
                "streaming_before_outage": streaming,
                "during_outage": during.json(),
                "settled_after_restart_ms": settled.then(|| started.elapsed().as_millis()),
                "after": after.json(),
            }),
            cx,
        )
        .await;
        // A run the restarted authority still calls live, with nothing left to
        // finish it, must at least yield to Stop.
        if !settled {
            driver.update(cx, |driver, cx| driver.stop(cx));
            let stopped = until_within(Duration::from_secs(30), cx, |cx| {
                !turn(&driver, cx).can_cancel
            })
            .await;
            let after = turn(&driver, cx);
            self.step(
                "stop-clears-orphaned-run",
                stopped && !after.run_state.is_live(),
                serde_json::json!({ "settled_within_30s": stopped, "after": after.json() }),
                cx,
            )
            .await;
        }
        // The same conversation answers again, or, if the authority never
        // closed the lost run, says to start a new one without waiting it out.
        let before = self
            .submit(
                &driver,
                "Without using tools, in one sentence: what was the first question I asked in this conversation?",
                cx,
            )
            .await;
        let started = Instant::now();
        let mut answered = false;
        let mut stuck = false;
        until_within(Duration::from_secs(120), cx, |cx| {
            let turn = turn(&driver, cx);
            answered = turn.messages >= before + 2 && !turn.can_cancel && !turn.run_state.is_live();
            stuck = !answered
                && turn
                    .notice
                    .as_deref()
                    .is_some_and(|notice| notice.contains("new conversation"));
            answered || stuck
        })
        .await;
        let elapsed = started.elapsed().as_millis();
        let after = turn(&driver, cx);
        self.step(
            "answers-after-outage",
            answered && after.run_state == ThreadRunState::Idle && !after.answer.is_empty(),
            serde_json::json!({ "settled_ms": answered.then_some(elapsed), "after": after.json() }),
            cx,
        )
        .await;
        if !answered {
            self.shot("stuck-conversation", cx).await;
            self.step(
                "stuck-conversation-is-explained",
                stuck && !after.can_cancel && !after.run_state.is_live(),
                serde_json::json!({ "explained_ms": stuck.then_some(elapsed), "after": after.json() }),
                cx,
            )
            .await;
            let opened = new_conversation(&driver, cx).await;
            self.step(
                "new-conversation-after-outage",
                opened,
                serde_json::json!({ "card": turn(&driver, cx).json() }),
                cx,
            )
            .await;
        }

        // 6. A second window opens the place while a run is live and
        // follows that run, as a relaunch would.
        let before = self
            .submit(
                &driver,
                "Without using tools, list twelve well-known rivers, one sentence about each.",
                cx,
            )
            .await;
        let streaming = until_within(Duration::from_secs(90), cx, |cx| {
            working(&turn(&driver, cx), before)
        })
        .await;
        let second = self
            .handle
            .update(cx, |_, window, cx| {
                let workbench = cx.new(|cx| crate::theorem::Workbench::new(window, cx));
                let driver = workbench.read(cx).driver().clone();
                (workbench, driver)
            })
            .unwrap();
        let mut adopted = false;
        let mut counts = BTreeSet::new();
        let started = Instant::now();
        let settled = until_within(Duration::from_secs(120), cx, |cx| {
            let turn = turn(&second.1, cx);
            if turn.can_cancel {
                adopted = true;
                counts.insert(turn.parts);
            }
            adopted && !turn.can_cancel
        })
        .await;
        // Both windows see the authority write the answer into the
        // conversation, after the run's stream ends; stopping the authority
        // before then would leave the conversation linked to the run.
        let written = until_within(Duration::from_secs(60), cx, |cx| {
            [&driver, &second.1].into_iter().all(|driver| {
                driver.read_with(cx, |driver, _| {
                    let projection = driver.projection();
                    !projection.composer.is_sending && !projection.thread.run_state.is_live()
                })
            })
        })
        .await;
        let after = turn(&second.1, cx);
        let original = turn(&driver, cx);
        self.step(
            "second-window-follows-live-run",
            streaming && adopted && settled && written && !after.answer.is_empty(),
            serde_json::json!({
                "streaming_in_first": streaming,
                "second_saw_live_run": adopted,
                "second_live_part_counts": counts.len(),
                "settled_ms": settled.then(|| started.elapsed().as_millis()),
                "written_in_both": written,
                "second": after.json(),
                "first": original.json(),
            }),
            cx,
        )
        .await;
        drop(second);

        // 7. The model provider refuses the authority's key.
        self.store("stop", cx).await;
        self.store("start-bad-provider", cx).await;
        let followed = self
            .send(&driver, "Say hello.", Duration::from_secs(60), None, cx)
            .await;
        self.step(
            "provider-failure-is-shown",
            followed.written.is_some() && followed.last.run_state == ThreadRunState::Failed,
            followed.json(),
            cx,
        )
        .await;

        // 8. The authority refuses the credential: signed out, then back.
        self.store("stop", cx).await;
        self.store("start-refusing", cx).await;
        driver.update(cx, |driver, cx| driver.retry(cx));
        let settled = until_within(Duration::from_secs(30), cx, |cx| {
            !matches!(
                phase(&driver, cx),
                ChatHostPhase::Loading | ChatHostPhase::Unhydrated
            )
        })
        .await;
        let refused = turn(&driver, cx);
        self.step(
            "refused-credential-signs-out",
            settled && refused.phase == ChatHostPhase::Unauthenticated,
            serde_json::json!({
                "settled_within_30s": settled,
                "card": refused.json(),
                "sign_in": driver.read_with(cx, |driver, _| driver.sign_in_url().map(|url| url.to_string())),
            }),
            cx,
        )
        .await;
        self.store("stop", cx).await;
        self.store("start", cx).await;
        let connected = self.connect(&driver, cx).await;
        self.step(
            "reconnects-after-sign-in",
            connected,
            serde_json::json!({ "card": turn(&driver, cx).json() }),
            cx,
        )
        .await;
    }

    /// Retry, and wait for the authority to load the workspace.
    async fn connect(&mut self, driver: &Entity<TheoremDriver>, cx: &mut AsyncApp) -> bool {
        driver.update(cx, |driver, cx| driver.retry(cx));
        until_within(Duration::from_secs(30), cx, |cx| {
            driver.read_with(cx, |driver, _| {
                let projection = driver.projection();
                projection.phase == ChatHostPhase::Ready
            })
        })
        .await
    }

    /// Send `text` once the composer would take it, as a person can press
    /// Send only then, and return how many messages the conversation held.
    async fn submit(&self, driver: &Entity<TheoremDriver>, text: &str, cx: &mut AsyncApp) -> usize {
        until_within(Duration::from_secs(30), cx, |cx| {
            driver.read_with(cx, |driver, _| {
                driver.projection().composer.affordance(text).is_send()
            })
        })
        .await;
        driver.update(cx, |driver, cx| {
            let projection = driver.projection();
            let before = projection.thread.messages.len();
            let quote = projection.composer.quote.clone();
            driver.composer_intent(
                ComposerIntent::Submit {
                    text: text.into(),
                    quote,
                    steer: false,
                    context: Vec::new(),
                },
                cx,
            );
            before
        })
    }

    /// Send `text` and follow its reply until the authority has written the
    /// settled turn or `limit` passes, capturing `streaming` the first time
    /// part of it shows.
    async fn send(
        &mut self,
        driver: &Entity<TheoremDriver>,
        text: &str,
        limit: Duration,
        mut streaming: Option<&str>,
        cx: &mut AsyncApp,
    ) -> Followed {
        let before = self.submit(driver, text, cx).await;
        let started = Instant::now();
        let mut first_part = None;
        let mut counts = BTreeSet::new();
        let mut settled = None;
        let mut written = None;
        while started.elapsed() < limit {
            let turn = turn(driver, cx);
            let replied = turn.messages >= before + 2;
            if working(&turn, before) {
                first_part.get_or_insert(started.elapsed());
                counts.insert(turn.parts);
                if let Some(name) = streaming.take() {
                    self.shot(name, cx).await;
                }
            }
            if settled.is_none() && replied && !turn.can_cancel && !turn.run_state.is_live() {
                settled = Some(started.elapsed());
            }
            if settled.is_some() && !turn.sending {
                written = Some(started.elapsed());
                break;
            }
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
        }
        Followed {
            first_part,
            live_part_counts: counts.len(),
            settled,
            written,
            last: turn(driver, cx),
        }
    }

    async fn store(&mut self, verb: &str, cx: &mut AsyncApp) {
        let script = self.store.clone();
        let command = verb.to_owned();
        let ran = cx
            .background_spawn(
                async move { std::process::Command::new(&script).arg(&command).output() },
            )
            .await;
        let (ok, said) = match ran {
            Ok(out) => (
                out.status.success(),
                String::from_utf8_lossy(&out.stdout).trim().to_owned(),
            ),
            Err(error) => (false, error.to_string()),
        };
        self.steps.push(serde_json::json!({
            "store": verb, "passed": ok, "said": said,
        }));
    }

    /// Record a step with its capture, and rewrite the report.
    async fn step(
        &mut self,
        name: &str,
        passed: bool,
        detail: serde_json::Value,
        cx: &mut AsyncApp,
    ) {
        self.shot(name, cx).await;
        self.steps.push(serde_json::json!({
            "step": name, "passed": passed, "detail": detail,
        }));
        let report = serde_json::json!({
            "authority": std::env::var("THEOREMWEB_AUTHORITY_URL").ok(),
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

/// The run started after `before` messages is live and has shown some of its
/// work.
/// Start a new conversation and wait for its empty transcript.
async fn new_conversation(driver: &Entity<TheoremDriver>, cx: &mut AsyncApp) -> bool {
    driver.update(cx, |driver, cx| driver.new_conversation(cx));
    until_within(Duration::from_secs(15), cx, |cx| {
        driver.read_with(cx, |driver, _| {
            let projection = driver.projection();
            projection.phase == ChatHostPhase::Ready
                && projection.selected_thread_id.is_some()
                && projection.thread.messages.is_empty()
        })
    })
    .await
}

fn working(turn: &Turn, before: usize) -> bool {
    turn.messages >= before + 2 && turn.can_cancel && turn.parts > 0
}

fn turn(driver: &Entity<TheoremDriver>, cx: &mut AsyncApp) -> Turn {
    driver.read_with(cx, |driver, _| Turn::of(driver.projection()))
}

fn phase(driver: &Entity<TheoremDriver>, cx: &mut AsyncApp) -> ChatHostPhase {
    driver.read_with(cx, |driver, _| driver.projection().phase)
}
