//! One thing the launcher can show, and what can be done with it.

use std::path::PathBuf;

/// What an item is, for good: the same file, conversation or command has the
/// same id every time it is found, so the selection can stay on it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId(String);

impl ItemId {
    /// `kind` says what sort of thing, `key` which one: a path, a
    /// conversation's id, a command's name.
    pub fn new(kind: &str, key: &str) -> Self {
        Self(format!("{kind}:{key}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The sort of thing an item is, which decides its icon and its verbs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    App,
    Command,
    Conversation,
    File,
    Folder,
    /// An address to open.
    Link,
    /// A search of the web for what was typed.
    WebSearch,
    /// What was typed, asked of Theo.
    Ask,
}

impl Kind {
    /// The word for it beside the title.
    pub fn label(self) -> &'static str {
        match self {
            Kind::App => "Application",
            Kind::Command => "Command",
            Kind::Conversation => "Conversation",
            Kind::File => "File",
            Kind::Folder => "Folder",
            Kind::Link => "Address",
            Kind::WebSearch => "Web search",
            Kind::Ask => "Theo",
        }
    }
}

/// Where an item lives, said beside it so a file on this Mac is never taken
/// for a record somewhere else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Origin {
    ThisMac,
    Theorem,
    Platinum,
    Web,
}

impl Origin {
    pub fn label(self) -> &'static str {
        match self {
            Origin::ThisMac => "This Mac",
            Origin::Theorem => "Theorem",
            Origin::Platinum => "Platinum",
            Origin::Web => "Web",
        }
    }
}

/// What an item points at. The host acts on this; nothing here does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// An application, a file or a folder on this Mac.
    Path(PathBuf),
    /// A conversation, by the authority's id for it.
    Conversation(String),
    /// A command, by the name the host registered it under.
    Command(String),
    Address(String),
    WebSearch(String),
    /// A question for Theo, as it was typed.
    Ask(String),
}

/// Something that can be done with an item. The first of an item's verbs is
/// what Enter does; the row says so before it is pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Verb {
    /// Open it with whatever opens it: launch the application, or bring its
    /// window forward if it is running.
    Open,
    /// Show it in Finder.
    Reveal,
    CopyPath,
    CopyAddress,
    /// Attach it to the conversation and talk about it.
    AskAbout,
    /// Carry on a conversation.
    Resume,
    Run,
    /// Ask Theo what was typed.
    Ask,
    /// Search the web for what was typed.
    Search,
}

impl Verb {
    /// What the row and the button say.
    pub fn label(self) -> &'static str {
        match self {
            Verb::Open => "Open",
            Verb::Reveal => "Reveal in Finder",
            Verb::CopyPath => "Copy Path",
            Verb::CopyAddress => "Copy Address",
            Verb::AskAbout => "Ask Theo About This",
            Verb::Resume => "Resume",
            Verb::Run => "Run",
            Verb::Ask => "Ask Theo",
            Verb::Search => "Search",
        }
    }
}

/// One line of what is known about an item, shown beside it: its path, when
/// it was changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fact {
    pub name: String,
    pub value: String,
}

impl Fact {
    pub fn new(name: &str, value: impl Into<String>) -> Self {
        Self {
            name: name.to_owned(),
            value: value.into(),
        }
    }
}

/// One thing the launcher can show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: ItemId,
    pub kind: Kind,
    pub title: String,
    /// The second line: the folder a file is in, when a conversation was
    /// last touched.
    pub detail: String,
    pub origin: Origin,
    pub target: Target,
    /// Other words it answers to: an application's other name, a command's
    /// alias. Matched, never shown.
    pub keywords: Vec<String>,
    pub facts: Vec<Fact>,
    /// The passage that matched, or the start of the file, when there is one
    /// to show.
    pub excerpt: Option<String>,
    /// Shown before anything is typed.
    pub pinned: bool,
    /// How well it answers the query, higher is better. Set by whoever
    /// matched it; standing items are scored again on every keystroke.
    pub score: u32,
}

impl Item {
    pub fn new(id: ItemId, kind: Kind, title: impl Into<String>, target: Target) -> Self {
        let origin = match kind {
            Kind::App | Kind::File | Kind::Folder => Origin::ThisMac,
            Kind::Conversation | Kind::Ask => Origin::Theorem,
            Kind::Command => Origin::Platinum,
            Kind::Link | Kind::WebSearch => Origin::Web,
        };
        Self {
            id,
            kind,
            title: title.into(),
            detail: String::new(),
            origin,
            target,
            keywords: Vec::new(),
            facts: Vec::new(),
            excerpt: None,
            pinned: false,
            score: 0,
        }
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }

    pub fn origin(mut self, origin: Origin) -> Self {
        self.origin = origin;
        self
    }

    pub fn keywords(mut self, keywords: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.keywords = keywords.into_iter().map(Into::into).collect();
        self
    }

    pub fn fact(mut self, name: &str, value: impl Into<String>) -> Self {
        self.facts.push(Fact::new(name, value));
        self
    }

    pub fn excerpt(mut self, excerpt: impl Into<String>) -> Self {
        self.excerpt = Some(excerpt.into());
        self
    }

    pub fn pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }

    /// What can be done with it. The first is what Enter does.
    pub fn verbs(&self) -> &'static [Verb] {
        match self.kind {
            Kind::App | Kind::Folder => &[Verb::Open, Verb::Reveal, Verb::CopyPath],
            Kind::File => &[Verb::Open, Verb::AskAbout, Verb::Reveal, Verb::CopyPath],
            Kind::Conversation => &[Verb::Resume],
            Kind::Command => &[Verb::Run],
            Kind::Link => &[Verb::Open, Verb::CopyAddress],
            Kind::WebSearch => &[Verb::Search],
            Kind::Ask => &[Verb::Ask],
        }
    }

    /// What Enter does here.
    pub fn primary(&self) -> Verb {
        self.verbs()[0]
    }

    /// Whether there is more to show beside it than its row already says.
    pub fn has_preview(&self) -> bool {
        !self.facts.is_empty() || self.excerpt.is_some()
    }
}
