//! Theo's body as data: a shape is a list of named parts, and any shape
//! blends into any other part by part. The screen is one shape; the face is
//! another; a form not thought of yet is a third, and it morphs to and from
//! the others with no new transition written for it.
//!
//! A part says where it is, how round, what it is filled with and how much
//! it shows. A part one shape has and the other lacks fades where it is. A
//! `Transition` says, per part, over which stretch of the change its place
//! moves and its alpha changes, so one part can set off after another or
//! arrive first; parts it does not name take the whole change.
//!
//! Nothing here draws: the window's code paints each `Fill` (`screen.rs`).
//!
//! SOURCING: plain interpolation; the easing is the standard cubic in-out
//! curve. Theo's face forms keep their own engine (`theo`): this is only for
//! the body the face sits in.

use std::borrow::Cow;

/// A rectangle in a window, in points from its top left, y down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Area {
    pub fn lerp(self, to: Area, t: f64) -> Area {
        Area {
            x: lerp(self.x, to.x, t),
            y: lerp(self.y, to.y, t),
            w: lerp(self.w, to.w, t),
            h: lerp(self.h, to.h, t),
        }
    }

    pub fn centre(self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub fn union(self, other: Area) -> Area {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Area {
            x,
            y,
            w: (self.x + self.w).max(other.x + other.w) - x,
            h: (self.y + self.h).max(other.y + other.h) - y,
        }
    }
}

pub fn lerp(from: f64, to: f64, t: f64) -> f64 {
    from + (to - from) * t
}

/// The cubic in-out curve: slow from rest, quick through the middle, slow
/// into place.
pub fn ease(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// What a part is filled with. The window's code knows how to paint each.
#[derive(Clone, Debug, PartialEq)]
pub enum Fill {
    /// Theo's dark body.
    Ink,
    /// The white the content sits on.
    Backing,
    /// The face's own drawing: its plate and its matter.
    Face,
    /// The last picture of the page; the live page, which takes input, once
    /// the screen has settled.
    Still,
    /// A control strip, by name: the screen's bar is `"screen.bar"`.
    Controls(Cow<'static, str>),
}

/// One part of a shape.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub name: Cow<'static, str>,
    pub area: Area,
    pub radius: f64,
    pub fill: Fill,
    /// How much of it shows, 0 to 1.
    pub alpha: f64,
}

impl Part {
    pub fn new(name: impl Into<Cow<'static, str>>, area: Area, radius: f64, fill: Fill) -> Self {
        Self {
            name: name.into(),
            area,
            radius,
            fill,
            alpha: 1.0,
        }
    }

    pub fn alpha(mut self, alpha: f64) -> Self {
        self.alpha = alpha;
        self
    }
}

/// A shape: its parts, painted in order, first at the back.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shape {
    pub parts: Vec<Part>,
}

impl Shape {
    pub fn new(parts: impl IntoIterator<Item = Part>) -> Self {
        Self {
            parts: parts.into_iter().collect(),
        }
    }

    pub fn part(&self, name: &str) -> Option<&Part> {
        self.parts.iter().find(|part| part.name == name)
    }
}

/// A stretch of a change, 0 to 1, eased.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub from: f64,
    pub to: f64,
}

impl Span {
    pub const WHOLE: Span = Span { from: 0.0, to: 1.0 };

    pub fn new(from: f64, to: f64) -> Self {
        Self { from, to }
    }

    /// Where in this stretch `t` is, eased; a stretch of no length is a step.
    pub fn at(self, t: f64) -> f64 {
        if self.to <= self.from {
            return if t >= self.from { 1.0 } else { 0.0 };
        }
        ease((t - self.from) / (self.to - self.from))
    }
}

/// When each part changes, as a change from one shape to another goes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transition {
    /// Per part: the stretch its place, size and corner move over, and the
    /// stretch its alpha changes over.
    pub timing: Vec<(Cow<'static, str>, Span, Span)>,
}

impl Transition {
    pub fn time(mut self, part: impl Into<Cow<'static, str>>, shape: Span, alpha: Span) -> Self {
        self.timing.push((part.into(), shape, alpha));
        self
    }

    fn spans(&self, part: &str) -> (Span, Span) {
        self.timing
            .iter()
            .find(|(name, _, _)| name == part)
            .map_or((Span::WHOLE, Span::WHOLE), |&(_, shape, alpha)| {
                (shape, alpha)
            })
    }

    /// `from` become `to` as far as `t`, 0 to 1. Parts keep `to`'s order,
    /// with those only `from` has after the part they followed there.
    pub fn blend(&self, from: &Shape, to: &Shape, t: f64) -> Shape {
        let mut parts: Vec<Part> = Vec::with_capacity(from.parts.len() + to.parts.len());
        for part in &to.parts {
            let (shape, alpha) = self.spans(&part.name);
            let (s, a) = (shape.at(t), alpha.at(t));
            parts.push(match from.part(&part.name) {
                Some(start) => Part {
                    name: part.name.clone(),
                    area: start.area.lerp(part.area, s),
                    radius: lerp(start.radius, part.radius, s),
                    // The fill is the one being arrived at, once halfway.
                    fill: if s < 0.5 {
                        start.fill.clone()
                    } else {
                        part.fill.clone()
                    },
                    alpha: lerp(start.alpha, part.alpha, a),
                },
                None => Part {
                    alpha: part.alpha * a,
                    ..part.clone()
                },
            });
        }
        for (index, part) in from.parts.iter().enumerate() {
            if to.part(&part.name).is_some() {
                continue;
            }
            let (_, alpha) = self.spans(&part.name);
            let leaving = Part {
                alpha: part.alpha * (1.0 - alpha.at(t)),
                ..part.clone()
            };
            // After whatever it followed in `from`, or at the back.
            let after = from.parts[..index]
                .iter()
                .rev()
                .find_map(|before| parts.iter().position(|p| p.name == before.name));
            parts.insert(after.map_or(0, |at| at + 1), leaving);
        }
        Shape { parts }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(x: f64, w: f64) -> Area {
        Area {
            x,
            y: 0.0,
            w,
            h: 10.0,
        }
    }

    #[test]
    fn a_blend_starts_at_one_shape_and_ends_at_the_other() {
        let a = Shape::new([Part::new("body", area(0.0, 10.0), 2.0, Fill::Ink)]);
        let b = Shape::new([Part::new("body", area(0.0, 100.0), 8.0, Fill::Ink)]);
        let change = Transition::default();
        assert_eq!(change.blend(&a, &b, 0.0), a);
        assert_eq!(change.blend(&a, &b, 1.0), b);
        let half = change.blend(&a, &b, 0.5);
        assert_eq!(half.parts[0].area.w, 55.0);
        assert_eq!(half.parts[0].radius, 5.0);
    }

    #[test]
    fn a_part_only_one_shape_has_fades_where_it_is() {
        let a = Shape::new([
            Part::new("body", area(0.0, 10.0), 0.0, Fill::Ink),
            Part::new("face", area(1.0, 8.0), 0.0, Fill::Face),
        ]);
        let b = Shape::new([
            Part::new("body", area(0.0, 50.0), 0.0, Fill::Ink),
            Part::new("bar", area(0.0, 50.0), 0.0, Fill::Controls("bar".into())),
        ]);
        let change = Transition::default();
        let start = change.blend(&a, &b, 0.0);
        assert_eq!(start.part("face").map(|p| p.alpha), Some(1.0));
        assert_eq!(start.part("bar").map(|p| p.alpha), Some(0.0));
        // The face stays in front of the body, where it was.
        let names: Vec<_> = start.parts.iter().map(|p| p.name.as_ref()).collect();
        assert_eq!(names, ["body", "face", "bar"]);
        let end = change.blend(&a, &b, 1.0);
        assert_eq!(
            end.part("face").map(|p| (p.alpha, p.area)),
            Some((0.0, area(1.0, 8.0)))
        );
        assert_eq!(end.part("bar").map(|p| p.alpha), Some(1.0));
    }

    #[test]
    fn each_part_keeps_its_own_time() {
        let a = Shape::new([Part::new("eye", area(0.0, 10.0), 0.0, Fill::Backing).alpha(0.0)]);
        let b = Shape::new([Part::new("eye", area(0.0, 110.0), 0.0, Fill::Backing)]);
        let change = Transition::default().time("eye", Span::new(0.5, 1.0), Span::new(0.0, 0.25));
        let early = change.blend(&a, &b, 0.25);
        assert_eq!(early.parts[0].area.w, 10.0, "not set off yet");
        assert_eq!(early.parts[0].alpha, 1.0, "but already shown");
        assert_eq!(change.blend(&a, &b, 1.0), b);
    }

    #[test]
    fn a_step_changes_at_its_moment() {
        assert_eq!(Span::new(0.6, 0.6).at(0.59), 0.0);
        assert_eq!(Span::new(0.6, 0.6).at(0.6), 1.0);
    }
}
