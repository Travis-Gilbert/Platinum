//! Which form the one Theo window has, where it sits in each form, and what
//! the native window must do when the person asks for another.
//!
//! The presence and the workspace are presentations of the same session, so
//! a change of form never copies or recreates anything: it moves one window
//! and changes which views observe the session. The body's change between
//! the two is `theo_body::morph::Morph` (Platinum PR #25), which already
//! reverses mid-flight and completes the same lifecycle with motion reduced.
//!
//! The window is moved once, at the start of an expansion (to the workspace
//! frame) and once at the end of a collapse (back to the presence frame);
//! between the two only the content inside it animates. A platform that
//! moves windows at once therefore loses nothing.
//!
//! `step` returns effects and performs none, as Rune's own state machines do
//! (`rune::machine`); `platform::apply` turns them into window calls.

use serde::{Deserialize, Serialize};
use theo_body::morph::{Morph, Settled};

/// A window frame: its top-left on the desktop in physical pixels (the space
/// Rune's `WindowHandle::set_position` takes) and its size in points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub x: i32,
    pub y: i32,
    pub w: f32,
    pub h: f32,
}

/// The presence's size: the face and room for its travel, as the GPUI desk
/// measures it (`theo_desk::body::BODY_SIZE`).
pub const PRESENCE_SIZE: (f32, f32) = (264.0, 132.0);
/// The workspace's first size, and the least it may be resized to.
pub const WORKSPACE_SIZE: (f32, f32) = (1120.0, 720.0);
pub const WORKSPACE_MIN: (f32, f32) = (560.0, 380.0);

/// The form the person last chose; restored on launch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Form {
    #[default]
    Presence,
    Workspace,
}

/// Who asked. Only the person, or a restore of what the person chose, may
/// change the form or take focus; background activity never does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    User,
    Restore,
    Background,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Request {
    /// Show Theo where it was, in the form it had.
    Summon,
    /// Hide Theo. Nothing it holds or runs stops.
    Dismiss,
    Expand,
    Collapse,
    /// The person moved or resized the window to this frame.
    Moved(Frame),
}

/// What the host must do. Applied in order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    /// Show the window; `activate` brings Theo to the front and makes it key.
    Show {
        activate: bool,
    },
    Hide,
    /// Move and size the window at once (the content does any animating).
    Place(Frame),
    /// Float above other windows: the presence does, the workspace does not.
    Float(bool),
    /// Put keyboard focus in the form's primary input.
    Focus(Form),
    /// Give the keyboard back to the application that had it.
    ReturnFocus,
    /// The frames or the form changed: write the preferences.
    Persist,
}

/// Why a request changed nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Background activity may not change the form.
    NotTheUser,
    AlreadyThere,
    Hidden,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Presentation {
    morph: Morph,
    visible: bool,
    presence: Frame,
    workspace: Frame,
    /// Whether to focus the form's input once its change settles.
    focus_on_settle: bool,
}

pub type Outcome = Result<Vec<Effect>, Refusal>;

impl Presentation {
    /// A presentation in `form`, hidden, at the given frames.
    pub fn new(form: Form, presence: Frame, workspace: Frame) -> Self {
        let mut morph = Morph::default();
        if form == Form::Workspace {
            morph.open(0.0, true);
            morph.tick(0.0);
        }
        Self {
            morph,
            visible: false,
            presence,
            workspace,
            focus_on_settle: false,
        }
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    /// The form the presentation is in or heading for.
    pub fn form(&self) -> Form {
        use theo_body::morph::Form as Body;
        match self.morph.form() {
            Body::Face | Body::Closing { .. } => Form::Presence,
            Body::Opening { .. } | Body::Screen => Form::Workspace,
        }
    }

    /// 0 the presence, 1 the workspace, between while changing.
    pub fn progress(&self, now: f64) -> f64 {
        self.morph.progress(now)
    }

    /// Whether the workspace is settled and takes input.
    pub fn workspace_interactive(&self) -> bool {
        self.morph.interactive()
    }

    /// Whether anything of the workspace is drawn: settled, opening or closing.
    pub fn shows_workspace(&self) -> bool {
        self.morph.shows_screen()
    }

    pub fn presence_frame(&self) -> Frame {
        self.presence
    }

    pub fn workspace_frame(&self) -> Frame {
        self.workspace
    }

    /// The frame the window has in the current form.
    pub fn frame(&self) -> Frame {
        if self.shows_workspace() {
            self.workspace
        } else {
            self.presence
        }
    }

    /// Takes a request at `now` (milliseconds); `still` is reduced motion.
    pub fn step(&mut self, request: Request, cause: Cause, now: f64, still: bool) -> Outcome {
        match request {
            Request::Summon => self.summon(cause),
            Request::Dismiss => self.dismiss(cause),
            Request::Expand => self.expand(cause, now, still),
            Request::Collapse => self.collapse(cause, now, still),
            Request::Moved(frame) => Ok(self.moved(frame)),
        }
    }

    fn summon(&mut self, cause: Cause) -> Outcome {
        if self.visible {
            return Err(Refusal::AlreadyThere);
        }
        self.visible = true;
        let activate = cause != Cause::Background;
        let mut effects = vec![
            Effect::Place(self.frame()),
            Effect::Float(!self.shows_workspace()),
            Effect::Show { activate },
        ];
        if activate {
            effects.push(Effect::Focus(self.form()));
        }
        Ok(effects)
    }

    fn dismiss(&mut self, cause: Cause) -> Outcome {
        if cause == Cause::Background {
            return Err(Refusal::NotTheUser);
        }
        if !self.visible {
            return Err(Refusal::Hidden);
        }
        self.visible = false;
        Ok(vec![Effect::Hide, Effect::ReturnFocus])
    }

    fn expand(&mut self, cause: Cause, now: f64, still: bool) -> Outcome {
        if cause == Cause::Background {
            return Err(Refusal::NotTheUser);
        }
        if self.form() == Form::Workspace {
            return Err(Refusal::AlreadyThere);
        }
        // A collapse under way turns back; the window has not moved yet.
        let window_moves = !self.shows_workspace();
        self.morph.open(now, still);
        self.focus_on_settle = true;
        let mut effects = Vec::new();
        if window_moves {
            effects.extend([Effect::Place(self.workspace), Effect::Float(false)]);
        }
        if !self.visible {
            self.visible = true;
            effects.push(Effect::Show { activate: true });
        }
        effects.push(Effect::Persist);
        Ok(effects)
    }

    fn collapse(&mut self, cause: Cause, now: f64, still: bool) -> Outcome {
        if cause == Cause::Background {
            return Err(Refusal::NotTheUser);
        }
        if self.form() == Form::Presence {
            return Err(Refusal::AlreadyThere);
        }
        self.morph.close(now, false, still);
        self.focus_on_settle = cause == Cause::User;
        Ok(vec![Effect::Persist])
    }

    fn moved(&mut self, frame: Frame) -> Vec<Effect> {
        // Mid-change the window is the workspace's frame, sized for the
        // animation; only a settled form's frame is the person's choice.
        match self.morph.form() {
            theo_body::morph::Form::Face => self.presence = frame,
            theo_body::morph::Form::Screen => self.workspace = frame,
            _ => return Vec::new(),
        }
        vec![Effect::Persist]
    }

    /// Moves time on; returns what the host must do now that a change settled.
    pub fn tick(&mut self, now: f64) -> Vec<Effect> {
        let Some(settled) = self.morph.tick(now) else {
            return Vec::new();
        };
        let focus = std::mem::take(&mut self.focus_on_settle);
        let mut effects = match settled {
            Settled::Opened => Vec::new(),
            Settled::Collapsed | Settled::Closed => {
                vec![Effect::Place(self.presence), Effect::Float(true)]
            }
        };
        if focus && self.visible {
            effects.push(Effect::Focus(self.form()));
        }
        effects
    }
}

/// Where the presence sits inside the workspace window, in points from the
/// window's top-left: so the body grows out of the face where it was.
/// Kept inside the window when the two frames do not overlap.
pub fn presence_in_workspace(presence: Frame, workspace: Frame, scale: f64) -> (f32, f32) {
    let x = ((presence.x - workspace.x) as f64 / scale) as f32;
    let y = ((presence.y - workspace.y) as f64 / scale) as f32;
    (
        x.clamp(0.0, (workspace.w - presence.w).max(0.0)),
        y.clamp(0.0, (workspace.h - presence.h).max(0.0)),
    )
}

/// A workspace frame centred on the presence and kept on the display, for a
/// first expansion with no frame the person chose.
pub fn workspace_around(presence: Frame, display: Frame, scale: f64) -> Frame {
    let w = WORKSPACE_SIZE.0.min(display.w);
    let h = WORKSPACE_SIZE.1.min(display.h);
    let px = |points: f32| (points as f64 * scale).round() as i32;
    let centre_x = presence.x + px(presence.w) / 2;
    let centre_y = presence.y + px(presence.h) / 2;
    let x = (centre_x - px(w) / 2).clamp(display.x, display.x + px(display.w) - px(w));
    let y = (centre_y - px(h) / 2).clamp(display.y, display.y + px(display.h) - px(h));
    Frame { x, y, w, h }
}

/// The presence's home on a display: its bottom right, a margin in.
pub fn presence_home(display: Frame, scale: f64) -> Frame {
    let px = |points: f32| (points as f64 * scale).round() as i32;
    let margin = px(24.0);
    Frame {
        x: display.x + px(display.w) - px(PRESENCE_SIZE.0) - margin,
        y: display.y + px(display.h) - px(PRESENCE_SIZE.1) - margin,
        w: PRESENCE_SIZE.0,
        h: PRESENCE_SIZE.1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theo_body::morph::{CLOSE_MS, OPEN_MS};

    const PRESENCE: Frame = Frame {
        x: 2400,
        y: 1500,
        w: 264.0,
        h: 132.0,
    };
    const WORKSPACE: Frame = Frame {
        x: 400,
        y: 200,
        w: 1120.0,
        h: 720.0,
    };

    fn shown() -> Presentation {
        let mut p = Presentation::new(Form::Presence, PRESENCE, WORKSPACE);
        p.step(Request::Summon, Cause::User, 0.0, false).unwrap();
        p
    }

    #[test]
    fn summoning_shows_the_presence_floating_and_focused() {
        let mut p = Presentation::new(Form::Presence, PRESENCE, WORKSPACE);
        let effects = p.step(Request::Summon, Cause::User, 0.0, false).unwrap();
        assert_eq!(
            effects,
            vec![
                Effect::Place(PRESENCE),
                Effect::Float(true),
                Effect::Show { activate: true },
                Effect::Focus(Form::Presence),
            ]
        );
        assert!(p.visible());
    }

    #[test]
    fn a_background_summon_shows_without_taking_the_keyboard() {
        let mut p = Presentation::new(Form::Presence, PRESENCE, WORKSPACE);
        let effects = p
            .step(Request::Summon, Cause::Background, 0.0, false)
            .unwrap();
        assert!(effects.contains(&Effect::Show { activate: false }));
        assert!(!effects.iter().any(|e| matches!(e, Effect::Focus(_))));
    }

    #[test]
    fn expanding_moves_the_window_once_then_focuses_when_settled() {
        let mut p = shown();
        let effects = p.step(Request::Expand, Cause::User, 1000.0, false).unwrap();
        assert_eq!(effects[0], Effect::Place(WORKSPACE));
        assert_eq!(effects[1], Effect::Float(false));
        assert!(p.shows_workspace() && !p.workspace_interactive());
        assert_eq!(p.tick(1000.0 + OPEN_MS / 2.0), vec![]);
        assert_eq!(
            p.tick(1000.0 + OPEN_MS),
            vec![Effect::Focus(Form::Workspace)]
        );
        assert!(p.workspace_interactive());
    }

    #[test]
    fn collapsing_moves_the_window_back_only_once_the_body_is_home() {
        let mut p = shown();
        p.step(Request::Expand, Cause::User, 0.0, true).unwrap();
        p.tick(0.0);
        let effects = p.step(Request::Collapse, Cause::User, 10.0, false).unwrap();
        assert_eq!(effects, vec![Effect::Persist]);
        assert_eq!(
            p.frame(),
            WORKSPACE,
            "the window stays put while the body shrinks"
        );
        let settled = p.tick(10.0 + CLOSE_MS);
        assert_eq!(
            settled,
            vec![
                Effect::Place(PRESENCE),
                Effect::Float(true),
                Effect::Focus(Form::Presence)
            ]
        );
    }

    #[test]
    fn a_reversed_collapse_does_not_move_the_window_again() {
        let mut p = shown();
        p.step(Request::Expand, Cause::User, 0.0, true).unwrap();
        p.tick(0.0);
        p.step(Request::Collapse, Cause::User, 10.0, false).unwrap();
        let effects = p.step(Request::Expand, Cause::User, 50.0, false).unwrap();
        assert!(!effects.iter().any(|e| matches!(e, Effect::Place(_))));
        assert!(p.progress(50.0) > 0.5, "it turns back from where it got to");
    }

    #[test]
    fn reduced_motion_completes_the_same_lifecycle() {
        let mut p = shown();
        p.step(Request::Expand, Cause::User, 0.0, true).unwrap();
        assert_eq!(p.progress(0.0), 1.0);
        assert_eq!(p.tick(0.0), vec![Effect::Focus(Form::Workspace)]);
        p.step(Request::Collapse, Cause::User, 5.0, true).unwrap();
        assert_eq!(p.tick(5.0)[0], Effect::Place(PRESENCE));
    }

    #[test]
    fn background_activity_never_changes_the_form_or_hides_theo() {
        let mut p = shown();
        for request in [Request::Expand, Request::Dismiss] {
            assert_eq!(
                p.step(request, Cause::Background, 0.0, false),
                Err(Refusal::NotTheUser)
            );
        }
        p.step(Request::Expand, Cause::User, 0.0, true).unwrap();
        p.tick(0.0);
        assert_eq!(
            p.step(Request::Collapse, Cause::Background, 1.0, false),
            Err(Refusal::NotTheUser)
        );
        assert!(p.workspace_interactive());
    }

    #[test]
    fn each_form_remembers_its_own_frame() {
        let mut p = shown();
        let dragged = Frame {
            x: 100,
            y: 120,
            ..PRESENCE
        };
        assert_eq!(
            p.step(Request::Moved(dragged), Cause::User, 0.0, false),
            Ok(vec![Effect::Persist])
        );
        p.step(Request::Expand, Cause::User, 0.0, true).unwrap();
        p.tick(0.0);
        let resized = Frame {
            w: 1400.0,
            ..WORKSPACE
        };
        p.step(Request::Moved(resized), Cause::User, 1.0, false)
            .unwrap();
        assert_eq!(p.presence_frame(), dragged);
        assert_eq!(p.workspace_frame(), resized);
        // Mid-change frames are the animation's, not the person's.
        p.step(Request::Collapse, Cause::User, 2.0, false).unwrap();
        p.step(
            Request::Moved(Frame { x: 0, ..resized }),
            Cause::User,
            3.0,
            false,
        )
        .unwrap();
        assert_eq!(p.workspace_frame(), resized);
    }

    #[test]
    fn dismissing_hides_and_returns_the_keyboard_and_summon_restores_the_form() {
        let mut p = shown();
        p.step(Request::Expand, Cause::User, 0.0, true).unwrap();
        p.tick(0.0);
        assert_eq!(
            p.step(Request::Dismiss, Cause::User, 1.0, false),
            Ok(vec![Effect::Hide, Effect::ReturnFocus])
        );
        let effects = p.step(Request::Summon, Cause::User, 2.0, false).unwrap();
        assert_eq!(effects[0], Effect::Place(WORKSPACE));
        assert_eq!(effects.last(), Some(&Effect::Focus(Form::Workspace)));
    }

    #[test]
    fn a_workspace_restored_on_launch_starts_settled() {
        let p = Presentation::new(Form::Workspace, PRESENCE, WORKSPACE);
        assert!(p.workspace_interactive());
        assert_eq!(p.frame(), WORKSPACE);
    }

    #[test]
    fn the_face_grows_out_of_where_it_was_and_stays_in_the_window() {
        let inside = presence_in_workspace(
            Frame {
                x: 800,
                y: 600,
                ..PRESENCE
            },
            WORKSPACE,
            2.0,
        );
        assert_eq!(inside, (200.0, 200.0));
        let far = presence_in_workspace(PRESENCE, WORKSPACE, 2.0);
        assert_eq!(far, (WORKSPACE.w - PRESENCE.w, WORKSPACE.h - PRESENCE.h));
    }

    #[test]
    fn a_first_workspace_is_centred_on_the_presence_and_kept_on_the_display() {
        let display = Frame {
            x: 0,
            y: 0,
            w: 1512.0,
            h: 945.0,
        };
        let home = presence_home(display, 2.0);
        assert_eq!(home.x + (264 * 2) + 48, 3024);
        let ws = workspace_around(home, display, 2.0);
        assert_eq!(ws.x + (ws.w as i32 * 2), 3024, "kept on the right edge");
        assert_eq!(ws.y + (ws.h as i32 * 2), 1890, "and the bottom");
    }
}
