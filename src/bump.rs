//! Bumps: pieces of the window's frame that reach into a card, as the places
//! do at a card's leading edge and Theorem's composer at its foot, or that
//! stand alone on the window's edge. One outline serves a bump three ways:
//! the card runs around it, its body fills it and the card's edge traces it,
//! so the three cannot part. Its base flares into the edge it stands on along
//! an eased fillet, the way a continuous corner meets a straight edge, rather
//! than a circular arc's sudden turn. Its body fills from clear at its base
//! to the bump's own color, so the bump rises out of the frame rather than
//! sitting on it, while the edge is one stroke of one weight around the card
//! and its bumps together, so no line ends beside another.
//!
//! SOURCING: GPUI's `canvas`, `PathBuilder` and `linear_gradient`; the dot
//! grid is gpui-whiteboard's `paint_board`, the one Platinum's canvas draws;
//! the outline is Zephium's `composer-hole.ts` (frame/src/features/work/lib),
//! turned to stand on any edge. No GPUI Kit component cuts a card around
//! another element.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    App, Background, Bounds, Hsla, IntoElement, Path, PathBuilder, Pixels, Point, Styled, Window,
    canvas, linear_color_stop, linear_gradient, point, px, transparent_black,
};
use gpui_whiteboard::{Camera, paint_board};

/// How far into its body a bump's color reaches full strength, from its base.
const FADE: f32 = 0.45;
/// The radius of a card's corners and of a bump's tip, against the theme's
/// large radius.
const ROUND: f32 = 1.5;
/// How far a bump's base flares along the edge it stands on, against the
/// theme's large radius.
const FLARE: f32 = 2.;
/// How much of a flare's curve eases in.
const EASE: f32 = 0.56;

/// The bounds of a bump, recorded as its element lays out and read when its
/// card paints, later in the same frame.
#[derive(Clone, Default)]
pub struct Slot(Rc<Cell<Option<Bounds<Pixels>>>>);

impl Slot {
    /// Put in a `relative` bump, it fills it and records its bounds.
    pub fn recorder(&self) -> impl IntoElement {
        let slot = self.0.clone();
        canvas(move |bounds, _, _| slot.set(Some(bounds)), |_, _, _, _| {})
            .absolute()
            .top_0()
            .left_0()
            .size_full()
    }

    fn get(&self) -> Option<Bounds<Pixels>> {
        self.0.get()
    }
}

/// A bump's fill when it stands alone over the frame: from clear at its
/// leading base to the bump's color.
pub fn leading_fill(cx: &App) -> Background {
    fade(90., cx.theme().sidebar_border)
}

fn fade(angle: f32, color: Hsla) -> Background {
    linear_gradient(
        angle,
        linear_color_stop(color.opacity(0.), 0.),
        linear_color_stop(color, FADE),
    )
}

/// A card: the theme's raised surface under a dot grid, edged, its outline
/// running around the bumps recorded in `leading` (on its leading edge) and
/// `bottom` (on its foot), whose bodies it fills. Put it first in a
/// `relative` card, which it fills.
pub fn card(leading: Option<Slot>, bottom: Option<Slot>, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let (surface, dots, edge, radius) = (
        theme.popover,
        theme.border,
        theme.sidebar_border,
        theme.radius_lg,
    );
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let notches = Notches::new(
                bounds,
                radius,
                leading.as_ref().and_then(Slot::get),
                bottom.as_ref().and_then(Slot::get),
            );
            if let Ok(path) = notches.outline(PathBuilder::fill()).build() {
                window.paint_path(path, surface);
            }
            for band in notches.dotted() {
                let camera = Camera {
                    x: f32::from(band.origin.x - bounds.origin.x),
                    y: f32::from(band.origin.y - bounds.origin.y),
                    zoom: 1.,
                };
                paint_board(band, camera, transparent_black(), dots, window);
            }
            for bump in notches.bumps() {
                bump.fill(fade(bump.angle(), edge), window);
            }
            // One stroke for the whole edge, the notches with it, so the
            // line keeps its weight from the card's sides into each flare
            // and no segment ends beside another.
            if let Ok(path) = notches.outline(PathBuilder::stroke(px(1.))).build() {
                window.paint_path(path, edge);
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// A bump standing alone on the window's leading edge, as the places do
/// beside a navigator or over the tabs: its body filled with `fill` and
/// edged as a card is, the line running into the window's edge. Put it
/// first in the `relative` bump, which it fills.
pub fn standing(fill: Background, cx: &App) -> impl IntoElement {
    let (edge, radius) = (cx.theme().sidebar_border, cx.theme().radius_lg);
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let bump = Bump::new(
                Base::Leading(bounds.left()),
                bounds.top(),
                bounds.bottom(),
                bounds.size.width,
                radius,
            );
            bump.fill(fill, window);
            if let Some(line) = bump.edge() {
                window.paint_path(line, edge);
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// The edge a bump stands on: a vertical one at `x`, the bump reaching
/// right, or a card's foot at `y`, the bump reaching up.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Base {
    Leading(Pixels),
    Foot(Pixels),
}

/// A bump: the edge it stands on, the span it takes along that edge, how far
/// it reaches from it, and the radius of its tip and its flares.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Bump {
    base: Base,
    from: Pixels,
    to: Pixels,
    reach: Pixels,
    tip: Pixels,
    flare: Pixels,
}

impl Bump {
    fn new(base: Base, from: Pixels, to: Pixels, reach: Pixels, radius: Pixels) -> Self {
        let tip = (radius * ROUND).min((to - from) / 2.).min(reach);
        // The flare meets the tip's corner at most, never past it.
        let flare = (radius * FLARE).min(reach - tip).max(px(0.));
        Self {
            base,
            from,
            to,
            reach,
            tip,
            flare,
        }
    }

    /// The point `along` the base and `out` from it. Both edges keep the
    /// same handedness, so one outline serves either.
    fn point(&self, along: Pixels, out: Pixels) -> Point<Pixels> {
        match self.base {
            Base::Leading(x) => point(x + out, along),
            Base::Foot(y) => point(along, y - out),
        }
    }

    /// The gradient's angle, from the base toward the tip.
    fn angle(&self) -> f32 {
        match self.base {
            Base::Leading(_) => 90.,
            Base::Foot(_) => 0.,
        }
    }

    /// Where the outline leaves the base, past the far end of the span.
    fn start(&self) -> Point<Pixels> {
        self.point(self.to + self.flare, px(0.))
    }

    /// Where it comes back to the base, past the near end.
    fn end(&self) -> Point<Pixels> {
        self.point(self.from - self.flare, px(0.))
    }

    /// The outline from `start` to `end`: up the far wall out of its flare,
    /// round the tip and down the near wall into its flare. The path is at
    /// `start`.
    fn trace(&self, path: &mut PathBuilder) {
        let (a0, a1, d, r, f) = (self.from, self.to, self.reach, self.tip, self.flare);
        let k = f * (1. - EASE);
        let p = |along, out| self.point(along, out);
        path.cubic_bezier_to(p(a1, f), p(a1 + k, px(0.)), p(a1, k));
        path.line_to(p(a1, d - r));
        arc(path, r, false, p(a1 - r, d));
        path.line_to(p(a0 + r, d));
        arc(path, r, false, p(a0, d - r));
        path.line_to(p(a0, f));
        path.cubic_bezier_to(p(a0 - f, px(0.)), p(a0, k), p(a0 - k, px(0.)));
    }

    /// The bump and its flares, closed along its base.
    fn body(&self) -> Option<Path<Pixels>> {
        let mut path = PathBuilder::fill();
        path.move_to(self.start());
        self.trace(&mut path);
        path.close();
        path.build().ok()
    }

    /// The outline alone, open at the base.
    fn edge(&self) -> Option<Path<Pixels>> {
        let mut path = PathBuilder::stroke(px(1.));
        path.move_to(self.start());
        self.trace(&mut path);
        path.build().ok()
    }

    fn fill(&self, fill: Background, window: &mut Window) {
        if let Some(body) = self.body() {
            window.paint_path(body, fill);
        }
    }

    /// What the bump covers, flares included.
    fn extent(&self) -> Bounds<Pixels> {
        let (a, b) = (
            self.point(self.from - self.flare, px(0.)),
            self.point(self.to + self.flare, self.reach),
        );
        Bounds::from_corners(
            point(a.x.min(b.x), a.y.min(b.y)),
            point(a.x.max(b.x), a.y.max(b.y)),
        )
    }
}

/// A card's geometry: its bounds and corner radius, and the bumps cut into
/// it, each kept clear of the card's corners.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Notches {
    card: Bounds<Pixels>,
    corner: Pixels,
    /// The theme's large radius, which the corners and flares scale from.
    radius: Pixels,
    leading: Option<Bump>,
    bottom: Option<Bump>,
}

impl Notches {
    fn new(
        card: Bounds<Pixels>,
        radius: Pixels,
        leading: Option<Bounds<Pixels>>,
        bottom: Option<Bounds<Pixels>>,
    ) -> Self {
        let corner = radius * ROUND;
        // A flare and the card's own corner both fit between a bump and a
        // corner of the card.
        let clear = corner + radius * FLARE;
        let leading = leading.and_then(|bump| {
            let top = bump.top().max(card.top() + clear);
            let bottom = bump.bottom().min(card.bottom() - clear);
            let tip = bump.right().min(card.left() + card.size.width / 2.);
            (bottom > top && tip > card.left()).then(|| {
                Bump::new(
                    Base::Leading(card.left()),
                    top,
                    bottom,
                    tip - card.left(),
                    radius,
                )
            })
        });
        let bottom = bottom.and_then(|bump| {
            let left = bump.left().max(card.left() + clear);
            let right = bump.right().min(card.right() - clear);
            let tip = bump.top().max(card.top() + card.size.height / 2.);
            (right > left && card.bottom() > tip).then(|| {
                Bump::new(
                    Base::Foot(card.bottom()),
                    left,
                    right,
                    card.bottom() - tip,
                    radius,
                )
            })
        });
        Self {
            card,
            corner,
            radius,
            leading,
            bottom,
        }
    }

    fn bumps(&self) -> impl Iterator<Item = &Bump> {
        self.leading.iter().chain(&self.bottom)
    }

    /// The card's outline, clockwise from its top-left corner, running in
    /// around each bump.
    fn outline(&self, mut path: PathBuilder) -> PathBuilder {
        let (card, r) = (self.card, self.corner);
        let (left, top, right, foot) = (card.left(), card.top(), card.right(), card.bottom());
        let around = |path: &mut PathBuilder, bump: &Bump| {
            path.line_to(bump.start());
            bump.trace(path);
        };
        path.move_to(point(left + r, top));
        path.line_to(point(right - r, top));
        arc(&mut path, r, true, point(right, top + r));
        path.line_to(point(right, foot - r));
        arc(&mut path, r, true, point(right - r, foot));
        if let Some(bump) = &self.bottom {
            around(&mut path, bump);
        }
        path.line_to(point(left + r, foot));
        arc(&mut path, r, true, point(left, foot - r));
        if let Some(bump) = &self.leading {
            around(&mut path, bump);
        }
        path.line_to(point(left, top + r));
        arc(&mut path, r, true, point(left + r, top));
        path.close();
        path
    }

    /// Where the dots go: the card inside its corners, less each bump, its
    /// flares and a margin around them.
    fn dotted(&self) -> Vec<Bounds<Pixels>> {
        let (corner, margin) = (self.corner, self.radius);
        let inside = Bounds::from_corners(
            self.card.origin + point(corner, corner),
            self.card.bottom_right() - point(corner, corner),
        );
        let holes = self.bumps().map(|bump| {
            let extent = bump.extent();
            Bounds::from_corners(
                extent.origin - point(margin, margin),
                extent.bottom_right() + point(margin, margin),
            )
        });
        holes.fold(vec![inside], |bands, hole| {
            bands
                .into_iter()
                .flat_map(|band| without(band, hole))
                .collect()
        })
    }
}

fn arc(path: &mut PathBuilder, radius: Pixels, sweep: bool, to: Point<Pixels>) {
    path.arc_to(point(radius, radius), px(0.), false, sweep, to);
}

/// `rect` less `hole`: the bands above and below the hole, and either side
/// of it.
fn without(rect: Bounds<Pixels>, hole: Bounds<Pixels>) -> Vec<Bounds<Pixels>> {
    if !rect.intersects(&hole) {
        return vec![rect];
    }
    let (left, top, right, bottom) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    let (hole_top, hole_bottom) = (hole.top().max(top), hole.bottom().min(bottom));
    [
        (point(left, top), point(right, hole_top)),
        (point(left, hole_bottom), point(right, bottom)),
        (
            point(left, hole_top),
            point(hole.left().max(left), hole_bottom),
        ),
        (
            point(hole.right().min(right), hole_top),
            point(right, hole_bottom),
        ),
    ]
    .into_iter()
    .filter(|(a, b)| b.x > a.x && b.y > a.y)
    .map(|(a, b)| Bounds::from_corners(a, b))
    .collect()
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Bounds, point, px, size};

    use super::{Base, Bump, Notches, without};

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<gpui_kit::Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
    }

    fn area(bands: &[Bounds<gpui_kit::Pixels>]) -> f32 {
        bands
            .iter()
            .map(|band| f32::from(band.size.width) * f32::from(band.size.height))
            .sum()
    }

    #[test]
    fn a_rect_less_a_hole_inside_it_is_four_bands_around_the_hole() {
        let bands = without(rect(0., 0., 100., 100.), rect(40., 40., 20., 20.));
        assert_eq!(bands.len(), 4);
        assert_eq!(area(&bands), 100. * 100. - 20. * 20.);
        for band in &bands {
            assert!(!band.intersects(&rect(40., 40., 20., 20.)), "{band:?}");
        }
    }

    #[test]
    fn a_hole_through_an_edge_leaves_three_bands_and_a_hole_outside_leaves_the_rect() {
        let bands = without(rect(0., 0., 100., 100.), rect(-10., 40., 30., 20.));
        assert_eq!(bands.len(), 3);
        assert_eq!(area(&bands), 100. * 100. - 20. * 20.);
        assert_eq!(
            without(rect(0., 0., 100., 100.), rect(200., 0., 10., 10.)),
            vec![rect(0., 0., 100., 100.)]
        );
    }

    #[test]
    fn bumps_are_kept_clear_of_the_card_corners_and_on_its_edges() {
        let card = rect(0., 0., 400., 300.);
        // A leading bump measured a little off the edge, and taller than
        // the card allows; a bottom bump wider than the card, measured past
        // its foot, as the composer reaches down to the window's edge.
        let notches = Notches::new(
            card,
            px(8.),
            Some(rect(1., -20., 40., 400.)),
            Some(rect(-50., 240., 600., 66.)),
        );
        // Clear of a corner by its 12px radius and a 16px flare.
        let leading = notches.leading.unwrap();
        assert_eq!(leading.base, Base::Leading(px(0.)));
        assert_eq!(
            (leading.from, leading.to, leading.reach),
            (px(28.), px(272.), px(41.))
        );
        let bottom = notches.bottom.unwrap();
        assert_eq!(bottom.base, Base::Foot(px(300.)));
        assert_eq!(
            (bottom.from, bottom.to, bottom.reach),
            (px(28.), px(372.), px(60.))
        );
    }

    #[test]
    fn a_bump_that_does_not_fit_is_not_cut() {
        let card = rect(0., 0., 30., 30.);
        let notches = Notches::new(card, px(8.), Some(rect(0., 0., 10., 30.)), None);
        assert_eq!(notches.leading, None);
    }

    #[test]
    fn a_shallow_bump_flares_no_further_than_its_tip_allows() {
        let bump = Bump::new(Base::Leading(px(0.)), px(0.), px(100.), px(20.), px(8.));
        assert_eq!((bump.tip, bump.flare), (px(12.), px(8.)));
        // The outline leaves and meets its base past either end by the flare.
        assert_eq!(bump.start(), point(px(0.), px(108.)));
        assert_eq!(bump.end(), point(px(0.), px(-8.)));
        let foot = Bump::new(Base::Foot(px(300.)), px(100.), px(300.), px(60.), px(8.));
        assert_eq!((foot.tip, foot.flare), (px(12.), px(16.)));
        assert_eq!(foot.start(), point(px(316.), px(300.)));
        assert_eq!(foot.extent(), rect(84., 240., 232., 60.));
    }

    #[test]
    fn no_dot_falls_in_a_bump_its_flares_or_its_margin() {
        let notches = Notches::new(
            rect(0., 0., 400., 300.),
            px(8.),
            Some(rect(0., 100., 40., 100.)),
            Some(rect(100., 240., 200., 60.)),
        );
        let bands = notches.dotted();
        // Each bump, its 16px flares along its base, and an 8px margin.
        let holes = [rect(-8., 76., 56., 148.), rect(76., 232., 248., 76.)];
        for band in &bands {
            for hole in &holes {
                assert!(!band.intersects(hole), "{band:?} meets {hole:?}");
            }
        }
        // Everything inside the 12px corners but the holes is dotted.
        let inside = 376. * 276.;
        let cut = (48. - 12.) * 148. + 248. * (288. - 232.);
        assert_eq!(area(&bands), inside - cut);
    }
}
