//! Host arbitration only; whiteboard owns hit testing and every board gesture.
use super::{CAPTION, CARD, SpatialCanvas};
use gpui_kit::{
    Context, Keystroke, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ScrollWheelEvent, Window, px,
};
use gpui_whiteboard::engine::{Button, Frame, Hit, Mods};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
pub(crate) struct InputOwnership {
    pub space: Option<Keystroke>,
    pub space_dragged: bool,
    pub space_pending: bool,
    pub space_generation: u64,
    pub space_editable: bool,
    pub pending_events: Vec<gpui_kit::PlatformInput>,
    pub board_drag: bool,
    pub select_tool: bool,
}

pub(crate) type SharedInput = Rc<RefCell<InputOwnership>>;

impl InputOwnership {
    pub fn hold_space(&mut self, key: &Keystroke) {
        if self.space.is_none() {
            self.space_generation = self.space_generation.wrapping_add(1);
            self.space = Some(key.clone());
            self.space_dragged = false;
        }
    }

    pub fn cancel_space(&mut self) {
        self.space_generation = self.space_generation.wrapping_add(1);
        self.space = None;
        self.space_pending = false;
        self.space_editable = false;
        self.space_dragged = false;
        self.pending_events.clear();
    }

    pub fn release_space(&mut self) -> Option<Keystroke> {
        self.space.take().filter(|_| !self.space_dragged)
    }
}

impl SpatialCanvas {
    pub(super) fn press(
        &mut self,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page.read(cx).controls.popup.is_some() {
            return;
        }
        if self.input.borrow().space_pending {
            self.input
                .borrow_mut()
                .pending_events
                .push(gpui_kit::PlatformInput::MouseDown(ev.clone()));
            cx.stop_propagation();
            return;
        }
        let at = self.local(ev.position);
        let world = self.camera.to_world(at);
        let r = self.card();
        let inside = matches!(self.engine.hit(world), Some(Hit::Card(CARD)));
        let caption = inside && world[1] < r[1] + CAPTION as f64;
        let space = self.input.borrow().space.is_some();
        if inside && !caption && !space && !self.cached {
            return;
        }
        let pans = space || (!inside && !ev.modifiers.shift && !self.input.borrow().select_tool);
        let button = match ev.button {
            MouseButton::Left => Button::Primary,
            MouseButton::Right => Button::Secondary,
            _ => Button::Middle,
        };
        let frame = Frame {
            selection: &self.selection,
            view: self.camera,
            engaged: None,
            hovered: None,
        };
        let events = self.engine.press(
            &mut self.gesture,
            at,
            button,
            Mods {
                extend: ev.modifiers.shift,
                pan_held: pans,
                alt: false,
            },
            ev.click_count,
            &frame,
        );
        self.input.borrow_mut().board_drag = self.gesture.is_some();
        // Keep page focus during Space-pan so an un-dragged Space can still be delivered.
        if !space {
            window.focus(&self.focus, cx);
        }
        self.apply(events, cx);
        cx.stop_propagation();
    }

    pub(super) fn drag(&mut self, ev: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.input.borrow().space_pending {
            self.input
                .borrow_mut()
                .pending_events
                .push(gpui_kit::PlatformInput::MouseMove(ev.clone()));
            cx.stop_propagation();
            return;
        }
        if self.gesture.is_none() {
            return;
        }
        let at = self.local(ev.position);
        let frame = Frame {
            selection: &self.selection,
            view: self.camera,
            engaged: None,
            hovered: None,
        };
        let events = self.engine.drag_to(&mut self.gesture, at, &frame);
        if !events.is_empty() && self.input.borrow().space.is_some() {
            self.input.borrow_mut().space_dragged = true;
        }
        self.apply(events, cx);
        cx.stop_propagation();
    }

    pub(super) fn release(
        &mut self,
        ev: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.input.borrow().space_pending {
            self.input
                .borrow_mut()
                .pending_events
                .push(gpui_kit::PlatformInput::MouseUp(ev.clone()));
            cx.stop_propagation();
            return;
        }
        if self.gesture.is_none() {
            return;
        }
        let at = self.local(ev.position);
        let frame = Frame {
            selection: &self.selection,
            view: self.camera,
            engaged: None,
            hovered: None,
        };
        let cached_move = self.cached
            && matches!(
                self.gesture,
                Some(gpui_whiteboard::engine::Gesture::Move { .. })
            );
        let events = self.engine.release(&mut self.gesture, at, &frame);
        let activate = cached_move
            && !events
                .iter()
                .any(|e| matches!(e, gpui_whiteboard::engine::EngineEvent::Commit(_)));
        self.input.borrow_mut().board_drag = false;
        self.apply(events, cx);
        self.schedule_settle(cx);
        if activate {
            self.set_cached(false, window, cx);
        }
        cx.stop_propagation();
    }

    pub(super) fn wheel(&mut self, ev: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let d = ev.delta.pixel_delta(px(16.));
        if let Some(event) = self.engine.wheel(
            self.local(ev.position),
            [f32::from(d.x) as f64, f32::from(d.y) as f64],
            ev.modifiers.platform,
            &self.frame(),
        ) {
            self.apply(vec![event], cx);
        }
        cx.stop_propagation();
    }
}
