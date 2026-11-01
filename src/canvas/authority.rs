//! The board's link to Theorem's canvas authority (`theorem.canvas.v1`).
//!
//! Each card and note is an object on a saved Theorem Space with a stable id.
//! A gesture becomes one revision-checked mutation, the same call an agent's
//! `canvas_mutate` makes, and the board takes the authority's answer: a
//! receipt, or a refusal carrying the newer object, shown as a conflict.
//! Objects another surface or an agent placed are drawn as typed preview
//! cards. Content stays with its owner: a card refers to its page's address
//! and a note to its document in the notes store, never to a copy.
//!
//! Only an explicit "Remove from board" deletes an object. A closed tab's
//! object stays on the shared board and is drawn here as a preview. Without
//! `THEOREMWEB_AUTHORITY_URL` and `THEOREMWEB_AUTH_TOKEN_FILE` the board
//! stays local.
use crate::session::format::CanvasLink;
use std::collections::{BTreeMap, BTreeSet};
use theorem_canvas::{
    Actor, CanvasBoard, CanvasMutation, CanvasObject, CanvasReceipt, CanvasRefusal, CanvasRequest,
    NewContent, ObjectType, Operation, Rect, RefusalCode,
};

/// Board ids from here up are previews: objects this board shows but did
/// not place. They are numbered afresh each run and never saved.
pub(crate) const PREVIEW_BASE: u64 = 1 << 62;
/// TheoremWeb's World Space, unless `PLATINUM_CANVAS_SPACE` names another.
const DEFAULT_SPACE: &str = "space:world";
/// A rect within this many world units of the authority's is the same rect.
const TOLERANCE: f64 = 0.5;

/// What a card or note is, for the object that stands for it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum LocalKind {
    /// A browser card: its page's address is the content reference.
    Page { url: String, title: String },
    /// A note card: the notes store owns the document `document_id`.
    Note { document_id: String, title: String },
    /// An object placed elsewhere, drawn here as a preview.
    Preview,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LocalObject {
    pub local: u64,
    pub rect: [f64; 4],
    pub kind: LocalKind,
}

/// A typed preview card for an object Platinum does not draw natively.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Preview {
    pub local: u64,
    pub kind: &'static str,
    pub title: String,
    pub detail: String,
    pub rect: [f64; 4],
}

/// What a fresh snapshot asks of the board.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Adopted {
    /// Cards and notes another writer moved.
    pub moves: Vec<(u64, [f64; 4])>,
    /// Every preview card, in board order.
    pub previews: Vec<Preview>,
    /// Cards and notes whose objects another writer removed.
    pub withdrawn: Vec<u64>,
}

/// What a refusal asks of the board.
#[derive(Debug, PartialEq)]
pub(crate) struct Conflict {
    pub message: String,
    /// The newer rect the authority holds, for the card to take.
    pub rect: Option<[f64; 4]>,
    /// The object is gone: take the card off this board as well.
    pub hide: bool,
}

/// The link between this board's ids and the authority's objects, and the
/// objects as this board last saw them. Pure: the canvas does the I/O.
#[derive(Debug, Default)]
pub(crate) struct Sync {
    /// Cards and notes this board placed, by board id.
    links: BTreeMap<u64, String>,
    /// Preview board ids, by object id.
    previews: BTreeMap<String, u64>,
    /// The authority's objects as last seen, by object id.
    seen: BTreeMap<String, CanvasObject>,
    /// Board ids with a mutation in flight; nothing more is sent for them
    /// until its answer arrives.
    busy: BTreeSet<u64>,
    /// Previews whose delete is in flight, so a snapshot does not redraw them.
    deleting: BTreeSet<u64>,
    revision: u64,
    /// Distinguishes this run's operation keys from another run's.
    run: String,
    sent: u64,
}

impl Sync {
    pub fn new(links: &[CanvasLink], run: impl Into<String>) -> Self {
        Self {
            links: links
                .iter()
                .filter(|link| link.local < PREVIEW_BASE)
                .map(|link| (link.local, link.id.clone()))
                .collect(),
            run: run.into(),
            ..Self::default()
        }
    }

    /// The links to keep with the board's saved state.
    pub fn links(&self) -> Vec<CanvasLink> {
        self.links
            .iter()
            .map(|(local, id)| CanvasLink {
                local: *local,
                id: id.clone(),
            })
            .collect()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn is_busy(&self, local: u64) -> bool {
        self.busy.contains(&local)
    }

    /// The mutation for `local` never reached the authority; it may be sent
    /// again.
    pub fn failed(&mut self, local: u64) {
        self.busy.remove(&local);
        self.deleting.remove(&local);
    }

    /// The object a board id stands for, placed here or previewed.
    fn object_of(&self, local: u64) -> Option<&String> {
        self.links.get(&local).or_else(|| {
            self.previews
                .iter()
                .find_map(|(id, preview)| (*preview == local).then_some(id))
        })
    }

    fn unlink(&mut self, local: u64) {
        let id = self.links.remove(&local).or_else(|| {
            let id = self.object_of(local).cloned();
            self.previews.retain(|_, preview| *preview != local);
            id
        });
        if let Some(id) = id {
            self.seen.remove(&id);
        }
    }

    /// The mutations that bring the authority to what this board shows.
    /// `removed` holds the cards the person took off the board.
    pub fn outgoing(
        &mut self,
        objects: &[LocalObject],
        removed: &[u64],
    ) -> Vec<(u64, CanvasRequest)> {
        let mut out = Vec::new();
        for object in objects {
            if self.busy.contains(&object.local) {
                continue;
            }
            let rect = rect(object.rect);
            let id = self.object_of(object.local).cloned();
            let mutation = match id.as_ref().and_then(|id| self.seen.get(id)) {
                Some(seen) if !same(&seen.rect, &rect) => CanvasMutation::MoveResize {
                    id: seen.id.clone(),
                    base_placement_revision: seen.placement_revision,
                    rect,
                },
                Some(_) => continue,
                // Linked, but no snapshot has named it yet.
                None if id.is_some() => continue,
                None => {
                    let (content, label) = match &object.kind {
                        LocalKind::Page { url, title } if !url.is_empty() => {
                            (reference(theorem_canvas::kind::BROWSER_VIEW, url), title)
                        }
                        LocalKind::Note { document_id, title } if !document_id.is_empty() => {
                            (reference(theorem_canvas::kind::NOTE, document_id), title)
                        }
                        _ => continue,
                    };
                    CanvasMutation::Create {
                        content,
                        rect,
                        label: (!label.is_empty()).then(|| label.clone()),
                    }
                }
            };
            out.push((object.local, self.request(object.local, mutation)));
        }
        for &local in removed {
            if self.busy.contains(&local) {
                continue;
            }
            let Some(seen) = self
                .object_of(local)
                .and_then(|id| self.seen.get(id))
                .cloned()
            else {
                continue;
            };
            if local >= PREVIEW_BASE {
                self.deleting.insert(local);
            }
            let mutation = CanvasMutation::Delete {
                id: seen.id,
                base_placement_revision: seen.placement_revision,
                base_content_revision: seen.content.revision,
            };
            out.push((local, self.request(local, mutation)));
        }
        out
    }

    fn request(&mut self, local: u64, mutation: CanvasMutation) -> CanvasRequest {
        self.busy.insert(local);
        self.sent += 1;
        let what = format!("{:?}", mutation.operation()).to_lowercase();
        CanvasRequest {
            operation_key: format!("platinum:{}:{}:{what}:{local}", self.run, self.sent),
            mutation,
        }
    }

    /// The authority accepted a mutation for `local`.
    pub fn receipt(&mut self, local: u64, receipt: &CanvasReceipt) {
        self.busy.remove(&local);
        self.deleting.remove(&local);
        self.revision = self.revision.max(receipt.revision);
        match &receipt.object {
            Some(object) => {
                if local >= PREVIEW_BASE {
                    self.previews.insert(object.id.clone(), local);
                } else {
                    self.links.insert(local, object.id.clone());
                }
                self.seen.insert(object.id.clone(), object.clone());
            }
            // A delete's receipt names no object.
            None => self.unlink(local),
        }
    }

    /// The authority refused `operation` for `local`.
    pub fn refusal(
        &mut self,
        local: u64,
        operation: Operation,
        refusal: &CanvasRefusal,
    ) -> Option<Conflict> {
        self.busy.remove(&local);
        self.deleting.remove(&local);
        let what = self
            .object_of(local)
            .and_then(|id| self.seen.get(id))
            .and_then(|object| object.label.clone())
            .map_or_else(
                || "this card".to_owned(),
                |label| format!("\u{201c}{label}\u{201d}"),
            );
        let conflict = |message: String| Conflict {
            message,
            rect: None,
            hide: false,
        };
        Some(match refusal.code {
            RefusalCode::StalePlacement | RefusalCode::StaleContent | RefusalCode::HumanEdited => {
                let rect = refusal.current.as_ref().map(|current| {
                    self.seen.insert(current.id.clone(), current.clone());
                    [
                        current.rect.x,
                        current.rect.y,
                        current.rect.w,
                        current.rect.h,
                    ]
                });
                if operation == Operation::Delete && local < PREVIEW_BASE {
                    // The card is already off this board; the newer object
                    // comes back as a preview with the next snapshot.
                    self.links.remove(&local);
                    return Some(conflict(format!(
                        "Someone changed {what} after you removed it, so it stays on the shared board."
                    )));
                }
                Conflict {
                    message: format!(
                        "Someone changed {what} first. The board shows their newer version."
                    ),
                    rect,
                    hide: false,
                }
            }
            RefusalCode::UnknownObject => {
                self.unlink(local);
                if operation == Operation::Delete {
                    return None;
                }
                Conflict {
                    message: format!(
                        "{what} was removed from the shared board. Show removed pages puts it back."
                    ),
                    rect: None,
                    hide: true,
                }
            }
            RefusalCode::UnknownSpace => conflict(format!(
                "The shared board {} has not been saved in Theorem yet.",
                refusal.space_id
            )),
            _ => conflict(format!(
                "The shared board did not take the change to {what}: {}",
                refusal.message
            )),
        })
    }

    /// Take a snapshot; `None` when it is older than one already taken.
    /// `known` holds every board id this board still has, removed cards
    /// included: a linked card it no longer has (a closed tab, a note not
    /// loaded) is drawn as a preview. The first snapshot after a start moves
    /// every card to where the authority holds it, so a session saved before
    /// someone else's change never writes over it.
    pub fn adopt(&mut self, board: &CanvasBoard, known: &BTreeSet<u64>) -> Option<Adopted> {
        if board.revision < self.revision {
            return None;
        }
        let mut adopted = Adopted::default();
        self.revision = board.revision;
        let by_id: BTreeMap<String, u64> = self
            .links
            .iter()
            .map(|(local, id)| (id.clone(), *local))
            .collect();
        let mut next = self
            .previews
            .values()
            .max()
            .map_or(PREVIEW_BASE, |last| last + 1);
        for object in &board.objects {
            match by_id.get(&object.id) {
                Some(local) if known.contains(local) => {
                    let moved = self.seen.get(&object.id).is_none_or(|seen| {
                        object.placement_revision > seen.placement_revision
                            && !same(&seen.rect, &object.rect)
                    });
                    if moved && !self.busy.contains(local) {
                        adopted.moves.push((*local, array(&object.rect)));
                    }
                }
                _ => {
                    let local = *self.previews.entry(object.id.clone()).or_insert_with(|| {
                        next += 1;
                        next - 1
                    });
                    if !self.deleting.contains(&local) {
                        adopted.previews.push(preview(local, object));
                    }
                }
            }
            self.seen.insert(object.id.clone(), object.clone());
        }
        let live: BTreeSet<&str> = board
            .objects
            .iter()
            .map(|object| object.id.as_str())
            .collect();
        self.previews.retain(|id, _| live.contains(id.as_str()));
        let gone: Vec<(u64, String)> = self
            .links
            .iter()
            .filter(|(_, id)| !live.contains(id.as_str()))
            .map(|(local, id)| (*local, id.clone()))
            .collect();
        for (local, id) in gone {
            self.links.remove(&local);
            self.seen.remove(&id);
            if known.contains(&local) && !self.busy.contains(&local) {
                adopted.withdrawn.push(local);
            }
        }
        Some(adopted)
    }
}

fn reference(kind: &str, id: &str) -> NewContent {
    NewContent::Reference {
        kind: kind.to_owned(),
        id: id.to_owned(),
        revision: 0,
        origin: None,
    }
}

fn rect([x, y, w, h]: [f64; 4]) -> Rect {
    Rect { x, y, w, h }
}

fn array(rect: &Rect) -> [f64; 4] {
    [rect.x, rect.y, rect.w, rect.h]
}

fn same(a: &Rect, b: &Rect) -> bool {
    [(a.x, b.x), (a.y, b.y), (a.w, b.w), (a.h, b.h)]
        .iter()
        .all(|(a, b)| (a - b).abs() <= TOLERANCE)
}

/// The word a preview card leads with, for each type.
pub(crate) fn type_label(object_type: ObjectType) -> &'static str {
    match object_type {
        ObjectType::BrowserView => "Browser view",
        ObjectType::Note => "Note",
        ObjectType::Document => "Document",
        ObjectType::Code => "Code",
        ObjectType::TerminalView => "Terminal",
        ObjectType::Diagram => "Diagram",
        ObjectType::MailReference => "Mail",
        ObjectType::HtmlPreview => "HTML preview",
        ObjectType::ProceduralScene => "Scene",
        ObjectType::AgentPresence => "Agent",
        ObjectType::Space => "Space",
        ObjectType::Other => "Object",
    }
}

fn preview(local: u64, object: &CanvasObject) -> Preview {
    let author = match &object.placement_actor {
        Actor::Agent { id } => format!("placed by agent {id}"),
        Actor::User => "placed by a person".to_owned(),
        Actor::System => "placed by Theorem".to_owned(),
    };
    let content = object
        .content
        .text
        .clone()
        .unwrap_or_else(|| object.content.id.clone());
    Preview {
        local,
        kind: type_label(object.object_type),
        title: object.label.clone().unwrap_or_else(|| content.clone()),
        detail: format!(
            "{content} \u{b7} revision {} \u{b7} {author}",
            object.content.revision
        ),
        rect: array(&object.rect),
    }
}

/// A fresh run id for operation keys.
pub(crate) fn run_id() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    format!("{}-{millis}", std::process::id())
}

pub(crate) struct Client {
    http: reqwest::Client,
    route: reqwest::Url,
    bearer: String,
    csrf: std::sync::Mutex<Option<String>>,
}

/// A mutation's outcome from the authority.
pub(crate) enum Answer {
    Accepted(CanvasReceipt),
    Refused(CanvasRefusal),
}

impl Client {
    /// `None` unless the native authority is configured.
    pub fn from_env() -> Option<Self> {
        let base = std::env::var("THEOREMWEB_AUTHORITY_URL").ok()?;
        let token = std::env::var("THEOREMWEB_AUTH_TOKEN_FILE").ok()?;
        let base = reqwest::Url::parse(base.trim())
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https"))?;
        let bearer = std::fs::read_to_string(token.trim())
            .ok()?
            .trim()
            .to_owned();
        if bearer.is_empty() {
            return None;
        }
        let space =
            std::env::var("PLATINUM_CANVAS_SPACE").unwrap_or_else(|_| DEFAULT_SPACE.to_owned());
        let space =
            percent_encoding::utf8_percent_encode(space.trim(), percent_encoding::NON_ALPHANUMERIC);
        let route = base.join(&format!("/api/spaces/v1/{space}/canvas")).ok()?;
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .ok()?;
        Some(Self {
            http,
            route,
            bearer,
            csrf: std::sync::Mutex::new(None),
        })
    }

    /// The board as the authority holds it; `None` before the Space is saved.
    pub async fn board(&self) -> Result<Option<CanvasBoard>, String> {
        let response = self
            .http
            .get(self.route.clone())
            .bearer_auth(&self.bearer)
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|error| error.without_url().to_string())?;
        match response.status() {
            reqwest::StatusCode::NOT_FOUND => Ok(None),
            status if status.is_success() => decoded(response).await.map(Some),
            status => Err(format!("the shared board answered {status}")),
        }
    }

    /// One mutation. A request is retried once with a fresh CSRF token;
    /// the operation key makes the retry return the first receipt.
    pub async fn mutate(&self, request: &CanvasRequest) -> Result<Answer, String> {
        for attempt in 0..2 {
            let csrf = self.csrf(attempt > 0).await?;
            let response = self
                .http
                .post(self.route.clone())
                .bearer_auth(&self.bearer)
                .header("x-csrf-token", csrf)
                .header("idempotency-key", &request.operation_key)
                .json(request)
                .send()
                .await
                .map_err(|error| error.without_url().to_string())?;
            let status = response.status();
            if status == reqwest::StatusCode::FORBIDDEN && attempt == 0 {
                continue;
            }
            if status.is_success() {
                return decoded(response).await.map(Answer::Accepted);
            }
            if matches!(status.as_u16(), 404 | 409 | 422)
                && let Ok(refusal) = decoded::<CanvasRefusal>(response).await
            {
                return Ok(Answer::Refused(refusal));
            }
            return Err(format!("the shared board answered {status}"));
        }
        Err("the shared board refused this session's credential".to_owned())
    }

    async fn csrf(&self, fresh: bool) -> Result<String, String> {
        if !fresh && let Some(token) = self.csrf.lock().ok().and_then(|token| token.clone()) {
            return Ok(token);
        }
        let token = theoremweb_agent_runtime::client::fetch_bootstrap()
            .await
            .map_err(|error| error.message().to_owned())?
            .csrf_token;
        if let Ok(mut slot) = self.csrf.lock() {
            *slot = Some(token.clone());
        }
        Ok(token)
    }
}

/// A response body as a canvas wire value. `theorem_canvas::decode` goes
/// through a JSON value because this graph enables serde_json's
/// `arbitrary_precision`, which `reqwest`'s own `json()` would trip over.
async fn decoded<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> Result<T, String> {
    let bytes = response
        .bytes()
        .await
        .map_err(|error| error.without_url().to_string())?;
    theorem_canvas::decode(&bytes).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use theorem_canvas::{ContentHome, ContentRef, Origin};

    fn object(id: &str, x: f64, placement_revision: u64, actor: Actor) -> CanvasObject {
        CanvasObject {
            id: id.into(),
            object_type: ObjectType::Document,
            content: ContentRef {
                kind: "document".into(),
                id: "doc:readme".into(),
                revision: 3,
                origin: Origin::Generated,
                author: actor.clone(),
                home: ContentHome::Owner,
                text: None,
            },
            placement_revision,
            placement_actor: actor,
            rect: Rect::new(x, 0., 300., 200.),
            label: Some("Readme".into()),
            operations: [Operation::MoveResize, Operation::Delete]
                .into_iter()
                .collect(),
            comments: vec![],
        }
    }

    fn board(revision: u64, objects: Vec<CanvasObject>) -> CanvasBoard {
        CanvasBoard {
            contract: theorem_canvas::CANVAS_CONTRACT.into(),
            space_id: "space:world".into(),
            revision,
            objects,
        }
    }

    fn receipt(revision: u64, object: Option<CanvasObject>) -> CanvasReceipt {
        CanvasReceipt {
            contract: theorem_canvas::CANVAS_CONTRACT.into(),
            operation_key: "k".into(),
            operation: Operation::Create,
            space_id: "space:world".into(),
            revision,
            actor: Actor::User,
            targets: vec![],
            object,
            reconciled: None,
        }
    }

    fn page(local: u64, x: f64) -> LocalObject {
        LocalObject {
            local,
            rect: [x, 0., 300., 200.],
            kind: LocalKind::Page {
                url: "https://example.com/".into(),
                title: "Example".into(),
            },
        }
    }

    fn placed(sync: &mut Sync, local: u64, id: &str) {
        let sent = sync.outgoing(&[page(local, 0.)], &[]);
        assert_eq!(sent.len(), 1);
        sync.receipt(local, &receipt(2, Some(object(id, 0., 2, Actor::User))));
    }

    #[test]
    fn a_card_is_created_once_then_moved_from_the_revision_it_saw() {
        let mut sync = Sync::new(&[], "run");
        let first = sync.outgoing(&[page(7, 0.)], &[]);
        assert!(matches!(
            &first[0].1.mutation,
            CanvasMutation::Create { content: NewContent::Reference { kind, id, .. }, .. }
                if kind == "browser_view" && id == "https://example.com/"
        ));
        assert!(
            sync.outgoing(&[page(7, 40.)], &[]).is_empty(),
            "nothing more is sent while the create is in flight"
        );
        sync.receipt(7, &receipt(2, Some(object("pl-1", 0., 2, Actor::User))));
        assert_eq!(
            sync.links(),
            vec![CanvasLink {
                local: 7,
                id: "pl-1".into()
            }]
        );
        assert!(sync.outgoing(&[page(7, 0.)], &[]).is_empty());
        let moved = sync.outgoing(&[page(7, 40.)], &[]);
        assert!(matches!(
            &moved[0].1.mutation,
            CanvasMutation::MoveResize { id, base_placement_revision: 2, .. } if id == "pl-1"
        ));
        assert_ne!(first[0].1.operation_key, moved[0].1.operation_key);
    }

    #[test]
    fn links_survive_a_restart_and_the_same_object_is_moved() {
        let links = vec![CanvasLink {
            local: 7,
            id: "pl-1".into(),
        }];
        let mut sync = Sync::new(&links, "after-restart");
        assert!(
            sync.outgoing(&[page(7, 0.)], &[]).is_empty(),
            "no second object before the snapshot names the first"
        );
        let adopted = sync
            .adopt(
                &board(5, vec![object("pl-1", 30., 4, Actor::User)]),
                &BTreeSet::from([7]),
            )
            .unwrap();
        assert_eq!(
            adopted.moves,
            vec![(7, [30., 0., 300., 200.])],
            "the authority's placement wins over the saved session"
        );
        let moved = sync.outgoing(&[page(7, 90.)], &[]);
        assert!(matches!(
            &moved[0].1.mutation,
            CanvasMutation::MoveResize { id, base_placement_revision: 4, .. } if id == "pl-1"
        ));
    }

    #[test]
    fn a_stale_move_shows_a_conflict_and_takes_the_newer_rect() {
        let mut sync = Sync::new(&[], "run");
        placed(&mut sync, 7, "pl-1");
        let sent = sync.outgoing(&[page(7, 40.)], &[]);
        assert_eq!(sent.len(), 1);
        let mut refusal = CanvasRefusal::new(RefusalCode::StalePlacement, "stale");
        refusal.current = Some(object(
            "pl-1",
            500.,
            3,
            Actor::Agent {
                id: "agent-a".into(),
            },
        ));
        let conflict = sync.refusal(7, Operation::MoveResize, &refusal).unwrap();
        assert!(conflict.message.contains("Readme"), "{}", conflict.message);
        assert_eq!(conflict.rect, Some([500., 0., 300., 200.]));
        assert!(!conflict.hide);
        assert!(
            sync.outgoing(&[page(7, 500.)], &[]).is_empty(),
            "the newer rect is now the base"
        );
        let again = sync.outgoing(&[page(7, 520.)], &[]);
        assert!(matches!(
            &again[0].1.mutation,
            CanvasMutation::MoveResize {
                base_placement_revision: 3,
                ..
            }
        ));
    }

    #[test]
    fn objects_placed_elsewhere_are_typed_previews_and_moves_elsewhere_are_adopted() {
        let mut sync = Sync::new(
            &[CanvasLink {
                local: 7,
                id: "pl-1".into(),
            }],
            "run",
        );
        let known = BTreeSet::from([7]);
        sync.adopt(&board(3, vec![object("pl-1", 0., 2, Actor::User)]), &known);
        let agent = Actor::Agent {
            id: "agent-a".into(),
        };
        let adopted = sync
            .adopt(
                &board(
                    4,
                    vec![
                        object("pl-1", 250., 3, agent.clone()),
                        object("pl-9", 900., 4, agent),
                    ],
                ),
                &known,
            )
            .unwrap();
        assert_eq!(adopted.moves, vec![(7, [250., 0., 300., 200.])]);
        let [preview] = adopted.previews.as_slice() else {
            panic!("{adopted:?}")
        };
        assert_eq!(
            (preview.local, preview.kind, preview.title.as_str()),
            (PREVIEW_BASE, "Document", "Readme")
        );
        assert!(
            preview.detail.contains("agent agent-a"),
            "{}",
            preview.detail
        );
        let again = sync
            .adopt(
                &board(
                    5,
                    vec![
                        object("pl-1", 250., 3, Actor::User),
                        object("pl-9", 900., 4, Actor::User),
                    ],
                ),
                &known,
            )
            .unwrap();
        assert_eq!(
            again.previews[0].local, PREVIEW_BASE,
            "a preview keeps its board id"
        );
        assert!(again.moves.is_empty());
        assert_eq!(
            sync.links(),
            vec![CanvasLink {
                local: 7,
                id: "pl-1".into()
            }],
            "previews are never saved"
        );
        assert_eq!(
            sync.adopt(&board(2, vec![]), &known),
            None,
            "an older snapshot changes nothing"
        );
        // A person moves the preview: the same mutation an agent would send.
        let moved = sync.outgoing(
            &[LocalObject {
                local: PREVIEW_BASE,
                rect: [950., 0., 300., 200.],
                kind: LocalKind::Preview,
            }],
            &[],
        );
        assert!(matches!(
            &moved[0].1.mutation,
            CanvasMutation::MoveResize { id, base_placement_revision: 4, .. } if id == "pl-9"
        ));
    }

    #[test]
    fn a_closed_tab_leaves_its_object_on_the_shared_board_as_a_preview() {
        let mut sync = Sync::new(&[], "run");
        placed(&mut sync, 7, "pl-1");
        assert!(
            sync.outgoing(&[], &[]).is_empty(),
            "closing a tab deletes nothing"
        );
        let adopted = sync
            .adopt(
                &board(3, vec![object("pl-1", 0., 2, Actor::User)]),
                &BTreeSet::new(),
            )
            .unwrap();
        assert_eq!(adopted.previews.len(), 1);
        assert!(adopted.withdrawn.is_empty());
        assert_eq!(
            sync.links().len(),
            1,
            "the link is kept for when the tab returns"
        );
    }

    #[test]
    fn only_an_explicit_removal_deletes_and_a_newer_change_survives_it() {
        let mut sync = Sync::new(&[], "run");
        placed(&mut sync, 7, "pl-1");
        let deleted = sync.outgoing(&[], &[7]);
        assert!(matches!(
            &deleted[0].1.mutation,
            CanvasMutation::Delete { id, base_placement_revision: 2, base_content_revision: 3 } if id == "pl-1"
        ));
        let mut refusal = CanvasRefusal::new(RefusalCode::StalePlacement, "stale");
        refusal.current = Some(object("pl-1", 300., 3, Actor::User));
        let conflict = sync.refusal(7, Operation::Delete, &refusal).unwrap();
        assert!(
            conflict.message.contains("stays on the shared board"),
            "{}",
            conflict.message
        );
        assert!(
            sync.outgoing(&[], &[7]).is_empty(),
            "the refused delete is not retried"
        );
        let adopted = sync
            .adopt(
                &board(4, vec![object("pl-1", 300., 3, Actor::User)]),
                &BTreeSet::from([7]),
            )
            .unwrap();
        assert_eq!(
            adopted.previews.len(),
            1,
            "the newer object comes back as a preview"
        );
    }

    #[test]
    fn an_object_removed_elsewhere_takes_its_card_off_this_board() {
        let mut sync = Sync::new(&[], "run");
        placed(&mut sync, 7, "pl-1");
        let adopted = sync.adopt(&board(9, vec![]), &BTreeSet::from([7])).unwrap();
        assert_eq!(adopted.withdrawn, vec![7]);
        assert!(sync.links().is_empty());
        let mut sync = Sync::new(&[], "run");
        placed(&mut sync, 7, "pl-1");
        sync.outgoing(&[page(7, 30.)], &[]);
        let conflict = sync
            .refusal(
                7,
                Operation::MoveResize,
                &CanvasRefusal::new(RefusalCode::UnknownObject, "gone"),
            )
            .unwrap();
        assert!(conflict.hide);
        let deleted = sync.outgoing(&[], &[]);
        assert!(deleted.is_empty());
    }
}
