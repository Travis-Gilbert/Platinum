//! Tests for the hosted seams. They drive the mounted view through GPUI's real
//! event dispatch (`VisualTestContext`), so a handler that is written but not
//! wired to the root element fails here instead of passing a direct call.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    Entity, KeyDownEvent, Keystroke, Modifiers, MouseButton, Pixels, Point, ScrollDelta,
    ScrollWheelEvent, TestAppContext, TouchPhase, VisualTestContext, hsla, point, px,
};

use super::*;

fn style() -> WhiteboardStyleFn {
    Rc::new(|| WhiteboardStyle {
        bg: hsla(0.0, 0.0, 1.0, 1.0),
        grid: hsla(0.0, 0.0, 0.8, 1.0),
        text: hsla(0.0, 0.0, 0.3, 1.0),
        ink: hsla(0.0, 0.0, 0.0, 1.0),
        panel: hsla(0.0, 0.0, 0.95, 1.0),
        panel_strong: hsla(0.0, 0.0, 0.98, 1.0),
        accent: hsla(0.6, 0.8, 0.5, 1.0),
        selection: hsla(0.6, 0.9, 0.5, 1.0),
        swatches: Vec::new(),
    })
}

fn rect(id: u64, x: f32, y: f32) -> Element {
    Element {
        id,
        kind: ElementKind::Rect(BoxGeom {
            x,
            y,
            w: 100.0,
            h: 80.0,
            width: 2.0,
            rotation: 0.0,
        }),
        stroke: None,
        fill: None,
        label: None,
        label_color: None,
        styles: Vec::new(),
        mindmap: None,
    }
}

fn rect_x(scene: &Scene, id: u64) -> f32 {
    match scene.elements.iter().find(|e| e.id == id).map(|e| &e.kind) {
        Some(ElementKind::Rect(b)) => b.x,
        other => panic!("element {id} is not a rect: {other:?}"),
    }
}

/// Everything a host would see, collected both ways it can be told.
struct Probe {
    via_hook: Rc<RefCell<Vec<WhiteboardEvent>>>,
    via_subscription: Rc<RefCell<Vec<WhiteboardEvent>>>,
    persisted: Rc<RefCell<Vec<String>>>,
}

impl Probe {
    fn commits(&self) -> Vec<WhiteboardEvent> {
        self.via_hook
            .borrow()
            .iter()
            .filter(|e| matches!(e, WhiteboardEvent::Commit { .. }))
            .cloned()
            .collect()
    }
}

/// Mount a board in a window and attach a probe. Clicking the canvas once puts
/// focus on it, as a user's first click would.
fn mount(
    scene: Scene,
    history: HistoryMode,
    camera: CameraMode,
    cx: &mut TestAppContext,
) -> (Entity<WhiteboardView>, Probe, &mut VisualTestContext) {
    let (view, cx) = cx.add_window_view(|_, cx| WhiteboardView::new(scene, style(), cx));
    let probe = Probe {
        via_hook: Rc::default(),
        via_subscription: Rc::default(),
        persisted: Rc::default(),
    };
    view.update(cx, |view, cx| {
        view.set_history_mode(history, cx);
        view.set_camera_mode(camera, cx);
        let sink = probe.via_hook.clone();
        view.set_on_commit(Rc::new(move |event, _| {
            sink.borrow_mut().push(event.clone())
        }));
        let persisted = probe.persisted.clone();
        view.set_on_change(Rc::new(move |json, _, _| persisted.borrow_mut().push(json)));
    });
    let sink = probe.via_subscription.clone();
    cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &WhiteboardEvent, _| {
            sink.borrow_mut().push(event.clone())
        })
        .detach();
    });
    draw(cx);
    (view, probe, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// The canvas origin in window coordinates, so scene points can be clicked.
fn origin(view: &Entity<WhiteboardView>, cx: &mut VisualTestContext) -> Point<Pixels> {
    view.read_with(cx, |v, _| v.bounds.get().origin)
}

fn at(origin: Point<Pixels>, x: f32, y: f32) -> Point<Pixels> {
    point(origin.x + px(x), origin.y + px(y))
}

fn set_tool(view: &Entity<WhiteboardView>, tool: Tool, cx: &mut VisualTestContext) {
    view.update(cx, |v, cx| v.set_tool(tool, cx));
}

fn key(cx: &mut VisualTestContext, stroke: &str) {
    cx.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse(stroke).unwrap(),
        is_held: false,
        prefer_character_input: false,
    });
}

#[gpui::test]
fn hosted_pen_stroke_commits_once_when_it_ends_and_never_mid_stroke(cx: &mut TestAppContext) {
    // Wrong implementations rejected: a Commit per mouse move (the count after
    // the drag would be more than one), a Commit on mouse-down or mid-drag (the
    // assertion between the moves), a Commit that fires twice because both the
    // hook and the flush report it, and a hosted view that still pushes onto its
    // own undo stack (`history`/`redo` must stay empty).
    let (view, probe, cx) = mount(Scene::default(), HistoryMode::Hosted, CameraMode::Local, cx);
    let o = origin(&view, cx);
    set_tool(&view, Tool::Pen, cx);

    cx.simulate_mouse_down(at(o, 200.0, 200.0), MouseButton::Left, Modifiers::none());
    for step in 1..=8 {
        cx.simulate_mouse_move(
            at(o, 200.0 + step as f32 * 15.0, 200.0 + step as f32 * 6.0),
            MouseButton::Left,
            Modifiers::none(),
        );
        assert!(
            probe.commits().is_empty(),
            "committed mid-stroke at step {step}"
        );
    }
    assert!(
        probe.via_subscription.borrow().is_empty(),
        "event emitted mid-stroke"
    );
    cx.simulate_mouse_up(at(o, 320.0, 248.0), MouseButton::Left, Modifiers::none());

    let commits = probe.commits();
    assert_eq!(commits.len(), 1, "exactly one Commit per completed gesture");
    let WhiteboardEvent::Commit {
        before,
        after,
        label,
    } = &commits[0]
    else {
        unreachable!()
    };
    assert!(before.elements.is_empty());
    assert_eq!(after.elements.len(), 1);
    assert!(matches!(after.elements[0].kind, ElementKind::Draw(_)));
    assert_eq!(label, "draw");
    // Both delivery routes agree, and the host's own persistence ran once.
    assert_eq!(
        probe.via_subscription.borrow().as_slice(),
        commits.as_slice()
    );
    assert_eq!(probe.persisted.borrow().len(), 1);
    view.read_with(cx, |v, _| {
        assert!(
            v.history.is_empty() && v.redo.is_empty(),
            "local undo stack used in Hosted"
        );
        assert_eq!(v.scene().elements.len(), 1);
    });
}

#[gpui::test]
fn local_history_is_unchanged_by_default(cx: &mut TestAppContext) {
    // Positive control for the test above: the same gesture on a default view
    // does use the local stack and emits nothing, so the previous test's empty
    // stack means something. Also pins that standalone undo still works.
    let (view, probe, cx) = mount(Scene::default(), HistoryMode::Local, CameraMode::Local, cx);
    let o = origin(&view, cx);
    set_tool(&view, Tool::Pen, cx);
    cx.simulate_mouse_down(at(o, 200.0, 200.0), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(at(o, 260.0, 230.0), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at(o, 300.0, 250.0), MouseButton::Left, Modifiers::none());

    view.read_with(cx, |v, _| {
        assert_eq!(v.history.len(), 1);
        assert_eq!(v.scene().elements.len(), 1);
    });
    assert!(probe.via_hook.borrow().is_empty() && probe.via_subscription.borrow().is_empty());

    draw(cx);
    key(cx, "cmd-z");
    view.read_with(cx, |v, _| {
        assert!(v.scene().elements.is_empty(), "local undo no longer works");
        assert_eq!(v.redo.len(), 1);
    });
    assert!(
        probe.via_hook.borrow().is_empty(),
        "Local mode must not ask a host to undo"
    );
}

#[gpui::test]
fn hosted_move_is_one_commit_and_a_click_that_moves_nothing_is_none(cx: &mut TestAppContext) {
    // Wrong implementations rejected: reporting a Commit for a selection click
    // (a no-op gesture), and reporting the move per frame.
    let scene = Scene {
        elements: vec![rect(1, 100.0, 100.0), rect(2, 400.0, 100.0)],
        ..Scene::default()
    };
    let (view, probe, cx) = mount(scene, HistoryMode::Hosted, CameraMode::Local, cx);
    let o = origin(&view, cx);
    set_tool(&view, Tool::Select, cx);

    // A click that selects and releases in place changes nothing.
    cx.simulate_mouse_down(at(o, 150.0, 140.0), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at(o, 150.0, 140.0), MouseButton::Left, Modifiers::none());
    assert!(
        probe.commits().is_empty(),
        "a plain click reported a Commit"
    );

    // A drag moves element 1 by (+60, +30).
    cx.simulate_mouse_down(at(o, 150.0, 140.0), MouseButton::Left, Modifiers::none());
    for dx in [10.0, 25.0, 40.0, 60.0] {
        cx.simulate_mouse_move(
            at(o, 150.0 + dx, 140.0 + dx / 2.0),
            MouseButton::Left,
            Modifiers::none(),
        );
        assert!(probe.commits().is_empty(), "committed mid-drag at +{dx}");
    }
    cx.simulate_mouse_up(at(o, 210.0, 170.0), MouseButton::Left, Modifiers::none());

    let commits = probe.commits();
    assert_eq!(commits.len(), 1);
    let WhiteboardEvent::Commit {
        before,
        after,
        label,
    } = &commits[0]
    else {
        unreachable!()
    };
    assert_eq!(label, "move");
    assert_eq!(
        rect_x(before, 1),
        100.0,
        "`before` must be the scene at gesture start"
    );
    assert!(rect_x(after, 1) > 100.0);
    assert_eq!(
        rect_x(before, 2),
        rect_x(after, 2),
        "an unselected element moved"
    );
    view.read_with(cx, |v, _| assert!(v.history.is_empty()));
}

#[gpui::test]
fn hosted_undo_and_redo_keys_request_and_change_nothing(cx: &mut TestAppContext) {
    // Wrong implementation rejected: binding the keys to a local pop (the scene
    // would change and nothing would be requested).
    let scene = Scene {
        elements: vec![rect(1, 100.0, 100.0)],
        ..Scene::default()
    };
    let (view, probe, cx) = mount(scene.clone(), HistoryMode::Hosted, CameraMode::Local, cx);
    let o = origin(&view, cx);
    // A click on empty canvas focuses the board without editing anything.
    set_tool(&view, Tool::Select, cx);
    cx.simulate_mouse_down(at(o, 700.0, 500.0), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at(o, 700.0, 500.0), MouseButton::Left, Modifiers::none());
    draw(cx);
    // Plant a local snapshot a buggy hosted view could pop: a correct one never
    // looks at it, so the scene must stay exactly as it is.
    view.update(cx, |v, _| v.history.push(Scene::default()));

    key(cx, "cmd-z");
    key(cx, "cmd-shift-z");
    let seen = probe.via_hook.borrow().clone();
    assert_eq!(
        seen,
        [
            WhiteboardEvent::UndoRequested,
            WhiteboardEvent::RedoRequested
        ]
    );
    assert_eq!(probe.via_subscription.borrow().as_slice(), seen.as_slice());
    view.read_with(cx, |v, _| {
        assert_eq!(v.scene().elements, scene.elements);
        assert_eq!(v.history.len(), 1, "hosted undo touched the local stack");
        assert!(v.redo.is_empty());
    });
}

#[gpui::test]
fn apply_external_replaces_elements_without_committing_or_persisting(cx: &mut TestAppContext) {
    // Wrong implementations rejected: an `apply_external` that emits a Commit
    // (the host would see its own undo as a new edit, and loop), one that calls
    // `on_change` (an echo write), and one that clobbers the camera.
    let (view, probe, cx) = mount(Scene::default(), HistoryMode::Hosted, CameraMode::Local, cx);
    let o = origin(&view, cx);
    set_tool(&view, Tool::Pen, cx);
    cx.simulate_mouse_down(at(o, 200.0, 200.0), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(at(o, 250.0, 230.0), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at(o, 300.0, 260.0), MouseButton::Left, Modifiers::none());
    let drawn = probe.commits();
    assert_eq!(drawn.len(), 1);
    let (events_before, writes_before) = (
        probe.via_hook.borrow().len(),
        probe.persisted.borrow().len(),
    );

    // The host answers an undo by pushing the "before" scene back, plus a camera
    // the scene happens to carry (which must be ignored).
    let WhiteboardEvent::Commit { before, .. } = &drawn[0] else {
        unreachable!()
    };
    let mut restored = before.clone();
    restored.camera = Camera {
        x: 999.0,
        y: 999.0,
        zoom: 3.0,
    };
    view.update(cx, |v, cx| v.apply_external(restored, cx));

    assert_eq!(
        probe.via_hook.borrow().len(),
        events_before,
        "apply_external emitted"
    );
    assert_eq!(probe.via_subscription.borrow().len(), events_before);
    assert_eq!(
        probe.persisted.borrow().len(),
        writes_before,
        "apply_external wrote"
    );
    view.read_with(cx, |v, _| {
        assert!(v.scene().elements.is_empty());
        assert_eq!(v.camera(), Camera::default());
        assert!(v.history.is_empty());
    });

    // And the very next gesture is a fresh Commit whose `before` is the state
    // the host pushed, not the stale pre-undo scene.
    cx.simulate_mouse_down(at(o, 300.0, 300.0), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(at(o, 340.0, 330.0), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at(o, 380.0, 360.0), MouseButton::Left, Modifiers::none());
    let all = probe.commits();
    assert_eq!(all.len(), 2);
    let WhiteboardEvent::Commit { before: b2, .. } = &all[1] else {
        unreachable!()
    };
    assert!(b2.elements.is_empty());
}

#[gpui::test]
fn an_abandoned_empty_text_box_is_a_no_op_not_a_commit(cx: &mut TestAppContext) {
    // Wrong implementations rejected: committing when the text tool places the
    // empty element (a Commit mid-edit), and committing the add/remove pair as a
    // change when the empty box is dropped again.
    let (view, probe, cx) = mount(Scene::default(), HistoryMode::Hosted, CameraMode::Local, cx);
    let o = origin(&view, cx);
    set_tool(&view, Tool::Text, cx);
    cx.simulate_mouse_down(at(o, 300.0, 300.0), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at(o, 300.0, 300.0), MouseButton::Left, Modifiers::none());
    view.read_with(cx, |v, _| {
        assert!(
            v.editing.is_some(),
            "the text tool should be editing the new box"
        );
        assert_eq!(v.scene().elements.len(), 1);
    });
    assert!(
        probe.commits().is_empty(),
        "committed while the text box was still open"
    );

    // Click away without typing: the empty box is removed and nothing changed.
    set_tool(&view, Tool::Select, cx);
    draw(cx);
    cx.simulate_mouse_down(at(o, 700.0, 500.0), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at(o, 700.0, 500.0), MouseButton::Left, Modifiers::none());
    view.read_with(cx, |v, _| {
        assert!(v.editing.is_none());
        assert!(v.scene().elements.is_empty());
    });
    assert!(
        probe.commits().is_empty(),
        "an empty text box that vanished was reported"
    );
}

#[gpui::test]
fn host_api_edits_commit_once_without_a_gesture(cx: &mut TestAppContext) {
    // `add_embed` is called by the host mid-update and never flushes; a hosted
    // view must still report it, once.
    let (view, probe, cx) = mount(Scene::default(), HistoryMode::Hosted, CameraMode::Local, cx);
    view.update(cx, |v, cx| v.add_embed(7, "A page", 50.0, 60.0, cx));
    let commits = probe.commits();
    assert_eq!(commits.len(), 1);
    let WhiteboardEvent::Commit {
        before,
        after,
        label,
    } = &commits[0]
    else {
        unreachable!()
    };
    assert!(before.elements.is_empty());
    assert_eq!(after.elements.len(), 1);
    assert_eq!(label, "add page");
}

#[gpui::test]
fn context_menu_hook_replaces_the_builtin_menu(cx: &mut TestAppContext) {
    // Wrong implementations rejected: a hook that fires but still opens the
    // built-in menu (`context_menu` would be Some), a hook handed the window
    // point instead of the scene point, and a hit-test that ignores z-order.
    let scene = Scene {
        elements: vec![rect(1, 100.0, 100.0), rect(2, 140.0, 120.0)],
        camera: Camera {
            x: 10.0,
            y: 20.0,
            zoom: 2.0,
        },
    };
    let (view, _probe, cx) = mount(scene, HistoryMode::Local, CameraMode::Local, cx);
    let o = origin(&view, cx);
    // Paste wired + a selection: the built-in menu would open for a right-click.
    view.update(cx, |v, cx| {
        v.set_on_paste(Rc::new(|_, _| None));
        v.select(&[1], cx);
    });
    draw(cx);

    // Positive control: with no hook the built-in menu opens.
    cx.simulate_mouse_down(at(o, 300.0, 300.0), MouseButton::Right, Modifiers::none());
    view.read_with(cx, |v, _| {
        assert!(v.context_menu.is_some(), "control: built-in menu")
    });
    view.update(cx, |v, _| v.context_menu = None);

    let calls: Rc<RefCell<Vec<([f32; 2], Option<u64>, Point<Pixels>)>>> = Rc::default();
    let sink = calls.clone();
    view.update(cx, |v, _| {
        v.set_on_context_menu(Rc::new(move |scene_pt, hit, window_pt, _, _| {
            sink.borrow_mut().push((scene_pt, hit, window_pt));
        }));
    });
    draw(cx);

    // Window point (300, 300) at zoom 2, camera (10, 20) is scene (160, 170):
    // inside both rects (element 2 is on top), so the hit is 2.
    cx.simulate_mouse_down(at(o, 300.0, 300.0), MouseButton::Right, Modifiers::none());
    // Window point (20, 20) is scene (20, 30): empty canvas.
    cx.simulate_mouse_down(at(o, 20.0, 20.0), MouseButton::Right, Modifiers::none());

    let calls = calls.borrow();
    assert_eq!(calls.len(), 2, "hook should fire once per right-click");
    assert_eq!(calls[0].1, Some(2));
    assert!(
        (calls[0].0[0] - 160.0).abs() < 0.01 && (calls[0].0[1] - 170.0).abs() < 0.01,
        "{:?}",
        calls[0].0
    );
    assert_eq!(calls[0].2, at(o, 300.0, 300.0));
    assert_eq!(calls[1].1, None);
    view.read_with(cx, |v, _| {
        assert!(
            v.context_menu.is_none(),
            "built-in menu opened over the hook"
        )
    });
}

#[gpui::test]
fn hosted_camera_reports_requests_and_moves_only_on_set_camera(cx: &mut TestAppContext) {
    // Wrong implementations rejected: a hosted view that still pans itself (the
    // camera assertion after the scroll), one that reports nothing, and one that
    // dirties the scene (an on_change write for a camera the host owns).
    let (view, probe, cx) = mount(Scene::default(), HistoryMode::Local, CameraMode::Hosted, cx);
    let o = origin(&view, cx);
    cx.simulate_event(ScrollWheelEvent {
        position: at(o, 300.0, 300.0),
        delta: ScrollDelta::Pixels(point(px(10.0), px(40.0))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    // Ctrl-scroll zooms about the pointer.
    cx.simulate_event(ScrollWheelEvent {
        position: at(o, 300.0, 300.0),
        delta: ScrollDelta::Pixels(point(px(0.0), px(80.0))),
        modifiers: Modifiers::control(),
        touch_phase: TouchPhase::Moved,
    });

    let seen = probe.via_hook.borrow().clone();
    assert_eq!(seen.len(), 2, "{seen:?}");
    let WhiteboardEvent::CameraRequested { camera, intent } = &seen[0] else {
        panic!("{seen:?}")
    };
    assert!(matches!(intent, CameraIntent::Pan { dx, dy } if *dx == 10.0 && *dy == 40.0));
    assert_eq!((camera.x, camera.y), (-10.0, -40.0));
    assert!(matches!(
        &seen[1],
        WhiteboardEvent::CameraRequested { intent: CameraIntent::ZoomAbout { .. }, camera } if camera.zoom > 1.0
    ));
    view.read_with(cx, |v, _| {
        assert_eq!(
            v.camera(),
            Camera::default(),
            "a hosted camera moved itself"
        );
        assert!(!v.dirty);
    });
    assert!(probe.persisted.borrow().is_empty());

    // The host agrees and applies the camera it was offered; zoom is clamped.
    let WhiteboardEvent::CameraRequested { camera, .. } = seen[0].clone() else {
        unreachable!()
    };
    view.update(cx, |v, cx| v.set_camera(camera, cx));
    view.read_with(cx, |v, _| assert_eq!(v.camera(), camera));
    view.update(cx, |v, cx| {
        v.set_camera(
            Camera {
                x: 0.0,
                y: 0.0,
                zoom: 500.0,
            },
            cx,
        );
    });
    view.read_with(cx, |v, _| assert_eq!(v.camera().zoom, MAX_ZOOM));
    // The host cannot feed the dot grid a pan so large that f32 stops advancing.
    view.update(cx, |v, cx| {
        v.set_camera(
            Camera {
                x: 7e10,
                y: -7e10,
                zoom: MAX_ZOOM,
            },
            cx,
        );
    });
    view.read_with(cx, |v, _| {
        assert_eq!((v.camera().x, v.camera().y), (MAX_PAN, -MAX_PAN));
        assert!(!v.dirty);
    });
    assert!(probe.persisted.borrow().is_empty());
    assert_eq!(probe.via_hook.borrow().len(), seen.len());
    // A non-finite camera from a buggy host is refused, not stored.
    view.update(cx, |v, cx| {
        v.set_camera(
            Camera {
                x: f32::NAN,
                y: 0.0,
                zoom: 1.0,
            },
            cx,
        );
    });
    view.read_with(cx, |v, _| assert_eq!(v.camera().zoom, MAX_ZOOM));
}

#[gpui::test]
fn local_camera_still_pans_itself(cx: &mut TestAppContext) {
    // Positive control for the hosted-camera test: the default view moves its
    // own camera on the same input and reports nothing.
    let (view, probe, cx) = mount(Scene::default(), HistoryMode::Local, CameraMode::Local, cx);
    let o = origin(&view, cx);
    cx.simulate_event(ScrollWheelEvent {
        position: at(o, 300.0, 300.0),
        delta: ScrollDelta::Pixels(point(px(10.0), px(40.0))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    view.read_with(cx, |v, _| {
        assert_eq!((v.camera().x, v.camera().y), (-10.0, -40.0))
    });
    assert!(probe.via_hook.borrow().is_empty());
    assert_eq!(
        probe.persisted.borrow().len(),
        0,
        "camera persists on the next flush, as before"
    );
}

#[gpui::test]
fn retained_elements_survive_an_edit_and_their_ids_are_reserved(cx: &mut TestAppContext) {
    // Wrong implementations rejected: a view that saves `scene.to_json()` (the
    // retained element would vanish on the first edit) and one that hands a new
    // element the id of an element it is holding aside.
    let kept = r#"{"id":4,"kind":{"hologram":{"k":1}},"zz":1.0e2}"#;
    let doc = format!(
        r#"{{"camera":{{"x":0.0,"y":0.0,"zoom":1.0}},"elements":[{},{kept}]}}"#,
        serde_json::to_string(&rect(3, 500.0, 400.0)).unwrap()
    );
    let parsed = Scene::parse(&doc).unwrap();
    assert_eq!(parsed.retained.len(), 1);
    let (view, cx) = cx.add_window_view(|_, cx| WhiteboardView::from_parsed(parsed, style(), cx));
    let persisted: Rc<RefCell<Vec<String>>> = Rc::default();
    let sink = persisted.clone();
    view.update(cx, |v, _| {
        v.set_on_change(Rc::new(move |json, _, _| sink.borrow_mut().push(json)))
    });
    draw(cx);
    view.read_with(cx, |v, _| assert_eq!(v.retained().len(), 1));

    let o = origin(&view, cx);
    set_tool(&view, Tool::Pen, cx);
    cx.simulate_mouse_down(at(o, 100.0, 100.0), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(at(o, 140.0, 130.0), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at(o, 180.0, 160.0), MouseButton::Left, Modifiers::none());

    let saved = persisted.borrow();
    assert_eq!(saved.len(), 1);
    assert!(
        saved[0].contains(kept),
        "the unreadable element was dropped on save: {}",
        saved[0]
    );
    let reread = Scene::parse(&saved[0]).unwrap();
    assert_eq!(reread.scene.elements.len(), 2);
    assert_eq!(reread.retained.len(), 1);
    // The only readable element besides the pen stroke has id 3, so without a
    // reservation the stroke would take id 4, the held-aside element's id.
    let ids: Vec<u64> = reread.scene.elements.iter().map(|e| e.id).collect();
    assert!(
        ids.iter().all(|id| *id != 4),
        "new element reused a held-aside id: {ids:?}"
    );
}

#[gpui::test]
fn retained_elements_are_announced_in_both_views(cx: &mut TestAppContext) {
    // Wrong implementations rejected: a view that holds unreadable elements
    // without saying so (the board looks complete while part of it is not
    // shown), a read-only view that drops the note, and a note that counts
    // something other than the retained elements.
    const ONE: &str = "whiteboard-retained-note: 1 unreadable element kept, not shown";
    const TWO: &str = "whiteboard-retained-note: 2 unreadable elements kept, not shown";
    let doc = format!(
        r#"{{"camera":{{"x":0.0,"y":0.0,"zoom":1.0}},"elements":[{},{},{}]}}"#,
        serde_json::to_string(&rect(3, 500.0, 400.0)).unwrap(),
        r#"{"id":4,"kind":{"hologram":{"k":1}}}"#,
        r#"{"id":5,"kind":{"rect":{"x":"wide"}}}"#,
    );
    let parsed = Scene::parse(&doc).unwrap();
    assert_eq!(parsed.retained.len(), 2);
    let (view, cx) = cx.add_window_view(|_, cx| WhiteboardView::from_parsed(parsed, style(), cx));
    // Control: the selector is live only while elements are held aside.
    view.update(cx, |v, cx| {
        v.retained.clear();
        cx.notify();
    });
    draw(cx);
    assert!(cx.debug_bounds(ONE).is_none() && cx.debug_bounds(TWO).is_none());
    view.update(cx, |v, cx| {
        v.retained = Scene::parse(&doc).unwrap().retained;
        cx.notify();
    });
    draw(cx);
    assert!(cx.debug_bounds(TWO).is_some(), "editable view has no note");
    assert!(cx.debug_bounds(ONE).is_none());

    view.update(cx, |v, cx| v.set_read_only(true, cx));
    draw(cx);
    assert!(cx.debug_bounds(TWO).is_some(), "read-only view has no note");
}

fn text(id: u64, x: f32, y: f32, content: &str) -> Element {
    Element {
        id,
        kind: ElementKind::Text(TextGeom {
            x,
            y,
            content: content.to_owned(),
            size: 24.0,
            rotation: 0.0,
            measured_w: 0.0,
            measured_h: 0.0,
        }),
        stroke: None,
        fill: None,
        label: None,
        label_color: None,
        styles: Vec::new(),
        mindmap: None,
    }
}

fn measured(scene: &Scene, id: u64) -> (f32, f32) {
    match scene.elements.iter().find(|e| e.id == id).map(|e| &e.kind) {
        Some(ElementKind::Text(t)) => (t.measured_w, t.measured_h),
        other => panic!("element {id} is not text: {other:?}"),
    }
}

#[test]
fn text_equality_ignores_the_measured_extent() {
    // Wrong implementation rejected: a derived `PartialEq` on `TextGeom`, which
    // would make a repaint's measurement look like an edit. Every persisted
    // field still counts.
    let a = text(1, 10.0, 20.0, "hi");
    let mut b = a.clone();
    if let ElementKind::Text(t) = &mut b.kind {
        t.measured_w = 41.5;
        t.measured_h = 18.0;
    }
    assert_eq!(a, b);
    let edits: [fn(&mut TextGeom); 5] = [
        |t: &mut TextGeom| t.x += 1.0,
        |t: &mut TextGeom| t.y += 1.0,
        |t: &mut TextGeom| t.content.push('!'),
        |t: &mut TextGeom| t.size += 1.0,
        |t: &mut TextGeom| t.rotation += 0.1,
    ];
    for edit in edits {
        let mut c = a.clone();
        if let ElementKind::Text(t) = &mut c.kind {
            edit(t);
        }
        assert_ne!(a, c);
    }
}

#[gpui::test]
fn a_repaint_that_measures_text_is_not_a_commit(cx: &mut TestAppContext) {
    // A gesture's `before` can hold a text the board had not measured yet (a
    // host applied the scene and the gesture began before the frame that
    // measures it); by mouse-up the frame has filled in the measured extent.
    // Input cannot reach that order (a click always follows a frame), so the
    // gesture's snapshot is set back to unmeasured directly. The drag goes out
    // and back, so nothing the board stores changed.
    // Wrong implementation rejected: comparing the measured extent when deciding
    // whether a gesture changed anything (a "move" Commit that moved nothing).
    let (view, probe, cx) = mount(Scene::default(), HistoryMode::Hosted, CameraMode::Local, cx);
    view.update(cx, |v, cx| {
        v.apply_external(
            Scene {
                elements: vec![text(7, 100.0, 100.0, "measure me")],
                ..Scene::default()
            },
            cx,
        )
    });
    draw(cx);
    let o = origin(&view, cx);
    set_tool(&view, Tool::Select, cx);
    let start = at(o, 110.0, 110.0);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        at(o, 150.0, 110.0),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    cx.simulate_mouse_move(start, Some(MouseButton::Left), Modifiers::none());
    view.update(cx, |v, _| {
        assert_eq!(v.selection(), &[7], "the press missed the text");
        let before = v.hosted_before.as_mut().expect("the drag took a snapshot");
        for e in &mut before.elements {
            if let ElementKind::Text(t) = &mut e.kind {
                (t.measured_w, t.measured_h) = (0.0, 0.0);
            }
        }
    });
    draw(cx);
    view.read_with(cx, |v, _| {
        assert!(measured(&v.scene, 7).0 > 0.0, "the frame did not measure");
        let ElementKind::Text(t) = &v.scene.elements[0].kind else {
            panic!("not text")
        };
        assert_eq!((t.x, t.y), (100.0, 100.0), "the drag did not come back");
    });
    cx.simulate_mouse_up(start, MouseButton::Left, Modifiers::none());
    assert!(
        probe.commits().is_empty(),
        "a repaint was reported as an edit: {:?}",
        probe.commits()
    );
}

fn rect_geom(scene: &Scene, id: u64) -> BoxGeom {
    match scene.elements.iter().find(|e| e.id == id).map(|e| &e.kind) {
        Some(ElementKind::Rect(b)) => *b,
        other => panic!("element {id} is not a rect: {other:?}"),
    }
}

fn drag(cx: &mut VisualTestContext, from: Point<Pixels>, to: Point<Pixels>) {
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    for step in 1..=4 {
        let t = step as f32 / 4.0;
        cx.simulate_mouse_move(
            point(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t),
            MouseButton::Left,
            Modifiers::none(),
        );
    }
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
}

#[gpui::test]
fn a_held_element_is_never_moved_or_resized_by_a_gesture(cx: &mut TestAppContext) {
    // Wrong implementations rejected: a selection drag that carries the held
    // member along with the rest, and a held element that still offers its
    // corner resize grip.
    let scene = Scene {
        elements: vec![rect(1, 100.0, 100.0), rect(2, 400.0, 100.0)],
        ..Scene::default()
    };
    let (view, probe, cx) = mount(scene, HistoryMode::Hosted, CameraMode::Local, cx);
    let o = origin(&view, cx);
    set_tool(&view, Tool::Select, cx);
    view.update(cx, |v, cx| {
        v.set_constraints([1], [], cx);
        v.select(&[1, 2], cx);
    });
    draw(cx);

    // Dragging the free member moves it and leaves the held one in place.
    drag(cx, at(o, 450.0, 140.0), at(o, 510.0, 170.0));
    view.read_with(cx, |v, _| {
        assert_eq!(rect_x(&v.scene, 2), 460.0, "the free member did not move");
        assert_eq!(rect_x(&v.scene, 1), 100.0, "the held member moved");
    });
    let commits = probe.commits();
    assert_eq!(commits.len(), 1);
    let WhiteboardEvent::Commit { after, .. } = &commits[0] else {
        unreachable!()
    };
    assert_eq!(rect_x(after, 1), 100.0);

    // The held element alone: its bottom-right corner offers no resize.
    view.update(cx, |v, cx| v.select(&[1], cx));
    draw(cx);
    let held_before = view.read_with(cx, |v, _| rect_geom(&v.scene, 1));
    drag(cx, at(o, 200.0, 180.0), at(o, 260.0, 240.0));
    view.read_with(cx, |v, _| {
        assert_eq!(
            rect_geom(&v.scene, 1),
            held_before,
            "a held element changed"
        )
    });
    assert_eq!(probe.commits().len(), 1, "a held element reported an edit");
}

#[gpui::test]
fn an_axis_aligned_element_is_never_offered_rotation(cx: &mut TestAppContext) {
    // Wrong implementations rejected: a rotate grip on an axis-aligned element
    // selected alone, and a group rotate grip on a selection that contains one.
    // A free rect still rotates, so the grip positions used here are live.
    let scene = Scene {
        elements: vec![rect(1, 100.0, 300.0), rect(2, 400.0, 300.0)],
        ..Scene::default()
    };
    let (view, _probe, cx) = mount(scene, HistoryMode::Hosted, CameraMode::Local, cx);
    let o = origin(&view, cx);
    set_tool(&view, Tool::Select, cx);
    view.update(cx, |v, cx| v.set_constraints([], [1], cx));
    let rotation = |view: &Entity<WhiteboardView>, cx: &mut VisualTestContext, id| {
        view.read_with(cx, |v, _| rect_geom(&v.scene, id).rotation)
    };
    let grip = |view: &Entity<WhiteboardView>, cx: &mut VisualTestContext, id| {
        view.read_with(cx, |v, _| {
            let kind = &v.scene.elements.iter().find(|e| e.id == id).unwrap().kind;
            let (x, y) = crate::geometry::rotate_handle_screen(kind, v.scene.camera, o);
            point(px(x), px(y))
        })
    };
    let group_grip = |view: &Entity<WhiteboardView>, cx: &mut VisualTestContext| {
        view.read_with(cx, |v, _| {
            let bb = v.selection_bbox().unwrap();
            let (x, y) = crate::geometry::rotate_handle_for_bbox(bb, v.scene.camera, o);
            point(px(x), px(y))
        })
    };
    let sideways = |p: Point<Pixels>| point(p.x + px(80.0), p.y + px(80.0));

    // Control: the free rect turns from its grip.
    view.update(cx, |v, cx| v.select(&[2], cx));
    draw(cx);
    let p = grip(&view, cx, 2);
    drag(cx, p, sideways(p));
    assert!(
        rotation(&view, cx, 2) != 0.0,
        "the free rect did not rotate"
    );

    // The axis-aligned rect alone offers no grip.
    view.update(cx, |v, cx| v.select(&[1], cx));
    draw(cx);
    let p = grip(&view, cx, 1);
    drag(cx, p, sideways(p));
    assert_eq!(rotation(&view, cx, 1), 0.0, "an axis-aligned rect rotated");

    // Nor does a group that contains it.
    view.update(cx, |v, cx| v.select(&[1, 2], cx));
    draw(cx);
    let turned = rotation(&view, cx, 2);
    let p = group_grip(&view, cx);
    drag(cx, p, sideways(p));
    assert_eq!(
        rotation(&view, cx, 1),
        0.0,
        "a group turned an axis-aligned rect"
    );
    assert_eq!(
        rotation(&view, cx, 2),
        turned,
        "a group with an axis-aligned member turned"
    );
}

fn embed(id: u64, x: f32, y: f32) -> Element {
    Element {
        id,
        kind: ElementKind::Embed(EmbedGeom {
            page_id: id as i64,
            title: format!("card {id}"),
            x,
            y,
            w: 200.0,
            h: 120.0,
        }),
        stroke: None,
        fill: None,
        label: None,
        label_color: None,
        styles: Vec::new(),
        mindmap: None,
    }
}

#[test]
fn an_embed_offers_its_four_edge_midpoints() {
    // Wrong implementation rejected: an embed that is not connector-capable
    // (upstream), which offers no points at all.
    let card = embed(1, 100.0, 50.0);
    assert!(crate::geometry::connector_capable(&card.kind));
    assert_eq!(
        crate::geometry::connector_points(&card.kind),
        vec![
            [200.0, 50.0],
            [300.0, 110.0],
            [200.0, 170.0],
            [100.0, 110.0]
        ]
    );
}

#[gpui::test]
fn a_connection_drawn_between_two_cards_anchors_to_both(cx: &mut TestAppContext) {
    // Wrong implementation rejected: embeds that offer no connector, so the
    // drag from the selected card's right connector draws nothing, or a line
    // that floats free of the card it was dropped on.
    let scene = Scene {
        elements: vec![embed(1, 100.0, 300.0), embed(2, 500.0, 300.0)],
        ..Scene::default()
    };
    let (view, probe, cx) = mount(scene, HistoryMode::Hosted, CameraMode::Local, cx);
    let o = origin(&view, cx);
    set_tool(&view, Tool::Select, cx);
    view.update(cx, |v, cx| v.select(&[1], cx));
    draw(cx);
    // Card 1's right connector button sits outside its right edge midpoint.
    let button = view.read_with(cx, |v, _| {
        crate::paint::connector_button_centers(&v.scene.elements[0].kind, v.scene.camera, o)[1]
    });
    drag(cx, button, at(o, 500.0, 360.0));

    let commits = probe.commits();
    assert_eq!(commits.len(), 1, "no connection was drawn");
    let WhiteboardEvent::Commit { after, .. } = &commits[0] else {
        unreachable!()
    };
    let arrow = after
        .elements
        .iter()
        .find_map(|e| match &e.kind {
            ElementKind::Arrow(s) => Some(s.clone()),
            _ => None,
        })
        .expect("the connection is an arrow");
    assert_eq!(
        arrow.start_anchor,
        Some(SegmentAnchor {
            element_id: 1,
            connector: 1
        })
    );
    assert_eq!(
        arrow.end_anchor,
        Some(SegmentAnchor {
            element_id: 2,
            connector: 3
        })
    );
}
