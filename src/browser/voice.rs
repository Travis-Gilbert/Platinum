//! Window-owned dictation; input/navigation invalidates its captured target.
use super::Browser;
use gpui_kit::component::{WindowExt as _, notification::Notification, speech::SpeechEvent};
use gpui_kit::{Context, Focusable as _, Window};

impl Browser {
    pub(super) fn toggle_dictation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.speech.read(cx).status().is_active() {
            self.speech.update(cx, |speech, cx| speech.stop(cx));
            return;
        }
        if self.voice_prepare.is_some() {
            return;
        }
        self.dictation_generation = self.dictation_generation.wrapping_add(1);
        let generation = self.dictation_generation;
        if let Some(page) = self.tabs[self.active]
            .page
            .clone()
            .filter(|p| p.focus_handle(cx).is_focused(window))
        {
            let view = page.read(cx).webview.clone();
            let (send, receive) = futures::channel::oneshot::channel();
            view.native_text(servo::NativeTextRequest::Snapshot, move |result| {
                let _ = send.send(result);
            });
            cx.spawn_in(window, async move |this, cx| {
                let result = crate::voice::native_result(receive, cx.background_executor()).await;
                let _ = this.update_in(cx, |this, window, cx| {
                    if this.dictation_generation != generation { return; }
                    match result {
                        Ok(Some(context)) => {
                            this.dictation_target = Some(crate::voice::Target::Web(crate::voice::WebDestination::new(view, context, window, cx)));
                            this.begin_dictation(window, cx);
                        }
                        Ok(None) => window.push_notification(Notification::info("Focus an editable text field. This editor is unavailable to dictation or contains a separate non-editable region."), cx),
                        Err(e) => window.push_notification(Notification::error(e.to_string()), cx),
                    }
                });
            }).detach();
            return;
        }
        let Some(destination) = crate::voice::Destination::capture(window, cx) else {
            window.push_notification(
                Notification::info("Focus a text field before starting dictation."),
                cx,
            );
            return;
        };
        self.dictation_target = Some(crate::voice::Target::Native(destination));
        self.begin_dictation(window, cx);
    }

    fn begin_dictation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if crate::voice::ready() {
            self.speech.update(cx, |speech, cx| speech.start(cx));
        } else {
            window.push_notification(Notification::info("Preparing local Parakeet dictation. The model is downloaded once; your audio stays on this Mac."), cx);
            self.voice_prepare = Some(cx.spawn_in(window, async move |this, cx| {
                let result = cx
                    .background_executor()
                    .spawn(async { crate::voice::prepare() })
                    .await;
                let _ = this.update_in(cx, |this, window, cx| {
                    this.voice_prepare = None;
                    match result {
                        Ok(()) if this.dictation_target.is_some() => {
                            this.speech.update(cx, |speech, cx| speech.start(cx))
                        }
                        Ok(()) => {}
                        Err(error) => {
                            this.dictation_target = None;
                            window.push_notification(
                                Notification::error(format!(
                                    "Could not prepare local dictation: {error}"
                                )),
                                cx,
                            );
                        }
                    }
                    cx.notify();
                });
            }));
        }
        cx.notify();
    }

    pub(super) fn cancel_dictation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dictation_generation = self.dictation_generation.wrapping_add(1);
        if let Some(target) = self.dictation_target.take() {
            target.cancel(window, cx);
        }
        if self.speech.read(cx).status().is_active() {
            self.speech.update(cx, |speech, cx| speech.cancel(cx));
        }
    }

    pub(super) fn speech_event(
        &mut self,
        event: &SpeechEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            SpeechEvent::Partial(text) => {
                if !self
                    .dictation_target
                    .as_mut()
                    .is_some_and(|target| target.partial(text, window, cx))
                {
                    self.cancel_dictation(window, cx);
                }
            }
            SpeechEvent::Final(text) => match self.dictation_target.take() {
                Some(crate::voice::Target::Native(mut target)) => {
                    if text.is_empty() {
                        target.cancel(window, cx);
                    } else {
                        target.apply(text, true, window, cx);
                    }
                }
                Some(crate::voice::Target::Web(target)) => target.finish(text),
                _ => {}
            },
            SpeechEvent::Cancelled => {
                self.dictation_target = None;
            }
            SpeechEvent::Error(error) => {
                self.cancel_dictation(window, cx);
                window.push_notification(Notification::error(format!("Dictation: {error}")), cx);
            }
            SpeechEvent::Started => {}
        }
        cx.notify();
    }
}

impl Browser {
    pub(super) fn speak(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.speaker.speak(text) {
            window.push_notification(Notification::error(error.to_string()), cx);
        }
    }
    pub(super) fn read_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) =
            crate::voice::Destination::selected_text(window, cx).filter(|s| !s.is_empty())
        {
            self.speak(&text, window, cx);
            return;
        }
        let Some(page) = self.tabs[self.active].page.clone() else {
            window.push_notification(
                Notification::info("Select text in a page, note, or composer to read aloud."),
                cx,
            );
            return;
        };
        let tab = self.tabs[self.active].id;
        let navigation = page.read(cx).delegate.navigation_starts.get();
        let view = page.read(cx).webview.clone();
        let (send, receive) = futures::channel::oneshot::channel();
        view.evaluate_javascript("(() => { const e = document.activeElement; const text = e && ['INPUT','TEXTAREA'].includes(e.tagName) && e.type !== 'password' ? e.value.slice(e.selectionStart, e.selectionEnd) : String(window.getSelection()); return text.slice(0, 32768); })()", move |result| { let _ = send.send(result); });
        cx.spawn_in(window, async move |this, cx| {
            use futures::FutureExt as _;
            let response = receive.fuse();
            let timeout = cx
                .background_executor()
                .timer(std::time::Duration::from_secs(3))
                .fuse();
            futures::pin_mut!(response, timeout);
            let result =
                futures::select! { result = response => Some(result), _ = timeout => None };
            let _ = this.update_in(cx, |this, window, cx| {
                if this.tabs[this.active].id != tab
                    || this.tabs[this.active].page.as_ref().is_none_or(|current| {
                        current.entity_id() != page.entity_id()
                            || current.read(cx).delegate.navigation_starts.get() != navigation
                    })
                {
                    return;
                }
                match result {
                    Some(Ok(Ok(servo::JSValue::String(text)))) => this.speak(&text, window, cx),
                    _ => window.push_notification(
                        Notification::error("Could not read the page selection"),
                        cx,
                    ),
                }
            });
        })
        .detach();
    }
    pub(super) fn voice_action(
        &mut self,
        action: theorem_surface_contracts::intent::VoiceAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use theorem_surface_contracts::intent::VoiceAction;
        match action {
            VoiceAction::Settings | VoiceAction::InstallTts | VoiceAction::DeleteModels => {
                self.voice_settings(window, cx)
            }
            VoiceAction::ReadAloud => self.read_selection(window, cx),
            VoiceAction::Stop => self.speaker.stop(),
            VoiceAction::Pause => self.speaker.pause(),
            VoiceAction::InstallStt => self.toggle_dictation(window, cx),
            VoiceAction::NextSpeed => {
                self.speaker.preferences.words_per_minute =
                    match self.speaker.preferences.words_per_minute {
                        0..=159 => 180,
                        160..=209 => 240,
                        _ => 140,
                    };
                self.schedule_save(cx);
                self.voice_settings(window, cx);
            }
            VoiceAction::NextVoice => {
                let voices = crate::spoken::Speaker::voices();
                let index = voices
                    .iter()
                    .position(|(id, _)| Some(id) == self.speaker.preferences.voice.as_ref())
                    .map(|i| i + 1)
                    .unwrap_or(0);
                self.speaker.preferences.voice = voices.get(index).map(|(id, _)| id.clone());
                self.schedule_save(cx);
                self.voice_settings(window, cx);
            }
        }
    }
    pub(super) fn voice_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_kit::component::{
            Sizable as _, StyledExt as _,
            button::{Button, ButtonVariants as _},
            h_flex, v_flex,
        };
        use gpui_kit::{
            InteractiveElement as _, ParentElement as _, SharedString,
            StatefulInteractiveElement as _, Styled as _, div, px,
        };
        let preferences = self.speaker.preferences.clone();
        let voices = crate::spoken::Speaker::voices();
        window.open_sheet(cx, move |sheet, _, _| {
            sheet.title("Voice").size(px(440.)).child(
                v_flex().gap_3().p_4()
                    .child(div().font_semibold().child("Voice input"))
                    .child("Parakeet · English · on this Mac")
                    .child("Focus a text field and press Control Shift Space. Press again to finish, or Escape to cancel. Audio stays on this Mac.")
                    .child(div().text_sm().child(if crate::voice::ready() { "Model ready" } else { "The pinned model downloads when dictation first starts." }))
                    .child(div().font_semibold().child("Spoken output"))
                    .child("Installed macOS voices")
                    .child(h_flex().gap_2().children([140,180,240].map(|rate| Button::new(SharedString::from(format!("rate:{rate}"))).small().ghost().label(format!("{rate} wpm{}​", if rate == preferences.words_per_minute { " · selected" } else { "" })).on_click(move |_, window, cx| window.dispatch_action(Box::new(crate::actions::SpeechRate { words_per_minute: rate }), cx)))))
                    .child(div().id("installed-voices").max_h(px(280.)).overflow_y_scroll().child(
                        v_flex().gap_1()
                            .child(Button::new("voice-default").small().ghost().label(if preferences.voice.is_none() { "System default · selected" } else { "System default" }).on_click(|_, window, cx| window.dispatch_action(Box::new(crate::actions::ChooseVoice { id: None }), cx)))
                            .children(voices.iter().map(|(id, name)| {
                                let id = id.clone();
                                Button::new(SharedString::from(format!("voice:{id}"))).small().ghost().label(format!("{name}{}", if preferences.voice.as_ref() == Some(&id) { " · selected" } else { "" })).on_click(move |_, window, cx| window.dispatch_action(Box::new(crate::actions::ChooseVoice { id: Some(id.clone()) }), cx))
                            }))
                    ))
                    .child(Button::new("voice-test").small().label("Read selected text").on_click(|_, window, cx| window.dispatch_action(Box::new(crate::actions::ReadSelection), cx)))
                    .child(Button::new("voice-stop").small().ghost().label("Stop speaking").on_click(|_, window, cx| window.dispatch_action(Box::new(crate::actions::StopSpeaking), cx)))
                    .child(h_flex().gap_2()
                        .child(Button::new("voice-pause").small().ghost().label("Pause").on_click(|_, window, cx| window.dispatch_action(Box::new(crate::actions::PauseSpeaking), cx)))
                        .child(Button::new("voice-resume").small().ghost().label("Resume").on_click(|_, window, cx| window.dispatch_action(Box::new(crate::actions::ResumeSpeaking), cx))))
            )
        });
    }
}
