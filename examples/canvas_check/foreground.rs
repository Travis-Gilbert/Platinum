use super::*;
use gpui_kit::{Subscription, Window};

struct Observers {
    _subscriptions: Vec<Subscription>,
}

pub struct Guard {
    _observers: Entity<Observers>,
    state: Rc<RefCell<State>>,
}

struct State {
    started: Instant,
    dir: PathBuf,
    allow_panels: bool,
    events: Vec<Value>,
    failure: Option<String>,
    polls: u64,
    last_poll: Instant,
    max_poll_gap_ms: f64,
}

impl State {
    fn sample(&mut self, source: &str, window: &Window) {
        let panel = self.allow_panels && own_key_panel();
        let active = window.is_window_active();
        let key = appkit_key(window);
        let visible = window.is_visible();
        if source == "poll" {
            self.polls += 1;
            self.max_poll_gap_ms = self
                .max_poll_gap_ms
                .max(self.last_poll.elapsed().as_secs_f64() * 1000.);
            self.last_poll = Instant::now();
        }
        if source != "poll" || !visible || (!key && !panel) || key != active {
            self.events.push(
                json!({"elapsed_ms":self.started.elapsed().as_secs_f64()*1000.,
                "source":source,"active":active,"appkit_key":key,"visibility":format!("{:?}",window.visibility()),
                "visible":visible,"owned_key_panel":panel}),
            );
        }
        if self.failure.is_none() && (!visible || (!key && !panel)) {
            self.failure = Some("needs the foreground: window inactive, hidden or occluded; run stopped without retry".into());
        }
    }

    fn save(&self) -> anyhow::Result<()> {
        record(
            &self.dir,
            "foreground.json",
            &json!({"events":self.events,
            "polls":self.polls,"max_poll_gap_ms":self.max_poll_gap_ms,"failure":self.failure,
            "monitor":"GPUI activation/visibility observers plus 16 ms polling of AppKit's key window; failures latch",
            "allow_owned_native_panel_focus":self.allow_panels}),
        )
    }
}

/// AppKit's own answer for this window. GPUI's `is_window_active` is set by
/// a deferred task, so it trails a key change by a turn of the executor.
fn appkit_key(window: &Window) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Some(mtm) = objc2::MainThreadMarker::new() else {
        return false;
    };
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return false;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return false;
    };
    // SAFETY: the handle's view is GPUI's, alive as long as the window, and
    // this is the main thread, where AppKit's views are used.
    let view: &objc2_app_kit::NSView = unsafe { handle.ns_view.cast().as_ref() };
    objc2_app_kit::NSApplication::sharedApplication(mtm).isActive()
        && view.window().is_some_and(|window| window.isKeyWindow())
}

fn own_key_panel() -> bool {
    let Some(mtm) = objc2::MainThreadMarker::new() else {
        return false;
    };
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    app.isActive()
        && app.keyWindow().is_some_and(|window| {
            window.isVisible()
                && (window
                    .downcast_ref::<objc2_app_kit::NSColorPanel>()
                    .is_some()
                    || window
                        .downcast_ref::<objc2_app_kit::NSOpenPanel>()
                        .is_some())
        })
}

impl Guard {
    pub fn new(
        window: NativeWindow,
        dir: PathBuf,
        allow_panels: bool,
        cx: &mut AsyncApp,
    ) -> anyhow::Result<Self> {
        let state = Rc::new(RefCell::new(State {
            started: Instant::now(),
            dir,
            allow_panels,
            events: Vec::new(),
            failure: None,
            polls: 0,
            last_poll: Instant::now(),
            max_poll_gap_ms: 0.,
        }));
        let observers = window.update(cx, |_, w, cx| {
            cx.new(|cx| {
                state.borrow_mut().sample("start", w);
                let activation = state.clone();
                let visibility = state.clone();
                Observers {
                    _subscriptions: vec![
                        cx.observe_window_activation(w, move |_, w, cx| {
                            // AppKit finishes transferring focus to its own panel before checking it.
                            if allow_panels {
                                let state = activation.clone();
                                w.defer(cx, move |w, _| state.borrow_mut().sample("activation", w));
                            } else {
                                activation.borrow_mut().sample("activation", w);
                            }
                        }),
                        cx.observe_window_visibility(w, move |_, _, w, _| {
                            visibility.borrow_mut().sample("visibility", w)
                        }),
                    ],
                }
            })
        })?;
        state.borrow().save()?;
        Ok(Self {
            _observers: observers,
            state,
        })
    }

    pub async fn watch(&self, window: NativeWindow, cx: &mut AsyncApp) -> anyhow::Result<bool> {
        loop {
            window.update(cx, |_, w, _| self.state.borrow_mut().sample("poll", w))?;
            if let Some(error) = self.state.borrow().failure.clone() {
                self.state.borrow().save()?;
                anyhow::bail!(error);
            }
            pause(cx, 16).await;
        }
    }

    pub fn finish(&self) -> anyhow::Result<()> {
        self.state.borrow().save()?;
        if let Some(error) = &self.state.borrow().failure {
            anyhow::bail!(error.clone());
        }
        Ok(())
    }
}
