//! The gesture engine alone: hit testing, handles, drags, the marquee and the
//! camera over a presented scene, with no view and no Space.

use super::*;
use crate::{BoxGeom, EmbedGeom, SegGeom, SegmentStyle};

fn card(id: u64, x: f32, y: f32, w: f32, h: f32) -> Element {
    Element {
        id,
        kind: ElementKind::Embed(EmbedGeom {
            page_id: id as i64,
            title: String::new(),
            x,
            y,
            w,
            h,
        }),
        stroke: None,
        fill: None,
        label: None,
        label_color: None,
        styles: Vec::new(),
        mindmap: None,
    }
}

fn region(id: u64, x: f32, y: f32, w: f32, h: f32) -> Element {
    let mut e = card(id, 0.0, 0.0, 0.0, 0.0);
    e.kind = ElementKind::Rect(BoxGeom {
        x,
        y,
        w,
        h,
        width: 1.0,
        rotation: 0.0,
    });
    e
}

fn line(id: u64) -> Element {
    let mut e = card(id, 0.0, 0.0, 0.0, 0.0);
    e.kind = ElementKind::Line(SegGeom {
        x1: 0.0,
        y1: 0.0,
        x2: 1000.0,
        y2: 1000.0,
        width: 2.0,
        style: SegmentStyle::Solid,
        start_anchor: None,
        end_anchor: None,
    });
    e
}

/// Region 10 (holding 1 and 2) behind cards 1, 2 and 3; card 4 is held; an
/// inert line crosses everything.
fn engine() -> Engine {
    let mut e = Engine::new();
    e.present(
        Scene {
            camera: crate::Camera::default(),
            elements: vec![
                region(10, 0.0, 0.0, 600.0, 400.0),
                card(1, 100.0, 100.0, 200.0, 120.0),
                card(2, 350.0, 100.0, 200.0, 120.0),
                card(3, 700.0, 100.0, 200.0, 120.0),
                card(4, 700.0, 400.0, 200.0, 120.0),
                line(20),
            ],
        },
        [
            (
                10,
                Role::Region {
                    members: vec![1, 2],
                },
            ),
            (1, Role::Card { connectable: true }),
            (2, Role::Card { connectable: true }),
            (3, Role::Card { connectable: false }),
            (4, Role::Card { connectable: true }),
            (20, Role::Inert),
        ],
        [4],
    );
    e
}

fn frame(selection: &[u64]) -> Frame<'_> {
    Frame {
        selection,
        view: View::default(),
        engaged: None,
        hovered: None,
    }
}

fn commit_of(events: &[EngineEvent]) -> (&Scene, &Scene, &str) {
    match events {
        [
            EngineEvent::Commit(WhiteboardEvent::Commit {
                before,
                after,
                label,
            }),
        ] => (before, after, label),
        other => panic!("expected one commit, got {other:?}"),
    }
}

fn frame_of(scene: &Scene, id: u64) -> Bounds {
    bounds_of(&scene.elements.iter().find(|e| e.id == id).unwrap().kind)
}

#[test]
fn hit_takes_the_topmost_card_else_the_smallest_region_and_never_an_inert_line() {
    let e = engine();
    assert_eq!(e.hit([150.0, 150.0]), Some(Hit::Card(1)));
    assert_eq!(e.hit([320.0, 300.0]), Some(Hit::Region(10)));
    // The line crosses (800, 800); nothing else is there.
    assert_eq!(e.hit([800.0, 800.0]), None);
    // Stacked cards: the later one is on top.
    let mut top = engine();
    let mut scene = top.scene().clone();
    scene.elements.push(card(5, 150.0, 150.0, 50.0, 50.0));
    top.present(
        scene,
        [
            (1, Role::Card { connectable: true }),
            (5, Role::Card { connectable: true }),
        ],
        [],
    );
    assert_eq!(top.hit([160.0, 160.0]), Some(Hit::Card(5)));
}

#[test]
fn one_drag_over_a_selection_is_one_commit_and_the_preview_follows() {
    // Wrong implementation rejected: a commit per card, or a commit per move
    // event, would leave more than one event at release.
    let e = engine();
    let sel = [1, 2];
    let f = frame(&sel);
    let mut g = None;
    let events = e.press(
        &mut g,
        [150.0, 150.0],
        Button::Primary,
        Mods::default(),
        1,
        &f,
    );
    assert!(
        events.is_empty(),
        "pressing a selected card keeps the selection"
    );
    for x in [160.0, 200.0, 230.0] {
        assert!(e.drag_to(&mut g, [x, 170.0], &f).is_empty());
    }
    assert_eq!(
        e.preview(&g, 2, [350.0, 100.0, 200.0, 120.0]),
        [430.0, 120.0, 200.0, 120.0]
    );
    assert_eq!(
        e.preview(&g, 3, [700.0, 100.0, 200.0, 120.0]),
        [700.0, 100.0, 200.0, 120.0]
    );
    let events = e.release(&mut g, [230.0, 170.0], &f);
    let (before, after, label) = commit_of(&events);
    assert_eq!(label, "move");
    assert_eq!(before, e.scene());
    assert_eq!(frame_of(after, 1), [180.0, 120.0, 200.0, 120.0]);
    assert_eq!(frame_of(after, 2), [430.0, 120.0, 200.0, 120.0]);
    assert_eq!(frame_of(after, 3), frame_of(before, 3));
    assert!(g.is_none());
}

#[test]
fn a_press_in_place_on_a_multi_selection_narrows_it() {
    let e = engine();
    let sel = [1, 2];
    let f = frame(&sel);
    let mut g = None;
    e.press(
        &mut g,
        [150.0, 150.0],
        Button::Primary,
        Mods::default(),
        1,
        &f,
    );
    assert_eq!(
        e.release(&mut g, [151.0, 151.0], &f),
        vec![EngineEvent::Select(vec![1])]
    );
    // The additive modifier toggles instead, and an unselected card replaces.
    let mut g = None;
    let extend = Mods {
        extend: true,
        ..Mods::default()
    };
    assert_eq!(
        e.press(&mut g, [150.0, 150.0], Button::Primary, extend, 1, &f),
        vec![EngineEvent::Select(vec![2])]
    );
    let mut g = None;
    assert_eq!(
        e.press(
            &mut g,
            [750.0, 150.0],
            Button::Primary,
            Mods::default(),
            1,
            &f
        ),
        vec![EngineEvent::Select(vec![3])]
    );
}

#[test]
fn a_held_card_never_moves_and_the_whole_gesture_is_refused() {
    // Wrong implementation rejected: moving the free cards and dropping the
    // held one would split a refused gesture.
    let e = engine();
    let sel = [3, 4];
    let f = frame(&sel);
    let mut g = None;
    e.press(
        &mut g,
        [750.0, 150.0],
        Button::Primary,
        Mods::default(),
        1,
        &f,
    );
    e.drag_to(&mut g, [850.0, 150.0], &f);
    assert_eq!(
        e.preview(&g, 3, [700.0, 100.0, 200.0, 120.0]),
        [700.0, 100.0, 200.0, 120.0]
    );
    assert_eq!(
        e.release(&mut g, [850.0, 150.0], &f),
        vec![EngineEvent::Held {
            elements: vec![4],
            delta: [100.0, 0.0],
            label: "move".into()
        }]
    );
    // No resize handle is offered on a held card.
    let sel = [4];
    let f = frame(&sel);
    assert!(!matches!(
        e.hover([900.0, 520.0], &f),
        Some(Hover::Resize(..))
    ));
    // A nudge of a held card is refused the same way.
    assert!(matches!(
        e.nudge(&f, [1.0, 0.0], false),
        Some(EngineEvent::Held { .. })
    ));
}

#[test]
fn a_region_press_selects_its_stored_members_and_carries_them() {
    let e = engine();
    let f = frame(&[]);
    let mut g = None;
    assert_eq!(
        e.press(
            &mut g,
            [320.0, 300.0],
            Button::Primary,
            Mods::default(),
            1,
            &f
        ),
        vec![EngineEvent::Select(vec![1, 2])]
    );
    e.drag_to(&mut g, [340.0, 330.0], &f);
    let events = e.release(&mut g, [340.0, 330.0], &f);
    let (_, after, label) = commit_of(&events);
    assert_eq!(label, "move group");
    assert_eq!(frame_of(after, 10), [20.0, 30.0, 600.0, 400.0]);
    assert_eq!(frame_of(after, 1), [120.0, 130.0, 200.0, 120.0]);
    assert_eq!(frame_of(after, 2), [370.0, 130.0, 200.0, 120.0]);
    // Card 3 is not a member even when a region is dragged over it.
    assert_eq!(frame_of(after, 3), [700.0, 100.0, 200.0, 120.0]);
}

#[test]
fn the_marquee_selects_cards_wholly_inside_it_in_paint_order() {
    let e = engine();
    let sel = [3];
    let f = frame(&sel);
    let mut g = None;
    // Empty ground clears a selection unless the press is additive.
    assert_eq!(
        e.press(
            &mut g,
            [50.0, 600.0],
            Button::Primary,
            Mods::default(),
            1,
            &f
        ),
        vec![EngineEvent::Select(Vec::new())]
    );
    e.drag_to(&mut g, [580.0, 50.0], &f);
    assert_eq!(Engine::marquee(&g), Some([50.0, 50.0, 530.0, 550.0]));
    assert_eq!(
        e.release(&mut g, [580.0, 50.0], &f),
        vec![EngineEvent::Select(vec![1, 2])]
    );
    // Additive keeps the base; a partly covered card is not taken.
    let mut g = None;
    let extend = Mods {
        extend: true,
        ..Mods::default()
    };
    assert!(
        e.press(&mut g, [50.0, 600.0], Button::Primary, extend, 1, &f)
            .is_empty()
    );
    assert_eq!(
        e.release(&mut g, [400.0, 50.0], &f),
        vec![EngineEvent::Select(vec![3, 1])]
    );
    // A click on the ground is no marquee.
    let mut g = None;
    e.press(&mut g, [50.0, 600.0], Button::Primary, extend, 1, &f);
    assert!(e.release(&mut g, [51.0, 601.0], &f).is_empty());
}

#[test]
fn a_secondary_press_resolves_on_release_and_never_clears_the_selection() {
    let e = engine();
    let sel = [1];
    let f = frame(&sel);
    let mut g = None;
    // In place, on another card: a context request naming it.
    assert!(
        e.press(
            &mut g,
            [750.0, 150.0],
            Button::Secondary,
            Mods::default(),
            1,
            &f
        )
        .is_empty()
    );
    assert_eq!(
        e.release(&mut g, [751.0, 150.0], &f),
        vec![EngineEvent::Context {
            at: [750.0, 150.0],
            target: Some(3),
            extend: false
        }]
    );
    // On the ground: a context request with no target, no Select.
    let mut g = None;
    e.press(
        &mut g,
        [50.0, 600.0],
        Button::Secondary,
        Mods::default(),
        1,
        &f,
    );
    assert_eq!(
        e.release(&mut g, [50.0, 600.0], &f),
        vec![EngineEvent::Context {
            at: [50.0, 600.0],
            target: None,
            extend: false
        }]
    );
    // Past the slop it pans from where it began, as a camera request only.
    let mut g = None;
    e.press(
        &mut g,
        [50.0, 600.0],
        Button::Secondary,
        Mods::default(),
        1,
        &f,
    );
    assert_eq!(
        e.drag_to(&mut g, [70.0, 600.0], &f),
        vec![EngineEvent::Camera(View {
            x: -20.0,
            y: 0.0,
            zoom: 1.0
        })]
    );
    assert!(e.release(&mut g, [70.0, 600.0], &f).is_empty());
}

#[test]
fn panning_and_zooming_are_camera_requests_inside_the_one_range() {
    // Wrong implementation rejected: a camera change reported as a commit, or
    // a zoom outside the Space's range.
    let e = engine();
    let f = frame(&[]);
    for (button, mods) in [
        (Button::Middle, Mods::default()),
        (
            Button::Primary,
            Mods {
                pan_held: true,
                ..Mods::default()
            },
        ),
        (
            Button::Primary,
            Mods {
                alt: true,
                ..Mods::default()
            },
        ),
    ] {
        let mut g = None;
        assert!(
            e.press(&mut g, [150.0, 150.0], button, mods, 1, &f)
                .is_empty()
        );
        assert!(Engine::pans(&g));
        assert_eq!(
            e.drag_to(&mut g, [160.0, 140.0], &f),
            vec![EngineEvent::Camera(View {
                x: -10.0,
                y: 10.0,
                zoom: 1.0
            })]
        );
        assert!(e.release(&mut g, [160.0, 140.0], &f).is_empty());
    }
    let Some(EngineEvent::Camera(v)) = e.wheel([100.0, 100.0], [0.0, 1.0e6], true, &f) else {
        panic!()
    };
    assert_eq!(v.zoom, MAX_ZOOM);
    let Some(EngineEvent::Camera(v)) = e.wheel([100.0, 100.0], [0.0, -1.0e6], true, &f) else {
        panic!()
    };
    assert_eq!(v.zoom, MIN_ZOOM);
    // The world point under the pointer stays put.
    let w = v.to_world([100.0, 100.0]);
    assert!((w[0] - 100.0).abs() < 1e-9 && (w[1] - 100.0).abs() < 1e-9);
    assert_eq!(
        e.wheel([0.0, 0.0], [5.0, -7.0], false, &f),
        Some(EngineEvent::Camera(View {
            x: -5.0,
            y: 7.0,
            zoom: 1.0
        }))
    );
    let EngineEvent::Camera(v) = e.zoom_step(&f, [800.0, 600.0], 2.0) else {
        panic!()
    };
    assert_eq!(v.zoom, 2.0);
    let back = Frame { view: v, ..f };
    let EngineEvent::Camera(v) = e.zoom_reset(&back, [800.0, 600.0]) else {
        panic!()
    };
    assert_eq!(v.zoom, 1.0);
    assert_eq!(
        e.pan_step(&f, [1.0, 0.0]),
        EngineEvent::Camera(View {
            x: 80.0,
            y: 0.0,
            zoom: 1.0
        })
    );
}

#[test]
fn a_corner_handle_resizes_a_selected_card_no_smaller_than_the_minimum() {
    let e = engine();
    let sel = [1];
    let f = frame(&sel);
    assert_eq!(
        e.hover([300.0, 220.0], &f),
        Some(Hover::Resize(1, Corner::BottomRight))
    );
    let mut g = None;
    e.press(
        &mut g,
        [300.0, 220.0],
        Button::Primary,
        Mods::default(),
        1,
        &f,
    );
    e.drag_to(&mut g, [110.0, 110.0], &f);
    assert_eq!(
        e.preview(&g, 1, [100.0, 100.0, 200.0, 120.0]),
        [100.0, 100.0, 60.0, 40.0]
    );
    let events = e.release(&mut g, [110.0, 110.0], &f);
    let (_, after, label) = commit_of(&events);
    assert_eq!(label, "resize");
    assert_eq!(frame_of(after, 1), [100.0, 100.0, 60.0, 40.0]);
    // A region offers its bottom-right corner while hovered.
    assert_eq!(
        e.hover([600.0, 400.0], &frame(&[])),
        Some(Hover::Resize(10, Corner::BottomRight))
    );
}

#[test]
fn a_side_dot_draws_a_connection_onto_another_card_only() {
    let e = engine();
    let sel = [1];
    let f = frame(&sel);
    assert_eq!(
        e.hover([300.0, 160.0], &f),
        Some(Hover::Connect(1, Side::Right))
    );
    let mut g = None;
    e.press(
        &mut g,
        [300.0, 160.0],
        Button::Primary,
        Mods::default(),
        1,
        &f,
    );
    e.drag_to(&mut g, [400.0, 150.0], &f);
    assert_eq!(
        e.pending_connection(&g),
        Some(([300.0, 160.0], [400.0, 150.0]))
    );
    assert_eq!(
        e.release(&mut g, [400.0, 150.0], &f),
        vec![EngineEvent::Connect {
            from: 1,
            side: Side::Right,
            to: 2
        }]
    );
    // A card that is not connectable offers no dots.
    let sel = [3];
    assert_ne!(
        e.hover([900.0, 160.0], &frame(&sel)),
        Some(Hover::Connect(3, Side::Right))
    );
    // Released over its own card or the ground, nothing.
    let mut g = None;
    e.press(
        &mut g,
        [300.0, 160.0],
        Button::Primary,
        Mods::default(),
        1,
        &f,
    );
    assert!(e.release(&mut g, [250.0, 150.0], &f).is_empty());
}

#[test]
fn double_presses_open_cards_edit_regions_and_create_on_the_ground() {
    let e = engine();
    let f = frame(&[]);
    let mut g = None;
    assert_eq!(
        e.press(
            &mut g,
            [150.0, 150.0],
            Button::Primary,
            Mods::default(),
            2,
            &f
        ),
        vec![EngineEvent::Open(1)]
    );
    assert_eq!(
        e.press(
            &mut g,
            [320.0, 300.0],
            Button::Primary,
            Mods::default(),
            2,
            &f
        ),
        vec![EngineEvent::EditRegion(10)]
    );
    assert_eq!(
        e.press(
            &mut g,
            [50.0, 600.0],
            Button::Primary,
            Mods::default(),
            2,
            &f
        ),
        vec![EngineEvent::Create([50.0, 600.0])]
    );
    // The engaged card's content owns its press.
    let engaged = Frame {
        engaged: Some(1),
        ..f
    };
    let mut g = None;
    assert!(
        e.press(
            &mut g,
            [150.0, 150.0],
            Button::Primary,
            Mods::default(),
            1,
            &engaged
        )
        .is_empty()
    );
    assert!(g.is_none());
}

#[test]
fn a_nudge_is_one_commit_of_the_selected_cards() {
    let e = engine();
    let sel = [1, 2];
    let events: Vec<EngineEvent> = e
        .nudge(&frame(&sel), [0.0, 1.0], true)
        .into_iter()
        .collect();
    let (_, after, label) = commit_of(&events);
    assert_eq!(label, "nudge");
    assert_eq!(frame_of(after, 1), [100.0, 150.0, 200.0, 120.0]);
    assert_eq!(frame_of(after, 2), [350.0, 150.0, 200.0, 120.0]);
    assert!(e.nudge(&frame(&[]), [0.0, 1.0], true).is_none());
}

#[test]
fn the_view_range_is_the_engine_range() {
    assert_eq!(crate::MIN_ZOOM, MIN_ZOOM as f32);
    assert_eq!(crate::MAX_ZOOM, MAX_ZOOM as f32);
}
