//! The files' formats: JSON for the pins and the session, and a raw frame
//! for a site's icon. No GPUI here, so the round trips are tested alone.

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
    pub sidebar: bool,
    /// How wide the sidebar was dragged; `None` leaves it at its default.
    pub sidebar_width: Option<f32>,
    pub active: Option<u64>,
    /// Parents before their children and siblings in order: the tree's order.
    pub tabs: Vec<SavedTab>,
    /// The tabs in the order the dock holds them.
    pub dock: Vec<u64>,
}

pub fn pins_to_json(pins: &[Pin]) -> Value {
    let pins: Vec<Value> = pins
        .iter()
        .map(|pin| json!({ "id": pin.id, "url": pin.url.as_str(), "title": pin.title }))
        .collect();
    json!({ "version": VERSION, "pins": pins })
}

/// None when the file is not one Frame wrote.
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
        "active": session.active,
        "tabs": tabs,
        "dock": session.dock,
    })
}

/// None when the file is not one Frame wrote. A tab whose address no longer
/// parses comes back as a new tab rather than losing the rest.
pub fn session_from_json(value: &Value) -> Option<Session> {
    let tabs = value
        .get("tabs")?
        .as_array()?
        .iter()
        .map(|tab| {
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
        .collect::<Option<Vec<_>>>()?;
    let dock = value
        .get("dock")
        .and_then(Value::as_array)
        .map(|dock| dock.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default();
    Some(Session {
        sidebar: value
            .get("sidebar")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        sidebar_width: value
            .get("sidebar_width")
            .and_then(Value::as_f64)
            .map(|width| width as f32),
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
            sidebar: true,
            sidebar_width: Some(312.),
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
            dock: vec![3, 1, 7],
        };
        assert_eq!(session_from_json(&session_to_json(&session)), Some(session));
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
