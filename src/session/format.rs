//! The files' formats: JSON for the pins and the session, and a raw frame
//! for a site's icon. No GPUI here, so the round trips are tested alone.

use std::collections::HashSet;

use serde_json::{Map, Value, json};
use url::Url;

const VERSION: u64 = 1;
/// A favicon file's first bytes, before its width and height.
const ICON_MAGIC: &[u8; 4] = b"FRI1";

#[derive(Clone, Debug, PartialEq)]
pub struct Pin {
    pub id: u64,
    pub url: Url,
    pub title: String,
}

/// What a tab showed, enough to show it again.
#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    Start,
    Page { url: Url, title: String },
    Search { query: String, ask: bool },
}

impl Content {
    pub fn title(&self) -> &str {
        match self {
            Self::Start => "New Tab",
            Self::Page { url, title } if title.trim().is_empty() => {
                url.host_str().unwrap_or(url.as_str())
            }
            Self::Page { title, .. } => title,
            Self::Search { query, .. } => query,
        }
    }

    pub fn url(&self) -> Option<&Url> {
        match self {
            Self::Page { url, .. } => Some(url),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SavedTab {
    pub id: u64,
    pub content: Content,
    /// The pinned site the tab belongs to; such tabs are not in the tree.
    pub pin: Option<u64>,
    pub parent: Option<u64>,
    pub opener: Option<u64>,
    pub collapsed: bool,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Session {
    pub visit_session: i64,
    pub voice: crate::spoken::Preferences,
    /// Spatial presentation, independent of the live tab and dock lifetimes.
    pub board: BoardState,
    pub sidebar: bool,
    /// How wide the sidebar was dragged; `None` leaves it at its default.
    pub sidebar_width: Option<f32>,
    /// The places bump stands over the tabs and the workspace too.
    pub places_pinned: bool,
    pub active: Option<u64>,
    /// Parents before their children and siblings in order: the tree's order.
    pub tabs: Vec<SavedTab>,
    /// How the dock held the tabs: its groups and the splits between them.
    pub dock: Layout,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct BoardState {
    pub hidden: Vec<u64>,
    pub connections: Vec<[u64; 2]>,
    pub groups: Vec<Vec<u64>>,
    pub cards: Vec<CardPlacement>,
    pub notes: Vec<NotePlacement>,
    pub camera: Option<[f64; 3]>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CardPlacement {
    pub tab: u64,
    pub rect: [f64; 4],
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NotePlacement {
    pub id: u64,
    pub path: String,
    pub rect: [f64; 4],
}

impl BoardState {
    /// Reject invalid geometry before it reaches layout or the GPU. Missing
    /// fields in older sessions leave the board at its default arrangement.
    pub fn validate(mut self) -> Self {
        let mut ids = HashSet::new();
        self.cards.retain(|card| {
            ids.insert(card.tab)
                && card.rect.iter().all(|n| n.is_finite() && n.abs() < 1e7)
                && (60. ..=8192.).contains(&card.rect[2])
                && (40. ..=8192.).contains(&card.rect[3])
        });
        self.notes.retain(|note| {
            ids.insert(note.id)
                && !note.path.contains("..")
                && !std::path::Path::new(&note.path).is_absolute()
                && note.rect.iter().all(|n| n.is_finite() && n.abs() < 1e7)
                && (60. ..=8192.).contains(&note.rect[2])
                && (40. ..=8192.).contains(&note.rect[3])
        });
        self.camera = self.camera.filter(|c| {
            c.iter().all(|n| n.is_finite() && n.abs() < 1e7) && (0.02..=4.).contains(&c[2])
        });
        self
    }
}

impl Session {
    /// Add a tab for each of `contents` after the others, at the top of the
    /// tree, leaving out what a tab shows already.
    pub fn add_tabs(&mut self, contents: impl IntoIterator<Item = Content>) {
        let mut id = self.tabs.iter().map(|tab| tab.id + 1).max().unwrap_or(0);
        for content in contents {
            let shown = self.tabs.iter().any(|tab| match content.url() {
                Some(url) => tab.content.url() == Some(url),
                None => tab.content == content,
            });
            if shown {
                continue;
            }
            self.tabs.push(SavedTab {
                id,
                content,
                pin: None,
                parent: None,
                opener: None,
                collapsed: false,
                name: None,
            });
            id += 1;
        }
    }
}

/// Tab groups, side by side or stacked, as the dock holds them.
#[derive(Clone, Debug, PartialEq)]
pub enum Layout {
    /// Each child with the width or height it was drawn at. The dock keeps
    /// their proportions in a window of another size.
    Split {
        vertical: bool,
        children: Vec<(Layout, Option<f32>)>,
    },
    /// A group's tabs, and which one it shows.
    Group { tabs: Vec<u64>, shown: usize },
}

impl Default for Layout {
    fn default() -> Self {
        Self::Group {
            tabs: Vec::new(),
            shown: 0,
        }
    }
}

impl Layout {
    /// The tabs, group by group: left to right, top to bottom.
    pub fn tabs(&self) -> Vec<u64> {
        match self {
            Self::Split { children, .. } => children
                .iter()
                .flat_map(|(child, _)| child.tabs())
                .collect(),
            Self::Group { tabs, .. } => tabs.clone(),
        }
    }

    /// This layout holding exactly `ids`: a tab it names that is not one of
    /// them, or that it named before, goes, and those it lacks join its last
    /// group. A group left empty goes, and a split left with one child
    /// becomes that child.
    pub fn settle(self, ids: &[u64]) -> Self {
        let mut placed = HashSet::new();
        let mut layout = self
            .keep(&mut |id| ids.contains(&id) && placed.insert(id))
            .unwrap_or_default();
        let missing = ids.iter().filter(|id| !placed.contains(*id)).copied();
        layout.append(missing.collect());
        layout
    }

    fn keep(self, keep: &mut impl FnMut(u64) -> bool) -> Option<Self> {
        match self {
            Self::Split { vertical, children } => {
                let mut children: Vec<_> = children
                    .into_iter()
                    .filter_map(|(child, size)| Some((child.keep(keep)?, size)))
                    .collect();
                match children.len() {
                    0 => None,
                    1 => children.pop().map(|(child, _)| child),
                    _ => Some(Self::Split { vertical, children }),
                }
            }
            Self::Group { tabs, shown } => {
                let shown_tab = tabs.get(shown).copied();
                let tabs: Vec<u64> = tabs.into_iter().filter(|id| keep(*id)).collect();
                let shown = shown_tab
                    .and_then(|shown| tabs.iter().position(|id| *id == shown))
                    .unwrap_or(0);
                (!tabs.is_empty()).then_some(Self::Group { tabs, shown })
            }
        }
    }

    fn append(&mut self, ids: Vec<u64>) {
        match self {
            Self::Split { children, .. } => {
                if let Some((last, _)) = children.last_mut() {
                    last.append(ids);
                }
            }
            Self::Group { tabs, .. } => tabs.extend(ids),
        }
    }
}

pub fn pins_to_json(pins: &[Pin]) -> Value {
    let pins: Vec<Value> = pins
        .iter()
        .map(|pin| json!({ "id": pin.id, "url": pin.url.as_str(), "title": pin.title }))
        .collect();
    json!({ "version": VERSION, "pins": pins })
}

/// None when the file is not one Platinum wrote.
pub fn pins_from_json(value: &Value) -> Option<Vec<Pin>> {
    let pins = value.get("pins")?.as_array()?;
    pins.iter()
        .map(|pin| {
            Some(Pin {
                id: pin.get("id")?.as_u64()?,
                url: Url::parse(pin.get("url")?.as_str()?).ok()?,
                title: string(pin, "title").unwrap_or_default(),
            })
        })
        .collect()
}

pub fn session_to_json(session: &Session) -> Value {
    let tabs: Vec<Value> = session
        .tabs
        .iter()
        .map(|tab| {
            let mut entry = Map::new();
            entry.insert("id".into(), tab.id.into());
            match &tab.content {
                Content::Start => {}
                Content::Page { url, title } => {
                    entry.insert("url".into(), url.as_str().into());
                    entry.insert("title".into(), title.as_str().into());
                }
                Content::Search { query, ask } => {
                    entry.insert("search".into(), query.as_str().into());
                    entry.insert("ask".into(), (*ask).into());
                }
            }
            let optional = [
                ("pin", tab.pin),
                ("parent", tab.parent),
                ("opener", tab.opener),
            ];
            for (key, value) in optional {
                if let Some(value) = value {
                    entry.insert(key.into(), value.into());
                }
            }
            if tab.collapsed {
                entry.insert("collapsed".into(), true.into());
            }
            if let Some(name) = &tab.name {
                entry.insert("name".into(), name.as_str().into());
            }
            Value::Object(entry)
        })
        .collect();
    json!({
        "version": VERSION,
        "sidebar": session.sidebar,
        "sidebar_width": session.sidebar_width,
        "places_pinned": session.places_pinned,
        "visit_session": session.visit_session,
        "board": session.board,
        "voice": session.voice,
        "active": session.active,
        "tabs": tabs,
        "dock": layout_to_json(&session.dock),
    })
}

fn layout_to_json(layout: &Layout) -> Value {
    match layout {
        Layout::Split { vertical, children } => {
            let sizes: Vec<Option<f32>> = children.iter().map(|(_, size)| *size).collect();
            let children: Vec<Value> = children
                .iter()
                .map(|(child, _)| layout_to_json(child))
                .collect();
            json!({
                "split": if *vertical { "vertical" } else { "horizontal" },
                "sizes": sizes,
                "children": children,
            })
        }
        Layout::Group { tabs, shown } => json!({ "tabs": tabs, "shown": shown }),
    }
}

/// An older file's dock is a list of tabs: one group. A child that is not a
/// layout is left out, and its tabs join the last group (`Layout::settle`).
fn layout_from_json(value: &Value) -> Option<Layout> {
    if let Some(tabs) = value.as_array() {
        return Some(Layout::Group {
            tabs: tabs.iter().filter_map(Value::as_u64).collect(),
            shown: 0,
        });
    }
    if let Some(tabs) = value.get("tabs").and_then(Value::as_array) {
        return Some(Layout::Group {
            tabs: tabs.iter().filter_map(Value::as_u64).collect(),
            shown: number(value, "shown").unwrap_or(0) as usize,
        });
    }
    let vertical = match value.get("split")?.as_str()? {
        "horizontal" => false,
        "vertical" => true,
        _ => return None,
    };
    let sizes = value.get("sizes").and_then(Value::as_array);
    let children = value
        .get("children")?
        .as_array()?
        .iter()
        .enumerate()
        .filter_map(|(ix, child)| {
            let size = sizes
                .and_then(|sizes| sizes.get(ix)?.as_f64())
                .filter(|size| *size > 0.)
                .map(|size| size as f32);
            Some((layout_from_json(child)?, size))
        })
        .collect();
    Some(Layout::Split { vertical, children })
}

/// None when the file is not one Platinum wrote. A tab whose address no longer
/// parses comes back as a new tab, and one without an id is left out,
/// rather than losing the rest.
pub fn session_from_json(value: &Value) -> Option<Session> {
    let tabs = value
        .get("tabs")?
        .as_array()?
        .iter()
        .filter_map(|tab| {
            let content = if let Some(query) = string(tab, "search") {
                Content::Search {
                    query,
                    ask: tab.get("ask").and_then(Value::as_bool).unwrap_or(false),
                }
            } else if let Some(url) = string(tab, "url").and_then(|url| Url::parse(&url).ok()) {
                Content::Page {
                    url,
                    title: string(tab, "title").unwrap_or_default(),
                }
            } else {
                Content::Start
            };
            Some(SavedTab {
                id: tab.get("id")?.as_u64()?,
                content,
                pin: number(tab, "pin"),
                parent: number(tab, "parent"),
                opener: number(tab, "opener"),
                collapsed: tab
                    .get("collapsed")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                name: string(tab, "name"),
            })
        })
        .collect();
    let dock = value
        .get("dock")
        .and_then(layout_from_json)
        .unwrap_or_default();
    Some(Session {
        visit_session: value
            .get("visit_session")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        voice: value
            .get("voice")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default(),
        board: value
            .get("board")
            .cloned()
            .and_then(|v| serde_json::from_value::<BoardState>(v).ok())
            .unwrap_or_default()
            .validate(),
        sidebar: value
            .get("sidebar")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        sidebar_width: value
            .get("sidebar_width")
            .and_then(Value::as_f64)
            .map(|width| width as f32),
        places_pinned: value
            .get("places_pinned")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        active: number(value, "active"),
        tabs,
        dock,
    })
}

/// A site's icon on disk: the magic, width and height (little-endian), then
/// the BGRA pixels GPUI draws.
pub fn encode_icon(width: u32, height: u32, bgra: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + bgra.len());
    out.extend_from_slice(ICON_MAGIC);
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(bgra);
    out
}

pub fn decode_icon(bytes: &[u8]) -> Option<(u32, u32, &[u8])> {
    let rest = bytes.strip_prefix(ICON_MAGIC)?;
    let width = u32::from_le_bytes(rest.get(0..4)?.try_into().ok()?);
    let height = u32::from_le_bytes(rest.get(4..8)?.try_into().ok()?);
    let pixels = rest.get(8..)?;
    (pixels.len() as u64 == u64::from(width) * u64::from(height) * 4)
        .then_some((width, height, pixels))
}

/// A host as a file name: anything but letters, digits, dots and dashes
/// (an IPv6 address's brackets and colons) becomes an underscore.
pub fn icon_file_name(host: &str) -> String {
    let safe: String = host
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("{safe}.icon")
}

fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_owned)
}

fn number(value: &Value, key: &str) -> Option<u64> {
    value.get(key)?.as_u64()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn pins_round_trip() {
        let pins = vec![
            Pin {
                id: 1,
                url: url("https://www.notion.so/"),
                title: "Notion".into(),
            },
            Pin {
                id: 4,
                url: url("https://calendar.google.com/"),
                title: String::new(),
            },
        ];
        assert_eq!(pins_from_json(&pins_to_json(&pins)), Some(pins));
        assert_eq!(pins_from_json(&json!({ "nope": 1 })), None);
    }

    #[test]
    fn sessions_round_trip() {
        let session = Session {
            visit_session: 0,
            voice: Default::default(),
            board: BoardState::default(),
            sidebar: true,
            sidebar_width: Some(312.),
            places_pinned: true,
            active: Some(3),
            tabs: vec![
                SavedTab {
                    id: 1,
                    content: Content::Search {
                        query: "tree style tabs".into(),
                        ask: false,
                    },
                    pin: None,
                    parent: None,
                    opener: None,
                    collapsed: true,
                    name: Some("Browser research".into()),
                },
                SavedTab {
                    id: 3,
                    content: Content::Page {
                        url: url("https://github.com/piroor/treestyletab"),
                        title: "Tree Style Tab".into(),
                    },
                    pin: None,
                    parent: Some(1),
                    opener: Some(1),
                    collapsed: false,
                    name: None,
                },
                SavedTab {
                    id: 7,
                    content: Content::Start,
                    pin: Some(2),
                    parent: None,
                    opener: None,
                    collapsed: false,
                    name: None,
                },
            ],
            // The pinned tab on the left; on the right, two groups stacked.
            dock: Layout::Split {
                vertical: false,
                children: vec![
                    (
                        Layout::Group {
                            tabs: vec![7],
                            shown: 0,
                        },
                        Some(400.),
                    ),
                    (
                        Layout::Split {
                            vertical: true,
                            children: vec![
                                (
                                    Layout::Group {
                                        tabs: vec![3],
                                        shown: 0,
                                    },
                                    Some(250.),
                                ),
                                (
                                    Layout::Group {
                                        tabs: vec![1],
                                        shown: 0,
                                    },
                                    None,
                                ),
                            ],
                        },
                        Some(600.),
                    ),
                ],
            },
        };
        assert_eq!(session_from_json(&session_to_json(&session)), Some(session));
    }

    #[test]
    fn an_older_files_dock_is_one_group() {
        let value = json!({ "tabs": [{ "id": 2 }, { "id": 5 }], "dock": [5, 2] });
        let session = session_from_json(&value).unwrap();
        assert_eq!(
            session.dock,
            Layout::Group {
                tabs: vec![5, 2],
                shown: 0
            }
        );
    }

    #[test]
    fn a_layout_settles_on_the_tabs_there_are() {
        let group = |tabs: &[u64], shown| Layout::Group {
            tabs: tabs.to_vec(),
            shown,
        };
        // Tab 9 is gone, 2 is named twice, and 6 was never placed.
        let layout = Layout::Split {
            vertical: false,
            children: vec![
                (group(&[1, 9, 2], 2), Some(300.)),
                (group(&[9], 0), Some(200.)),
                (
                    Layout::Split {
                        vertical: true,
                        children: vec![(group(&[2], 0), None), (group(&[4], 0), None)],
                    },
                    Some(500.),
                ),
            ],
        };
        let settled = layout.settle(&[1, 2, 4, 6]);
        assert_eq!(
            settled,
            Layout::Split {
                vertical: false,
                children: vec![
                    (group(&[1, 2], 1), Some(300.)),
                    (group(&[4, 6], 0), Some(500.)),
                ],
            },
            "the group that showed 2 still does; a split left with one group is it"
        );
        assert_eq!(settled.tabs(), [1, 2, 4, 6]);
        assert_eq!(
            Layout::default().settle(&[3]),
            group(&[3], 0),
            "no layout: one group"
        );
    }

    #[test]
    fn a_tab_with_a_bad_address_comes_back_as_a_new_tab() {
        let value = json!({ "tabs": [{ "id": 2, "url": "not a url", "title": "x" }] });
        let session = session_from_json(&value).unwrap();
        assert_eq!(session.tabs[0].content, Content::Start);
        assert!(!session.sidebar);
        assert_eq!(
            session.sidebar_width, None,
            "an older file keeps the default"
        );
        assert!(!session.places_pinned, "an older file keeps the default");
    }

    #[test]
    fn a_tab_without_an_id_is_left_out_and_the_rest_kept() {
        let value =
            json!({ "tabs": [{ "id": 1 }, { "url": "https://example.org/" }, { "id": 3 }] });
        let session = session_from_json(&value).unwrap();
        let ids: Vec<u64> = session.tabs.iter().map(|tab| tab.id).collect();
        assert_eq!(ids, [1, 3]);
    }

    #[test]
    fn untitled_pages_are_called_by_their_host() {
        let page = Content::Page {
            url: url("https://example.org/a"),
            title: " ".into(),
        };
        assert_eq!(page.title(), "example.org");
    }

    #[test]
    fn icons_round_trip_and_reject_short_files() {
        let pixels = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let bytes = encode_icon(2, 1, &pixels);
        assert_eq!(decode_icon(&bytes), Some((2, 1, &pixels[..])));
        assert_eq!(decode_icon(&bytes[..bytes.len() - 1]), None);
        assert_eq!(decode_icon(b"PNG..."), None);
    }

    #[test]
    fn hosts_become_safe_file_names() {
        assert_eq!(icon_file_name("www.YouTube.com"), "www.youtube.com.icon");
        assert_eq!(icon_file_name("[::1]"), "___1_.icon");
    }
}

#[cfg(test)]
mod spatial_tests {
    use super::*;
    #[test]
    fn board_round_trip_preserves_identity_geometry_camera_and_visit_namespace() {
        let session = Session {
            visit_session: 42,
            board: BoardState {
                cards: vec![CardPlacement {
                    tab: 7,
                    rect: [10., 20., 1200., 832.],
                }],
                notes: vec![NotePlacement {
                    id: u64::MAX - 1,
                    path: "Note.md".into(),
                    rect: [1300., 20., 480., 400.],
                }],
                camera: Some([27., -18., 0.4]),
                ..Default::default()
            },
            ..Default::default()
        };
        let value = session_to_json(&session);
        let loaded = session_from_json(&value).unwrap();
        assert_eq!(loaded.board, session.board);
        assert_eq!(loaded.visit_session, 42);
    }
    #[test]
    fn corrupt_board_geometry_and_duplicate_identifiers_do_not_reach_renderer() {
        let board = BoardState {
            cards: vec![
                CardPlacement {
                    tab: 3,
                    rect: [0., 0., 100., 100.],
                },
                CardPlacement {
                    tab: 3,
                    rect: [0., 0., 200., 200.],
                },
                CardPlacement {
                    tab: 8,
                    rect: [0., 0., f64::NAN, 100.],
                },
            ],
            notes: vec![
                NotePlacement {
                    id: 3,
                    path: "Note.md".into(),
                    rect: [0., 0., 100., 100.],
                },
                NotePlacement {
                    id: 4,
                    path: "../escape.md".into(),
                    rect: [0., 0., 100., 100.],
                },
            ],
            camera: Some([0., 0., f64::INFINITY]),
            ..Default::default()
        }
        .validate();
        assert_eq!(board.cards.len(), 1);
        assert!(board.notes.is_empty() && board.camera.is_none());
    }
}
