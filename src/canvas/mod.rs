//! Persistent spatial workspace. The view-less whiteboard Engine is hosted:
//! it requests camera changes and emits commits; this host owns both values.
pub(crate) mod authority;
pub(crate) mod input;
pub(crate) mod surface;

use crate::page::Page;
use crate::session::format::{BoardState, CardPlacement, NotePlacement};
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _, button::ButtonVariants as _, menu::ContextMenuExt as _,
};
use gpui_kit::{
    App, AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Point, Render, Styled as _, Subscription, Task,
    Window, canvas, div, px, size,
};
use gpui_whiteboard::engine::{Engine, EngineEvent, Frame, Gesture, Role, View};
use gpui_whiteboard::{
    BoardThumbnailView, Camera, Element, ElementKind, EmbedGeom, LocalThumbnailSnapshot, Scene,
    WhiteboardEvent, WhiteboardStyle,
};
use authority::PREVIEW_BASE;
use input::SharedInput;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

const CARD: u64 = 1;

/// Browser-owned live pages; tab identity is independent of position.
#[derive(Clone)]
pub(crate) struct BoardPage {
    pub id: u64,
    pub opener: Option<u64>,
    pub title: String,
    /// The page's address: its card's content reference on the shared board.
    pub url: Option<String>,
    pub page: Option<Entity<Page>>,
}

#[derive(Clone)]
pub(crate) enum BoardEvent {
    Open(u64),
    EditNote(Entity<crate::notes::Note>),
    PageOperation(u64, crate::actions::CanvasOperation),
    Paste(String),
    Changed,
    Focused,
}
impl gpui_kit::EventEmitter<BoardEvent> for SpatialCanvas {}
const CAPTION: f32 = 32.;

pub(crate) struct SpatialCanvas {
    page: Option<Entity<Page>>,
    pages: Vec<BoardPage>,
    notes: Vec<(u64, Entity<crate::notes::Note>)>,
    active_card: u64,
    context_card: Option<u64>,
    hidden: Vec<u64>,
    connections: Vec<[u64; 2]>,
    groups: Vec<Vec<u64>>,
    shown: bool,
    pub notes_inline: bool,
    _observations: Vec<Subscription>,
    painter: Entity<BoardThumbnailView>,
    colors: Rc<std::cell::RefCell<gpui_kit::component::Theme>>,
    engine: Engine,
    scene: Scene,
    camera: View,
    gesture: Option<Gesture>,
    selection: Vec<u64>,
    history: Vec<WhiteboardEvent>,
    focus: FocusHandle,
    input: SharedInput,
    bounds: Bounds<Pixels>,
    moving: bool,
    pending_overview: bool,
    cached: bool,
    settle: Option<Task<()>>,
    motion: Option<Task<()>>,
    springs: Option<[gpui_kit::SpringState; 3]>,
    _page_events: Option<Subscription>,
    _controls: Option<Subscription>,
    /// The shared board in Theorem, when the native authority is configured.
    shared: Option<Arc<authority::Client>>,
    link: authority::Sync,
    previews: Vec<authority::Preview>,
    /// Previews the person removed, until the authority answers.
    removing: Vec<u64>,
    conflict: Option<String>,
    /// Nothing is sent before a snapshot names what is already there.
    ready: bool,
    pushing: Option<Task<()>>,
    dirty: bool,
    _poll: Option<Task<()>>,
}

/// How often the board takes a fresh snapshot of the shared board.
const POLL: Duration = Duration::from_secs(2);

impl SpatialCanvas {
    pub fn new(page: Entity<Page>, cx: &mut Context<Self>) -> Self {
        Self::with_page(Some(page), cx)
    }

    pub fn with_page(page: Option<Entity<Page>>, cx: &mut Context<Self>) -> Self {
        let world = page
            .as_ref()
            .and_then(|page| page.read(cx).bounds)
            .map(|b| b.size)
            .unwrap_or(size(px(1280.), px(800.)));
        let input = SharedInput::default();
        let focus = cx.focus_handle();
        if let Some(page) = &page {
            page.update(cx, |p, _| {
                p.canvas_input = Some(input.clone());
                p.canvas_focus = Some(focus.clone());
            });
        }
        let mut scene = Scene {
            elements: vec![Element {
                id: CARD,
                kind: ElementKind::Embed(EmbedGeom {
                    page_id: 1,
                    title: page
                        .as_ref()
                        .map(|p| p.read(cx).title().to_string())
                        .unwrap_or_default(),
                    x: 48.,
                    y: 48.,
                    w: f32::from(world.width),
                    h: f32::from(world.height) + CAPTION,
                }),
                stroke: None,
                fill: None,
                label: None,
                label_color: None,
                styles: Vec::new(),
                mindmap: None,
            }],
            ..Default::default()
        };
        if page.is_none() {
            scene.elements.clear();
        }
        let colors = Rc::new(std::cell::RefCell::new(cx.theme().clone()));
        let palette = colors.clone();
        let painter = cx.new(|_| {
            BoardThumbnailView::new(
                LocalThumbnailSnapshot::for_scene_all_content(scene.clone(), 1280., 800.),
                Rc::new(move || {
                    let theme = palette.borrow();
                    WhiteboardStyle {
                        bg: theme.background,
                        grid: theme.border,
                        text: theme.muted_foreground,
                        ink: theme.foreground,
                        panel: theme.secondary,
                        panel_strong: theme.popover,
                        accent: theme.accent,
                        selection: theme.primary,
                        swatches: vec![],
                    }
                }),
            )
        });
        let mut engine = Engine::new();
        engine.present(
            scene.clone(),
            [(CARD, Role::Card { connectable: true })],
            [],
        );
        let events = page
            .as_ref()
            .map(|page| cx.observe(page, |_, _, cx| cx.notify()));
        let controls = page.as_ref().map(|page| {
            cx.subscribe(page, |this, _, event, cx| {
                if matches!(event, crate::page::PageEvent::RevealCanvas) {
                    let r = this.card();
                    let fit = (f32::from(this.bounds.size.width) as f64 / (r[2] + 96.))
                        .min(f32::from(this.bounds.size.height) as f64 / (r[3] + 96.));
                    this.zoom_to(this.camera.zoom.min(fit), cx);
                    this.reveal_page_point([r[2] as f32 / 2., (r[3] as f32 - CAPTION) / 2.], cx);
                }
            })
        });
        let shared = authority::Client::from_env().map(Arc::new);
        let poll = shared.clone().map(|client| {
            cx.spawn(async move |this, cx| {
                loop {
                    let board = async_compat::Compat::new(client.board()).await;
                    if this.update(cx, |this, cx| this.adopted(board, cx)).is_err() {
                        return;
                    }
                    cx.background_executor().timer(POLL).await;
                }
            })
        });
        Self {
            pages: page
                .as_ref()
                .map(|page| {
                    vec![BoardPage {
                        id: CARD,
                        opener: None,
                        title: page.read(cx).title().to_string(),
                        url: page.read(cx).url().map(|url| url.to_string()),
                        page: Some(page.clone()),
                    }]
                })
                .unwrap_or_default(),
            notes: Vec::new(),
            active_card: CARD,
            context_card: None,
            hidden: Vec::new(),
            connections: Vec::new(),
            groups: Vec::new(),
            shown: true,
            notes_inline: true,
            _observations: Vec::new(),
            page,
            painter,
            colors,
            engine,
            scene,
            camera: View::default(),
            gesture: None,
            selection: Vec::new(),
            history: Vec::new(),
            focus,
            input,
            bounds: Bounds::default(),
            moving: false,
            pending_overview: false,
            cached: false,
            settle: None,
            motion: None,
            springs: None,
            _page_events: events,
            _controls: controls,
            shared,
            link: authority::Sync::new(&[], authority::run_id()),
            previews: Vec::new(),
            removing: Vec::new(),
            conflict: None,
            ready: false,
            pushing: None,
            dirty: false,
            _poll: poll,
        }
    }

    /// Every card, note and preview as the shared board sees it.
    fn local_objects(&self, cx: &App) -> Vec<authority::LocalObject> {
        self.scene
            .elements
            .iter()
            .filter_map(|e| {
                let ElementKind::Embed(g) = &e.kind else {
                    return None;
                };
                let kind = if let Some(page) = self.pages.iter().find(|p| p.id == e.id) {
                    authority::LocalKind::Page {
                        url: page.url.clone().unwrap_or_default(),
                        title: page.title.clone(),
                    }
                } else if let Some((_, note)) = self.notes.iter().find(|(id, _)| *id == e.id) {
                    let note = note.read(cx);
                    authority::LocalKind::Note {
                        document_id: note.id.clone(),
                        title: note.title(cx),
                    }
                } else if e.id >= PREVIEW_BASE {
                    authority::LocalKind::Preview
                } else {
                    return None;
                };
                Some(authority::LocalObject {
                    local: e.id,
                    rect: [g.x as f64, g.y as f64, g.w as f64, g.h as f64],
                    kind,
                })
            })
            .collect()
    }

    /// Send what changed on this board to the shared board, one mutation
    /// at a time; a change made meanwhile is sent when these are answered.
    fn push(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.shared.clone() else {
            return;
        };
        if self.pushing.is_some() || !self.ready {
            self.dirty = true;
            return;
        }
        let objects = self.local_objects(cx);
        let removed: Vec<u64> = self.hidden.iter().chain(&self.removing).copied().collect();
        let requests = self.link.outgoing(&objects, &removed);
        if requests.is_empty() {
            return;
        }
        self.pushing = Some(cx.spawn(async move |this, cx| {
            for (local, request) in requests {
                let operation = request.mutation.operation();
                let answer = async_compat::Compat::new(client.mutate(&request)).await;
                if this
                    .update(cx, |this, cx| this.answered(local, operation, answer, cx))
                    .is_err()
                {
                    return;
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.pushing = None;
                if std::mem::take(&mut this.dirty) {
                    this.push(cx);
                }
            });
        }));
    }

    fn answered(
        &mut self,
        local: u64,
        operation: theorem_canvas::Operation,
        answer: Result<authority::Answer, String>,
        cx: &mut Context<Self>,
    ) {
        self.removing.retain(|id| *id != local);
        match answer {
            Ok(authority::Answer::Accepted(receipt)) => {
                self.link.receipt(local, &receipt);
                if let Some(theorem_canvas::Reconciled::PersonOverAgent { previous_actor, .. }) =
                    &receipt.reconciled
                {
                    let who = match previous_actor {
                        theorem_canvas::Actor::Agent { id } => format!("agent {id}"),
                        _ => "Theorem".to_owned(),
                    };
                    self.conflict = Some(format!(
                        "Your change replaced a newer placement by {who}."
                    ));
                }
            }
            Ok(authority::Answer::Refused(refusal)) => {
                if let Some(conflict) = self.link.refusal(local, operation, &refusal) {
                    if let Some(rect) = conflict.rect {
                        self.place(local, rect);
                    }
                    if conflict.hide {
                        self.withdraw(local);
                    }
                    self.conflict = Some(conflict.message);
                }
            }
            Err(message) => {
                self.link.failed(local);
                self.conflict = Some(format!("The shared board could not be reached: {message}"));
            }
        }
        self.present();
        cx.emit(BoardEvent::Changed);
        cx.notify();
    }

    /// Take a snapshot of the shared board.
    fn adopted(
        &mut self,
        board: Result<Option<theorem_canvas::CanvasBoard>, String>,
        cx: &mut Context<Self>,
    ) {
        let board = match board {
            Ok(Some(board)) => board,
            // The Space has not been saved in Theorem yet: nothing to share.
            Ok(None) => return,
            Err(message) => {
                self.conflict = Some(format!("The shared board could not be reached: {message}"));
                cx.notify();
                return;
            }
        };
        let known = self
            .scene
            .elements
            .iter()
            .map(|e| e.id)
            .chain(self.hidden.iter().copied())
            .collect();
        let Some(adopted) = self.link.adopt(&board, &known) else {
            return;
        };
        let changed = !adopted.moves.is_empty()
            || !adopted.withdrawn.is_empty()
            || adopted.previews != self.previews;
        if changed {
            self.take(adopted, cx);
        }
        // Only now, with every card where the authority holds it, does this
        // board send what it has that the authority does not.
        if !self.ready {
            self.ready = true;
            self.dirty = false;
            self.push(cx);
        }
    }

    /// Show a snapshot's changes: moves, removals and previews.
    fn take(&mut self, adopted: authority::Adopted, cx: &mut Context<Self>) {
        for (local, rect) in adopted.moves {
            self.place(local, rect);
        }
        if !adopted.withdrawn.is_empty() {
            let count = adopted.withdrawn.len();
            for local in adopted.withdrawn {
                self.withdraw(local);
            }
            self.conflict = Some(format!(
                "{count} {} removed from the shared board elsewhere. Show removed pages puts {} back.",
                if count == 1 { "card was" } else { "cards were" },
                if count == 1 { "it" } else { "them" },
            ));
        }
        let shown: Vec<u64> = adopted.previews.iter().map(|p| p.local).collect();
        self.scene.elements.retain(|e| {
            e.id < PREVIEW_BASE || shown.contains(&e.id) || self.link.is_busy(e.id)
        });
        for preview in &adopted.previews {
            let title = format!("{} \u{b7} {}", preview.kind, preview.title);
            let [x, y, w, h] = preview.rect.map(|n| n as f32);
            match self.scene.elements.iter_mut().find(|e| e.id == preview.local) {
                Some(e) => {
                    if let ElementKind::Embed(g) = &mut e.kind
                        && !self.link.is_busy(preview.local)
                    {
                        (g.x, g.y, g.w, g.h, g.title) = (x, y, w, h, title);
                    }
                }
                None => self.scene.elements.push(Element {
                    id: preview.local,
                    kind: ElementKind::Embed(EmbedGeom {
                        page_id: 0,
                        title,
                        x,
                        y,
                        w,
                        h,
                    }),
                    stroke: None,
                    fill: None,
                    label: None,
                    label_color: None,
                    styles: vec![],
                    mindmap: None,
                }),
            }
        }
        self.selection.retain(|id| *id < PREVIEW_BASE || shown.contains(id));
        self.previews = adopted.previews;
        self.present();
        cx.emit(BoardEvent::Changed);
        cx.notify();
    }

    /// Move a card to where the shared board holds it.
    fn place(&mut self, local: u64, rect: [f64; 4]) {
        if let Some(e) = self.scene.elements.iter_mut().find(|e| e.id == local)
            && let ElementKind::Embed(g) = &mut e.kind
        {
            [g.x, g.y, g.w, g.h] = rect.map(|n| n as f32);
        }
    }

    /// Take a card off this board because its object is gone.
    fn withdraw(&mut self, local: u64) {
        if local < PREVIEW_BASE && !self.hidden.contains(&local) {
            self.hidden.push(local);
        }
        self.scene.elements.retain(|e| e.id != local);
        self.selection.retain(|id| *id != local);
    }

    /// Why the shared board refused or reconciled the last change.
    pub fn conflict(&self) -> Option<&str> {
        self.conflict.as_deref()
    }

    /// The preview cards shown, for the native check.
    #[cfg(feature = "snapshot")]
    pub fn previews(&self) -> &[authority::Preview] {
        &self.previews
    }

    /// Move a card as a finished drag does: the new rect, then the same push.
    #[cfg(feature = "snapshot")]
    pub fn move_card(&mut self, local: u64, rect: [f64; 4], cx: &mut Context<Self>) {
        self.place(local, rect);
        self.present();
        self.push(cx);
        cx.emit(BoardEvent::Changed);
        cx.notify();
    }

    /// Where a card is on this board.
    #[cfg(feature = "snapshot")]
    pub fn rect(&self, local: u64) -> Option<[f64; 4]> {
        self.engine.bounds(local)
    }

    pub fn probe_state(&self) -> serde_json::Value {
        serde_json::json!({"camera": [self.camera.x, self.camera.y, self.camera.zoom],
            "card": self.card(), "selection": self.selection,
            "marquee": matches!(self.gesture, Some(Gesture::Marquee { .. })),
            "commits": self.history.len(), "board_drag": self.input.borrow().board_drag,
            "space": self.input.borrow().space.is_some(), "cached":self.cached})
    }

    pub fn set_cached(&mut self, cached: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.cached = cached;
        if let Some(page) = &self.page {
            page.update(cx, |p, cx| {
                p.canvas_cached = cached;
                p.set_visible(!cached);
                p.camera_moved(cx);
            });
        }
        let focus = if cached {
            self.focus.clone()
        } else {
            self.page
                .as_ref()
                .map(|p| p.focus_handle(cx))
                .unwrap_or_else(|| self.focus.clone())
        };
        window.focus(&focus, cx);
        cx.notify();
    }

    pub fn detach(&self, cx: &mut App) {
        self.input.borrow_mut().cancel_space();
        for page in self.pages.iter().filter_map(|p| p.page.as_ref()) {
            page.update(cx, |page, cx| {
                page.canvas_cached = false;
                page.canvas_input = None;
                page.canvas_focus = None;
                page.canvas_zoom = 1.;
                page.canvas_world = None;
                cx.notify();
            });
        }
    }

    pub fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.shown = shown;
        if !shown {
            self.detach(cx);
        }
        cx.notify();
    }

    fn command(&mut self, operation: crate::actions::CanvasOperation, cx: &mut Context<Self>) {
        use crate::actions::CanvasOperation as Op;
        let id = self.context_card;
        match operation {
            Op::Open => {
                if let Some(id) = id {
                    self.apply(vec![EngineEvent::Open(id)], cx);
                }
            }
            Op::Pin | Op::Duplicate | Op::Provenance => {
                if let Some(id) = id {
                    cx.emit(BoardEvent::PageOperation(id, operation));
                }
            }
            Op::Remove => {
                if let Some(id) = id {
                    if id >= PREVIEW_BASE {
                        // Not this board's to hide: the object leaves the
                        // shared board, and returns if the authority refuses.
                        self.removing.push(id);
                    } else {
                        self.hidden.push(id);
                    }
                    self.scene.elements.retain(|e| e.id != id);
                    self.selection.retain(|i| *i != id);
                    self.present();
                    self.push(cx);
                }
            }
            Op::Connect => {
                if let Some(id) = id {
                    for other in &self.selection {
                        if *other != id && !self.connections.contains(&[id, *other]) {
                            self.connections.push([id, *other]);
                        }
                    }
                }
            }
            Op::Group => {
                if self.selection.len() >= 2 {
                    self.groups.push(self.selection.clone());
                    self.present();
                }
            }
            Op::Paste => {
                if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    cx.emit(BoardEvent::Paste(text));
                }
            }
            Op::ShowAll => {
                self.hidden.clear();
                self.conflict = None;
                self.sync(self.pages.clone(), self.active_card, cx);
                for (_, note) in self.notes.clone() {
                    self.add_note(note, None, cx);
                }
            }
        }
        cx.emit(BoardEvent::Changed);
        cx.notify();
    }

    pub fn note(&self, id: u64) -> Option<Entity<crate::notes::Note>> {
        self.notes
            .iter()
            .find(|(n, _)| *n == id)
            .map(|(_, n)| n.clone())
    }
    pub fn selected_cards(&self) -> Vec<u64> {
        self.selection.clone()
    }
    pub fn selected_note_text(&self, cx: &App) -> String {
        self.notes
            .iter()
            .filter(|(id, _)| self.selection.contains(id))
            .map(|(_, note)| {
                let note = note.read(cx);
                format!(
                    "## Note {}\nDocument: {}\n\n{}\n\n",
                    note.path,
                    note.id,
                    note.editor.read(cx).text()
                )
            })
            .collect()
    }

    pub fn state(&self, cx: &App) -> BoardState {
        BoardState {
            canvas_links: self.link.links(),
            hidden: self.hidden.clone(),
            connections: self.connections.clone(),
            groups: self.groups.clone(),
            camera: Some([self.camera.x, self.camera.y, self.camera.zoom]),
            notes: self
                .notes
                .iter()
                .filter_map(|(id, note)| {
                    let rect = self.engine.bounds(*id)?;
                    Some(NotePlacement {
                        id: *id,
                        path: note.read(cx).path.clone(),
                        rect,
                    })
                })
                .collect(),
            cards: self
                .scene
                .elements
                .iter()
                .filter(|e| self.pages.iter().any(|p| p.id == e.id))
                .filter_map(|e| match &e.kind {
                    ElementKind::Embed(g) => Some(CardPlacement {
                        tab: e.id,
                        rect: [g.x as f64, g.y as f64, g.w as f64, g.h as f64],
                    }),
                    _ => None,
                })
                .collect(),
        }
    }

    pub fn restore(&mut self, state: BoardState, cx: &mut Context<Self>) {
        let state = state.validate();
        self.link = authority::Sync::new(&state.canvas_links, authority::run_id());
        self.ready = false;
        self.hidden = state.hidden;
        self.connections = state.connections;
        self.groups = state.groups;
        self.scene.elements.retain(|e| !self.hidden.contains(&e.id));
        for card in state
            .cards
            .into_iter()
            .chain(state.notes.into_iter().map(|n| CardPlacement {
                tab: n.id,
                rect: n.rect,
            }))
        {
            if let Some(e) = self.scene.elements.iter_mut().find(|e| e.id == card.tab)
                && let ElementKind::Embed(g) = &mut e.kind
            {
                [g.x, g.y, g.w, g.h] = card.rect.map(|n| n as f32);
            }
        }
        if let Some([x, y, zoom]) = state.camera {
            self.camera = View { x, y, zoom };
        }
        self.present();
        self.push(cx);
        cx.notify();
    }

    fn present(&mut self) {
        // Region geometry is derived from durable membership and card positions.
        // Whiteboard's existing Region gesture moves all members in one commit.
        self.scene
            .elements
            .retain(|e| matches!(e.kind, ElementKind::Embed(_)));
        let mut scene = self.scene.clone();
        let mut roles: Vec<_> = self
            .pages
            .iter()
            .map(|p| (p.id, Role::Card { connectable: true }))
            .chain(
                self.notes
                    .iter()
                    .map(|(id, _)| (*id, Role::Card { connectable: true })),
            )
            .chain(
                self.previews
                    .iter()
                    .map(|p| (p.local, Role::Card { connectable: false })),
            )
            .collect();
        let mut region_id = u64::MAX;
        for members in &self.groups {
            let cards: Vec<_> = self
                .scene
                .elements
                .iter()
                .filter(|e| members.contains(&e.id))
                .collect();
            if cards.len() < 2 {
                continue;
            }
            let bounds: Vec<_> = cards
                .iter()
                .filter_map(|e| match &e.kind {
                    ElementKind::Embed(r) => Some([r.x as f64, r.y as f64, r.w as f64, r.h as f64]),
                    _ => None,
                })
                .collect();
            let Some([x, y, w, h]) = group_rect(&bounds) else {
                continue;
            };
            while scene.elements.iter().any(|e| e.id == region_id) {
                region_id -= 1;
            }
            let mut region = (*cards[0]).clone();
            region.id = region_id;
            region.kind = ElementKind::Rect(gpui_whiteboard::BoxGeom {
                x: x as f32,
                y: y as f32,
                w: w as f32,
                h: h as f32,
                width: 1.,
                rotation: 0.,
            });
            scene.elements.insert(0, region);
            roles.push((
                region_id,
                Role::Region {
                    members: cards.iter().map(|e| e.id).collect(),
                },
            ));
            region_id -= 1;
        }
        self.engine.present(scene, roles, []);
    }

    /// Synchronize references and titles without replacing saved geometry.
    /// Unloaded tabs stay as cards and only wake when explicitly opened.
    pub fn sync(&mut self, pages: Vec<BoardPage>, active: u64, cx: &mut Context<Self>) {
        self.detach(cx);
        self.scene.elements.retain(|e| {
            e.id >= PREVIEW_BASE
                || pages.iter().any(|p| p.id == e.id)
                || self.notes.iter().any(|(id, _)| *id == e.id)
        });
        for (index, p) in pages.iter().enumerate() {
            if self.hidden.contains(&p.id) {
                continue;
            }
            if let Some(e) = self.scene.elements.iter_mut().find(|e| e.id == p.id) {
                if let ElementKind::Embed(g) = &mut e.kind {
                    g.title.clone_from(&p.title);
                }
            } else {
                let parent = p
                    .opener
                    .and_then(|id| self.scene.elements.iter().find(|e| e.id == id));
                let (x, y) = match parent.map(|e| &e.kind) {
                    Some(ElementKind::Embed(g)) => (g.x + g.w + 96., g.y + 80.),
                    _ => (
                        (index % 3) as f32 * 1440. + 48.,
                        (index / 3) as f32 * 960. + 48.,
                    ),
                };
                let world = p
                    .page
                    .as_ref()
                    .and_then(|p| p.read(cx).bounds)
                    .map(|b| b.size)
                    .unwrap_or(size(px(1280.), px(800.)));
                self.scene.elements.push(Element {
                    id: p.id,
                    kind: ElementKind::Embed(EmbedGeom {
                        page_id: p.id as i64,
                        title: p.title.clone(),
                        x,
                        y,
                        w: f32::from(world.width).max(160.),
                        h: f32::from(world.height).max(120.) + CAPTION,
                    }),
                    stroke: None,
                    fill: None,
                    label: None,
                    label_color: None,
                    styles: vec![],
                    mindmap: None,
                });
            }
        }
        self.page = pages
            .iter()
            .find(|p| p.id == active)
            .and_then(|p| p.page.clone());
        self.active_card = active;
        self._observations = pages
            .iter()
            .filter_map(|p| p.page.as_ref())
            .map(|page| cx.observe(page, |_, _, cx| cx.notify()))
            .collect();
        self.pages = pages;
        self.selection.retain(|id| {
            *id >= PREVIEW_BASE
                || self.pages.iter().any(|p| p.id == *id)
                || self.notes.iter().any(|(n, _)| n == id)
        });
        self.present();
        self.push(cx);
        cx.notify();
    }

    pub fn add_note(
        &mut self,
        note: Entity<crate::notes::Note>,
        saved: Option<NotePlacement>,
        cx: &mut Context<Self>,
    ) {
        let existing = self
            .notes
            .iter()
            .find(|(_, n)| n.entity_id() == note.entity_id())
            .map(|(id, _)| *id);
        if existing.is_some_and(|id| self.engine.bounds(id).is_some()) {
            return;
        }
        let id = existing
            .or_else(|| saved.as_ref().map(|s| s.id))
            .unwrap_or_else(|| {
                self.notes
                    .iter()
                    .map(|(id, _)| *id)
                    .min()
                    .unwrap_or(u64::MAX)
                    - 1
            });
        self.hidden.retain(|hidden| *hidden != id);
        let active = self.card();
        let bottom = self
            .scene
            .elements
            .iter()
            .filter_map(|e| self.engine.bounds(e.id))
            .map(|r| r[1] + r[3])
            .fold(active[1] + active[3], f64::max);
        let rect = saved.map(|s| s.rect).unwrap_or([
            active[0] + (active[2] - 420.) / 2.,
            bottom + 64.,
            420.,
            480.,
        ]);
        let [x, y, w, h] = rect.map(|n| n as f32);
        self.scene.elements.push(Element {
            id,
            kind: ElementKind::Embed(EmbedGeom {
                page_id: 0,
                title: note.read(cx).title(cx),
                x,
                y,
                w,
                h,
            }),
            stroke: None,
            fill: None,
            label: None,
            label_color: None,
            styles: vec![],
            mindmap: None,
        });
        if existing.is_none() {
            self.notes.push((id, note));
        }
        self.present();
        self.push(cx);
        cx.emit(BoardEvent::Changed);
        cx.notify();
    }

    pub fn arrange(&mut self, cx: &mut Context<Self>) {
        for (index, element) in self.scene.elements.iter_mut().enumerate() {
            if let ElementKind::Embed(g) = &mut element.kind {
                g.x = 48. + (index % 3) as f32 * 1440.;
                g.y = 48. + (index / 3) as f32 * 960.;
            }
        }
        self.present();
        self.push(cx);
        self.overview(cx);
        cx.emit(BoardEvent::Changed);
    }

    fn animate_to(&mut self, target: View, leave: bool, cx: &mut Context<Self>) {
        self.motion = None;
        if cx.reduce_motion() {
            self.camera = target;
            self.moving = false;
            if leave {
                cx.emit(BoardEvent::Focused);
            }
            cx.emit(BoardEvent::Changed);
            cx.notify();
            return;
        }
        let start = [
            self.camera.x as f32,
            self.camera.y as f32,
            self.camera.zoom as f32,
        ];
        let mut states = self
            .springs
            .unwrap_or(start.map(|position| gpui_kit::SpringState {
                position,
                velocity: 0.,
            }));
        let targets = [target.x as f32, target.y as f32, target.zoom as f32];
        self.moving = true;
        self.motion = Some(cx.spawn(async move |this, cx| {
            let spring = gpui_kit::SpringConfig::new(400., 40., 1.);
            let mut last = std::time::Instant::now();
            for frame in 0..180 {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let dt = last.elapsed().as_secs_f32().min(0.1);
                last = std::time::Instant::now();
                for i in 0..3 {
                    states[i] = spring.step(states[i], targets[i], dt);
                }
                let done = frame == 179
                    || (0..3).all(|i| {
                        spring.is_settled(states[i], targets[i], if i == 2 { 0.001 } else { 0.1 })
                    });
                if this
                    .update(cx, |this, cx| {
                        this.camera = if done {
                            target
                        } else {
                            View {
                                x: states[0].position as f64,
                                y: states[1].position as f64,
                                zoom: states[2].position as f64,
                            }
                        };
                        this.springs = (!done).then_some(states);
                        this.moving = !done;
                        if done {
                            cx.emit(BoardEvent::Changed);
                            if leave {
                                cx.emit(BoardEvent::Focused);
                            }
                        }
                        cx.notify();
                    })
                    .is_err()
                    || done
                {
                    break;
                }
            }
        }));
    }

    pub fn focus_current(&mut self, leave: bool, cx: &mut Context<Self>) {
        let r = self.card();
        let zoom = if leave { 1. } else { 0.75 };
        let w = f32::from(self.bounds.size.width) as f64;
        let h = f32::from(self.bounds.size.height) as f64;
        self.animate_to(
            View {
                x: r[0] + r[2] / 2. - w / (2. * zoom),
                y: r[1] + r[3] / 2. - h / (2. * zoom),
                zoom,
            },
            leave,
            cx,
        );
    }

    pub fn overview(&mut self, cx: &mut Context<Self>) {
        if self.bounds.size.width <= px(0.) || self.bounds.size.height <= px(0.) {
            self.pending_overview = true;
            return;
        }
        let rects: Vec<_> = self
            .scene
            .elements
            .iter()
            .filter_map(|p| self.engine.bounds(p.id))
            .collect();
        if rects.is_empty() {
            return;
        }
        let x = rects.iter().map(|r| r[0]).fold(f64::INFINITY, f64::min);
        let y = rects.iter().map(|r| r[1]).fold(f64::INFINITY, f64::min);
        let right = rects
            .iter()
            .map(|r| r[0] + r[2])
            .fold(f64::NEG_INFINITY, f64::max);
        let bottom = rects
            .iter()
            .map(|r| r[1] + r[3])
            .fold(f64::NEG_INFINITY, f64::max);
        let width = f32::from(self.bounds.size.width).max(800.) as f64;
        let height = f32::from(self.bounds.size.height).max(600.) as f64;
        let zoom = (width / (right - x + 96.))
            .min(height / (bottom - y + 96.))
            .clamp(0.05, 1.);
        self.animate_to(
            View {
                x: x - 48. / zoom,
                y: y - 48. / zoom,
                zoom,
            },
            false,
            cx,
        );
    }

    /// The page the canvas lays out.
    pub fn page(&self) -> Option<&Entity<Page>> {
        self.page.as_ref()
    }

    fn frame(&self) -> Frame<'_> {
        Frame {
            selection: &self.selection,
            view: self.camera,
            engaged: None,
            hovered: None,
        }
    }

    fn local(&self, p: Point<Pixels>) -> [f64; 2] {
        [
            f32::from(p.x - self.bounds.origin.x) as f64,
            f32::from(p.y - self.bounds.origin.y) as f64,
        ]
    }

    fn card(&self) -> [f64; 4] {
        self.engine.preview(
            &self.gesture,
            self.active_card,
            self.engine
                .bounds(self.active_card)
                .unwrap_or([48., 48., 1280., 832.]),
        )
    }

    fn apply(&mut self, events: Vec<EngineEvent>, cx: &mut Context<Self>) {
        self.motion = None;
        self.springs = None;
        for event in events {
            match event {
                EngineEvent::Camera(view) => {
                    self.camera = view;
                    self.moving = true;
                    for page in self.pages.iter().filter_map(|p| p.page.as_ref()) {
                        page.update(cx, |p, cx| p.camera_moved(cx));
                    }
                    self.schedule_settle(cx);
                }
                EngineEvent::Select(selection) => self.selection = selection,
                EngineEvent::Commit(commit @ WhiteboardEvent::Commit { .. }) => {
                    if let WhiteboardEvent::Commit { after, .. } = &commit {
                        self.scene = after.clone();
                    }
                    if self.history.len() == 100 {
                        self.history.remove(0);
                    }
                    self.history.push(commit);
                    self.present();
                    self.push(cx);
                    cx.emit(BoardEvent::Changed);
                }
                EngineEvent::Connect { from, to, .. } => {
                    if !self.connections.contains(&[from, to]) {
                        self.connections.push([from, to]);
                    }
                }
                EngineEvent::Open(id) => {
                    if let Some((_, note)) = self.notes.iter().find(|(n, _)| *n == id) {
                        cx.emit(BoardEvent::EditNote(note.clone()));
                    } else if id < PREVIEW_BASE {
                        cx.emit(BoardEvent::Open(id));
                    }
                }
                _ => {}
            }
        }
        cx.emit(BoardEvent::Changed);
        cx.notify();
    }

    fn schedule_settle(&mut self, cx: &mut Context<Self>) {
        self.settle = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(180))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.gesture.is_none() {
                    this.moving = false;
                    cx.notify();
                }
            });
        }));
    }

    pub fn zoom_to(&mut self, zoom: f64, cx: &mut Context<Self>) {
        let event = self.engine.zoom_step(
            &self.frame(),
            [
                f32::from(self.bounds.size.width) as f64,
                f32::from(self.bounds.size.height) as f64,
            ],
            zoom / self.camera.zoom,
        );
        self.apply(vec![event], cx);
    }

    pub fn pan_by(&mut self, delta: [f64; 2], cx: &mut Context<Self>) {
        if let Some(event) = self.engine.wheel([0., 0.], delta, false, &self.frame()) {
            self.apply(vec![event], cx);
        }
    }

    pub fn reveal_page_point(&mut self, css: [f32; 2], cx: &mut Context<Self>) {
        let r = self.card();
        let screen = self
            .camera
            .to_screen([r[0] + css[0] as f64, r[1] + CAPTION as f64 + css[1] as f64]);
        self.pan_by(
            [
                f32::from(self.bounds.size.width) as f64 / 2. - screen[0],
                f32::from(self.bounds.size.height) as f64 / 2. - screen[1],
            ],
            cx,
        );
    }
}

impl Focusable for SpatialCanvas {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SpatialCanvas {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        *self.colors.borrow_mut() = cx.theme().clone();
        let camera = Camera {
            x: self.camera.x as f32,
            y: self.camera.y as f32,
            zoom: self.camera.zoom as f32,
        };
        let mut scene = self.scene.clone();
        for element in &mut scene.elements {
            if let ElementKind::Embed(g) = &mut element.kind {
                let r = self.engine.preview(
                    &self.gesture,
                    element.id,
                    [g.x as f64, g.y as f64, g.w as f64, g.h as f64],
                );
                [g.x, g.y, g.w, g.h] = r.map(|n| n as f32);
            }
        }
        let selected: Vec<_> = scene
            .elements
            .iter()
            .filter(|e| self.selection.contains(&e.id))
            .map(|e| e.kind.clone())
            .collect();
        let mut snapshot =
            LocalThumbnailSnapshot::for_scene_all_content(scene.clone(), 1280., 800.);
        snapshot.spec.camera = camera;
        self.painter.update(cx, |p, _| p.set_snapshot(snapshot));
        let z = camera.zoom;
        let mut surfaces = Vec::new();
        let mut trails = Vec::new();
        for binding in &self.pages {
            let Some(rect) = self.engine.bounds(binding.id) else {
                continue;
            };
            let r = self.engine.preview(&self.gesture, binding.id, rect);
            if let Some(parent) = binding.opener.and_then(|id| self.engine.bounds(id)) {
                trails.push((
                    self.camera
                        .to_screen([parent[0] + parent[2], parent[1] + 16.]),
                    self.camera.to_screen([r[0], r[1] + 16.]),
                ));
            }
            let Some(page) = &binding.page else { continue };
            let pos = self.camera.to_screen([r[0], r[1] + CAPTION as f64]);
            let world = size(px(r[2] as f32), px(r[3] as f32 - CAPTION));
            let cached = self.cached || binding.id != self.active_card;
            page.update(cx, |p, _| {
                p.canvas_input = Some(self.input.clone());
                p.canvas_focus = Some(self.focus.clone());
                p.canvas_world = Some(world);
                p.canvas_zoom = z;
                p.canvas_cached = cached;
                p.canvas_settled = !cached && !self.moving && self.gesture.is_none();
                p.canvas_cap = window.viewport_size();
                p.corners = gpui_kit::Corners::all(px(8.));
            });
            surfaces.push(
                div()
                    .absolute()
                    .left(px(pos[0] as f32))
                    .top(px(pos[1] as f32))
                    .w(world.width * z)
                    .h(world.height * z)
                    .overflow_hidden()
                    .child(page.clone()),
            );
        }
        for (id, note) in &self.notes {
            let Some(rect) = self.engine.bounds(*id) else {
                continue;
            };
            let r = self.engine.preview(&self.gesture, *id, rect);
            let pos = self.camera.to_screen([r[0], r[1] + CAPTION as f64]);
            surfaces.push(
                div()
                    .absolute()
                    .left(px(pos[0] as f32))
                    .top(px(pos[1] as f32))
                    .w(px(r[2] as f32 * z))
                    .h(px((r[3] as f32 - CAPTION) * z))
                    .overflow_hidden()
                    .child(if self.notes_inline && z >= 0.75 {
                        note.clone().into_any_element()
                    } else {
                        div()
                            .size_full()
                            .p_3()
                            .bg(cx.theme().popover)
                            .text_color(cx.theme().foreground)
                            .text_size(px((16. * z).max(10.)))
                            .child(
                                note.read(cx)
                                    .editor
                                    .read(cx)
                                    .text()
                                    .chars()
                                    .take(2000)
                                    .collect::<String>(),
                            )
                            .into_any_element()
                    }),
            );
        }
        // Objects placed elsewhere: a typed preview card, never a copy of
        // their content.
        for preview in &self.previews {
            let Some(rect) = self.engine.bounds(preview.local) else {
                continue;
            };
            let r = self.engine.preview(&self.gesture, preview.local, rect);
            let pos = self.camera.to_screen([r[0], r[1] + CAPTION as f64]);
            let small = px((12. * z).max(9.));
            surfaces.push(
                div()
                    .absolute()
                    .left(px(pos[0] as f32))
                    .top(px(pos[1] as f32))
                    .w(px(r[2] as f32 * z))
                    .h(px((r[3] as f32 - CAPTION) * z))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_3()
                    .bg(cx.theme().secondary)
                    .text_color(cx.theme().foreground)
                    .child(
                        div()
                            .text_size(small)
                            .text_color(cx.theme().muted_foreground)
                            .child(preview.kind),
                    )
                    .child(
                        div()
                            .text_size(px((16. * z).max(10.)))
                            .child(preview.title.clone()),
                    )
                    .child(
                        div()
                            .text_size(small)
                            .text_color(cx.theme().muted_foreground)
                            .child(preview.detail.clone()),
                    ),
            );
        }
        let conflict = self.conflict.clone().map(|message| {
            div()
                .absolute()
                // Below the browser's board toolbar (`board_toolbar`, top-left),
                // so a long message is never drawn under it.
                .top(px(48.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    div()
                        .id("canvas-conflict")
                        .flex()
                        .items_center()
                        .gap_3()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().popover)
                        .text_color(cx.theme().foreground)
                        .child(message)
                        .child(
                            gpui_kit::component::button::Button::new("canvas-conflict-dismiss")
                                .small()
                                .ghost()
                                .label("Dismiss")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.conflict = None;
                                    cx.notify();
                                })),
                        ),
                )
        });
        for [from, to] in &self.connections {
            if let (Some(a), Some(b)) = (self.engine.bounds(*from), self.engine.bounds(*to)) {
                trails.push((
                    self.camera.to_screen([a[0] + a[2], a[1] + a[3] / 2.]),
                    self.camera.to_screen([b[0], b[1] + b[3] / 2.]),
                ));
            }
        }
        let group_bounds: Vec<_> = self
            .groups
            .iter()
            .filter_map(|members| {
                let bounds: Vec<_> = members
                    .iter()
                    .filter_map(|id| {
                        self.engine
                            .bounds(*id)
                            .map(|r| self.engine.preview(&self.gesture, *id, r))
                    })
                    .collect();
                let [x, y, w, h] = group_rect(&bounds)?;
                Some((
                    self.camera.to_screen([x, y]),
                    self.camera.to_screen([x + w, y + h]),
                ))
            })
            .collect();
        let popup = self.page.as_ref().and_then(|page| {
            let popup_page = page.downgrade();
            page.read(cx)
                .controls
                .popup
                .as_ref()
                .map(|(menu, position, _)| {
                    gpui_kit::deferred(
                        gpui_kit::anchored()
                            .position(*position)
                            .snap_to_window_with_margin(px(8.))
                            .child(
                                div().relative().child(menu.clone()).child(
                                    gpui_kit::canvas(
                                        move |bounds, _, cx| {
                                            let _ = popup_page.update(cx, |p, _| {
                                                p.controls.popup_bounds = Some(bounds)
                                            });
                                        },
                                        |_, _, _, _| {},
                                    )
                                    // Without an inset, layout leaves it where it would
                                    // flow: after the menu, at its bottom edge.
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .size_full(),
                                ),
                            ),
                    )
                    .with_priority(2)
                })
        });
        let marquee = match &self.gesture {
            Some(Gesture::Marquee { start, now, .. }) => Some((*start, *now)),
            _ => None,
        };
        let color = cx.theme().primary;
        let weak = cx.weak_entity();
        div()
            .id("spatial-canvas")
            .track_focus(&self.focus)
            .on_action(
                cx.listener(|this, action: &crate::actions::CanvasAction, _, cx| {
                    this.command(action.operation, cx)
                }),
            )
            .relative()
            .size_full()
            .overflow_hidden()
            .capture_any_mouse_down(cx.listener(Self::press))
            .capture_any_mouse_up(cx.listener(Self::release))
            .on_mouse_move(cx.listener(Self::drag))
            .on_scroll_wheel(cx.listener(Self::wheel))
            .capture_pinch(cx.listener(|this, ev: &gpui_kit::PinchEvent, _, cx| {
                let mut view = this.camera;
                view.zoom_around(this.local(ev.position), (1. + ev.delta as f64).max(0.01));
                this.apply(vec![EngineEvent::Camera(view)], cx);
                cx.stop_propagation();
            }))
            .on_key_down(cx.listener(|this, ev: &gpui_kit::KeyDownEvent, _, cx| {
                match ev.keystroke.key.as_str() {
                    "space" => this.input.borrow_mut().hold_space(&ev.keystroke),
                    "v" => this.input.borrow_mut().select_tool = true,
                    "h" => this.input.borrow_mut().select_tool = false,
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .on_key_up(cx.listener(|this, ev: &gpui_kit::KeyUpEvent, _, _| {
                if ev.keystroke.key == "space" {
                    this.input.borrow_mut().release_space();
                }
            }))
            .child(
                canvas(
                    move |b, _, cx| {
                        let _ = weak.update(cx, |s, cx| {
                            s.bounds = b;
                            if s.pending_overview {
                                s.pending_overview = false;
                                s.overview(cx);
                            }
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(div().absolute().inset_0().child(self.painter.clone()))
            .children(surfaces)
            .children(popup)
            .children(conflict)
            .child(
                canvas(
                    |_, _, _| {},
                    move |b, _, window, _| {
                        for (a, c) in group_bounds {
                            window.paint_quad(gpui_kit::outline(
                                gpui_kit::Bounds::from_corners(
                                    b.origin + gpui_kit::point(px(a[0] as f32), px(a[1] as f32)),
                                    b.origin + gpui_kit::point(px(c[0] as f32), px(c[1] as f32)),
                                ),
                                color.opacity(0.3),
                                gpui_kit::BorderStyle::Solid,
                            ));
                        }
                        for (a, c) in trails {
                            let mut path = gpui_kit::PathBuilder::stroke(px(1.));
                            path.move_to(
                                b.origin + gpui_kit::point(px(a[0] as f32), px(a[1] as f32)),
                            );
                            path.line_to(
                                b.origin + gpui_kit::point(px(c[0] as f32), px(c[1] as f32)),
                            );
                            if let Ok(path) = path.build() {
                                window.paint_path(path, color.opacity(0.45));
                            }
                        }
                        for kind in selected {
                            gpui_whiteboard::paint_selection(
                                &kind, camera, b.origin, color, window,
                            );
                        }
                        if let Some((a, c)) = marquee {
                            gpui_whiteboard::paint_marquee(
                                [a[0] as f32, a[1] as f32],
                                [c[0] as f32, c[1] as f32],
                                camera,
                                b.origin,
                                color,
                                window,
                            );
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .context_menu({
                let board = cx.weak_entity();
                move |menu, _, cx| {
                    use crate::actions::{CanvasAction, CanvasOperation as Op};
                    let hit = board.read_with(cx, |b, _| b.context_card).ok().flatten();
                    let action = |operation| {
                        Box::new(CanvasAction { operation }) as Box<dyn gpui_kit::Action>
                    };
                    let page = hit.is_some_and(|id| {
                        board
                            .read_with(cx, |b, _| b.pages.iter().any(|p| p.id == id))
                            .unwrap_or(false)
                    });
                    if hit.is_some_and(|id| id >= PREVIEW_BASE) {
                        menu.menu("Remove from shared board", action(Op::Remove))
                    } else if hit.is_some() && !page {
                        menu.menu("Open note", action(Op::Open))
                            .menu("Inspect provenance", action(Op::Provenance))
                            .menu("Connect to selected cards", action(Op::Connect))
                            .separator()
                            .menu("Remove from board", action(Op::Remove))
                    } else if hit.is_some() {
                        menu.menu("Open", action(Op::Open))
                            .menu("Pin page", action(Op::Pin))
                            .menu("Duplicate page", action(Op::Duplicate))
                            .menu("Connect to selected cards", action(Op::Connect))
                            .menu("Inspect provenance", action(Op::Provenance))
                            .separator()
                            .menu("Remove from board", action(Op::Remove))
                    } else {
                        menu.menu("New note", Box::new(crate::actions::NewCanvasNote))
                            .menu("Add page", Box::new(crate::actions::NewTab))
                            .menu("Paste", action(Op::Paste))
                            .menu("Group selected cards", action(Op::Group))
                            .menu("Arrange cards", Box::new(crate::actions::ArrangeCanvas))
                            .menu("Show removed pages", action(Op::ShowAll))
                            .menu("Visit trail", Box::new(crate::actions::ShowVisitTrail))
                    }
                }
            })
    }
}

fn group_rect(bounds: &[[f64; 4]]) -> Option<[f64; 4]> {
    if bounds.is_empty() {
        return None;
    }
    let x = bounds.iter().map(|r| r[0]).fold(f64::INFINITY, f64::min) - 24.;
    let y = bounds.iter().map(|r| r[1]).fold(f64::INFINITY, f64::min) - 24.;
    let right = bounds
        .iter()
        .map(|r| r[0] + r[2])
        .fold(f64::NEG_INFINITY, f64::max)
        + 24.;
    let bottom = bounds
        .iter()
        .map(|r| r[1] + r[3])
        .fold(f64::NEG_INFINITY, f64::max)
        + 24.;
    Some([x, y, right - x, bottom - y])
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_whiteboard::engine::{Button, Hit, Mods};

    #[gpui_kit::test]
    fn grouped_cards_move_together_and_restore(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
        });
        let board = cx.new(|cx| SpatialCanvas::with_page(None, cx));
        board.update(cx, |board, cx| {
            let pages: Vec<_> = [1, 2]
                .into_iter()
                .map(|id| BoardPage {
                    id,
                    opener: None,
                    title: format!("Page {id}"),
                    url: None,
                    page: None,
                })
                .collect();
            board.sync(pages.clone(), 1, cx);
            let before = board.state(cx);
            board.selection = vec![1, 2];
            board.command(crate::actions::CanvasOperation::Group, cx);
            // The padded group edge is outside both cards and their handles.
            let start = [28., 28.];
            assert!(matches!(board.engine.hit(start), Some(Hit::Region(_))));
            let at = board.camera.to_screen(start);
            let mut gesture = None;
            let events = board.engine.press(
                &mut gesture,
                at,
                Button::Primary,
                Mods::default(),
                1,
                &board.frame(),
            );
            board.apply(events, cx);
            let end = board.camera.to_screen([start[0] + 100., start[1] + 60.]);
            let events = board.engine.drag_to(&mut gesture, end, &board.frame());
            board.apply(events, cx);
            let events = board.engine.release(&mut gesture, end, &board.frame());
            board.apply(events, cx);
            let saved = board.state(cx);
            assert_eq!(saved.groups, vec![vec![1, 2]]);
            assert_eq!(saved.cards.len(), 2);
            for (old, new) in before.cards.iter().zip(&saved.cards) {
                assert_eq!(
                    new.rect,
                    [
                        old.rect[0] + 100.,
                        old.rect[1] + 60.,
                        old.rect[2],
                        old.rect[3]
                    ]
                );
            }
            let mut restored = SpatialCanvas::with_page(None, cx);
            restored.sync(pages, 1, cx);
            restored.restore(saved.clone(), cx);
            assert_eq!(restored.state(cx), saved);
        });
    }
}
