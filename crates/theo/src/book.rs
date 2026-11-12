//! Every named performance: the built-in ones and those the agent or the
//! person defined. A definition is never changed: defining a name again adds
//! a version, and `name@2` still means what it meant, because what it uses is
//! written down by version when it is saved.
use crate::channels::Channels;
use crate::json::{Json, js_number};
use crate::memory::Shelf;
use crate::ordered::Ordered;
use crate::program::{Context, Program, Refusal, as_program, compile};
use crate::tables::tables;

/// Who wrote a performance or a rule: the face's own, the agent, or the person.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum By {
    Theo,
    Agent,
    Person,
}

impl By {
    pub fn name(self) -> &'static str {
        match self {
            By::Theo => "theo",
            By::Agent => "agent",
            By::Person => "person",
        }
    }

    pub fn parse(name: &str) -> Option<By> {
        [By::Theo, By::Agent, By::Person]
            .into_iter()
            .find(|by| by.name() == name)
    }
}

/// A named performance: what it is for, and its program.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub version: f64,
    pub note: String,
    /// `{steps, keep?}`, as written.
    pub program: Json,
    pub by: By,
}

impl Entry {
    fn read(value: &Json) -> Option<Entry> {
        Some(Entry {
            name: value.str_at("name")?.to_owned(),
            version: value.num_at("version")?,
            note: value.str_at("note").unwrap_or("").to_owned(),
            program: value.get("program")?.clone(),
            by: value.str_at("by").and_then(By::parse)?,
        })
    }

    pub fn to_json(&self) -> Json {
        Json::Obj(vec![
            ("name".to_owned(), Json::Str(self.name.clone())),
            ("version".to_owned(), Json::Num(self.version)),
            ("note".to_owned(), Json::Str(self.note.clone())),
            ("program".to_owned(), self.program.clone()),
            ("by".to_owned(), Json::Str(self.by.name().to_owned())),
        ])
    }

    /// `name@version`.
    pub fn reference(&self) -> String {
        format!("{}@{}", self.name, js_number(self.version))
    }
}

const SHELF_KEY: &str = "theorem-ui:theo:book";
/// The most added performances the book holds, every version counted, and the largest one, in characters of JSON.
pub const ENTRIES: usize = 64;
pub const SIZE: usize = 6000;

/// A lowercase letter, then `least..=most` more of lowercase letters, digits and `extra`.
pub(crate) fn is_name(text: &str, least: usize, most: usize, extra: char) -> bool {
    let mut letters = text.chars();
    if !letters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
    {
        return false;
    }
    let rest = letters.as_str();
    (least..=most).contains(&rest.len())
        && rest
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == extra)
}

/// The first `units` UTF-16 units of a text, never splitting a character.
pub(crate) fn clip(text: &str, units: usize) -> String {
    let mut used = 0;
    text.chars()
        .take_while(|c| {
            used += c.len_utf16();
            used <= units
        })
        .collect()
}

/// How long a text is as the web counts it.
pub(crate) fn length(text: &str) -> usize {
    text.encode_utf16().count()
}

/// What `Number(text)` is for the version after an `@`.
fn version_of(text: &str) -> f64 {
    let text = text.trim();
    if text.is_empty() {
        return 0.0;
    }
    if !text
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'))
    {
        return f64::NAN;
    }
    text.parse().unwrap_or(f64::NAN)
}

/// A reference split into its name and the version after the `@`, if it has one.
fn split(reference: &str) -> (&str, Option<&str>) {
    let mut parts = reference.split('@');
    (parts.next().unwrap_or(""), parts.next())
}

/// Calls `visit` with the name of every `use` step in a list of steps, however deep.
fn each_use(steps: Option<&Json>, visit: &mut dyn FnMut(&str), depth: usize) {
    let Some(steps) = steps.and_then(Json::list) else {
        return;
    };
    if depth > 32 {
        return;
    }
    for step in steps {
        if step.obj().is_none() {
            continue;
        }
        if let (Some("use"), Some(name)) = (step.str_at("op"), step.str_at("name")) {
            visit(name);
        }
        each_use(step.get("steps"), visit, depth + 1);
        each_use(step.get("of"), visit, depth + 1);
        if let Some(one) = step.get("step") {
            each_use(Some(&Json::List(vec![one.clone()])), visit, depth + 1);
        }
    }
}

/// Rewrites the name of every `use` step that `pin` has a pinned name for.
fn pin_uses(steps: &mut [Json], pin: &dyn Fn(&str) -> Option<String>, depth: usize) {
    if depth > 32 {
        return;
    }
    for step in steps {
        let Json::Obj(fields) = step else { continue };
        let uses = fields
            .iter()
            .any(|(key, value)| key == "op" && value.str() == Some("use"));
        for (key, value) in fields.iter_mut() {
            match (key.as_str(), value) {
                ("name", Json::Str(name)) if uses => {
                    if let Some(pinned) = pin(name) {
                        *name = pinned;
                    }
                }
                ("steps" | "of", Json::List(inner)) => pin_uses(inner, pin, depth + 1),
                ("step", inner) => pin_uses(std::slice::from_mut(inner), pin, depth + 1),
                _ => {}
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Book {
    /// Every version of each name, oldest first; built-in names first.
    entries: Ordered<Vec<Entry>>,
}

impl Book {
    pub fn new(shelf: Option<&dyn Shelf>) -> Self {
        let mut entries = Ordered::new();
        for entry in tables().book.iter().filter_map(Entry::read) {
            let name = entry.name.clone();
            entries.set(&name, vec![entry]);
        }
        // A shelf that cannot be read is an empty shelf.
        let saved = shelf
            .and_then(|shelf| shelf.get_item(SHELF_KEY))
            .and_then(|text| Json::parse(&text).ok());
        for entry in saved
            .as_ref()
            .and_then(Json::list)
            .unwrap_or(&[])
            .iter()
            .filter_map(Entry::read)
        {
            if !is_name(&entry.name, 1, 31, '-') {
                continue;
            }
            match entries.get_mut(&entry.name) {
                Some(versions) => versions.push(entry),
                None => {
                    let name = entry.name.clone();
                    entries.set(&name, vec![entry]);
                }
            }
        }
        Self { entries }
    }

    /// The newest version of a name, or the version named by `name@n`.
    pub fn get(&self, reference: &str) -> Option<&Entry> {
        let (name, version) = split(reference);
        let versions = self.entries.get(name)?;
        match version {
            None => versions.last(),
            Some(version) => {
                let version = version_of(version);
                versions.iter().find(|entry| entry.version == version)
            }
        }
    }

    /// The program a reference names, for the checker.
    pub fn lookup(&self, reference: &str) -> Option<Program> {
        as_program(&self.get(reference)?.program)
    }

    /// The newest version of each performance, built-in ones first.
    pub fn list(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values().filter_map(|versions| versions.last())
    }

    fn defined(&self) -> impl Iterator<Item = &Entry> {
        self.entries
            .values()
            .flatten()
            .filter(|entry| entry.by != By::Theo)
    }

    /// Adds a performance, or a new version of one. The program is checked
    /// first against the face as it is; a refusal says where and why.
    pub fn define(
        &mut self,
        name: &str,
        program: &Json,
        note: &str,
        by: By,
        channels: &Channels,
        shelf: Option<&mut dyn Shelf>,
    ) -> Result<Entry, Refusal> {
        let refuse = |path: &str, reason: String| {
            Err(Refusal {
                path: path.to_owned(),
                reason,
            })
        };
        if !is_name(name, 1, 31, '-') {
            return refuse(
                "name",
                "A name is 2 to 32 lowercase letters, digits or hyphens, starting with a letter."
                    .to_owned(),
            );
        }
        let size = length(&program.to_string());
        if size > SIZE {
            return refuse(
                "program",
                format!(
                    "A saved performance is at most {SIZE} characters of JSON; this is {size}."
                ),
            );
        }
        let lookup = |reference: &str| self.lookup(reference);
        let mut context = Context::new(channels);
        context.lookup = Some(&lookup);
        compile(program, &context)?;
        if self.defined().count() >= ENTRIES {
            return refuse(
                "name",
                format!(
                    "The book holds at most {ENTRIES} added performances, each version counted; remove one first."
                ),
            );
        }
        let Some(mut saved) = as_program(program) else {
            return refuse("", "This is not a program.".to_owned());
        };
        // What it uses is the version there is today, so a later revision of that name does not change this one.
        pin_uses(
            &mut saved.steps,
            &|used| {
                if used.contains('@') {
                    return None;
                }
                self.get(used).map(Entry::reference)
            },
            0,
        );
        let mut written = vec![("steps".to_owned(), Json::List(saved.steps))];
        if saved.keep {
            written.push(("keep".to_owned(), Json::Bool(true)));
        }
        let last = self
            .entries
            .get(name)
            .and_then(|versions| versions.last())
            .map_or(0.0, |entry| entry.version);
        let entry = Entry {
            name: name.to_owned(),
            version: last + 1.0,
            note: clip(note, 240),
            program: Json::Obj(written),
            by,
        };
        match self.entries.get_mut(name) {
            Some(versions) => versions.push(entry.clone()),
            None => self.entries.set(name, vec![entry.clone()]),
        }
        self.save(shelf);
        Ok(entry)
    }

    /// Removes every added version of a name, or one version given as
    /// `name@version`. The face's own performances stay, and so does a version
    /// another added performance uses.
    pub fn remove(&mut self, reference: &str, shelf: Option<&mut dyn Shelf>) -> Result<(), String> {
        let (name, version) = split(reference);
        let version = version.map(version_of);
        let goes = |entry: &Entry| {
            entry.name == name
                && entry.by != By::Theo
                && version.is_none_or(|version| entry.version == version)
        };
        if !self.defined().any(goes) {
            return Err(format!(
                "Nothing added is named \"{reference}\"; Theo\u{2019}s own performances stay."
            ));
        }
        let mut users: Vec<String> = Vec::new();
        for entry in self.defined().filter(|entry| !goes(entry)) {
            each_use(
                entry.program.get("steps"),
                &mut |used| {
                    if self.get(used).is_some_and(goes) {
                        let user = entry.reference();
                        if !users.contains(&user) {
                            users.push(user);
                        }
                    }
                },
                0,
            );
        }
        if !users.is_empty() {
            return Err(format!(
                "\"{reference}\" is used by {}; remove {} first.",
                users.join(", "),
                if users.len() == 1 {
                    "that version"
                } else {
                    "those versions"
                }
            ));
        }
        let kept: Vec<Entry> = self
            .entries
            .get(name)
            .map(|versions| {
                versions
                    .iter()
                    .filter(|entry| !goes(entry))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if kept.is_empty() {
            self.entries.remove(name);
        } else {
            self.entries.set(name, kept);
        }
        self.save(shelf);
        Ok(())
    }

    fn save(&self, shelf: Option<&mut dyn Shelf>) {
        if let Some(shelf) = shelf {
            let defined = Json::List(self.defined().map(Entry::to_json).collect());
            shelf.set_item(SHELF_KEY, &defined.to_string());
        }
    }
}
