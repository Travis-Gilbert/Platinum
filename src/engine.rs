//! The one Servo instance, shared by every tab: built on the GPUI main
//! thread, woken through GPUI's foreground executor rather than by polling.
//!
//! SOURCING: `ServoBuilder` and Servo's `EventLoopWaker`; GPUI's foreground
//! executor and a `futures` channel carry the wake to the main thread.
//! Ported from Theorem UI's desktop host (`desktop/src/engine.rs`).

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui_kit::{App, AsyncApp, Global};
use servo::{CookieSource, EventLoopWaker, Opts, Preferences, Servo, ServoBuilder};
use url::Url;

/// Servo calls `wake` from its own threads; this only posts to the foreground.
/// `pending` folds a burst of wakes into one turn of the event loop.
#[derive(Clone)]
struct ChannelWaker {
    sender: UnboundedSender<()>,
    pending: Arc<AtomicBool>,
}

impl EventLoopWaker for ChannelWaker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(self.clone())
    }

    fn wake(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            let _ = self.sender.unbounded_send(());
        }
    }
}

pub struct Engine {
    pub servo: Servo,
    /// Runs after every turn of Servo's event loop, so the delegates' queued
    /// events reach their views once Servo has let go of them. A callback
    /// that returns false is dropped, as a closed tab's does.
    after_spin: Vec<Box<dyn FnMut(&mut App) -> bool>>,
}

impl Global for Engine {}

impl Engine {
    /// Build Servo with durable storage under `profile_dir` and start the task
    /// that turns its wake-ups into `spin_event_loop` calls.
    pub fn start(profile_dir: &Path, cx: &mut App) {
        let (sender, receiver) = unbounded();
        let pending = Arc::new(AtomicBool::new(false));
        let mut opts = Opts::default();
        opts.config_dir = Some(profile_dir.to_path_buf());
        // `:has()` is off by default in Servo, and much of the web uses it.
        let preferences = Preferences {
            layout_css_has_selector_enabled: true,
            dom_media_source_enabled: true,
            ..Preferences::default()
        };
        let servo = ServoBuilder::default()
            .opts(opts)
            .preferences(preferences)
            .event_loop_waker(Box::new(ChannelWaker {
                sender,
                pending: pending.clone(),
            }))
            .build();
        servo.setup_logging();
        cx.set_global(Engine {
            servo,
            after_spin: Vec::new(),
        });
        cx.spawn(async move |cx| spin(receiver, pending, cx).await)
            .detach();
    }

    pub fn servo(cx: &App) -> Servo {
        cx.global::<Engine>().servo.clone()
    }

    /// The `Cookie` header Servo would send to `url`, for a request Platinum
    /// makes itself: a download.
    pub fn cookie_header(url: &Url, cx: &App) -> Option<String> {
        let engine = cx.try_global::<Engine>()?;
        let cookies = engine
            .servo
            .site_data_manager()
            .cookies_for_url(url.clone(), CookieSource::HTTP);
        let header = cookies
            .iter()
            .map(|cookie| format!("{}={}", cookie.name(), cookie.value()))
            .collect::<Vec<_>>()
            .join("; ");
        (!header.is_empty()).then_some(header)
    }

    /// The user agent Servo sends, so Platinum's own requests look the same.
    pub fn user_agent() -> String {
        servo::prefs::get().user_agent.clone()
    }

    pub fn after_spin(callback: impl FnMut(&mut App) -> bool + 'static, cx: &mut App) {
        cx.global_mut::<Engine>()
            .after_spin
            .push(Box::new(callback));
    }
}

async fn spin(mut wakes: UnboundedReceiver<()>, pending: Arc<AtomicBool>, cx: &mut AsyncApp) {
    while wakes.next().await.is_some() {
        pending.store(false, Ordering::Release);
        cx.update(|cx| {
            let servo = Engine::servo(cx);
            servo.spin_event_loop();
            // Take the callbacks out so they can reach `cx` mutably.
            let mut callbacks = std::mem::take(&mut cx.global_mut::<Engine>().after_spin);
            callbacks.retain_mut(|callback| callback(cx));
            let engine = cx.global_mut::<Engine>();
            callbacks.append(&mut engine.after_spin);
            engine.after_spin = callbacks;
        });
    }
}
