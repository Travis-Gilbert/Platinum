use super::*;

impl Browser {
    pub(super) fn board_toolbar(&self, cx: &Context<Self>) -> impl IntoElement {
        h_flex()
            .absolute()
            .top_2()
            .left_2()
            .gap_1()
            .p_1()
            .rounded_lg()
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("board-focus")
                    .small()
                    .ghost()
                    .label("Focus page")
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(crate::actions::ShowCanvas), cx)
                    }),
            )
            .child(
                Button::new("board-overview")
                    .small()
                    .ghost()
                    .label("Overview")
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(crate::actions::OverviewCanvas), cx)
                    }),
            )
            .child(
                Button::new("board-notes")
                    .small()
                    .ghost()
                    .label("Notes")
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(crate::actions::ToggleNotes), cx)
                    }),
            )
            .child(
                Button::new("board-visit-trail")
                    .small()
                    .ghost()
                    .label("Visit trail")
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(crate::actions::ShowVisitTrail), cx)
                    }),
            )
            .child(
                Button::new("board-agent")
                    .small()
                    .ghost()
                    .label("Conversation")
                    .on_click(|_, window, cx| {
                        window.dispatch_action(
                            Box::new(crate::actions::OpenPlace {
                                place: crate::places::Place::Agent,
                            }),
                            cx,
                        )
                    }),
            )
    }
}

impl Browser {
    pub(super) fn board_operation(
        &mut self,
        id: u64,
        operation: crate::actions::CanvasOperation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::actions::CanvasOperation as Op;
        if let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) {
            match operation {
                Op::Pin => self.pin_tab(id, cx),
                Op::Duplicate => {
                    if let Some(url) = tab.url(cx) {
                        self.go(url, Some(Origin::From(id)), true, window, cx);
                    }
                }
                Op::Provenance => {
                    let opener = self
                        .tree
                        .node(id)
                        .and_then(|n| n.opener)
                        .and_then(|opener| self.tabs.iter().find(|t| t.id == opener));
                    let source = opener
                        .map(|t| format!("Opened from {}", t.title(cx)))
                        .unwrap_or_else(|| "Opened directly".into());
                    window.push_notification(
                        Notification::info(format!(
                            "{source}\nTab {id}\n{}",
                            tab.url(cx).map(|u| u.to_string()).unwrap_or_default()
                        )),
                        cx,
                    );
                }
                _ => {}
            }
        } else if let Some(note) = self.canvas.as_ref().and_then(|b| b.read(cx).note(id)) {
            let note = note.read(cx);
            window.push_notification(
                Notification::info(format!(
                    "{}\nDocument {}\nSources are retained in the note.",
                    note.path, note.id
                )),
                cx,
            );
        }
    }
    pub(super) fn paste_on_board(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(url) = frame_profile::web_url(text.trim()) {
            self.go(url, Some(Origin::Root), true, window, cx);
        } else if text.len() <= 65_536 {
            self.ensure_notes(window, cx);
            self.notes_sidebar = true;
            if let Some(notes) = &self.notebook {
                notes.update(cx, |n, cx| n.create_text(text.to_string(), window, cx));
            }
        } else {
            window.push_notification(
                Notification::error("Paste less than 64 KiB into a note at a time."),
                cx,
            );
        }
    }
}
