//! Theo's screen form as numbers: where the window goes when the face opens
//! into a screen, how the plate, the eyes and the content backing move
//! between the two forms, and the life of the surface session through
//! opening, collapse, hiding and close.
//!
//! Nothing here draws or touches a window; `screen.rs` does both from these
//! answers. The live page is never resized by the morph: the shell and a
//! still of the page move, and the page is shown again, at its settled size,
//! once the screen has settled.
//!
//! The face and the screen are two `shape::Shape`s and the change between
//! them one `shape::Transition`; another form of Theo's body is another
//! shape, and blends with these the same way.
//!
//! SOURCING: plain arithmetic over `mac::Rect` and `shape`. The activity is
//! `presence`'s own type.

use presence::{Activity, Phase, Signal, Standing};

use super::mac::Rect;
pub use super::shape::Area;
use super::shape::{Fill, Part, Shape, Span, Transition};
pub use crate::delegate::Playback;

/// The screen's bezel: thin at the sides and bottom, and at the top as tall
/// as the controls it carries, in points.
pub const BEZEL_SIDE: f64 = 6.0;
pub const BEZEL_TOP: f64 = 34.0;
pub const BEZEL_BOTTOM: f64 = 6.0;
/// The screen's outer corner and the content backing's corner, in points.
pub const SCREEN_RADIUS: f64 = 16.0;
pub const BACKING_RADIUS: f64 = 10.0;
/// The plate's corner in the face form, as `body.rs` draws it.
pub const PLATE_RADIUS: f64 = 21.6;
/// The screen's first size, and the least it may be resized to, in points.
pub const SCREEN_SIZE: (f64, f64) = (800.0, 520.0);
pub const SCREEN_MIN: (f64, f64) = (360.0, 240.0);
/// How long the face takes to open into the screen, and to close back, in
/// milliseconds.
pub const OPEN_MS: f64 = 480.0;
pub const CLOSE_MS: f64 = 360.0;
/// The two halves of the backing overlap by this much past the middle, so
/// the eyes meet as one surface rather than two touching ones.
const MEET: f64 = 2.0;

/// `t` from where `from` begins to where `to` ends, held between 0 and 1.
fn span(t: f64, from: f64, to: f64) -> f64 {
    ((t - from) / (to - from)).clamp(0.0, 1.0)
}

// Where the windows go ----------------------------------------------------------

/// The screen's window, `size` in all, opened from the face's window `face`:
/// centred on the face where it can be, never larger than the visible
/// frame `visible`, and kept on it. AppKit's coordinates, y up.
pub fn screen_frame(face: Rect, visible: Rect, size: (f64, f64)) -> Rect {
    let w = size.0.max(SCREEN_MIN.0).min(visible.w);
    let h = size.1.max(SCREEN_MIN.1).min(visible.h);
    let x = face.x + face.w / 2.0 - w / 2.0;
    let y = face.y + face.h / 2.0 - h / 2.0;
    Rect {
        x: x.clamp(visible.x, visible.x + visible.w - w),
        y: y.clamp(visible.y, visible.y + visible.h - h),
        w,
        h,
    }
}

/// Where the face is inside the screen's window: from the window's top
/// left, y down, as GPUI lays it out.
pub fn face_in_window(screen: Rect, face: Rect) -> Area {
    Area {
        x: face.x - screen.x,
        y: (screen.y + screen.h) - (face.y + face.h),
        w: face.w,
        h: face.h,
    }
}

/// Where the face's window goes when the screen at `screen` closes: where
/// the face sat inside the screen's window (`inside`, as `face_in_window`
/// gave it, scaled with the screen if it was resized), kept on `visible`.
/// So the face comes back where the screen is, moved or not.
pub fn face_frame(screen: Rect, opened: (f64, f64), inside: Area, visible: Rect) -> Rect {
    // The face keeps its place as a fraction of the screen, so a screen made
    // smaller does not leave it hanging past its edge.
    let fx = if opened.0 > inside.w {
        inside.x / (opened.0 - inside.w)
    } else {
        0.5
    };
    let fy = if opened.1 > inside.h {
        inside.y / (opened.1 - inside.h)
    } else {
        0.5
    };
    let x = screen.x + (screen.w - inside.w).max(0.0) * fx.clamp(0.0, 1.0);
    let top = (screen.h - inside.h).max(0.0) * fy.clamp(0.0, 1.0);
    let y = screen.y + screen.h - top - inside.h;
    Rect {
        x: x.clamp(visible.x, (visible.x + visible.w - inside.w).max(visible.x)),
        y: y.clamp(visible.y, (visible.y + visible.h - inside.h).max(visible.y)),
        w: inside.w,
        h: inside.h,
    }
}

/// Where a window's bottom-left corner sits as a fraction of the visible
/// frame, as `mac::spot_of` measures a resting place.
pub fn spot(frame: Rect, visible: Rect) -> (f64, f64) {
    (
        ((frame.x - visible.x) / (visible.w - frame.w).max(1.0)).clamp(0.0, 1.0),
        ((frame.y - visible.y) / (visible.h - frame.h).max(1.0)).clamp(0.0, 1.0),
    )
}

// The shell between the forms ------------------------------------------------

/// The content backing: the screen's window less its bezel.
pub fn backing(size: (f64, f64)) -> Area {
    Area {
        x: BEZEL_SIDE,
        y: BEZEL_TOP,
        w: (size.0 - 2.0 * BEZEL_SIDE).max(0.0),
        h: (size.1 - BEZEL_TOP - BEZEL_BOTTOM).max(0.0),
    }
}

/// The parts of Theo's body, by name.
pub const BODY: &str = "body";
pub const FACE: &str = "face";
pub const EYE_LEFT: &str = "eye.left";
pub const EYE_RIGHT: &str = "eye.right";
pub const STILL: &str = "still";
pub const BAR: &str = "screen.bar";

/// The bar along the screen's top, in a window `size` big.
pub fn bar(size: (f64, f64)) -> Area {
    Area {
        x: BEZEL_SIDE,
        y: 0.0,
        w: (size.0 - 2.0 * BEZEL_SIDE).max(0.0),
        h: BEZEL_TOP,
    }
}

/// The face as a shape: its dark plate, its own drawing at `face`, and the
/// eyes, not yet shown as surfaces, where its matter draws them.
pub fn face_shape(face: Area, plate: Area, eyes: [Area; 2]) -> Shape {
    let eye_radius = eyes[0].w.min(eyes[0].h).min(eyes[1].w).min(eyes[1].h) / 2.0;
    let over = eyes[0].union(eyes[1]);
    Shape::new([
        Part::new(BODY, plate, PLATE_RADIUS, Fill::Ink),
        Part::new(FACE, face, 0.0, Fill::Face),
        Part::new(EYE_LEFT, eyes[0], eye_radius, Fill::Backing).alpha(0.0),
        Part::new(EYE_RIGHT, eyes[1], eye_radius, Fill::Backing).alpha(0.0),
        Part::new(STILL, over, eye_radius, Fill::Still).alpha(0.0),
        Part::new(
            BAR,
            Area { h: 0.0, ..plate },
            0.0,
            Fill::Controls(BAR.into()),
        )
        .alpha(0.0),
    ])
}

/// The screen as a shape, in a window `size` big: the body is the bezel,
/// the eyes are the two halves of the content backing, met in the middle,
/// with the still of the page over them and the bar along the top.
pub fn screen_shape(size: (f64, f64)) -> Shape {
    let inner = backing(size);
    let half = inner.w / 2.0;
    let left = Area {
        w: half + MEET,
        ..inner
    };
    let right = Area {
        x: inner.x + half - MEET,
        w: inner.w - half + MEET,
        ..inner
    };
    let outer = Area {
        x: 0.0,
        y: 0.0,
        w: size.0,
        h: size.1,
    };
    Shape::new([
        Part::new(BODY, outer, SCREEN_RADIUS, Fill::Ink),
        Part::new(EYE_LEFT, left, BACKING_RADIUS, Fill::Backing),
        Part::new(EYE_RIGHT, right, BACKING_RADIUS, Fill::Backing),
        Part::new(STILL, inner, BACKING_RADIUS, Fill::Still),
        Part::new(BAR, bar(size), 0.0, Fill::Controls(BAR.into())),
    ])
}

/// How the face opens into the screen: the body first, the eyes a moment
/// after it, broadening as the face's matter fades; the still once the
/// backing is mostly there, and the bar last. Closing runs it backwards.
pub fn opening() -> Transition {
    Transition::default()
        .time(BODY, Span::WHOLE, Span::WHOLE)
        .time(FACE, Span::WHOLE, Span::new(0.0, 0.3))
        .time(EYE_LEFT, Span::new(0.08, 1.0), Span::new(0.0, 0.3))
        .time(EYE_RIGHT, Span::new(0.08, 1.0), Span::new(0.0, 0.3))
        .time(STILL, Span::new(0.08, 1.0), Span::new(0.6, 0.6))
        .time(BAR, Span::new(0.8, 1.0), Span::new(0.8, 1.0))
}

/// The eyes as the face's matter draws them: the bright grains either side
/// of the plate's centre, each side's bounds, in window points. `grains` is
/// the face's frame (`stride` floats a grain: x, y, radius, then colour and
/// alpha at 7), `unit` points to a face unit and `centre` the plate's centre.
/// None when a side has no bright grain, as in a form with no eyes.
pub fn eye_boxes(
    grains: &[f32],
    stride: usize,
    unit: f64,
    centre: (f64, f64),
) -> Option<[Area; 2]> {
    let mut sides: [Option<Area>; 2] = [None, None];
    for grain in grains.chunks_exact(stride) {
        if grain[7] < 0.35 {
            continue;
        }
        let (x, y) = (f64::from(grain[0]) * unit, f64::from(grain[1]) * unit);
        let r = (f64::from(grain[2]) * unit).max(0.5);
        let dot = Area {
            x: centre.0 + x - r,
            y: centre.1 + y - r,
            w: 2.0 * r,
            h: 2.0 * r,
        };
        let side = &mut sides[usize::from(x > 0.0)];
        *side = Some(side.map_or(dot, |seen| seen.union(dot)));
    }
    Some([sides[0]?, sides[1]?])
}

/// Two eyes where the face's matter shows none: either side of the plate's
/// centre, a third of the plate apart.
pub fn default_eyes(plate: Area) -> [Area; 2] {
    let (cx, cy) = plate.centre();
    let (w, h) = (plate.w * 0.16, plate.h * 0.34);
    let gap = plate.w / 6.0;
    [
        Area {
            x: cx - gap - w / 2.0,
            y: cy - h / 2.0,
            w,
            h,
        },
        Area {
            x: cx + gap - w / 2.0,
            y: cy - h / 2.0,
            w,
            h,
        },
    ]
}

// The form over time ---------------------------------------------------------

/// Which form Theo's body has, and where a change between them stands.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Form {
    /// The face; the surface, if there is one, is collapsed.
    #[default]
    Face,
    /// Opening into the screen since `since` (milliseconds).
    Opening { since: f64 },
    /// The screen, settled and interactive.
    Screen,
    /// Closing back into the face since `since`; `close` ends the surface
    /// session once the face is back.
    Closing { since: f64, close: bool },
}

/// A change of form that has just finished.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Settled {
    Opened,
    Collapsed,
    Closed,
}

/// The morph between the face and the screen. With motion reduced the form
/// changes at once; nothing else about it changes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Morph {
    form: Form,
}

impl Morph {
    pub fn form(&self) -> Form {
        self.form
    }

    /// How far toward the screen the body is at `now`: 0 the face, 1 the screen.
    pub fn progress(&self, now: f64) -> f64 {
        match self.form {
            Form::Face => 0.0,
            Form::Screen => 1.0,
            Form::Opening { since } => span(now, since, since + OPEN_MS),
            Form::Closing { since, .. } => 1.0 - span(now, since, since + CLOSE_MS),
        }
    }

    /// Begins opening into the screen. A close under way turns back from
    /// where it has got to.
    pub fn open(&mut self, now: f64, still: bool) {
        let at = self.progress(now);
        self.form = match self.form {
            Form::Screen | Form::Opening { .. } => return,
            _ if still => Form::Screen,
            _ => Form::Opening {
                since: now - at * OPEN_MS,
            },
        };
    }

    /// Begins closing back into the face; `close` ends the session when the
    /// face is back. An opening under way turns back from where it has got to.
    pub fn close(&mut self, now: f64, close: bool, still: bool) {
        let at = self.progress(now);
        self.form = match self.form {
            Form::Face => return,
            Form::Closing { since, close: was } => Form::Closing {
                since,
                close: was || close,
            },
            _ if still => Form::Closing {
                since: now - CLOSE_MS,
                close,
            },
            _ => Form::Closing {
                since: now - (1.0 - at) * CLOSE_MS,
                close,
            },
        };
    }

    /// Moves time on to `now`, and says which change has just finished.
    pub fn tick(&mut self, now: f64) -> Option<Settled> {
        match self.form {
            Form::Opening { since } if now >= since + OPEN_MS => {
                self.form = Form::Screen;
                Some(Settled::Opened)
            }
            Form::Closing { since, close } if now >= since + CLOSE_MS => {
                self.form = Form::Face;
                Some(if close {
                    Settled::Closed
                } else {
                    Settled::Collapsed
                })
            }
            _ => None,
        }
    }

    /// Whether the body is anything but the face now.
    pub fn shows_screen(&self) -> bool {
        self.form != Form::Face
    }

    /// Whether the live page is shown and takes input: only once settled.
    pub fn interactive(&self) -> bool {
        self.form == Form::Screen
    }
}

// The surface session --------------------------------------------------------

/// The activity id the screen's sound goes by on the face.
pub const SOUND_ACTIVITY: &str = "theo-screen:sound";
/// How long one measurement of the sound stands for, in milliseconds.
pub const LEVEL_FRESH_MS: f64 = 150.0;

/// One page session in Theo's screen. `P` is the page; the session keeps it
/// through opening, collapse, hiding and form changes, and lets go of it
/// only when closed. Stopping the sound is the page's own business: the
/// session keeps the page for it too.
#[derive(Debug)]
pub struct Session<P> {
    page: Option<P>,
    /// One for each page the session has held, so a stale report about an
    /// earlier page is refused.
    generation: u64,
    revision: u64,
    pub morph: Morph,
    pub pinned: bool,
    playback: Playback,
    /// Whether the sound activity is going on the face.
    sounding: bool,
}

impl<P> Default for Session<P> {
    fn default() -> Self {
        Self {
            page: None,
            generation: 0,
            revision: 0,
            morph: Morph::default(),
            pinned: false,
            playback: Playback::None,
            sounding: false,
        }
    }
}

impl<P> Session<P> {
    pub fn page(&self) -> Option<&P> {
        self.page.as_ref()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The page to show, made with `make` only when the session holds none.
    /// Says whether it was made.
    pub fn page_or(&mut self, make: impl FnOnce() -> Option<P>) -> bool {
        if self.page.is_some() {
            return false;
        }
        let Some(page) = make() else {
            return false;
        };
        self.page = Some(page);
        self.generation += 1;
        self.playback = Playback::None;
        true
    }

    /// Ends the session: the page goes, and with it what it was playing.
    /// Returns the page, for whatever must be done with it as it goes.
    pub fn end(&mut self) -> Option<P> {
        self.playback = Playback::None;
        self.page.take()
    }

    pub fn playback(&self) -> Playback {
        self.playback
    }

    /// The page's media session reported `playback` for the page of
    /// `generation`. A report about an earlier page is refused.
    pub fn report(&mut self, generation: u64, playback: Playback) -> bool {
        if generation != self.generation || self.page.is_none() {
            return false;
        }
        self.playback = playback;
        true
    }

    /// The sound activity for the face, when it has changed: going, with the
    /// boombox and the measured level, while the page plays; ended once it
    /// does not. `level` is a measurement of what is being heard and when
    /// (milliseconds), or None when nothing was measured; a level is never
    /// made up.
    pub fn sound(&mut self, level: Option<(f64, f64)>) -> Option<Activity> {
        let playing = self.page.is_some() && self.playback == Playback::Playing;
        if !playing && !self.sounding {
            return None;
        }
        self.revision += 1;
        let mut activity = Activity::new(SOUND_ACTIVITY, self.generation);
        activity.revision = self.revision;
        activity.environment = Some("desktop".to_owned());
        activity.surface = Some("theo-screen".to_owned());
        activity.standing = Standing::Ambient;
        activity.preferred_form = Some("boombox".to_owned());
        activity.available_actions = vec!["pause".to_owned(), "open".to_owned()];
        if playing {
            activity.phase = Phase::Active;
            activity.signals = level
                .map(|(value, at)| Signal {
                    name: "sound.level".to_owned(),
                    value,
                    unit: "fraction".to_owned(),
                    source: "platinum-audio-output".to_owned(),
                    at,
                    fresh_for: LEVEL_FRESH_MS,
                })
                .into_iter()
                .collect();
        } else {
            activity.phase = Phase::Completed;
        }
        self.sounding = playing;
        Some(activity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VISIBLE: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 1440.0,
        h: 875.0,
    };
    const FACE_SIZE: (f64, f64) = (264.0, 132.0);

    fn face_at(x: f64, y: f64) -> Rect {
        Rect {
            x,
            y,
            w: FACE_SIZE.0,
            h: FACE_SIZE.1,
        }
    }

    #[test]
    fn the_screen_opens_centred_on_the_face() {
        let face = face_at(600.0, 400.0);
        let screen = screen_frame(face, VISIBLE, SCREEN_SIZE);
        assert_eq!((screen.w, screen.h), SCREEN_SIZE);
        assert_eq!(screen.x + screen.w / 2.0, face.x + face.w / 2.0);
        assert_eq!(screen.y + screen.h / 2.0, face.y + face.h / 2.0);
    }

    #[test]
    fn the_screen_stays_on_the_visible_frame_from_a_corner() {
        // Theo's home, the bottom right.
        let face = face_at(VISIBLE.w - FACE_SIZE.0 - 16.0, 16.0);
        let screen = screen_frame(face, VISIBLE, SCREEN_SIZE);
        assert_eq!(screen.x + screen.w, VISIBLE.w);
        assert_eq!(screen.y, VISIBLE.y);
        // Too large a size is held to the frame, too small to the least.
        let huge = screen_frame(face, VISIBLE, (5000.0, 5000.0));
        assert_eq!((huge.w, huge.h), (VISIBLE.w, VISIBLE.h));
        let tiny = screen_frame(face, VISIBLE, (10.0, 10.0));
        assert_eq!((tiny.w, tiny.h), SCREEN_MIN);
    }

    #[test]
    fn the_face_comes_back_where_it_left_the_screen() {
        let face = face_at(600.0, 400.0);
        let screen = screen_frame(face, VISIBLE, SCREEN_SIZE);
        let inside = face_in_window(screen, face);
        assert_eq!(face_frame(screen, SCREEN_SIZE, inside, VISIBLE), face);
        // Moved with the screen.
        let moved = Rect {
            x: screen.x - 100.0,
            y: screen.y + 50.0,
            ..screen
        };
        let back = face_frame(moved, SCREEN_SIZE, inside, VISIBLE);
        assert_eq!((back.x, back.y), (face.x - 100.0, face.y + 50.0));
    }

    #[test]
    fn the_face_stays_inside_a_screen_made_smaller() {
        let face = face_at(VISIBLE.w - FACE_SIZE.0, 16.0);
        let screen = screen_frame(face, VISIBLE, SCREEN_SIZE);
        let inside = face_in_window(screen, face);
        // The person shrank the screen from its left edge.
        let smaller = Rect {
            x: screen.x + 300.0,
            w: screen.w - 300.0,
            ..screen
        };
        let back = face_frame(smaller, SCREEN_SIZE, inside, VISIBLE);
        assert!(back.x >= smaller.x && back.x + back.w <= smaller.x + smaller.w);
        assert!(back.y >= smaller.y && back.y + back.h <= smaller.y + smaller.h);
    }

    #[test]
    fn a_spot_reads_back_what_the_face_frame_says() {
        let face = face_at(VISIBLE.w - FACE_SIZE.0, 0.0);
        assert_eq!(spot(face, VISIBLE), (1.0, 0.0));
    }

    fn plate() -> Area {
        Area {
            x: 72.0,
            y: 27.6,
            w: 120.0,
            h: 76.8,
        }
    }

    fn face() -> Shape {
        let face = Area {
            x: 0.0,
            y: 0.0,
            w: 264.0,
            h: 132.0,
        };
        face_shape(face, plate(), default_eyes(plate()))
    }

    fn at(t: f64) -> Shape {
        opening().blend(&face(), &screen_shape(SCREEN_SIZE), t)
    }

    #[test]
    fn the_body_starts_as_the_face_and_ends_as_the_screen() {
        let start = at(0.0);
        let body = start.part(BODY).unwrap();
        assert_eq!((body.area, body.radius), (plate(), PLATE_RADIUS));
        assert_eq!(start.part(FACE).unwrap().alpha, 1.0);
        for hidden in [EYE_LEFT, EYE_RIGHT, STILL, BAR] {
            assert_eq!(start.part(hidden).unwrap().alpha, 0.0, "{hidden}");
        }
        let end = at(1.0);
        let body = end.part(BODY).unwrap();
        assert_eq!(body.area.w, SCREEN_SIZE.0);
        assert_eq!(body.radius, SCREEN_RADIUS);
        assert_eq!(end.part(FACE).unwrap().alpha, 0.0);
        assert_eq!(end.part(STILL).unwrap().area, backing(SCREEN_SIZE));
        assert_eq!(end.part(BAR).unwrap().alpha, 1.0);
        // The two eyes have become one backing inside the bezel.
        let (left, right) = (end.part(EYE_LEFT).unwrap(), end.part(EYE_RIGHT).unwrap());
        assert_eq!(left.area.union(right.area), backing(SCREEN_SIZE));
        assert!(left.area.x + left.area.w > right.area.x);
        assert_eq!(left.radius, BACKING_RADIUS);
        // The face is drawn over the body, not hidden behind it.
        let names: Vec<_> = end.parts.iter().map(|p| p.name.as_ref()).collect();
        assert_eq!(names.iter().position(|n| *n == FACE), Some(1));
    }

    #[test]
    fn the_still_shows_only_once_the_backing_is_mostly_there() {
        assert_eq!(at(0.59).part(STILL).unwrap().alpha, 0.0);
        assert_eq!(at(0.6).part(STILL).unwrap().alpha, 1.0);
    }

    #[test]
    fn the_body_grows_without_jumping() {
        let mut last = at(0.0).part(BODY).unwrap().area;
        for step in 1..=100 {
            let next = at(f64::from(step) / 100.0).part(BODY).unwrap().area;
            assert!(next.w >= last.w && next.h >= last.h);
            assert!((next.w - last.w) < 40.0);
            last = next;
        }
    }

    #[test]
    fn the_eyes_are_read_from_the_bright_grains() {
        // Two grains a side; a dim one in the middle is not an eye.
        let grain = |x: f32, y: f32, alpha: f32| [x, y, 1.0, 0.0, 1.0, 1.0, 1.0, alpha];
        let grains: Vec<f32> = [
            grain(-10.0, -2.0, 1.0),
            grain(-6.0, 2.0, 1.0),
            grain(6.0, -2.0, 0.9),
            grain(10.0, 2.0, 0.9),
            grain(0.5, 0.0, 0.1),
        ]
        .concat();
        let [left, right] = eye_boxes(&grains, 8, 2.0, (100.0, 50.0)).expect("two eyes");
        assert_eq!(
            left,
            Area {
                x: 78.0,
                y: 44.0,
                w: 12.0,
                h: 12.0
            }
        );
        assert_eq!(
            right,
            Area {
                x: 110.0,
                y: 44.0,
                w: 12.0,
                h: 12.0
            }
        );
        let one_sided: Vec<f32> = grain(-10.0, 0.0, 1.0).to_vec();
        assert!(eye_boxes(&one_sided, 8, 2.0, (0.0, 0.0)).is_none());
    }

    #[test]
    fn the_morph_opens_settles_and_closes() {
        let mut morph = Morph::default();
        morph.open(1000.0, false);
        assert!(morph.shows_screen() && !morph.interactive());
        assert_eq!(morph.tick(1000.0 + OPEN_MS / 2.0), None);
        assert!((morph.progress(1000.0 + OPEN_MS / 2.0) - 0.5).abs() < 1e-9);
        assert_eq!(morph.tick(1000.0 + OPEN_MS), Some(Settled::Opened));
        assert!(morph.interactive());
        morph.close(2000.0, false, false);
        assert!(!morph.interactive());
        assert_eq!(morph.tick(2000.0 + CLOSE_MS), Some(Settled::Collapsed));
        assert!(!morph.shows_screen());
    }

    #[test]
    fn a_reversed_morph_turns_back_from_where_it_was() {
        let mut morph = Morph::default();
        morph.open(0.0, false);
        let at = OPEN_MS * 0.25;
        let before = morph.progress(at);
        morph.close(at, false, false);
        assert!((morph.progress(at) - before).abs() < 1e-9);
        morph.open(at + 10.0, false);
        assert!((morph.progress(at + 10.0) - morph.progress(at + 10.0)).abs() < 1e-9);
        assert!(matches!(morph.form(), Form::Opening { .. }));
    }

    #[test]
    fn a_close_asked_during_a_collapse_still_closes() {
        let mut morph = Morph::default();
        morph.open(0.0, true);
        morph.close(10.0, false, false);
        morph.close(20.0, true, false);
        assert_eq!(morph.tick(10.0 + CLOSE_MS), Some(Settled::Closed));
    }

    #[test]
    fn reduced_motion_changes_form_at_once() {
        let mut morph = Morph::default();
        morph.open(0.0, true);
        assert_eq!(morph.form(), Form::Screen);
        morph.close(5.0, false, true);
        assert_eq!(morph.tick(5.0), Some(Settled::Collapsed));
    }

    #[test]
    fn the_session_keeps_its_page_through_collapse_and_hiding() {
        let mut made = 0;
        let mut session: Session<u32> = Session::default();
        let mut make = || {
            made += 1;
            Some(made)
        };
        assert!(session.page_or(&mut make));
        session.morph.open(0.0, true);
        session.morph.close(1.0, false, true);
        assert_eq!(session.morph.tick(1.0), Some(Settled::Collapsed));
        // Opened again, the same page is shown: nothing is made anew.
        session.morph.open(2.0, true);
        assert!(!session.page_or(&mut make));
        assert_eq!(session.page(), Some(&1));
        assert_eq!(made, 1);
    }

    #[test]
    fn closing_ends_the_page_and_a_new_one_has_a_new_generation() {
        let mut session: Session<&str> = Session::default();
        session.page_or(|| Some("first"));
        let first = session.generation();
        assert_eq!(session.end(), Some("first"));
        assert!(session.page().is_none());
        session.page_or(|| Some("second"));
        assert!(session.generation() > first);
        // What the first page said about its sound is refused now.
        assert!(!session.report(first, Playback::Playing));
        assert_eq!(session.playback(), Playback::None);
    }

    #[test]
    fn the_sound_activity_follows_playback_and_never_invents_a_level() {
        let mut session: Session<u8> = Session::default();
        assert!(session.sound(None).is_none());
        session.page_or(|| Some(1));
        assert!(session.report(session.generation(), Playback::Playing));
        let going = session.sound(None).expect("playing");
        assert_eq!(going.phase, Phase::Active);
        assert_eq!(going.preferred_form.as_deref(), Some("boombox"));
        assert!(going.signals.is_empty(), "no measurement, no level");
        let measured = session.sound(Some((0.4, 100.0))).expect("playing");
        assert!(measured.revision > going.revision);
        assert_eq!(measured.signals[0].name, "sound.level");
        assert!(measured.signals[0].fresh(200.0));
        assert!(!measured.signals[0].fresh(100.0 + LEVEL_FRESH_MS + 1.0));
        // Paused: the activity ends once, then there is nothing to say.
        session.report(session.generation(), Playback::Paused);
        assert_eq!(session.sound(None).map(|a| a.phase), Some(Phase::Completed));
        assert!(session.sound(None).is_none());
        // Closed while playing: ended too.
        session.report(session.generation(), Playback::Playing);
        session.sound(None);
        session.end();
        assert_eq!(session.sound(None).map(|a| a.phase), Some(Phase::Completed));
    }
}
