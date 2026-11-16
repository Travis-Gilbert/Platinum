//! One showing of the launcher: what was typed, what each lane holds for it,
//! which row is selected, and what Enter will do there.

use std::collections::BTreeMap;

use crate::item::{Item, ItemId, Kind, Origin, Target, Verb};
use crate::{matching, words};

/// Where results come from, and the heading they are shown under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Lane {
    /// What was typed, asked of Theo. Made here, never handed in.
    Ask,
    Apps,
    Commands,
    Conversations,
    Files,
    /// An address to open and a search of the web. Made here.
    Web,
}

impl Lane {
    pub fn label(self) -> &'static str {
        match self {
            Lane::Ask => "Theo",
            Lane::Apps => "Applications",
            Lane::Commands => "Commands",
            Lane::Conversations => "Conversations",
            Lane::Files => "Files",
            Lane::Web => "Web",
        }
    }

    /// What the launcher says while this lane has not answered.
    pub fn looking(self) -> &'static str {
        match self {
            Lane::Files => "Searching this Mac…",
            _ => "Searching…",
        }
    }
}

/// What the person narrowed the results to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Scope {
    #[default]
    All,
    Apps,
    Files,
    Conversations,
    Commands,
    Web,
}

impl Scope {
    pub const EVERY: [Scope; 6] = [
        Scope::All,
        Scope::Apps,
        Scope::Files,
        Scope::Conversations,
        Scope::Commands,
        Scope::Web,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Scope::All => "All",
            Scope::Apps => "Applications",
            Scope::Files => "Files",
            Scope::Conversations => "Conversations",
            Scope::Commands => "Commands",
            Scope::Web => "Web",
        }
    }

    /// Whether results from `lane` are shown under this scope.
    pub fn admits(self, lane: Lane) -> bool {
        match self {
            Scope::All => true,
            Scope::Apps => lane == Lane::Apps,
            Scope::Files => lane == Lane::Files,
            // Asking Theo is how a conversation starts.
            Scope::Conversations => matches!(lane, Lane::Conversations | Lane::Ask),
            Scope::Commands => lane == Lane::Commands,
            Scope::Web => lane == Lane::Web,
        }
    }

    fn step(self, by: isize) -> Scope {
        let every = Self::EVERY;
        let at = every.iter().position(|scope| *scope == self).unwrap_or(0) as isize;
        every[(at + by).rem_euclid(every.len() as isize) as usize]
    }
}

/// Whether the field is for finding things or for talking to Theo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Find,
    Talk,
}

/// One heading and the rows under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub lane: Lane,
    pub items: Vec<Item>,
}

/// Where a lane that is asked off the main thread stands for this query.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Waiting {
    /// Nothing asked, or the answer is in.
    #[default]
    Settled,
    /// Asked, not answered yet.
    Looking,
    /// It could not look, and why. Shown in the lane's place.
    Unavailable(String),
}

/// Why a delivery was not taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// It answers a query that has since changed.
    Stale,
    /// No source was registered for that lane.
    Unexpected,
}

/// What the person chose: the item, and what to do with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chosen {
    pub item: Item,
    pub verb: Verb,
}

/// What Escape did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Escaped {
    /// Left talking for finding; the conversation is untouched.
    LeftTalk,
    /// Cleared what was typed.
    Cleared,
    /// There was nothing left to step back from: close the launcher.
    Dismiss,
}

/// The fewest letters a lane asked off the main thread is troubled for.
const ASK_FROM: usize = 2;
/// The most rows a lane keeps for one query.
const KEEP: usize = 50;
/// The most rows a lane shows once the person narrowed to it.
const NARROWED: usize = 40;

#[derive(Clone, Debug, Default)]
struct Asked {
    items: Vec<Item>,
    waiting: Waiting,
}

/// One showing of the launcher.
#[derive(Clone, Debug, Default)]
pub struct Session {
    query: String,
    scope: Scope,
    mode: Mode,
    /// Counts the queries, so an answer can say which one it is for.
    generation: u64,
    standing: BTreeMap<Lane, Vec<Item>>,
    asked: BTreeMap<Lane, Asked>,
    /// The conversation a question would go to, or `None` for a new one.
    conversation: Option<String>,
    sections: Vec<Section>,
    selected: Option<ItemId>,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    // What the host hands in ------------------------------------------------

    /// Replaces a lane's standing items: every application, command or
    /// conversation there is, matched here against each keystroke. The
    /// selection stays on its item if the item is still there.
    pub fn set_standing(&mut self, lane: Lane, items: Vec<Item>) {
        self.standing.insert(lane, items);
        self.rebuild(false);
    }

    /// Says that `lane` has a source that will be asked for each query.
    pub fn expect(&mut self, lane: Lane) {
        self.asked.entry(lane).or_default();
    }

    /// Names the conversation a question would continue, so the row that
    /// asks says where the question goes. `None` is a new conversation.
    pub fn set_conversation(&mut self, title: Option<String>) {
        let title = title.filter(|title| !title.trim().is_empty());
        if title != self.conversation {
            self.conversation = title;
            self.rebuild(false);
        }
    }

    /// Takes a lane's answer for the query numbered `generation`. An answer
    /// for an older query is refused. The selection stays where it was.
    pub fn deliver(
        &mut self,
        lane: Lane,
        generation: u64,
        found: Result<Vec<Item>, String>,
    ) -> Result<(), Refusal> {
        if generation != self.generation {
            return Err(Refusal::Stale);
        }
        let asked = self.asked.get_mut(&lane).ok_or(Refusal::Unexpected)?;
        match found {
            Ok(mut items) => {
                items.truncate(KEEP);
                asked.items = items;
                asked.waiting = Waiting::Settled;
            }
            Err(why) => {
                asked.items.clear();
                asked.waiting = Waiting::Unavailable(why);
            }
        }
        self.rebuild(false);
        Ok(())
    }

    // What the person does --------------------------------------------------

    /// Takes what is in the field now. Returns whether it changed; if it
    /// did, the query has a new number and the first row is selected.
    pub fn set_query(&mut self, text: &str) -> bool {
        if text == self.query {
            return false;
        }
        self.query = text.to_owned();
        self.generation += 1;
        let query = self.query.trim().to_owned();
        let askable = query.chars().count() >= ASK_FROM;
        for asked in self.asked.values_mut() {
            if askable {
                // Rows that still answer to the longer query stay while the
                // lane looks again, so the list does not blink empty.
                asked.items.retain(|item| {
                    matching::best(
                        &query,
                        &item.title,
                        item.keywords.iter().map(String::as_str),
                    )
                    .is_some()
                });
                asked.waiting = Waiting::Looking;
            } else {
                asked.items.clear();
                asked.waiting = Waiting::Settled;
            }
        }
        self.rebuild(true);
        true
    }

    pub fn set_scope(&mut self, scope: Scope) {
        if scope != self.scope {
            self.scope = scope;
            self.rebuild(false);
        }
    }

    pub fn next_scope(&mut self) {
        self.set_scope(self.scope.step(1));
    }

    pub fn previous_scope(&mut self) {
        self.set_scope(self.scope.step(-1));
    }

    /// Moves the selection by `by` rows, around the ends.
    pub fn move_selection(&mut self, by: isize) {
        let ids: Vec<&ItemId> = self.items().map(|item| &item.id).collect();
        if ids.is_empty() {
            return;
        }
        let at = self
            .selected
            .as_ref()
            .and_then(|selected| ids.iter().position(|id| *id == selected));
        let next = match at {
            Some(at) => (at as isize + by).rem_euclid(ids.len() as isize) as usize,
            None if by < 0 => ids.len() - 1,
            None => 0,
        };
        let next = ids[next].clone();
        self.selected = Some(next);
    }

    /// Selects the row for `id`, if it is shown.
    pub fn select(&mut self, id: &ItemId) {
        if self.items().any(|item| &item.id == id) {
            self.selected = Some(id.clone());
        }
    }

    /// What Enter does: the selected item and its first verb.
    pub fn confirm(&self) -> Option<Chosen> {
        let item = self.selected()?;
        Some(Chosen {
            item: item.clone(),
            verb: item.primary(),
        })
    }

    /// The selected item with `verb`, when that can be done with it.
    pub fn choose(&self, verb: Verb) -> Option<Chosen> {
        let item = self.selected()?;
        item.verbs().contains(&verb).then(|| Chosen {
            item: item.clone(),
            verb,
        })
    }

    /// Turns the field over to talking with Theo.
    pub fn talk(&mut self) {
        self.mode = Mode::Talk;
    }

    /// Turns the field back to finding. The conversation is untouched.
    pub fn find(&mut self) {
        self.mode = Mode::Find;
    }

    /// Steps back once: out of talking, then out of what was typed, then
    /// out of the launcher.
    pub fn escape(&mut self) -> Escaped {
        if self.mode == Mode::Talk {
            self.mode = Mode::Find;
            Escaped::LeftTalk
        } else if !self.query.is_empty() {
            self.set_query("");
            Escaped::Cleared
        } else {
            Escaped::Dismiss
        }
    }

    // What is shown ---------------------------------------------------------

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn scope(&self) -> Scope {
        self.scope
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The number of the query in the field. A search carries it out and
    /// brings it back with its answer.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// Every row shown, in order.
    pub fn items(&self) -> impl Iterator<Item = &Item> {
        self.sections.iter().flat_map(|section| &section.items)
    }

    pub fn count(&self) -> usize {
        self.sections
            .iter()
            .map(|section| section.items.len())
            .sum()
    }

    pub fn selected(&self) -> Option<&Item> {
        let selected = self.selected.as_ref()?;
        self.items().find(|item| &item.id == selected)
    }

    /// The lanes to ask for the query in the field: those with a source,
    /// shown under the scope, that have not answered it.
    pub fn lanes_to_ask(&self) -> Vec<Lane> {
        self.asked
            .iter()
            .filter(|(lane, asked)| self.scope.admits(**lane) && asked.waiting == Waiting::Looking)
            .map(|(lane, _)| *lane)
            .collect()
    }

    /// The first lane shown that has not answered yet.
    pub fn looking(&self) -> Option<Lane> {
        self.lanes_to_ask().into_iter().next()
    }

    /// The lanes shown that could not look, and why.
    pub fn unavailable(&self) -> Vec<(Lane, &str)> {
        self.asked
            .iter()
            .filter(|(lane, _)| self.scope.admits(**lane))
            .filter_map(|(lane, asked)| match &asked.waiting {
                Waiting::Unavailable(why) => Some((*lane, why.as_str())),
                _ => None,
            })
            .collect()
    }

    // The rows --------------------------------------------------------------

    /// Works out the sections again. With `reset` the first row is selected;
    /// without, the selection stays on its item while the item is shown.
    fn rebuild(&mut self, reset: bool) {
        let query = self.query.trim().to_owned();
        let typing = !query.is_empty();
        let narrowed = self.scope != Scope::All;
        let mut sections: Vec<Section> = Vec::new();

        // Standing lanes, each matched now. Typed, the lane with the best
        // match leads, so "new tab" finds the command over an application
        // that merely holds those letters.
        let mut standing: Vec<(u32, Section)> = Vec::new();
        for lane in [Lane::Apps, Lane::Commands, Lane::Conversations] {
            if !self.scope.admits(lane) {
                continue;
            }
            let Some(all) = self.standing.get(&lane) else {
                continue;
            };
            let mut items: Vec<Item> = all
                .iter()
                .filter_map(|item| {
                    let score = if typing {
                        matching::best(
                            &query,
                            &item.title,
                            item.keywords.iter().map(String::as_str),
                        )?
                    } else if item.pinned || narrowed {
                        0
                    } else {
                        return None;
                    };
                    let mut item = item.clone();
                    item.score = score;
                    Some(item)
                })
                .collect();
            // Stable: what the host put first stays first among equals.
            items.sort_by_key(|item| std::cmp::Reverse(item.score));
            items.truncate(if narrowed {
                NARROWED
            } else {
                shown_in_all(lane)
            });
            if let Some(best) = items.first().map(|item| item.score) {
                standing.push((best, Section { lane, items }));
            }
        }
        standing.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        sections.extend(standing.into_iter().map(|(_, section)| section));

        // Asked lanes come after, in a fixed place: an answer that arrives
        // late lands beneath what is already there, never above it.
        for (lane, asked) in &self.asked {
            if !self.scope.admits(*lane) || asked.items.is_empty() {
                continue;
            }
            let mut items = asked.items.clone();
            items.truncate(if narrowed {
                NARROWED
            } else {
                shown_in_all(*lane)
            });
            sections.push(Section { lane: *lane, items });
        }

        if typing && self.scope.admits(Lane::Web) {
            let mut items = Vec::new();
            if let Some(address) = words::as_address(&query) {
                items.push(
                    Item::new(
                        ItemId::new("link", &address),
                        Kind::Link,
                        format!("Open {query}"),
                        Target::Address(address.clone()),
                    )
                    .detail(address),
                );
            }
            items.push(
                Item::new(
                    ItemId::new("web", ""),
                    Kind::WebSearch,
                    format!("Search the web for “{query}”"),
                    Target::WebSearch(query.clone()),
                )
                .detail("Opens in Platinum")
                .origin(Origin::Web),
            );
            sections.push(Section {
                lane: Lane::Web,
                items,
            });
        }

        // Asking is always its own row. It leads only when what was typed
        // reads as a question; a lookup that finds nothing does not ask.
        if typing && self.scope.admits(Lane::Ask) {
            let detail = match &self.conversation {
                Some(title) => format!("Continues “{title}”"),
                None => "Starts a new conversation".to_owned(),
            };
            let section = Section {
                lane: Lane::Ask,
                items: vec![
                    Item::new(
                        ItemId::new("ask", ""),
                        Kind::Ask,
                        query.clone(),
                        Target::Ask(query.clone()),
                    )
                    .detail(detail),
                ],
            };
            if words::reads_as_question(&query) {
                sections.insert(0, section);
            } else {
                sections.push(section);
            }
        }

        self.sections = sections;
        let kept = !reset
            && self
                .selected
                .as_ref()
                .is_some_and(|selected| self.items().any(|item| &item.id == selected));
        if !kept {
            let first = self.items().next().map(|item| item.id.clone());
            self.selected = first;
        }
    }
}

/// The most rows a lane shows among the others.
fn shown_in_all(lane: Lane) -> usize {
    match lane {
        Lane::Apps => 6,
        Lane::Commands => 5,
        Lane::Conversations => 4,
        Lane::Files => 8,
        Lane::Ask | Lane::Web => 2,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn app(name: &str) -> Item {
        let path = format!("/Applications/{name}.app");
        Item::new(
            ItemId::new("app", &path),
            Kind::App,
            name,
            Target::Path(PathBuf::from(path)),
        )
    }

    fn command(name: &str) -> Item {
        Item::new(
            ItemId::new("command", name),
            Kind::Command,
            name,
            Target::Command(name.to_owned()),
        )
    }

    fn conversation(id: &str, title: &str) -> Item {
        Item::new(
            ItemId::new("conversation", id),
            Kind::Conversation,
            title,
            Target::Conversation(id.to_owned()),
        )
    }

    fn file(path: &str) -> Item {
        let path = PathBuf::from(path);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        Item::new(
            ItemId::new("file", &path.to_string_lossy()),
            Kind::File,
            name,
            Target::Path(path),
        )
    }

    fn session() -> Session {
        let mut session = Session::new();
        session.set_standing(
            Lane::Apps,
            vec![
                app("Lumio Dev"),
                app("Flux"),
                app("Tabs Studio"),
                app("Mail"),
            ],
        );
        session.set_standing(
            Lane::Commands,
            vec![
                command("New Tab").pinned(true),
                command("New Conversation").pinned(true),
                command("Quit Platinum"),
            ],
        );
        session.set_standing(
            Lane::Conversations,
            vec![
                conversation("t1", "Postgres rollback decision").pinned(true),
                conversation("t2", "Launcher design"),
            ],
        );
        session.expect(Lane::Files);
        session
    }

    fn titles(session: &Session) -> Vec<&str> {
        session.items().map(|item| item.title.as_str()).collect()
    }

    fn lanes(session: &Session) -> Vec<Lane> {
        session
            .sections()
            .iter()
            .map(|section| section.lane)
            .collect()
    }

    #[test]
    fn before_anything_is_typed_only_what_is_pinned_shows() {
        let session = session();
        assert_eq!(
            titles(&session),
            ["New Tab", "New Conversation", "Postgres rollback decision"]
        );
        assert_eq!(session.selected().unwrap().title, "New Tab");
    }

    #[test]
    fn typing_matches_standing_items_best_first() {
        let mut session = session();
        session.set_query("lu");
        let found = titles(&session);
        assert_eq!(found[0], "Lumio Dev");
        assert!(found.contains(&"Flux"));
        assert!(!found.contains(&"Mail"));
    }

    #[test]
    fn the_lane_with_the_best_match_leads() {
        let mut session = session();
        session.set_query("new tab");
        assert_eq!(lanes(&session)[0], Lane::Commands);
        assert_eq!(session.selected().unwrap().title, "New Tab");
        // Enter on a command runs it.
        assert_eq!(session.confirm().unwrap().verb, Verb::Run);
    }

    #[test]
    fn a_lookup_never_asks_unless_the_asking_row_is_chosen() {
        let mut session = session();
        session.set_query("zzzzqqq");
        // Nothing matches, and Enter still does not ask Theo: the first row
        // is the web search, and asking is the last row.
        let first = session.confirm().unwrap();
        assert_eq!(first.verb, Verb::Search);
        assert_eq!(lanes(&session), [Lane::Web, Lane::Ask]);
    }

    #[test]
    fn a_question_leads_with_asking_and_says_where_it_goes() {
        let mut session = session();
        session.set_query("where did we discuss database rollback?");
        let chosen = session.confirm().unwrap();
        assert_eq!(chosen.verb, Verb::Ask);
        assert_eq!(chosen.item.detail, "Starts a new conversation");
        assert_eq!(
            chosen.item.target,
            Target::Ask("where did we discuss database rollback?".to_owned())
        );

        session.set_conversation(Some("Postgres rollback decision".to_owned()));
        assert_eq!(
            session.confirm().unwrap().item.detail,
            "Continues “Postgres rollback decision”"
        );
    }

    #[test]
    fn an_answer_for_an_older_query_is_refused() {
        let mut session = session();
        session.set_query("re");
        let old = session.generation();
        session.set_query("rep");
        assert_eq!(
            session.deliver(Lane::Files, old, Ok(vec![file("/a/report.md")])),
            Err(Refusal::Stale)
        );
        assert!(!titles(&session).contains(&"report.md"));
        assert_eq!(
            session.deliver(
                Lane::Files,
                session.generation(),
                Ok(vec![file("/a/report.md")])
            ),
            Ok(())
        );
        assert!(titles(&session).contains(&"report.md"));
    }

    #[test]
    fn an_answer_for_a_lane_with_no_source_is_refused() {
        let mut session = session();
        session.set_query("re");
        assert_eq!(
            session.deliver(Lane::Web, session.generation(), Ok(Vec::new())),
            Err(Refusal::Unexpected)
        );
    }

    #[test]
    fn the_selection_stays_on_its_item_when_slower_results_arrive() {
        let mut session = session();
        session.set_query("flu");
        session.move_selection(1);
        let held = session.selected().unwrap().id.clone();
        session
            .deliver(
                Lane::Files,
                session.generation(),
                Ok(vec![file("/a/flu-notes.md"), file("/a/influx.txt")]),
            )
            .unwrap();
        assert_eq!(session.selected().unwrap().id, held);
        // And the files landed beneath the lanes that were already there.
        let lanes = lanes(&session);
        let files = lanes.iter().position(|lane| *lane == Lane::Files).unwrap();
        let apps = lanes.iter().position(|lane| *lane == Lane::Apps).unwrap();
        assert!(files > apps);
    }

    #[test]
    fn a_new_query_selects_the_first_row_again() {
        let mut session = session();
        session.set_query("l");
        session.move_selection(2);
        session.set_query("lu");
        assert_eq!(session.selected().unwrap().title, "Lumio Dev");
    }

    #[test]
    fn the_same_text_again_is_not_a_new_query() {
        let mut session = session();
        assert!(session.set_query("lu"));
        let generation = session.generation();
        session.move_selection(1);
        let held = session.selected().unwrap().id.clone();
        assert!(!session.set_query("lu"));
        assert_eq!(session.generation(), generation);
        assert_eq!(session.selected().unwrap().id, held);
    }

    #[test]
    fn rows_that_still_match_stay_while_the_lane_looks_again() {
        let mut session = session();
        session.set_query("rep");
        session
            .deliver(
                Lane::Files,
                session.generation(),
                Ok(vec![file("/a/report.md"), file("/a/repo-list.txt")]),
            )
            .unwrap();
        session.set_query("repor");
        assert!(titles(&session).contains(&"report.md"));
        assert!(!titles(&session).contains(&"repo-list.txt"));
        assert_eq!(session.looking(), Some(Lane::Files));
    }

    #[test]
    fn one_letter_does_not_trouble_an_asked_lane() {
        let mut session = session();
        session.set_query("r");
        assert!(session.lanes_to_ask().is_empty());
        session.set_query("re");
        assert_eq!(session.lanes_to_ask(), [Lane::Files]);
        session
            .deliver(Lane::Files, session.generation(), Ok(Vec::new()))
            .unwrap();
        assert!(session.lanes_to_ask().is_empty());
        assert_eq!(session.looking(), None);
    }

    #[test]
    fn a_lane_is_asked_only_under_a_scope_that_shows_it() {
        let mut session = session();
        session.set_scope(Scope::Commands);
        session.set_query("report");
        assert!(session.lanes_to_ask().is_empty());
        // Narrowing to it later asks, for the same query.
        session.set_scope(Scope::Files);
        assert_eq!(session.lanes_to_ask(), [Lane::Files]);
    }

    #[test]
    fn a_scope_shows_only_its_lane_and_untyped_shows_all_of_it() {
        let mut session = session();
        session.set_scope(Scope::Apps);
        assert_eq!(lanes(&session), [Lane::Apps]);
        assert_eq!(session.count(), 4);
        session.set_query("ma");
        assert_eq!(titles(&session), ["Mail"]);
        session.set_scope(Scope::Conversations);
        // Asking is how a conversation starts, so it shows here too.
        assert_eq!(lanes(&session), [Lane::Ask]);
    }

    #[test]
    fn scopes_step_around() {
        let mut session = session();
        session.previous_scope();
        assert_eq!(session.scope(), Scope::Web);
        session.next_scope();
        session.next_scope();
        assert_eq!(session.scope(), Scope::Apps);
    }

    #[test]
    fn a_lane_that_cannot_look_says_why_in_its_place() {
        let mut session = session();
        session.set_query("report");
        session
            .deliver(
                Lane::Files,
                session.generation(),
                Err("Spotlight is not available.".to_owned()),
            )
            .unwrap();
        assert_eq!(
            session.unavailable(),
            [(Lane::Files, "Spotlight is not available.")]
        );
        assert_eq!(session.looking(), None);
        // The lanes that can look still show.
        assert!(session.count() > 0);
    }

    #[test]
    fn the_selection_goes_around_the_ends() {
        let mut session = session();
        session.move_selection(-1);
        assert_eq!(
            session.selected().unwrap().title,
            "Postgres rollback decision"
        );
        session.move_selection(1);
        assert_eq!(session.selected().unwrap().title, "New Tab");
    }

    #[test]
    fn a_verb_the_item_does_not_have_cannot_be_chosen() {
        let mut session = session();
        session.set_query("lumio");
        assert!(session.choose(Verb::Reveal).is_some());
        assert!(session.choose(Verb::AskAbout).is_none());
        session
            .deliver(
                Lane::Files,
                session.generation(),
                Ok(vec![file("/a/lumio-notes.md")]),
            )
            .unwrap();
        session.select(&ItemId::new("file", "/a/lumio-notes.md"));
        let chosen = session.choose(Verb::AskAbout).unwrap();
        assert_eq!(chosen.item.title, "lumio-notes.md");
    }

    #[test]
    fn escape_steps_out_of_talking_then_the_text_then_the_launcher() {
        let mut session = session();
        session.set_query("lu");
        session.talk();
        assert_eq!(session.escape(), Escaped::LeftTalk);
        assert_eq!(session.mode(), Mode::Find);
        // What was typed before talking is still there.
        assert_eq!(session.query(), "lu");
        assert_eq!(session.escape(), Escaped::Cleared);
        assert_eq!(session.query(), "");
        assert_eq!(session.escape(), Escaped::Dismiss);
    }

    #[test]
    fn an_address_offers_to_open_it_before_searching_for_it() {
        let mut session = Session::new();
        session.set_query("example.com");
        let first = session.confirm().unwrap();
        assert_eq!(first.verb, Verb::Open);
        assert_eq!(
            first.item.target,
            Target::Address("https://example.com".to_owned())
        );
    }

    #[test]
    fn replacing_a_lane_keeps_the_selection_on_what_is_still_there() {
        let mut session = session();
        session.set_query("lu");
        session.move_selection(1);
        let held = session.selected().unwrap().clone();
        session.set_standing(
            Lane::Apps,
            vec![app("Lumio Dev"), app("Flux"), app("Lunar")],
        );
        assert_eq!(session.selected().unwrap().id, held.id);
        // Gone from the lane, the selection falls back to the first row.
        session.set_standing(Lane::Apps, vec![app("Lunar")]);
        assert_eq!(session.selected().unwrap().title, "Lunar");
    }
}
