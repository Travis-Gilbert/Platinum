//! Tests for the Space adapter (Canvas 2.5 8.2, checks 31 and 32). The pure
//! tests drive `SpaceBoard` against a real `theorem_space` session with the
//! in-memory owners theorem-space tests itself with; the GPUI tests at the end
//! drive a mounted board through real event dispatch, so the seam is proven
//! from pointer input to the Space and back.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    Entity, Modifiers, MouseButton, Pixels, Point, ScrollDelta, ScrollWheelEvent, TestAppContext,
    TouchPhase, VisualTestContext, hsla, point, px,
};
use serde_json::{Value, json};
use theorem_space::id::{kind, project_space_id};
use theorem_space::{
    Actor, AgentGrant, CommandContext, ExecutionPolicy, INK_FORMAT, IdMint, InkContent,
    MemoryBindings, MemoryCatalog, MemoryRelationships, MemorySpaceStore, Navigator, ObjectRef,
    ObjectSummary, Outcome, PlacementId, PlacementTarget, PortDescriptor, PortDirection, PortRef,
    Rect, Space, SpaceCommand, SpaceId, SpaceRole, SpaceSession, world_space_id,
};

use super::space::*;
use super::*;

// ---- the owners ------------------------------------------------------------

struct World {
    catalog: MemoryCatalog,
    relationships: MemoryRelationships,
    bindings: MemoryBindings,
    store: MemorySpaceStore,
    nav: Navigator,
    mint: IdMint,
    grant: AgentGrant,
}

fn obj(k: &str, id: &str) -> ObjectRef {
    ObjectRef::new(k, id)
}

impl World {
    fn new() -> Self {
        let mut catalog = MemoryCatalog::default();
        let alpha = obj(kind::PROJECT, "alpha");
        catalog.insert(ObjectSummary::new(alpha.clone(), "Alpha", "Project"));
        for id in ["sim-a", "sim-b"] {
            let mut s = ObjectSummary::new(obj(kind::SIMULATION, id), id, "Simulation");
            s.projects = vec![alpha.clone()];
            catalog.insert(s);
        }
        let mut doc = ObjectSummary::new(obj(kind::DOCUMENT, "brief"), "Brief", "Document");
        doc.projects = vec![alpha];
        catalog.insert(doc);
        let mut bindings = MemoryBindings::default();
        bindings.ports.push((
            obj(kind::SIMULATION, "sim-a"),
            PortDescriptor {
                port: "state".into(),
                direction: PortDirection::Output,
                shape: "table".into(),
            },
        ));
        bindings.ports.push((
            obj(kind::SIMULATION, "sim-b"),
            PortDescriptor {
                port: "seed".into(),
                direction: PortDirection::Input,
                shape: "table".into(),
            },
        ));
        Self {
            catalog,
            relationships: MemoryRelationships::default(),
            bindings,
            store: MemorySpaceStore::default(),
            nav: Navigator::new(world_space_id()),
            mint: IdMint::new("desk"),
            grant: AgentGrant::default(),
        }
    }

    fn ctx(&mut self) -> CommandContext<'_> {
        CommandContext {
            actor: Actor::User,
            catalog: &self.catalog,
            relationships: &mut self.relationships,
            bindings: &mut self.bindings,
            store: &mut self.store,
            nav: &mut self.nav,
            mint: &mut self.mint,
            policy: &self.grant,
            resolver: None,
            viewport: theorem_space::Point::new(1400.0, 900.0),
        }
    }

    fn run(&mut self, s: &mut SpaceSession, cmd: SpaceCommand) -> Outcome {
        let mut ctx = self.ctx();
        theorem_space::execute(s, &mut ctx, cmd)
            .unwrap_or_else(|e| panic!("{e:?}"))
            .outcome
    }

    fn apply(&mut self, s: &mut SpaceSession, route: Route) -> Applied {
        let mut ctx = self.ctx();
        apply(s, &mut ctx, route).unwrap_or_else(|e| panic!("{e:?}"))
    }

    fn project(&self, board: &mut SpaceBoard, s: &SpaceSession) -> Projection {
        let lines = connections(&s.space, &self.relationships, &self.bindings);
        let camera = self.nav.view(&s.space.id).camera;
        board.project(&s.space, &lines, &camera, &default_title)
    }
}

fn placed(outcome: Outcome) -> PlacementId {
    match outcome {
        Outcome::Committed { placements, .. } => placements[0].clone(),
        other => panic!("expected a commit, got {other:?}"),
    }
}

/// Every piece of a Space the adapter could disturb. Revision and history are
/// left out on purpose: an undo is a new revision of the same content.
fn content(space: &Space) -> Value {
    json!({
        "placements": space.placements,
        "groups": space.groups,
        "connections": space.connections,
        "route_hints": space.route_hints,
        "title": space.title,
    })
}

fn element(id: u64, kind: ElementKind) -> Element {
    Element {
        id,
        kind,
        stroke: None,
        fill: None,
        label: None,
        label_color: None,
        styles: Vec::new(),
        mindmap: None,
    }
}

fn shape(id: u64, x: f32, y: f32, w: f32, h: f32) -> Element {
    element(
        id,
        ElementKind::Rect(BoxGeom {
            x,
            y,
            w,
            h,
            width: 2.0,
            rotation: 0.0,
        }),
    )
}

fn stroke(id: u64, points: &[[f32; 2]]) -> Element {
    element(
        id,
        ElementKind::Draw(Stroke {
            points: points.to_vec(),
            width: 3.0,
        }),
    )
}

fn value(e: &Element) -> Value {
    serde_json::to_value(e).unwrap()
}

/// What a board-side gesture leaves behind: the scene with `f` applied.
fn edited(scene: &Scene, f: impl FnOnce(&mut Vec<Element>)) -> Scene {
    let mut after = scene.clone();
    f(&mut after.elements);
    after
}

fn find(scene: &Scene, id: u64) -> &Element {
    scene
        .elements
        .iter()
        .find(|e| e.id == id)
        .unwrap_or_else(|| panic!("element {id} is not on the board"))
}

fn find_mut(elements: &mut [Element], id: u64) -> &mut Element {
    elements.iter_mut().find(|e| e.id == id).unwrap()
}

fn embed(scene: &Scene, id: u64) -> EmbedGeom {
    match &find(scene, id).kind {
        ElementKind::Embed(g) => g.clone(),
        other => panic!("{id} is not a card: {other:?}"),
    }
}

fn commit(before: &Scene, after: &Scene, label: &str) -> WhiteboardEvent {
    WhiteboardEvent::Commit {
        before: before.clone(),
        after: after.clone(),
        label: label.to_string(),
    }
}

/// The V1 fixture: object cards, a note, a nested Space portal, a group, all
/// three connection classes, and a drawing whose arrow is anchored to its own
/// shape.
struct Fixture {
    w: World,
    s: SpaceSession,
    a: PlacementId,
    b: PlacementId,
    doc: PlacementId,
    note: PlacementId,
    portal: PlacementId,
    ink: PlacementId,
    child: SpaceId,
}

fn fixture() -> Fixture {
    let mut w = World::new();
    let project = obj(kind::PROJECT, "alpha");
    let mut s = SpaceSession::new(
        Space::new(
            project_space_id(&project),
            SpaceRole::Project { project },
            "Alpha",
        ),
        "desk",
    );
    let r = |x, y, w, h| Some(Rect::new(x, y, w, h));
    let a = placed(w.run(
        &mut s,
        SpaceCommand::Place {
            object: obj(kind::SIMULATION, "sim-a"),
            rect: r(100.0, 100.0, 200.0, 120.0),
            near: None,
        },
    ));
    let b = placed(w.run(
        &mut s,
        SpaceCommand::Place {
            object: obj(kind::SIMULATION, "sim-b"),
            rect: r(500.0, 100.0, 200.0, 120.0),
            near: None,
        },
    ));
    let doc = placed(w.run(
        &mut s,
        SpaceCommand::Place {
            object: obj(kind::DOCUMENT, "brief"),
            rect: r(100.0, 400.0, 200.0, 120.0),
            near: None,
        },
    ));
    let note = placed(w.run(
        &mut s,
        SpaceCommand::PlaceNote {
            text: "Hello\nworld".into(),
            rect: r(500.0, 400.0, 160.0, 100.0),
        },
    ));
    let child = SpaceId::new("space:child-plans");
    let portal = placed(w.run(
        &mut s,
        SpaceCommand::PlaceSpace {
            space: child.clone(),
            rect: r(900.0, 100.0, 220.0, 160.0),
        },
    ));
    w.run(
        &mut s,
        SpaceCommand::Group {
            placements: vec![a.clone(), doc.clone()],
            label: Some("Left".into()),
        },
    );
    w.run(
        &mut s,
        SpaceCommand::Connect {
            from: note.clone(),
            to: b.clone(),
            label: Some("see".into()),
        },
    );
    w.run(
        &mut s,
        SpaceCommand::Relate {
            from: a.clone(),
            to: doc.clone(),
            kind: "cites".into(),
            label: None,
        },
    );
    w.run(
        &mut s,
        SpaceCommand::Bind {
            source: PortRef {
                object: obj(kind::SIMULATION, "sim-a"),
                port: "state".into(),
            },
            target: PortRef {
                object: obj(kind::SIMULATION, "sim-b"),
                port: "seed".into(),
            },
            policy: ExecutionPolicy::default(),
        },
    );
    // A drawing in local coordinates: a box, and an arrow anchored to it.
    let mut arrow = element(
        2,
        ElementKind::Arrow(SegGeom {
            x1: 70.0,
            y1: 30.0,
            x2: 140.0,
            y2: 30.0,
            width: 2.0,
            style: SegmentStyle::Solid,
            start_anchor: Some(SegmentAnchor {
                element_id: 1,
                connector: 1,
            }),
            end_anchor: None,
        }),
    );
    arrow.label = Some("flow".into());
    let ink = placed(w.run(
        &mut s,
        SpaceCommand::PlaceInk {
            elements: vec![value(&shape(1, 10.0, 10.0, 60.0, 40.0)), value(&arrow)],
            rect: r(900.0, 400.0, 160.0, 80.0),
        },
    ));
    Fixture {
        w,
        s,
        a,
        b,
        doc,
        note,
        portal,
        ink,
        child,
    }
}

fn card(board: &SpaceBoard, id: &PlacementId) -> u64 {
    board
        .element(&Handle::Placement(id.clone()))
        .unwrap_or_else(|| panic!("{id} has no card"))
}

fn ink_el(board: &SpaceBoard, id: &PlacementId, local: u64) -> u64 {
    board
        .element(&Handle::Ink {
            placement: id.clone(),
            local,
        })
        .unwrap()
}

fn edge(board: &SpaceBoard, class: EdgeClass) -> (u64, Handle) {
    let mut found: Vec<(u64, Handle)> = board
        .projection()
        .scene
        .elements
        .iter()
        .filter_map(|e| match board.handle(e.id) {
            Some(h @ Handle::Edge { class: c, .. }) if *c == class => Some((e.id, h.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(found.len(), 1, "{class:?}: {found:?}");
    found.remove(0)
}

fn one_command(route: &Route) -> &SpaceCommand {
    match route {
        Route::Commands {
            space: Some(cmd),
            owners,
            ..
        } if owners.is_empty() => cmd,
        other => panic!("expected one Space command, got {other:?}"),
    }
}

// ---- projection -------------------------------------------------------------

#[test]
fn projection_draws_every_v1_shape_with_its_class() {
    // Wrong implementations rejected: a projection that drops groups, nested
    // portals or notes (each card is looked up by identity); one that draws all
    // three connection classes alike (style and stroke are asserted per class);
    // one that ignores ink placement origins (the world position is checked);
    // and one that loses the ink arrow's anchor to its own box.
    let f = fixture();
    let mut board = SpaceBoard::new();
    let p = f.w.project(&mut board, &f.s);

    for id in [&f.a, &f.b, &f.doc, &f.note, &f.portal] {
        let g = embed(&p.scene, card(&board, id));
        let rect = f.s.space.placement(id).unwrap().rect;
        assert_eq!(
            (g.x, g.y, g.w, g.h),
            (rect.x as f32, rect.y as f32, rect.w as f32, rect.h as f32)
        );
        assert!(
            p.axis_aligned.contains(&card(&board, id)),
            "{id} may rotate"
        );
        assert_eq!(
            g.page_id as u64,
            card(&board, id),
            "open must name the card"
        );
    }
    assert_eq!(embed(&p.scene, card(&board, &f.note)).title, "Hello");
    assert_eq!(
        embed(&p.scene, card(&board, &f.portal)).title,
        f.child.to_string()
    );

    let group = &f.s.space.groups[0];
    let gid = board.element(&Handle::Group(group.id.clone())).unwrap();
    assert!(matches!(find(&p.scene, gid).kind, ElementKind::Rect(_)));
    assert_eq!(find(&p.scene, gid).label.as_deref(), Some("Left"));
    assert!(p.axis_aligned.contains(&gid));
    let gpos = p.scene.elements.iter().position(|e| e.id == gid).unwrap();
    let apos = p
        .scene
        .elements
        .iter()
        .position(|e| e.id == card(&board, &f.a))
        .unwrap();
    assert!(gpos < apos, "a region paints behind what it holds");

    // Ink: world coordinates, own anchor preserved.
    let bx = ink_el(&board, &f.ink, 1);
    let ar = ink_el(&board, &f.ink, 2);
    let ElementKind::Rect(g) = &find(&p.scene, bx).kind else {
        panic!()
    };
    assert_eq!((g.x, g.y), (910.0, 410.0));
    let ElementKind::Arrow(seg) = &find(&p.scene, ar).kind else {
        panic!()
    };
    assert_eq!((seg.x1, seg.x2), (970.0, 1040.0));
    assert_eq!(seg.start_anchor.map(|a| a.element_id), Some(bx));
    assert!(!p.axis_aligned.contains(&bx), "ink shapes rotate");

    // Connection classes.
    let (pres, _) = edge(&board, EdgeClass::Presentation);
    let (sem, sem_h) = edge(&board, EdgeClass::Semantic);
    let (exe, _) = edge(&board, EdgeClass::Executable);
    let seg = |id| match &find(&p.scene, id).kind {
        ElementKind::Arrow(s) | ElementKind::Line(s) => *s,
        other => panic!("{other:?}"),
    };
    assert_eq!(seg(pres).style, SegmentStyle::Solid);
    assert_eq!(find(&p.scene, pres).label.as_deref(), Some("see"));
    assert_eq!(find(&p.scene, pres).stroke, None);
    assert_eq!(seg(sem).style, SegmentStyle::Dashed);
    assert_eq!(seg(exe).style, SegmentStyle::Solid);
    assert!(
        find(&p.scene, exe).stroke.is_some(),
        "a binding shows its state"
    );
    assert_eq!(
        seg(pres).start_anchor.map(|a| a.element_id),
        Some(card(&board, &f.note))
    );
    assert_eq!(
        seg(pres).end_anchor.map(|a| a.element_id),
        Some(card(&board, &f.b))
    );
    let Handle::Edge { key, .. } = &sem_h else {
        panic!()
    };
    assert_eq!(key, &f.w.relationships.relationships[0].id);
    assert!(p.retained.is_empty());

    // The one hit test names Space identities.
    assert_eq!(
        board.hit([150.0, 150.0], 0.0),
        Some(&Handle::Placement(f.a.clone()))
    );
    assert_eq!(
        board.hit([920.0, 420.0], 0.0),
        Some(&Handle::Ink {
            placement: f.ink.clone(),
            local: 1
        })
    );
}

// ---- check 31: round trip ------------------------------------------------------

#[test]
fn round_trip_preserves_identities_through_projection_gestures_commands_and_back() {
    // Wrong implementations rejected: a projection that re-mints element ids
    // each time (ids are compared across re-projection and undo); a gesture
    // conversion that writes a whole new board (the untouched placements,
    // groups and connections are compared byte for byte); a conversion that
    // re-places a moved card as a new Placement (its id must survive); and one
    // that rewrites a drawing on a pure move (its ink revision must not change).
    let Fixture {
        mut w,
        mut s,
        a,
        doc,
        ink,
        b,
        ..
    } = fixture();
    let mut board = SpaceBoard::new();
    let p0 = w.project(&mut board, &s);
    let ids0: Vec<(u64, Handle)> = p0
        .scene
        .elements
        .iter()
        .map(|e| (e.id, board.handle(e.id).unwrap().clone()))
        .collect();

    // Move card `a` by (+50, +20).
    let ca = card(&board, &a);
    let after = edited(&p0.scene, |els| {
        if let ElementKind::Embed(g) = &mut find_mut(els, ca).kind {
            g.x += 50.0;
            g.y += 20.0;
        }
    });
    let route = board.route(&mut s, &Actor::User, &commit(&p0.scene, &after, "move"));
    assert!(
        matches!(one_command(&route), SpaceCommand::Arrange { .. }),
        "{route:?}"
    );
    let before_space = s.space.clone();
    w.apply(&mut s, route);
    assert_eq!(
        s.space.placement(&a).unwrap().rect,
        Rect::new(150.0, 120.0, 200.0, 120.0)
    );
    for p in &s.space.placements {
        if p.id != a {
            assert_eq!(Some(p), before_space.placement(&p.id), "{} changed", p.id);
        }
    }
    assert_eq!(s.space.connections, before_space.connections);
    assert_eq!(s.space.groups, before_space.groups);

    // Back to the board: same identities on the same element ids.
    let p1 = w.project(&mut board, &s);
    let ids1: Vec<(u64, Handle)> = p1
        .scene
        .elements
        .iter()
        .map(|e| (e.id, board.handle(e.id).unwrap().clone()))
        .collect();
    assert_eq!(ids0, ids1);
    assert_eq!(embed(&p1.scene, ca).x, 150.0);
    // The connections that follow `a` were re-routed by the projection.
    let (sem, _) = edge(&board, EdgeClass::Semantic);
    let (ElementKind::Arrow(seg) | ElementKind::Line(seg)) = &find(&p1.scene, sem).kind else {
        panic!()
    };
    assert_eq!(seg.start_anchor.map(|x| x.element_id), Some(ca));

    // Move the drawing as a whole: a frame move, content untouched.
    let stored = s.space.placement(&ink).unwrap().target.clone();
    let (bx, ar) = (ink_el(&board, &ink, 1), ink_el(&board, &ink, 2));
    let after = edited(&p1.scene, |els| {
        for id in [bx, ar] {
            crate::geometry::translate(&mut find_mut(els, id).kind, 30.0, -10.0);
        }
    });
    let route = board.route(&mut s, &Actor::User, &commit(&p1.scene, &after, "move"));
    assert!(
        matches!(one_command(&route), SpaceCommand::Arrange { .. }),
        "{route:?}"
    );
    w.apply(&mut s, route);
    let moved = s.space.placement(&ink).unwrap();
    assert_eq!(moved.rect, Rect::new(930.0, 390.0, 160.0, 80.0));
    assert_eq!(moved.target, stored, "a frame move rewrote the drawing");

    // A group drag carries its members and is one entry.
    let p2 = w.project(&mut board, &s);
    let group = s.space.groups[0].clone();
    let gid = board.element(&Handle::Group(group.id.clone())).unwrap();
    let after = edited(&p2.scene, |els| {
        crate::geometry::translate(&mut find_mut(els, gid).kind, -40.0, 0.0);
    });
    let entries = s.history.undo.len();
    let route = board.route(&mut s, &Actor::User, &commit(&p2.scene, &after, "move"));
    w.apply(&mut s, route);
    assert_eq!(s.history.undo.len(), entries + 1);
    assert_eq!(s.space.groups[0].rect, group.rect.translated(-40.0, 0.0));
    assert_eq!(s.space.placement(&a).unwrap().rect.x, 110.0);
    assert_eq!(s.space.placement(&doc).unwrap().rect.x, 60.0);
    assert_eq!(
        s.space.placement(&b).unwrap().rect.x,
        500.0,
        "a non-member moved"
    );

    // Undo three times: back to the start, with the same board identities.
    for _ in 0..3 {
        let route = board.route(&mut s, &Actor::User, &WhiteboardEvent::UndoRequested);
        w.apply(&mut s, route);
    }
    assert_eq!(content(&s.space), content(&before_space));
    let p3 = w.project(&mut board, &s);
    assert_eq!(p3.scene.elements, p0.scene.elements);
}

#[test]
fn edge_gestures_go_to_each_class_owner() {
    // Wrong implementations rejected: deleting a semantic relationship by
    // removing a Space connection (the relationship would survive in its
    // owner); deleting a binding through the Space (same); re-pointing an
    // owner's relationship by drawing (it must be refused, not silently
    // redrawn); and treating a newly drawn card-to-card arrow as ink.
    let Fixture {
        mut w,
        mut s,
        a,
        b,
        doc,
        note,
        ..
    } = fixture();
    let mut board = SpaceBoard::new();
    let p = w.project(&mut board, &s);
    let (sem, sem_h) = edge(&board, EdgeClass::Semantic);
    let (exe, _) = edge(&board, EdgeClass::Executable);
    let (pres, pres_h) = edge(&board, EdgeClass::Presentation);

    // Delete the semantic and executable edges in one gesture.
    let after = edited(&p.scene, |els| els.retain(|e| e.id != sem && e.id != exe));
    let entries = s.history.undo.len();
    let route = board.route(&mut s, &Actor::User, &commit(&p.scene, &after, "delete"));
    let Route::Commands { space, owners, .. } = &route else {
        panic!("{route:?}")
    };
    assert!(space.is_none(), "the Space owns neither: {space:?}");
    assert_eq!(owners.len(), 2);
    assert!(matches!(owners[0], SpaceCommand::Unrelate { .. }));
    assert!(matches!(owners[1], SpaceCommand::Unbind { .. }));
    let applied = w.apply(&mut s, route);
    assert_eq!(applied.owners.len(), 2);
    assert!(w.relationships.relationships.is_empty());
    assert!(w.bindings.bindings.is_empty());
    assert_eq!(
        s.history.undo.len(),
        entries,
        "owner removals are not Space history"
    );

    // Re-pointing the presentation connection keeps its identity.
    let p = w.project(&mut board, &s);
    let cdoc = card(&board, &doc);
    let after = edited(&p.scene, |els| {
        if let ElementKind::Arrow(seg) = &mut find_mut(els, pres).kind {
            seg.end_anchor = Some(SegmentAnchor {
                element_id: cdoc,
                connector: 0,
            });
        }
    });
    let Handle::Edge { key, .. } = &pres_h else {
        panic!()
    };
    let route = board.route(
        &mut s,
        &Actor::User,
        &commit(&p.scene, &after, "move endpoint"),
    );
    w.apply(&mut s, route);
    assert_eq!(s.space.connections.len(), 1);
    let c = &s.space.connections[0];
    assert_eq!(c.id.to_string(), *key);
    assert_eq!((&c.from, &c.to), (&note, &doc));
    assert_eq!(c.to_side, Some(theorem_space::Side::Top));
    assert_eq!(c.label.as_deref(), Some("see"));

    // Drawing a new arrow from card to card makes a presentation connection.
    let p = w.project(&mut board, &s);
    let (ca, cb) = (card(&board, &a), card(&board, &b));
    let fresh = p.scene.elements.iter().map(|e| e.id).max().unwrap() + 1;
    let after = edited(&p.scene, |els| {
        els.push(element(
            fresh,
            ElementKind::Arrow(SegGeom {
                x1: 300.0,
                y1: 160.0,
                x2: 500.0,
                y2: 160.0,
                width: 2.0,
                style: SegmentStyle::Solid,
                start_anchor: Some(SegmentAnchor {
                    element_id: ca,
                    connector: 1,
                }),
                end_anchor: Some(SegmentAnchor {
                    element_id: cb,
                    connector: 3,
                }),
            }),
        ))
    });
    let route = board.route(&mut s, &Actor::User, &commit(&p.scene, &after, "arrow"));
    w.apply(&mut s, route);
    let c = s.space.connections.iter().find(|c| c.from == a).unwrap();
    assert_eq!(c.to, b);
    assert_eq!(
        (c.from_side, c.to_side),
        (
            Some(theorem_space::Side::Right),
            Some(theorem_space::Side::Left)
        )
    );
    assert_eq!(c.to_end, theorem_space::End::Arrow);
    assert!(
        s.space
            .placements
            .iter()
            .all(|p| p.target.ink().is_none_or(|i| i.elements.len() == 2))
    );

    // Re-pointing a relationship by drawing is refused.
    w.run(
        &mut s,
        SpaceCommand::Relate {
            from: a.clone(),
            to: doc.clone(),
            kind: "cites".into(),
            label: None,
        },
    );
    let p = w.project(&mut board, &s);
    let (sem, _) = edge(&board, EdgeClass::Semantic);
    let cb = card(&board, &b);
    let after = edited(&p.scene, |els| {
        if let ElementKind::Arrow(seg) | ElementKind::Line(seg) = &mut find_mut(els, sem).kind {
            seg.end_anchor = Some(SegmentAnchor {
                element_id: cb,
                connector: 3,
            });
        }
    });
    let route = board.route(
        &mut s,
        &Actor::User,
        &commit(&p.scene, &after, "move endpoint"),
    );
    assert!(
        matches!(route, Route::Refused(Refusal::OwnerEndpoint { .. })),
        "{route:?}"
    );
    // A side change on it is a route hint, stored by the Space.
    let after = edited(&p.scene, |els| {
        if let ElementKind::Arrow(seg) | ElementKind::Line(seg) = &mut find_mut(els, sem).kind {
            let anchor = seg.end_anchor.unwrap();
            seg.end_anchor = Some(SegmentAnchor {
                connector: (anchor.connector + 1) % 4,
                ..anchor
            });
        }
    });
    let route = board.route(
        &mut s,
        &Actor::User,
        &commit(&p.scene, &after, "move endpoint"),
    );
    w.apply(&mut s, route);
    assert_eq!(s.space.route_hints.len(), 1);
    let _ = sem_h;
}

#[test]
fn opening_a_portal_enters_the_nested_space_projected_on_its_own_board() {
    // Wrong implementations rejected: open naming a card by the wrong identity
    // (the navigator must land in the child), and a child projected on the
    // parent's board (placement ids are unique only per Space, so the board must
    // reset its bindings).
    let Fixture {
        mut w,
        mut s,
        portal,
        child,
        a,
        ..
    } = fixture();
    let mut board = SpaceBoard::new();
    w.project(&mut board, &s);
    let page = card(&board, &portal) as i64;
    let open = board.open(page).unwrap();
    assert_eq!(
        open,
        SpaceCommand::Focus {
            placement: portal.clone()
        }
    );
    w.run(&mut s, open);
    assert_eq!(w.nav.current_space(), &child);
    assert_eq!(
        board.open(card(&board, &a) as i64),
        Some(SpaceCommand::Focus { placement: a })
    );

    let mut inner = SpaceSession::new(
        Space::new(child.clone(), SpaceRole::Custom, "Plans"),
        "desk",
    );
    let note = placed(w.run(
        &mut inner,
        SpaceCommand::PlaceNote {
            text: "inside".into(),
            rect: Some(Rect::new(0.0, 0.0, 100.0, 60.0)),
        },
    ));
    let p = w.project(&mut board, &inner);
    assert_eq!(p.scene.elements.len(), 1);
    assert_eq!(
        board.handle(p.scene.elements[0].id),
        Some(&Handle::Placement(note))
    );
}

// ---- check 32: one undo owner, hosted camera ---------------------------------

#[test]
fn one_drag_is_one_undo_entry_and_undo_restores_the_space_exactly() {
    // Wrong implementations rejected: one command per dragged element (the
    // entry count would be four); a board that also keeps its own undo (the
    // route for UndoRequested must be the Space's Undo, and the Space must come
    // back exactly); and a move written as remove-plus-add (identities change).
    let Fixture {
        mut w,
        mut s,
        a,
        b,
        note,
        ink,
        ..
    } = fixture();
    let mut board = SpaceBoard::new();
    let p = w.project(&mut board, &s);
    let moved = [card(&board, &a), card(&board, &b), card(&board, &note)];
    let inks = [ink_el(&board, &ink, 1), ink_el(&board, &ink, 2)];
    // A marquee selection dragged by (+25, +35).
    let after = edited(&p.scene, |els| {
        for id in moved.iter().chain(&inks) {
            crate::geometry::translate(&mut find_mut(els, *id).kind, 25.0, 35.0);
        }
    });
    let start = s.space.clone();
    let entries = s.history.undo.len();
    let route = board.route(&mut s, &Actor::User, &commit(&p.scene, &after, "move"));
    let applied = w.apply(&mut s, route);
    assert!(applied.space.is_some());
    assert_eq!(s.history.undo.len(), entries + 1, "one drag, one entry");
    for id in [&a, &b, &note, &ink] {
        assert_eq!(
            s.space.placement(id).unwrap().rect,
            start.placement(id).unwrap().rect.translated(25.0, 35.0)
        );
    }

    let route = board.route(&mut s, &Actor::User, &WhiteboardEvent::UndoRequested);
    assert_eq!(
        one_command(&route),
        &SpaceCommand::Undo { only_mine: false }
    );
    w.apply(&mut s, route);
    assert_eq!(content(&s.space), content(&start), "undo restored exactly");
    assert_eq!(s.history.undo.len(), entries);

    let route = board.route(&mut s, &Actor::User, &WhiteboardEvent::RedoRequested);
    assert_eq!(one_command(&route), &SpaceCommand::Redo);
    w.apply(&mut s, route);
    assert_eq!(s.space.placement(&a).unwrap().rect.x, 125.0);
}

#[test]
fn camera_motion_is_device_local_and_produces_no_space_command() {
    // Wrong implementations rejected: a camera written into the Space (revision
    // and content are compared), a camera that becomes a history entry, and a
    // board that ignores the Space's own zoom range.
    let Fixture { mut w, mut s, .. } = fixture();
    let mut board = SpaceBoard::new();
    w.project(&mut board, &s);
    let start = s.space.clone();
    let entries = s.history.undo.len();
    for (camera, intent) in [
        (
            Camera {
                x: -120.0,
                y: 40.0,
                zoom: 1.0,
            },
            CameraIntent::Pan {
                dx: 120.0,
                dy: -40.0,
            },
        ),
        (
            Camera {
                x: 0.0,
                y: 0.0,
                zoom: 8.0,
            },
            CameraIntent::ZoomAbout {
                x: 0.0,
                y: 0.0,
                factor: 8.0,
            },
        ),
    ] {
        let route = board.route(
            &mut s,
            &Actor::User,
            &WhiteboardEvent::CameraRequested { camera, intent },
        );
        let Route::Camera(view) = &route else {
            panic!("{route:?}")
        };
        assert!(view.zoom <= theorem_space::geom::MAX_ZOOM);
        let applied = w.apply(&mut s, route.clone());
        assert_eq!(
            applied,
            Applied {
                space: None,
                owners: Vec::new()
            }
        );
        // The host keeps it in the navigator's device-local view.
        w.nav.view_mut(&s.space.id).camera = *view;
    }
    assert_eq!(s.space, start, "the camera touched shared state");
    assert_eq!(s.history.undo.len(), entries);
    assert_eq!(
        w.nav.view(&s.space.id).camera.zoom,
        theorem_space::geom::MAX_ZOOM
    );
    // The next projection carries the local camera.
    let p = w.project(&mut board, &s);
    assert_eq!(p.scene.camera.zoom, theorem_space::geom::MAX_ZOOM as f32);
}

// ---- ink ------------------------------------------------------------------------

#[test]
fn ink_round_trips_through_placement_target_ink_with_its_version() {
    // Wrong implementations rejected: ink kept in a board-side store (the Space
    // placement must carry it); a content edit that does not bump the revision
    // or loses the format; a re-projection that drops the edit; and a new
    // stroke stored in world coordinates (it must be local to its frame).
    let Fixture { mut w, mut s, .. } = fixture();
    let mut board = SpaceBoard::new();
    let p = w.project(&mut board, &s);
    let fresh = p.scene.elements.iter().map(|e| e.id).max().unwrap() + 1;
    let pen = stroke(fresh, &[[1500.0, 900.0], [1520.0, 930.0], [1560.0, 910.0]]);
    let after = edited(&p.scene, |els| els.push(pen.clone()));
    let route = board.route(&mut s, &Actor::User, &commit(&p.scene, &after, "draw"));
    let SpaceCommand::PlaceInk { elements, rect } = one_command(&route).clone() else {
        panic!("{route:?}")
    };
    let rect = rect.unwrap();
    assert_eq!(
        (rect.x, rect.y),
        (1500.0 - f64::from(INK_PAD), 900.0 - f64::from(INK_PAD))
    );
    assert_eq!(elements.len(), 1);
    let Outcome::Committed { placements, .. } = w.apply(&mut s, route).space.unwrap().outcome
    else {
        panic!()
    };
    let drawing = placements[0].clone();
    let stored = s
        .space
        .placement(&drawing)
        .unwrap()
        .target
        .ink()
        .unwrap()
        .clone();
    assert_eq!(stored.format, INK_FORMAT);
    assert_eq!(stored.revision, 0);
    let local: Element = serde_json::from_value(stored.elements[0].clone()).unwrap();
    let ElementKind::Draw(st) = &local.kind else {
        panic!()
    };
    assert_eq!(
        st.points[0],
        [INK_PAD, INK_PAD],
        "stored local to its frame"
    );

    // Re-projected at the same place; then recoloured: one EditInk, revision 1.
    let p = w.project(&mut board, &s);
    let id = board
        .element(&Handle::Ink {
            placement: drawing.clone(),
            local: local.id,
        })
        .unwrap();
    let ElementKind::Draw(st) = &find(&p.scene, id).kind else {
        panic!()
    };
    assert_eq!(st.points, pen_points(&pen));
    let after = edited(&p.scene, |els| find_mut(els, id).stroke = Some(0xff0000ff));
    let route = board.route(&mut s, &Actor::User, &commit(&p.scene, &after, "color"));
    assert!(
        matches!(
            one_command(&route),
            SpaceCommand::EditInk {
                base_revision: 0,
                ..
            }
        ),
        "{route:?}"
    );
    w.apply(&mut s, route);
    let stored = s
        .space
        .placement(&drawing)
        .unwrap()
        .target
        .ink()
        .unwrap()
        .clone();
    assert_eq!((stored.revision, stored.format.as_str()), (1, INK_FORMAT));
    let p = w.project(&mut board, &s);
    assert_eq!(find(&p.scene, id).stroke, Some(0xff0000ff));
    assert_eq!(
        s.space.placement(&drawing).unwrap().rect,
        rect,
        "an in-place edit kept the frame"
    );

    // Undo returns revision 0.
    let route = board.route(&mut s, &Actor::User, &WhiteboardEvent::UndoRequested);
    w.apply(&mut s, route);
    assert_eq!(
        s.space
            .placement(&drawing)
            .unwrap()
            .target
            .ink()
            .unwrap()
            .revision,
        0
    );
}

fn pen_points(e: &Element) -> Vec<[f32; 2]> {
    match &e.kind {
        ElementKind::Draw(s) => s.points.clone(),
        _ => unreachable!(),
    }
}

#[test]
fn drawing_beside_a_drawing_joins_it_and_deleting_all_of_it_removes_the_placement() {
    // Wrong implementations rejected: a new drawing per stroke drawn inside an
    // existing frame (the drawing count is checked), and an emptied drawing
    // left as an invisible placement.
    let Fixture {
        mut w, mut s, ink, ..
    } = fixture();
    let mut board = SpaceBoard::new();
    let p = w.project(&mut board, &s);
    let count = s.space.placements.len();
    let fresh = p.scene.elements.iter().map(|e| e.id).max().unwrap() + 1;
    let after = edited(&p.scene, |els| {
        els.push(stroke(fresh, &[[1000.0, 440.0], [1030.0, 450.0]]))
    });
    let route = board.route(&mut s, &Actor::User, &commit(&p.scene, &after, "draw"));
    assert!(
        matches!(one_command(&route), SpaceCommand::EditInk { .. }),
        "{route:?}"
    );
    w.apply(&mut s, route);
    assert_eq!(s.space.placements.len(), count);
    let stored = s.space.placement(&ink).unwrap().target.ink().unwrap();
    assert_eq!(stored.elements.len(), 3);
    assert_eq!(stored.revision, 1);

    let p = w.project(&mut board, &s);
    let mine: Vec<u64> = p
        .scene
        .elements
        .iter()
        .filter(|e| board.handle(e.id).and_then(Handle::placement) == Some(&ink))
        .map(|e| e.id)
        .collect();
    assert_eq!(mine.len(), 3);
    let after = edited(&p.scene, |els| els.retain(|e| !mine.contains(&e.id)));
    let route = board.route(&mut s, &Actor::User, &commit(&p.scene, &after, "delete"));
    assert_eq!(
        one_command(&route),
        &SpaceCommand::RemovePlacements {
            placements: vec![ink.clone()]
        }
    );
    w.apply(&mut s, route);
    assert!(s.space.placement(&ink).is_none());
}

// ---- retained content -----------------------------------------------------------

#[test]
fn corrupt_and_unknown_content_is_retained_never_replaced_by_an_empty_board() {
    // Wrong implementations rejected: a parse failure that imports an empty
    // drawing (the Err must carry the original text); an import that drops an
    // unreadable element; an edit that rewrites a drawing without its unread
    // element (or moves it); a delete that removes a drawing still holding unread
    // content; and a drawing in an unknown format shown as empty or rewritten.
    let not_a_board = "{ this is not json";
    let err = import_board(not_a_board, theorem_space::Point::new(0.0, 0.0)).unwrap_err();
    assert_eq!(err.original_text(), not_a_board);

    let bad = json!({"id": 7, "kind": {"Hologram": {"depth": 3}}});
    let doc = json!({
        "elements": [value(&shape(1, 10.0, 10.0, 40.0, 40.0)), bad.clone(), value(&shape(2, 80.0, 10.0, 40.0, 40.0))],
    })
    .to_string();
    let SpaceCommand::PlaceInk { elements, rect } =
        import_board(&doc, theorem_space::Point::new(300.0, 300.0)).unwrap()
    else {
        panic!()
    };
    assert_eq!(elements.len(), 3);
    assert_eq!(
        elements[1], bad,
        "the unread element kept its slot, verbatim"
    );
    let rect = rect.unwrap();
    assert_eq!(
        (rect.x, rect.y),
        (300.0, 300.0),
        "unread content is not re-based"
    );

    let Fixture { mut w, mut s, .. } = fixture();
    let drawing = placed(w.run(
        &mut s,
        SpaceCommand::PlaceInk {
            elements: elements.clone(),
            rect: Some(rect),
        },
    ));
    let alien = placed(w.run(
        &mut s,
        SpaceCommand::PlaceInk {
            elements: Vec::new(),
            rect: Some(Rect::new(0.0, 800.0, 120.0, 90.0)),
        },
    ));
    let alien_ink = InkContent {
        format: "excalidraw.v2".into(),
        revision: 4,
        elements: vec![json!({"type": "freedraw", "points": [[0, 0], [5, 5]]})],
    };
    s.commit(theorem_space::Transaction::new(
        Actor::User,
        "fixture",
        vec![theorem_space::SpaceOp::SetInk {
            id: alien.clone(),
            ink: alien_ink.clone(),
        }],
    ))
    .unwrap();

    let mut board = SpaceBoard::new();
    let p = w.project(&mut board, &s);
    assert!(
        p.retained
            .iter()
            .any(|r| r.placement == drawing && r.index == Some(1))
    );
    assert!(
        p.retained
            .iter()
            .any(|r| r.placement == alien && r.index.is_none())
    );
    // The unknown-format drawing is a card, not an empty board.
    let alien_card = card(&board, &alien);
    assert_eq!(embed(&p.scene, alien_card).title, "Drawing (excalidraw.v2)");

    // Recolour a readable element: the unread one stays, at index 1.
    let one = ink_el(&board, &drawing, 1);
    let after = edited(&p.scene, |els| find_mut(els, one).fill = Some(0x00ff00ff));
    let route = board.route(&mut s, &Actor::User, &commit(&p.scene, &after, "color"));
    w.apply(&mut s, route);
    let stored = s
        .space
        .placement(&drawing)
        .unwrap()
        .target
        .ink()
        .unwrap()
        .clone();
    assert_eq!(stored.elements.len(), 3);
    assert_eq!(stored.elements[1], bad);
    assert_eq!(
        stored.elements[2], elements[2],
        "an untouched element was rewritten"
    );

    // Growing it leftward would re-base unread content: refused.
    let p = w.project(&mut board, &s);
    let fresh = p.scene.elements.iter().map(|e| e.id).max().unwrap() + 1;
    let after = edited(&p.scene, |els| {
        let mut e = shape(fresh, 290.0, 320.0, 20.0, 20.0);
        if let ElementKind::Rect(g) = &mut e.kind {
            g.x = 250.0;
        }
        els.push(e);
        // Anchor it into the drawing so it joins rather than starting a new one.
        els.push(element(
            fresh + 1,
            ElementKind::Arrow(SegGeom {
                x1: 260.0,
                y1: 330.0,
                x2: 320.0,
                y2: 330.0,
                width: 2.0,
                style: SegmentStyle::Solid,
                start_anchor: None,
                end_anchor: Some(SegmentAnchor {
                    element_id: one,
                    connector: 3,
                }),
            }),
        ));
    });
    let route = board.route(&mut s, &Actor::User, &commit(&p.scene, &after, "arrow"));
    assert!(
        matches!(route, Route::Refused(Refusal::RetainedContent { .. })),
        "{route:?}"
    );

    // Deleting every readable element keeps the drawing and its unread part.
    let mine: Vec<u64> = p
        .scene
        .elements
        .iter()
        .filter(|e| board.handle(e.id).and_then(Handle::placement) == Some(&drawing))
        .map(|e| e.id)
        .collect();
    let after = edited(&p.scene, |els| els.retain(|e| !mine.contains(&e.id)));
    let route = board.route(&mut s, &Actor::User, &commit(&p.scene, &after, "delete"));
    w.apply(&mut s, route);
    let stored = s
        .space
        .placement(&drawing)
        .expect("the drawing survived")
        .target
        .ink()
        .unwrap();
    assert_eq!(stored.elements, vec![bad.clone()]);

    // Moving the unknown-format card moves its frame; its content is untouched.
    let p = w.project(&mut board, &s);
    let after = edited(&p.scene, |els| {
        if let ElementKind::Embed(g) = &mut find_mut(els, alien_card).kind {
            g.x += 10.0;
        }
    });
    let route = board.route(&mut s, &Actor::User, &commit(&p.scene, &after, "move"));
    w.apply(&mut s, route);
    let moved = s.space.placement(&alien).unwrap();
    assert_eq!(moved.rect.x, 10.0);
    assert_eq!(moved.target, PlacementTarget::Ink { ink: alien_ink });
}

// ---- constraints ------------------------------------------------------------------

#[test]
fn pinned_geometry_refuses_gesture_movement() {
    // Wrong implementations rejected: a projection that does not hand the pin
    // to the board (held is checked); a conversion that lets a stale or forged
    // commit move a pinned card, a pinned drawing, or a group holding a pinned
    // member (each must be refused with the Space unchanged).
    let Fixture {
        mut w,
        mut s,
        a,
        ink,
        ..
    } = fixture();
    w.run(
        &mut s,
        SpaceCommand::Pin {
            placements: vec![a.clone(), ink.clone()],
            pinned: true,
        },
    );
    let mut board = SpaceBoard::new();
    let p = w.project(&mut board, &s);
    let ca = card(&board, &a);
    let group = s.space.groups[0].clone();
    let gid = board.element(&Handle::Group(group.id.clone())).unwrap();
    let (bx, ar) = (ink_el(&board, &ink, 1), ink_el(&board, &ink, 2));
    for id in [ca, gid, bx, ar] {
        assert!(p.held.contains(&id), "{id} is not held");
    }
    let start = s.space.clone();
    for ids in [vec![ca], vec![gid], vec![bx, ar]] {
        let after = edited(&p.scene, |els| {
            for id in &ids {
                crate::geometry::translate(&mut find_mut(els, *id).kind, 40.0, 0.0);
            }
        });
        let route = board.route(&mut s, &Actor::User, &commit(&p.scene, &after, "move"));
        assert!(
            matches!(route, Route::Refused(Refusal::Pinned { .. })),
            "{ids:?}: {route:?}"
        );
        w.apply(&mut s, route);
        assert_eq!(s.space, start);
    }
    // Released explicitly, it moves.
    w.run(
        &mut s,
        SpaceCommand::Pin {
            placements: vec![a.clone()],
            pinned: false,
        },
    );
    let p = w.project(&mut board, &s);
    assert!(!p.held.contains(&ca));
    let after = edited(&p.scene, |els| {
        crate::geometry::translate(&mut find_mut(els, ca).kind, 40.0, 0.0)
    });
    let route = board.route(&mut s, &Actor::User, &commit(&p.scene, &after, "move"));
    w.apply(&mut s, route);
    assert_eq!(s.space.placement(&a).unwrap().rect.x, 140.0);
}

#[test]
fn axis_aligned_surfaces_and_regions_refuse_rotation() {
    // Wrong implementations rejected: a rotated region written back as a moved
    // one (rotation is refused, not dropped), and a card resized by a rotate
    // gesture. Ink shapes still rotate (their rotation round-trips).
    let Fixture {
        mut w,
        mut s,
        a,
        ink,
        ..
    } = fixture();
    let mut board = SpaceBoard::new();
    let p = w.project(&mut board, &s);
    let gid = board
        .element(&Handle::Group(s.space.groups[0].id.clone()))
        .unwrap();
    let after = edited(&p.scene, |els| {
        if let ElementKind::Rect(g) = &mut find_mut(els, gid).kind {
            g.rotation = 0.4;
        }
    });
    let route = board.route(&mut s, &Actor::User, &commit(&p.scene, &after, "rotate"));
    assert!(
        matches!(route, Route::Refused(Refusal::AxisAligned { .. })),
        "{route:?}"
    );

    let ca = card(&board, &a);
    let after = edited(&p.scene, |els| {
        if let ElementKind::Embed(g) = &mut find_mut(els, ca).kind {
            g.w += 30.0;
        }
    });
    let route = board.route(&mut s, &Actor::User, &commit(&p.scene, &after, "rotate"));
    assert!(
        matches!(route, Route::Refused(Refusal::AxisAligned { .. })),
        "{route:?}"
    );

    let bx = ink_el(&board, &ink, 1);
    let after = edited(&p.scene, |els| {
        if let ElementKind::Rect(g) = &mut find_mut(els, bx).kind {
            g.rotation = 0.4;
        }
    });
    let route = board.route(&mut s, &Actor::User, &commit(&p.scene, &after, "rotate"));
    w.apply(&mut s, route);
    let p = w.project(&mut board, &s);
    let ElementKind::Rect(g) = &find(&p.scene, bx).kind else {
        panic!()
    };
    assert_eq!(g.rotation, 0.4);
}

// ---- the gesture engine over a Space (cards projection) ----------------------------

fn cards(w: &World, board: &mut SpaceBoard, s: &SpaceSession) -> Projection {
    let lines = connections(&s.space, &w.relationships, &w.bindings);
    let camera = w.nav.view(&s.space.id).camera;
    board.project_cards(&s.space, &lines, &camera)
}

fn engine_of(p: &Projection) -> crate::engine::Engine {
    let mut e = crate::engine::Engine::new();
    p.present(&mut e);
    e
}

fn eng_frame(selection: &[u64]) -> crate::engine::Frame<'_> {
    crate::engine::Frame {
        selection,
        view: crate::engine::View::default(),
        engaged: None,
        hovered: None,
    }
}

#[test]
fn the_cards_projection_gives_the_engine_every_placement_as_one_card() {
    // Wrong implementations rejected: drawing ink element by element on a
    // host that paints cards (the engine would move marks, not the frame),
    // and a region whose members are whatever lies inside it.
    let Fixture {
        w,
        s,
        a,
        b,
        doc,
        note,
        portal,
        ink,
        ..
    } = fixture();
    let mut board = SpaceBoard::new();
    let p = cards(&w, &mut board, &s);
    let roles: std::collections::HashMap<u64, crate::engine::Role> =
        p.roles.iter().cloned().collect();
    for (id, connectable) in [
        (&a, true),
        (&b, true),
        (&doc, true),
        (&note, true),
        (&portal, true),
        (&ink, false),
    ] {
        assert_eq!(
            roles[&card(&board, id)],
            crate::engine::Role::Card { connectable },
            "{id}"
        );
    }
    assert!(
        board
            .element(&Handle::Ink {
                placement: ink.clone(),
                local: 1
            })
            .is_none()
    );
    let group = s.space.placement(&a).unwrap().group.clone().unwrap();
    let region = board.element(&Handle::Group(group)).unwrap();
    assert_eq!(
        roles[&region],
        crate::engine::Role::Region {
            members: vec![card(&board, &a), card(&board, &doc)]
        }
    );
    for class in [
        EdgeClass::Presentation,
        EdgeClass::Semantic,
        EdgeClass::Executable,
    ] {
        let (id, _) = edge(&board, class);
        assert_eq!(roles[&id], crate::engine::Role::Inert, "{class:?}");
    }
    // The hit test is the Space's: the topmost card, else the smallest region.
    let e = engine_of(&p);
    assert_eq!(
        e.hit([150.0, 150.0]),
        Some(crate::engine::Hit::Card(card(&board, &a)))
    );
    let g = s.space.groups[0].rect;
    let gap = [g.x + 2.0, g.y + 2.0];
    assert_eq!(e.hit(gap), Some(crate::engine::Hit::Region(region)));
    assert!(
        s.space
            .placements
            .iter()
            .all(|p| !p.rect.contains(theorem_space::Point::new(gap[0], gap[1])))
    );
}

#[test]
fn an_engine_drag_over_several_cards_is_one_arrange_and_one_undo_entry() {
    // Wrong implementation rejected: a command per dragged card (entries + 3).
    let Fixture {
        mut w,
        mut s,
        a,
        b,
        ink,
        ..
    } = fixture();
    let mut board = SpaceBoard::new();
    let p = cards(&w, &mut board, &s);
    let e = engine_of(&p);
    let sel = [card(&board, &a), card(&board, &b), card(&board, &ink)];
    let f = eng_frame(&sel);
    let mut g = None;
    assert!(
        e.press(
            &mut g,
            [150.0, 150.0],
            crate::engine::Button::Primary,
            Default::default(),
            1,
            &f
        )
        .is_empty()
    );
    e.drag_to(&mut g, [170.0, 160.0], &f);
    e.drag_to(&mut g, [190.0, 185.0], &f);
    let events = e.release(&mut g, [190.0, 185.0], &f);
    let [crate::engine::EngineEvent::Commit(event)] = events.as_slice() else {
        panic!("{events:?}")
    };
    let start = s.space.clone();
    let entries = s.history.undo.len();
    let route = board.route(&mut s, &Actor::User, event);
    let SpaceCommand::Arrange {
        label,
        placements,
        groups,
    } = one_command(&route)
    else {
        panic!("{route:?}")
    };
    assert_eq!(label, "Move");
    assert_eq!(placements.len(), 3);
    assert!(groups.is_empty());
    w.apply(&mut s, route);
    assert_eq!(s.history.undo.len(), entries + 1);
    for id in [&a, &b, &ink] {
        let placed = s.space.placement(id).unwrap();
        assert_eq!(
            placed.rect,
            start.placement(id).unwrap().rect.translated(40.0, 35.0)
        );
        assert!(
            placed.authorship.arranged,
            "an intentional move marks Authored"
        );
    }
    // The ink's content is untouched: only its frame moved.
    assert_eq!(
        s.space.placement(&ink).unwrap().target,
        start.placement(&ink).unwrap().target
    );
    w.run(&mut s, SpaceCommand::Undo { only_mine: false });
    assert_eq!(content(&s.space), content(&start));
}

#[test]
fn an_engine_region_drag_moves_what_its_frame_holds() {
    let Fixture {
        mut w,
        mut s,
        a,
        doc,
        ..
    } = fixture();
    let mut board = SpaceBoard::new();
    let p = cards(&w, &mut board, &s);
    let e = engine_of(&p);
    let group = s.space.groups[0].clone();
    let f = eng_frame(&[]);
    let mut g = None;
    let press = [group.rect.x + 2.0, group.rect.y + 2.0];
    let events = e.press(
        &mut g,
        press,
        crate::engine::Button::Primary,
        Default::default(),
        1,
        &f,
    );
    assert_eq!(
        events,
        vec![crate::engine::EngineEvent::Select(vec![
            card(&board, &a),
            card(&board, &doc)
        ])]
    );
    let to = [press[0] + 30.0, press[1] - 20.0];
    e.drag_to(&mut g, to, &f);
    let events = e.release(&mut g, to, &f);
    let [crate::engine::EngineEvent::Commit(event)] = events.as_slice() else {
        panic!("{events:?}")
    };
    let route = board.route(&mut s, &Actor::User, event);
    let SpaceCommand::Arrange {
        label,
        placements,
        groups,
    } = one_command(&route).clone()
    else {
        panic!("{route:?}")
    };
    assert_eq!(label, "Move group");
    assert_eq!(
        groups,
        vec![(group.id.clone(), group.rect.translated(30.0, -20.0))]
    );
    let moved: Vec<PlacementId> = placements.iter().map(|(id, _)| id.clone()).collect();
    assert_eq!(moved.len(), 2);
    assert!(moved.contains(&a) && moved.contains(&doc));
    let before = s.space.clone();
    w.apply(&mut s, route);
    for id in [&a, &doc] {
        assert_eq!(
            s.space.placement(id).unwrap().rect,
            before.placement(id).unwrap().rect.translated(30.0, -20.0)
        );
    }
}

/// Group frames are purely spatial (spec section 7): a frame drag moves
/// every card whose centre lies inside the frame, and nothing else.
#[test]
fn a_frame_drag_leaves_a_card_dragged_out_and_carries_one_dragged_in() {
    // Wrong implementations rejected: carrying the stored member list (the
    // card dragged out moves, the reported bug); carrying only stored members
    // (the note dragged in stays behind); carrying everything (the untouched
    // card moves).
    let Fixture {
        mut w,
        mut s,
        a,
        b,
        doc,
        note,
        ..
    } = fixture();
    let group = s.space.groups[0].clone();
    assert_eq!(s.space.placement(&a).unwrap().group, Some(group.id.clone()));
    // Drag `a` (a stored member) far outside the frame, and the note (never
    // grouped) into it.
    let out_of_frame = s.space.placement(&a).unwrap().rect.translated(0.0, 900.0);
    let inside = group.rect.center();
    let into_frame = Rect::new(inside.x - 40.0, inside.y - 25.0, 80.0, 50.0);
    w.run(
        &mut s,
        SpaceCommand::Arrange {
            label: "move".into(),
            placements: vec![(a.clone(), out_of_frame), (note.clone(), into_frame)],
            groups: Vec::new(),
        },
    );
    assert_eq!(
        s.space.placement(&a).unwrap().group,
        Some(group.id.clone()),
        "the stored field is untouched"
    );
    let held: Vec<PlacementId> = s
        .space
        .contents_of(&group.id)
        .map(|p| p.id.clone())
        .collect();
    assert!(
        held.contains(&doc) && held.contains(&note) && !held.contains(&a),
        "{held:?}"
    );
    assert_eq!(s.space.group_holding(&note).map(|g| &g.id), Some(&group.id));
    assert_eq!(s.space.group_holding(&a), None);

    // The engine's region carries the frame's contents.
    let mut board = SpaceBoard::new();
    let p = cards(&w, &mut board, &s);
    let region = board.element(&Handle::Group(group.id.clone())).unwrap();
    let roles: std::collections::HashMap<u64, crate::engine::Role> =
        p.roles.iter().cloned().collect();
    let crate::engine::Role::Region { members } = &roles[&region] else {
        panic!("{:?}", roles[&region])
    };
    let mut members = members.clone();
    members.sort();
    let mut expected = vec![card(&board, &doc), card(&board, &note)];
    expected.sort();
    assert_eq!(members, expected);

    // A frame drag through the engine.
    let e = engine_of(&p);
    let f = eng_frame(&[]);
    let mut g = None;
    let press = [group.rect.x + 2.0, group.rect.y + 2.0];
    e.press(
        &mut g,
        press,
        crate::engine::Button::Primary,
        Default::default(),
        1,
        &f,
    );
    let to = [press[0] + 30.0, press[1] - 20.0];
    e.drag_to(&mut g, to, &f);
    let events = e.release(&mut g, to, &f);
    let [crate::engine::EngineEvent::Commit(event)] = events.as_slice() else {
        panic!("{events:?}")
    };
    let before = s.space.clone();
    let route = board.route(&mut s, &Actor::User, event);
    w.apply(&mut s, route);
    for id in [&doc, &note] {
        assert_eq!(
            s.space.placement(id).unwrap().rect,
            before.placement(id).unwrap().rect.translated(30.0, -20.0),
            "{id} is inside the frame and travels with it"
        );
    }
    for id in [&a, &b] {
        assert_eq!(
            s.space.placement(id).unwrap().rect,
            before.placement(id).unwrap().rect,
            "{id} is outside the frame"
        );
    }

    // The same through a forged commit that moves only the frame: the carry
    // on the Space side reads the frame too.
    let p = w.project(&mut board, &s);
    let gid = board.element(&Handle::Group(group.id.clone())).unwrap();
    let after = edited(&p.scene, |els| {
        crate::geometry::translate(&mut find_mut(els, gid).kind, 0.0, 50.0)
    });
    let before = s.space.clone();
    let route = board.route(&mut s, &Actor::User, &commit(&p.scene, &after, "move"));
    w.apply(&mut s, route);
    assert_eq!(
        s.space.placement(&note).unwrap().rect,
        before.placement(&note).unwrap().rect.translated(0.0, 50.0)
    );
    assert_eq!(
        s.space.placement(&a).unwrap().rect,
        before.placement(&a).unwrap().rect
    );
}

/// A frame carries the frames inside it, and what those hold, even where an
/// inner frame overhangs the outer one (spec section 7: "every object whose
/// center lies inside the frame", and a frame is an object).
#[test]
fn a_frame_drag_carries_a_frame_inside_it_and_the_card_on_its_overhang() {
    // Wrong implementations rejected: carrying only the cards whose centres
    // lie inside the outer frame (the inner frame and the overhang card stay
    // behind); carrying the inner frame but not what it holds (the overhang
    // card stays behind); carrying everything (the untouched card moves).
    let Fixture {
        mut w,
        mut s,
        a,
        b,
        doc,
        note,
        ..
    } = fixture();
    let outer = s.space.groups[0].clone();
    w.run(
        &mut s,
        SpaceCommand::Group {
            placements: vec![doc.clone()],
            label: Some("Inner".into()),
        },
    );
    let inner = s.space.groups[1].id.clone();
    // The inner frame's centre lies inside the outer frame; its foot hangs
    // below it, and the note sits on that overhang.
    let o = outer.rect;
    let inner_rect = Rect::new(o.x + 20.0, o.y + o.h / 2.0, o.w - 40.0, o.h * 0.8);
    let foot = o.y + o.h * 1.15;
    let on_overhang = Rect::new(inner_rect.center().x - 40.0, foot - 25.0, 80.0, 50.0);
    w.run(
        &mut s,
        SpaceCommand::Arrange {
            label: "move".into(),
            placements: vec![(note.clone(), on_overhang)],
            groups: vec![(inner.clone(), inner_rect)],
        },
    );
    assert!(o.contains(inner_rect.center()) && !o.contains(on_overhang.center()));
    assert_eq!(s.space.group_holding(&note).map(|g| &g.id), Some(&inner));

    // The outer region carries the inner frame's element and the overhang
    // card, not only what its own rect covers.
    let mut board = SpaceBoard::new();
    let p = cards(&w, &mut board, &s);
    let region = board.element(&Handle::Group(outer.id.clone())).unwrap();
    let inner_el = board.element(&Handle::Group(inner.clone())).unwrap();
    let roles: std::collections::HashMap<u64, crate::engine::Role> =
        p.roles.iter().cloned().collect();
    let crate::engine::Role::Region { members } = &roles[&region] else {
        panic!("{:?}", roles[&region])
    };
    let mut members = members.clone();
    members.sort();
    let mut expected = vec![
        card(&board, &a),
        card(&board, &doc),
        card(&board, &note),
        inner_el,
    ];
    expected.sort();
    assert_eq!(members, expected);

    // A frame drag through the engine.
    let e = engine_of(&p);
    let f = eng_frame(&[]);
    let mut g = None;
    let press = [o.x + 2.0, o.y + 2.0];
    e.press(
        &mut g,
        press,
        crate::engine::Button::Primary,
        Default::default(),
        1,
        &f,
    );
    let to = [press[0] + 30.0, press[1] - 20.0];
    e.drag_to(&mut g, to, &f);
    let events = e.release(&mut g, to, &f);
    let [crate::engine::EngineEvent::Commit(event)] = events.as_slice() else {
        panic!("{events:?}")
    };
    let before = s.space.clone();
    let route = board.route(&mut s, &Actor::User, event);
    w.apply(&mut s, route);
    for id in [&a, &doc, &note] {
        assert_eq!(
            s.space.placement(id).unwrap().rect,
            before.placement(id).unwrap().rect.translated(30.0, -20.0),
            "{id} travels with the outer frame"
        );
    }
    assert_eq!(
        s.space.group(&inner).unwrap().rect,
        before.group(&inner).unwrap().rect.translated(30.0, -20.0),
        "the inner frame travels with the outer one"
    );
    assert_eq!(
        s.space.placement(&b).unwrap().rect,
        before.placement(&b).unwrap().rect,
        "a card outside both frames stays"
    );

    // The same through a forged commit that moves only the outer frame: the
    // carry on the Space side reaches the inner frame and its overhang.
    let p = w.project(&mut board, &s);
    let gid = board.element(&Handle::Group(outer.id.clone())).unwrap();
    let after = edited(&p.scene, |els| {
        crate::geometry::translate(&mut find_mut(els, gid).kind, 0.0, 50.0)
    });
    let before = s.space.clone();
    let route = board.route(&mut s, &Actor::User, &commit(&p.scene, &after, "move"));
    w.apply(&mut s, route);
    assert_eq!(
        s.space.group(&inner).unwrap().rect,
        before.group(&inner).unwrap().rect.translated(0.0, 50.0)
    );
    assert_eq!(
        s.space.placement(&note).unwrap().rect,
        before.placement(&note).unwrap().rect.translated(0.0, 50.0)
    );
    assert_eq!(
        s.space.placement(&b).unwrap().rect,
        before.placement(&b).unwrap().rect
    );
}

#[test]
fn engine_camera_requests_round_trip_exactly_and_touch_no_shared_state() {
    let Fixture { w, s, .. } = fixture();
    let mut board = SpaceBoard::new();
    let p = cards(&w, &mut board, &s);
    let e = engine_of(&p);
    let camera = theorem_space::Camera {
        origin: theorem_space::Point::new(123.456_789_012_3, -98.765_432_1),
        zoom: 0.731_234_567_8,
    };
    assert_eq!(space_view(view_of(&camera)), camera, "no f32 round trip");
    let f = crate::engine::Frame {
        view: view_of(&camera),
        ..eng_frame(&[])
    };
    let mut g = None;
    e.press(
        &mut g,
        [10.0, 10.0],
        crate::engine::Button::Middle,
        Default::default(),
        1,
        &f,
    );
    let events = e.drag_to(&mut g, [50.0, 30.0], &f);
    let [crate::engine::EngineEvent::Camera(v)] = events.as_slice() else {
        panic!("{events:?}")
    };
    let mut expected = camera;
    expected.pan_screen(40.0, 20.0);
    assert_eq!(
        space_view(*v),
        expected,
        "the engine's pan is the Space's pan"
    );
    let Some(crate::engine::EngineEvent::Camera(v)) =
        e.wheel([300.0, 200.0], [0.0, 120.0], true, &f)
    else {
        panic!()
    };
    let mut expected = camera;
    expected.zoom_around(
        theorem_space::Point::new(300.0, 200.0),
        (120.0f64 * 0.0025).exp(),
    );
    assert_eq!(
        space_view(v),
        expected,
        "the engine's zoom is the Space's zoom"
    );
}

#[test]
fn the_board_and_the_space_share_one_zoom_range() {
    assert_eq!(crate::engine::MIN_ZOOM, theorem_space::geom::MIN_ZOOM);
    assert_eq!(crate::engine::MAX_ZOOM, theorem_space::geom::MAX_ZOOM);
    assert_eq!(crate::MIN_ZOOM, theorem_space::geom::MIN_ZOOM as f32);
    assert_eq!(crate::MAX_ZOOM, theorem_space::geom::MAX_ZOOM as f32);
    let wide = theorem_space::Camera {
        origin: theorem_space::Point::new(0.0, 0.0),
        zoom: 0.03,
    };
    // A Space camera inside the range presents unclamped on the board.
    assert_eq!(board_camera(&wide).zoom, 0.03);
    assert_eq!(space_camera(board_camera(&wide)).zoom, 0.03f32 as f64);
}

#[test]
fn a_compound_board_gesture_is_one_space_gesture_command() {
    // Wrong implementation rejected: an ApplyProposal (an agent-facing
    // proposal verb) or one command per change.
    let Fixture {
        mut w,
        mut s,
        a,
        ink,
        ..
    } = fixture();
    let mut board = SpaceBoard::new();
    let p = w.project(&mut board, &s);
    let gone = [card(&board, &a), ink_el(&board, &ink, 2)];
    let after = edited(&p.scene, |els| els.retain(|e| !gone.contains(&e.id)));
    let entries = s.history.undo.len();
    let start = s.space.clone();
    let route = board.route(&mut s, &Actor::User, &commit(&p.scene, &after, "delete"));
    let SpaceCommand::Gesture { label, ops } = one_command(&route) else {
        panic!("{route:?}")
    };
    assert_eq!(label, "Delete");
    assert!(
        ops.iter()
            .any(|op| matches!(op, theorem_space::SpaceOp::RemovePlacement { id } if id == &a))
    );
    assert!(
        ops.iter()
            .any(|op| matches!(op, theorem_space::SpaceOp::SetInk { id, .. } if id == &ink))
    );
    w.apply(&mut s, route);
    assert_eq!(s.history.undo.len(), entries + 1);
    assert!(s.space.placement(&a).is_none());
    w.run(&mut s, SpaceCommand::Undo { only_mine: false });
    assert_eq!(content(&s.space), content(&start));
}

#[test]
fn a_declared_turn_is_shown_on_the_board_and_taken_off_on_read_back() {
    // Wrong implementation rejected: storing the turned geometry, which would
    // double the turn on the next projection.
    let Fixture {
        mut w,
        mut s,
        a,
        ink,
        ..
    } = fixture();
    w.run(
        &mut s,
        SpaceCommand::Rotate {
            placements: vec![ink.clone()],
            rotation: 0.5,
        },
    );
    let stored = s.space.placement(&ink).unwrap().target.clone();
    let mut board = SpaceBoard::new();
    let p = w.project(&mut board, &s);
    let shape = ink_el(&board, &ink, 1);
    let ElementKind::Rect(g) = &find(&p.scene, shape).kind else {
        panic!()
    };
    assert!((g.rotation - 0.5).abs() < 1e-6, "the turn is shown");
    // Relabel the arrow only: the shape's stored JSON is written back as it was.
    let arrow = ink_el(&board, &ink, 2);
    let after = edited(&p.scene, |els| {
        find_mut(els, arrow).label = Some("flows".into())
    });
    let route = board.route(&mut s, &Actor::User, &commit(&p.scene, &after, "label"));
    w.apply(&mut s, route);
    let PlacementTarget::Ink { ink: now } = &s.space.placement(&ink).unwrap().target else {
        panic!()
    };
    let PlacementTarget::Ink { ink: was } = &stored else {
        panic!()
    };
    assert_eq!(now.elements[0], was.elements[0]);
    let back: Element = serde_json::from_value(now.elements[1].clone()).unwrap();
    let orig: Element = serde_json::from_value(was.elements[1].clone()).unwrap();
    assert_eq!(back.label.as_deref(), Some("flows"));
    let (ElementKind::Arrow(x), ElementKind::Arrow(y)) = (&back.kind, &orig.kind) else {
        panic!()
    };
    for (p, q) in [(x.x1, y.x1), (x.y1, y.y1), (x.x2, y.x2), (x.y2, y.y2)] {
        assert!((p - q).abs() < 1e-3, "stored unturned: {p} vs {q}");
    }
    assert_eq!(s.space.rotation(&ink), 0.5, "the turn itself is kept");
    // An object card is axis-aligned: the Space refuses to turn it.
    let mut ctx = w.ctx();
    assert!(
        theorem_space::execute(
            &mut s,
            &mut ctx,
            SpaceCommand::Rotate {
                placements: vec![a],
                rotation: 0.5
            }
        )
        .is_err()
    );
}

// ---- the mounted board -----------------------------------------------------------

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

fn mount(
    projection: Projection,
    cx: &mut TestAppContext,
) -> (
    Entity<WhiteboardView>,
    Rc<RefCell<Vec<WhiteboardEvent>>>,
    &mut VisualTestContext,
) {
    let (view, cx) = cx.add_window_view(|_, cx| WhiteboardView::new(Scene::default(), style(), cx));
    let events: Rc<RefCell<Vec<WhiteboardEvent>>> = Rc::default();
    view.update(cx, |view, cx| {
        view.present(projection, cx);
        let sink = events.clone();
        view.set_on_commit(Rc::new(move |event, _| {
            sink.borrow_mut().push(event.clone())
        }));
        view.set_tool(Tool::Select, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    (view, events, cx)
}

fn at(view: &Entity<WhiteboardView>, cx: &mut VisualTestContext, x: f32, y: f32) -> Point<Pixels> {
    let o = view.read_with(cx, |v, _| v.bounds.get().origin);
    point(o.x + px(x), o.y + px(y))
}

fn drag(view: &Entity<WhiteboardView>, cx: &mut VisualTestContext, from: [f32; 2], to: [f32; 2]) {
    let start = at(view, cx, from[0], from[1]);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    for t in [0.25, 0.5, 0.75, 1.0] {
        let p = at(
            view,
            cx,
            from[0] + (to[0] - from[0]) * t,
            from[1] + (to[1] - from[1]) * t,
        );
        cx.simulate_mouse_move(p, MouseButton::Left, Modifiers::none());
    }
    let end = at(view, cx, to[0], to[1]);
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
}

#[gpui::test]
fn mounted_drag_reaches_the_space_as_one_entry_and_comes_back(cx: &mut TestAppContext) {
    // Wrong implementations rejected: a board that applies the drag to its own
    // scene and never reports it (no event), a view in local history mode (its
    // own undo would hold the entry), and a host path that turns the drag into
    // more than one Space entry. The element id survives the round trip.
    let Fixture {
        mut w, mut s, b, ..
    } = fixture();
    let mut board = SpaceBoard::new();
    let p = w.project(&mut board, &s);
    let cb = card(&board, &b);
    let (view, events, cx) = mount(p, cx);
    // Card b is at (500, 100, 200, 120) and the camera is at the origin.
    drag(&view, cx, [560.0, 150.0], [620.0, 190.0]);
    let seen = events.borrow().clone();
    let commits: Vec<_> = seen
        .iter()
        .filter(|e| matches!(e, WhiteboardEvent::Commit { .. }))
        .collect();
    assert_eq!(commits.len(), 1, "{seen:?}");
    view.read_with(cx, |v, _| {
        assert!(v.history.is_empty(), "the board kept its own undo")
    });

    let WhiteboardEvent::Commit { after, .. } = commits[0] else {
        unreachable!()
    };
    let dropped = embed(after, cb);
    assert!(dropped.x > 500.0 && dropped.y > 100.0, "{dropped:?}");

    let entries = s.history.undo.len();
    let route = board.route(&mut s, &Actor::User, commits[0]);
    w.apply(&mut s, route);
    assert_eq!(s.history.undo.len(), entries + 1);
    let r = s.space.placement(&b).unwrap().rect;
    assert_eq!((r.x as f32, r.y as f32), (dropped.x, dropped.y));
    assert_eq!((r.w, r.h), (200.0, 120.0), "a move changed the size");

    let p = w.project(&mut board, &s);
    view.update(cx, |v, cx| v.present(p, cx));
    view.read_with(cx, |v, _| {
        let g = embed(&v.scene, cb);
        assert_eq!((g.x, g.y), (dropped.x, dropped.y));
    });
}

#[gpui::test]
fn mounted_board_does_not_move_a_held_card_and_reports_the_camera(cx: &mut TestAppContext) {
    // Wrong implementations rejected: a view that ignores the held set (the
    // pinned card would move and a commit would be reported), and a view whose
    // pan writes the scene instead of asking the host (a Commit would appear,
    // or the route would carry a command).
    let Fixture {
        mut w, mut s, b, ..
    } = fixture();
    w.run(
        &mut s,
        SpaceCommand::Pin {
            placements: vec![b.clone()],
            pinned: true,
        },
    );
    let mut board = SpaceBoard::new();
    let p = w.project(&mut board, &s);
    let cb = card(&board, &b);
    let (view, events, cx) = mount(p, cx);
    view.read_with(cx, |v, _| {
        assert!(v.is_held(cb) && v.is_axis_aligned(cb));
        // A card never offers rotation; an ink shape still does.
        assert!(!v.rotation_allowed(find(&v.scene, cb)));
        let shape = v
            .scene
            .elements
            .iter()
            .find(|e| {
                matches!(board.handle(e.id), Some(Handle::Ink { .. }))
                    && matches!(e.kind, ElementKind::Rect(_))
            })
            .unwrap();
        assert!(v.rotation_allowed(shape));
    });
    drag(&view, cx, [560.0, 150.0], [620.0, 190.0]);
    view.read_with(cx, |v, _| {
        assert_eq!(embed(&v.scene, cb).x, 500.0, "a pinned card moved")
    });
    assert!(
        events
            .borrow()
            .iter()
            .all(|e| !matches!(e, WhiteboardEvent::Commit { .. })),
        "{:?}",
        events.borrow()
    );

    let start = s.space.clone();
    let o = at(&view, cx, 300.0, 300.0);
    cx.simulate_event(ScrollWheelEvent {
        position: o,
        delta: ScrollDelta::Pixels(point(px(10.0), px(40.0))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    let seen = events.borrow().clone();
    let camera = seen
        .iter()
        .find(|e| matches!(e, WhiteboardEvent::CameraRequested { .. }))
        .unwrap_or_else(|| panic!("{seen:?}"));
    let route = board.route(&mut s, &Actor::User, camera);
    assert!(matches!(route, Route::Camera(_)), "{route:?}");
    let applied = w.apply(&mut s, route);
    assert_eq!(
        applied,
        Applied {
            space: None,
            owners: Vec::new()
        }
    );
    assert_eq!(s.space, start);
}

/// Rejects a report that hides kept content (an empty list for a drawing
/// holding an unread element), one that names the wrong slot, and one that
/// reads an unknown format as an empty, fully readable drawing.
#[test]
fn unreadable_names_each_kept_element_and_an_unknown_format() {
    let bad = json!({"id": 7, "kind": {"Hologram": {"depth": 3}}});
    let ink = InkContent {
        format: INK_FORMAT.into(),
        revision: 1,
        elements: vec![
            value(&shape(1, 0.0, 0.0, 10.0, 10.0)),
            bad,
            value(&shape(2, 20.0, 0.0, 10.0, 10.0)),
        ],
    };
    let kept = unreadable(&ink);
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].0, Some(1));
    let clean = InkContent {
        elements: vec![value(&shape(1, 0.0, 0.0, 10.0, 10.0))],
        ..ink.clone()
    };
    assert!(unreadable(&clean).is_empty());
    let alien = InkContent {
        format: "excalidraw.v2".into(),
        ..clean
    };
    assert_eq!(
        unreadable(&alien)
            .iter()
            .map(|(i, _)| *i)
            .collect::<Vec<_>>(),
        vec![None]
    );
}
