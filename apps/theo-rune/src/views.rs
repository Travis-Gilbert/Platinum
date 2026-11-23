//! What the one window shows: the presence, the body changing between
//! forms, and the workspace. Every view reads the session; none owns any of
//! it, so building a different view never touches a draft or the face.
//!
//! The change between forms is `theo_body::morph` (Platinum PR #25): the
//! face's shape and the screen's shape are blended by the morph's progress
//! and painted part by part. The workspace is the screen's content: drawn
//! from the moment it shows, taking input only once the change has settled,
//! so an outgoing or incoming layer is never live.

use rune::shape::Shape;
use rune::tok::{self, radius, type_};
use rune::{Color, Cx, El, Key, KeyEvent, Tx, canvas, col, el, row, spacer, text, text_input};
use theo_body::morph::{self, BAR, STILL};
use theo_body::shape::{Area, Fill, Part, ease};

use crate::app::{COMPOSER_KEY, FACE_KEY, Theo};
use crate::face::{self, VISOR_PX, plate_colour};
use crate::presentation::{Frame, presence_in_workspace};
use crate::surfaces::{Load, SurfaceRef};

pub fn view(s: &Theo, cx: &mut Cx<Theo>) -> El {
    s.dark.set(cx.system_dark());
    s.still.set(cx.system_reduced_motion());
    let presentation = &s.session.presentation;
    let root = el().key("theo").w_full().h_full().fill(Color::TRANSPARENT);
    if presentation.shows_workspace() {
        root.child(shell(s, cx))
    } else {
        root.child(presence(s))
    }
}

// The presence ---------------------------------------------------------------

/// The face where it rests: the space around it moves the window, the plate
/// is a control that opens the workspace.
fn presence(s: &Theo) -> El {
    let frame = s.session.presentation.presence_frame();
    let centre = (f64::from(frame.w) / 2.0, f64::from(frame.h) / 2.0);
    let drawing = authored(
        face::face(Shape::new(), &s.face, centre, 1.0),
        s,
        centre,
        1.0,
    );
    let (half_w, half_h) = s.face.plate_half();
    let plate = el()
        .key(FACE_KEY)
        .role("button")
        .label(status_label(s))
        .focusable(true)
        .size((half_w * 2.0) as f32, (half_h * 2.0) as f32)
        .absolute((centre.0 - half_w) as f32, (centre.1 - half_h) as f32)
        .radius(morph::PLATE_RADIUS as f32)
        .on_press_tx(|s: &mut Theo, tx: &mut Tx| s.expand(tx))
        .on_key(presence_key);
    el().key("presence")
        .w_full()
        .h_full()
        .window_drag()
        .child(
            canvas(drawing)
                .key("presence.face")
                .w_full()
                .h_full()
                .a11y_hidden(),
        )
        .child(plate)
}

fn presence_key(s: &mut Theo, event: &KeyEvent, tx: &mut Tx) -> bool {
    match event.key {
        Key::Enter => s.expand(tx),
        Key::Escape => s.toggle(tx),
        _ => return false,
    }
    true
}

/// What running behaviors draw, about the face's centre.
fn authored(shape: Shape, s: &Theo, centre: (f64, f64), alpha: f32) -> Shape {
    s.session
        .behaviors
        .drawings()
        .fold(shape, |shape, (_, drawing)| {
            drawing.paint(shape, centre, alpha)
        })
}

/// What the face is doing, in words, so it does not have to be read.
fn status_label(s: &Theo) -> String {
    let doing = s.session.companion.face.words();
    if doing.is_empty() {
        "Theo. Press to open the workspace.".to_owned()
    } else {
        format!("Theo, {doing}. Press to open the workspace.")
    }
}

// The shell between the forms --------------------------------------------------

/// The body at the morph's progress, with the workspace in its screen.
fn shell(s: &Theo, cx: &mut Cx<Theo>) -> El {
    let presentation = &s.session.presentation;
    let window = presentation.workspace_frame();
    let size = (f64::from(window.w), f64::from(window.h));
    let t = ease(presentation.progress(s.now));
    let blended = morph::opening().blend(&face_form(s, window), &morph::screen_shape(size), t);
    let dark = s.dark.get();
    let theme = tok::theme(dark);

    let mut body = Shape::new();
    let mut layers: Vec<El> = Vec::new();
    for part in &blended.parts {
        if part.alpha <= 0.001 {
            continue;
        }
        let alpha = part.alpha as f32;
        match (&part.fill, part.name.as_ref()) {
            (Fill::Ink, _) => {
                body = body.fill(rounded(part.area, part.radius), plate_colour().alpha(alpha))
            }
            (Fill::Backing, _) => {
                body = body.fill(rounded(part.area, part.radius), theme.bg.alpha(alpha))
            }
            (Fill::Face, _) => {
                body = face::face(body, &s.face, part.area.centre(), alpha);
                body = authored(body, s, part.area.centre(), alpha);
            }
            (Fill::Still, STILL) => layers.push(placed(workspace(s, cx), part).opacity(alpha)),
            (Fill::Controls(_), BAR) => layers.push(placed(bar(s, theme), part).opacity(alpha)),
            _ => {}
        }
    }
    let mut shell = el().key("shell").w_full().h_full().child(
        canvas(body)
            .key("shell.body")
            .w_full()
            .h_full()
            .a11y_hidden(),
    );
    for layer in layers {
        shell = shell.child(layer);
    }
    if !presentation.workspace_interactive() {
        // The layers are decorative until the change settles: nothing under
        // this takes a press.
        shell = shell.child(
            el().key("shell.inert")
                .w_full()
                .h_full()
                .absolute(0.0, 0.0)
                .on_background_press(|_: &mut Theo| {}),
        );
    }
    shell
}

/// The face as a shape in the workspace window, where the presence was.
fn face_form(s: &Theo, window: Frame) -> theo_body::shape::Shape {
    let presence = s.session.presentation.presence_frame();
    // Scale: physical pixels per point; the window's own is not known here,
    // and the presence and the workspace share a display.
    let (x, y) = presence_in_workspace(presence, window, 2.0);
    let face = Area {
        x: f64::from(x),
        y: f64::from(y),
        w: f64::from(presence.w),
        h: f64::from(presence.h),
    };
    let (half_w, half_h) = s.face.plate_half();
    let (cx, cy) = face.centre();
    let plate = Area {
        x: cx - half_w,
        y: cy - half_h,
        w: 2.0 * half_w,
        h: 2.0 * half_h,
    };
    let unit = VISOR_PX / s.face.width;
    let eyes = morph::eye_boxes(
        &s.face.grains,
        theo::particles::GRAIN_STRIDE,
        unit,
        (cx, cy),
    )
    .unwrap_or_else(|| morph::default_eyes(plate));
    morph::face_shape(face, plate, eyes)
}

/// A shape part's place, for an element drawn there.
fn placed(content: El, part: &Part) -> El {
    content
        .absolute(part.area.x as f32, part.area.y as f32)
        .size(part.area.w as f32, part.area.h as f32)
}

/// A rounded rectangle as a polygon.
fn rounded(area: Area, radius: f64) -> Vec<[f32; 2]> {
    const STEPS: usize = 6;
    let r = radius.min(area.w / 2.0).min(area.h / 2.0).max(0.0);
    let corners = [
        (
            area.x + area.w - r,
            area.y + r,
            -std::f64::consts::FRAC_PI_2,
        ),
        (area.x + area.w - r, area.y + area.h - r, 0.0),
        (area.x + r, area.y + area.h - r, std::f64::consts::FRAC_PI_2),
        (area.x + r, area.y + r, std::f64::consts::PI),
    ];
    let mut points = Vec::with_capacity(4 * (STEPS + 1));
    for (cx, cy, start) in corners {
        for step in 0..=STEPS {
            let angle = start + std::f64::consts::FRAC_PI_2 * step as f64 / STEPS as f64;
            points.push([(cx + r * angle.cos()) as f32, (cy + r * angle.sin()) as f32]);
        }
    }
    points
}

/// The bar along the screen's top: it moves the window and holds the way
/// back to the presence.
fn bar(s: &Theo, theme: tok::Theme) -> El {
    let collapse = text("Back to Theo")
        .key("bar.collapse")
        .role("button")
        .font(type_::LABEL)
        .color(theme.text_secondary)
        .on_press_tx(|s: &mut Theo, tx: &mut Tx| s.collapse(tx));
    row()
        .key("bar")
        .px(14.0)
        .gap(12.0)
        .items_center()
        .window_drag()
        .child(
            text("Theo")
                .font(type_::TITLE)
                .color(Color::WHITE.alpha(0.86)),
        )
        .child(
            text(s.status.clone())
                .font(type_::LABEL)
                .color(Color::WHITE.alpha(0.5)),
        )
        .child(spacer())
        .child(collapse)
}

// The workspace -------------------------------------------------------------

fn workspace(s: &Theo, cx: &mut Cx<Theo>) -> El {
    let theme = tok::theme(cx.system_dark());
    row()
        .key("workspace")
        .clip_rounded(morph::BACKING_RADIUS as f32)
        .fill(theme.bg)
        .on_key(workspace_key)
        .child(sidebar(s, theme))
        .child(surface(s, theme))
}

fn workspace_key(s: &mut Theo, event: &KeyEvent, tx: &mut Tx) -> bool {
    match event.key {
        Key::Escape => s.collapse(tx),
        _ => return false,
    }
    true
}

fn sidebar(s: &Theo, theme: tok::Theme) -> El {
    let front = s.session.surfaces.front().map(|f| f.reference.clone());
    let mut list = col().key("sidebar.list").gap(2.0);
    for surface in s.session.surfaces.iter() {
        let reference = surface.reference.clone();
        let chosen = front.as_ref() == Some(&reference);
        let name = title_of(&reference, &surface.edit.text);
        let mark = if surface.dirty() { " •" } else { "" };
        list = list.child(
            row()
                .key(format!("sidebar.{}.{}", reference.kind, reference.id))
                .role("button")
                .label(name.clone())
                .h(28.0)
                .px(10.0)
                .items_center()
                .radius(radius::SM)
                .fill(if chosen {
                    theme.press
                } else {
                    Color::TRANSPARENT
                })
                .child(
                    text(format!("{name}{mark}"))
                        .font(type_::BODY)
                        .color(theme.text),
                )
                .on_press(move |s: &mut Theo| {
                    s.session.surfaces.bring_to_front(&reference);
                }),
        );
    }
    col()
        .key("sidebar")
        .w(220.0)
        .h_full()
        .padding(10.0)
        .gap(8.0)
        .fill(theme.card)
        .child(
            text("Open")
                .font(type_::SECTION)
                .color(theme.text_secondary),
        )
        .child(list)
}

fn title_of(reference: &SurfaceRef, text: &str) -> String {
    match reference.kind.as_str() {
        "conversation" if reference.id == "new" => "New conversation".to_owned(),
        _ => text
            .lines()
            .next()
            .filter(|line| !line.trim().is_empty())
            .map_or_else(
                || format!("{} {}", reference.kind, reference.id),
                str::to_owned,
            ),
    }
}

/// The surface in front, in its editor.
fn surface(s: &Theo, theme: tok::Theme) -> El {
    let Some(front) = s.session.surfaces.front() else {
        return col().key("surface.none").grow(1.0).center().child(
            text("Nothing open")
                .font(type_::BODY)
                .color(theme.text_secondary),
        );
    };
    let reference = front.reference.clone();
    let state = match &front.load {
        Load::Loading => Some("Loading…".to_owned()),
        Load::Failed(reason) => Some(format!("Could not load: {reason}")),
        Load::Ready => None,
    };
    let editing = reference.clone();
    let field = text_input(&front.edit, type_::BODY.lh(20.0))
        .key(COMPOSER_KEY)
        .label(format!("{} editor", reference.kind))
        .grow(1.0)
        .w_full()
        .color(theme.text)
        .placeholder("Write to Theo", theme.text_secondary)
        .on_edit(move |s: &mut Theo| s.edit_of(&editing))
        // A focused field's keys never reach its ancestors' `on_key`.
        .on_input_key(workspace_key);
    let mut column = col()
        .key("surface")
        .grow(1.0)
        .h_full()
        .padding(16.0)
        .gap(10.0);
    if let Some(state) = state {
        column = column.child(text(state).font(type_::LABEL).color(theme.text_secondary));
    }
    column.child(field)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rounded_rectangle_stays_in_its_area() {
        let area = Area {
            x: 10.0,
            y: 20.0,
            w: 100.0,
            h: 40.0,
        };
        for [x, y] in rounded(area, 16.0) {
            assert!((10.0..=110.0).contains(&(x as f64)) && (20.0..=60.0).contains(&(y as f64)));
        }
    }

    #[test]
    fn a_new_conversation_and_a_note_have_readable_titles() {
        assert_eq!(
            title_of(&SurfaceRef::new("conversation", "new"), ""),
            "New conversation"
        );
        assert_eq!(
            title_of(&SurfaceRef::new("note", "n1"), "Groceries\nmilk"),
            "Groceries"
        );
        assert_eq!(title_of(&SurfaceRef::new("note", "n1"), ""), "note n1");
    }
}
