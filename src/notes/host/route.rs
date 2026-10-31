//! The `frame-notes:` URL grammar: what a request names, and the messages the
//! reading page sends the host.
//!
//! Everything in a URL is untrusted: a note's own markup can link to any
//! address. A path is accepted only as a plain sequence of visible vault
//! names, so no later step has to defend against `..`, hidden folders or
//! separators smuggled in through percent-encoding.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use url::Url;

pub use crate::engine::NOTES_SCHEME as SCHEME;

const MAX_PATH: usize = 1024;
const MAX_LINK: usize = 4096;
const MAX_KEY: usize = 1024;
const MAX_URL: usize = 8192;
const MAX_DIGEST: usize = 64;
/// The longest code block the copy button can send; a URL carries it.
pub const MAX_COPY: usize = 64 * 1024;

/// What a request URL names, under one vault's host.
#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    /// A bundled script, style or font: `/app/<path>`.
    App(String),
    /// A note's reading page: `/note/<vault path>?k=<token>`.
    Note { path: String, token: String },
    /// A file of the vault, such as an image: `/files/<vault path>`.
    File(String),
    /// An enabled CSS snippet of the vault: `/snippet/<name>`.
    Snippet(String),
    /// What a page preview shows: `/preview?from=<note>&link=<link>`.
    Preview { from: String, link: String },
}

/// Why a URL names nothing the handler serves.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A shape the grammar does not have.
    Unknown,
    /// A path that is not a plain visible vault path, or a missing parameter.
    Invalid,
}

impl Route {
    /// The route `url` names. The host is the caller's to check.
    pub fn parse(url: &Url) -> Result<Self, Refusal> {
        if url.scheme() != SCHEME {
            return Err(Refusal::Unknown);
        }
        let segments: Vec<&str> = url.path_segments().ok_or(Refusal::Unknown)?.collect();
        let (kind, rest) = segments.split_first().ok_or(Refusal::Unknown)?;
        let param = |name: &str| {
            url.query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.into_owned())
        };
        match *kind {
            "app" => path(rest).map(Self::App),
            "note" => Ok(Self::Note {
                path: path(rest)?,
                token: param("k").ok_or(Refusal::Invalid)?,
            }),
            "files" => path(rest).map(Self::File),
            "snippet" if rest.len() == 1 => path(rest).map(Self::Snippet),
            "snippet" => Err(Refusal::Invalid),
            "preview" if rest.is_empty() => {
                let from = param("from").ok_or(Refusal::Invalid)?;
                let link = param("link").ok_or(Refusal::Invalid)?;
                // A link names what it names; it is resolved against the
                // index, never against the disk, so it needs no path rules,
                // only a bound.
                if from.len() > MAX_PATH || link.len() > MAX_LINK || from.contains('\0') {
                    return Err(Refusal::Invalid);
                }
                Ok(Self::Preview { from, link })
            }
            _ => Err(Refusal::Unknown),
        }
    }
}

/// A vault path from URL path segments: visible names only, as one string.
fn path(segments: &[&str]) -> Result<String, Refusal> {
    let mut path = String::new();
    for raw in segments {
        let part = percent_decode_str(raw)
            .decode_utf8()
            .map_err(|_| Refusal::Invalid)?;
        // A leading dot covers `.`, `..` and hidden names alike.
        if part.is_empty()
            || part.starts_with('.')
            || part.contains(['/', '\\'])
            || part.chars().any(char::is_control)
        {
            return Err(Refusal::Invalid);
        }
        if !path.is_empty() {
            path.push('/');
        }
        path.push_str(&part);
    }
    if path.is_empty() || path.len() > MAX_PATH {
        return Err(Refusal::Invalid);
    }
    Ok(path)
}

/// Everything a fragment keeps escaped but the characters a heading or a block
/// link is made of in practice.
const FRAGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~')
    .remove(b'^');

/// The address of a note's reading page. `token` is this page load's; the
/// page sends it back with every message.
pub fn note_url(host: &str, path: &str, token: &str, fragment: Option<&str>) -> Url {
    let mut url = Url::parse(&format!("{SCHEME}://{host}/")).expect("a vault host is a valid host");
    url.path_segments_mut()
        .expect("a frame-notes URL has a path")
        .clear()
        .push("note")
        .extend(path.split('/'));
    url.query_pairs_mut().append_pair("k", token);
    if let Some(fragment) = fragment {
        url.set_fragment(Some(&utf8_percent_encode(fragment, FRAGMENT).to_string()));
    }
    url
}

/// A message from the reading page to the host.
#[derive(Debug, PartialEq)]
pub enum Act {
    /// The page's script is running and shows the rendering of `revision`
    /// that `digest` identifies.
    Ready { revision: u64, digest: String },
    /// An internal link was followed.
    Open { link: String },
    /// A link to the web was followed.
    Web { url: Url },
    /// The copy button of a code block was pressed.
    Copy { text: String },
    /// A callout was folded or unfolded; `seq` numbers the page's folds.
    Fold {
        key: String,
        collapsed: bool,
        seq: u64,
    },
    /// A task's checkbox was toggled, in the text of `revision`; `line` is the
    /// 0-based line of the task's list item.
    Task {
        line: u32,
        checked: bool,
        revision: u64,
    },
    /// Diagrams were allowed for this vault.
    AllowMermaid,
}

/// Why a URL is not an act of this page.
#[derive(Debug, PartialEq, Eq)]
pub enum ActError {
    /// Not an `/act/` address of this vault's host.
    NotAnAct,
    /// The page's token is missing or is another page's: a note's own link,
    /// or another page, sent it.
    Token,
    Unknown(String),
    Param(&'static str),
}

impl Act {
    /// The act `url` carries, if it is this page's: on `host`, with `token`.
    pub fn parse(url: &Url, host: &str, token: &str) -> Result<Self, ActError> {
        if url.scheme() != SCHEME || url.host_str() != Some(host) {
            return Err(ActError::NotAnAct);
        }
        let verb = match url.path_segments().map(|s| s.collect::<Vec<_>>()).as_deref() {
            Some(["act", verb]) => *verb,
            _ => return Err(ActError::NotAnAct),
        };
        let param = |name: &'static str| {
            url.query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.into_owned())
                .ok_or(ActError::Param(name))
        };
        if param("t").map_err(|_| ActError::Token)? != token {
            return Err(ActError::Token);
        }
        let number = |name: &'static str| {
            param(name)?
                .parse::<u64>()
                .map_err(|_| ActError::Param(name))
        };
        let flag = |name: &'static str| match param(name)?.as_str() {
            "1" => Ok(true),
            "0" => Ok(false),
            _ => Err(ActError::Param(name)),
        };
        let bounded = |name: &'static str, max: usize| {
            let value = param(name)?;
            if value.len() > max {
                return Err(ActError::Param(name));
            }
            Ok(value)
        };
        Ok(match verb {
            "ready" => Self::Ready {
                revision: number("revision")?,
                digest: bounded("digest", MAX_DIGEST)?,
            },
            "open" => Self::Open {
                link: bounded("link", MAX_LINK)?,
            },
            "web" => {
                let url = Url::parse(&bounded("url", MAX_URL)?).map_err(|_| ActError::Param("url"))?;
                if !matches!(url.scheme(), "http" | "https" | "mailto") {
                    return Err(ActError::Param("url"));
                }
                Self::Web { url }
            }
            "copy" => Self::Copy {
                text: bounded("text", MAX_COPY)?,
            },
            "fold" => Self::Fold {
                key: bounded("key", MAX_KEY)?,
                collapsed: flag("collapsed")?,
                seq: number("seq")?,
            },
            "task" => Self::Task {
                line: u32::try_from(number("line")?).map_err(|_| ActError::Param("line"))?,
                checked: flag("checked")?,
                revision: number("revision")?,
            },
            "mermaid-allow" => Self::AllowMermaid,
            other => return Err(ActError::Unknown(other.to_owned())),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    fn route(path: &str) -> Result<Route, Refusal> {
        Route::parse(&url(&format!("frame-notes://abc{path}")))
    }

    #[test]
    fn routes_name_the_five_kinds_of_request() {
        assert_eq!(
            route("/app/vendor/prismjs/prism.js"),
            Ok(Route::App("vendor/prismjs/prism.js".into()))
        );
        assert_eq!(
            route("/note/folder/Note.md?k=t0k"),
            Ok(Route::Note {
                path: "folder/Note.md".into(),
                token: "t0k".into()
            })
        );
        assert_eq!(
            route("/files/attachments/pic.png"),
            Ok(Route::File("attachments/pic.png".into()))
        );
        assert_eq!(
            route("/snippet/callouts.css"),
            Ok(Route::Snippet("callouts.css".into()))
        );
        assert_eq!(
            route("/preview?from=a%2Fb.md&link=Other%23Head"),
            Ok(Route::Preview {
                from: "a/b.md".into(),
                link: "Other#Head".into()
            })
        );
    }

    #[test]
    fn paths_are_decoded_once_and_stay_plain_names() {
        assert_eq!(
            route("/files/Unicode%20%C3%A9%20%E6%97%A5%E6%9C%AC.md"),
            Ok(Route::File("Unicode é 日本.md".into()))
        );
        // Every route that reads files refuses what could leave the vault or
        // reach its hidden folders, spelled any way a URL allows.
        for bad in [
            "/files/a%2Fb",
            "/files/a%5Cb",
            "/files/a%2f..%2fb",
            "/files/.obsidian/app.json",
            "/files/a/.hidden",
            "/files/.theorem/ids.json",
            "/files/a//b",
            "/files/",
            "/files/a%00b",
            "/files/a%0Ab",
            "/files/%FF",
            "/snippet/..%2fapp.json",
            "/snippet/a/b.css",
            "/note/.obsidian/app.json?k=x",
        ] {
            assert_eq!(route(bad), Err(Refusal::Invalid), "{bad}");
        }
        assert_eq!(
            route(&format!("/files/{}", "a/".repeat(600))),
            Err(Refusal::Invalid)
        );
    }

    #[test]
    fn dot_segments_are_resolved_by_the_url_before_a_route_sees_them() {
        // The URL parser removes `..` however it is spelled (`%2e%2e` too), so
        // the router never sees one: what would climb out of a folder lands
        // on a path that names no route, and what stays inside lands on a
        // plain name of the folder it was in.
        for climbing in [
            "/files/../secret",
            "/files/%2e%2e/secret",
            "/files/%2E%2E/secret",
            "/app/../Cargo.toml",
            "/app/%2e%2e/Cargo.toml",
        ] {
            assert_eq!(route(climbing), Err(Refusal::Unknown), "{climbing}");
        }
        assert_eq!(route("/files/a/%2e%2e/b"), Ok(Route::File("b".into())));
    }

    #[test]
    fn unknown_shapes_and_missing_parameters_are_refused() {
        assert_eq!(route("/"), Err(Refusal::Unknown));
        assert_eq!(route("/other/x"), Err(Refusal::Unknown));
        assert_eq!(route("/act/ready"), Err(Refusal::Unknown));
        assert_eq!(route("/note/a.md"), Err(Refusal::Invalid));
        assert_eq!(route("/preview?from=a.md"), Err(Refusal::Invalid));
        assert_eq!(route("/preview/x?from=a&link=b"), Err(Refusal::Unknown));
        assert_eq!(
            Route::parse(&url("https://abc/files/a.png")),
            Err(Refusal::Unknown)
        );
    }

    #[test]
    fn a_note_address_round_trips_whatever_the_file_is_called() {
        for name in [
            "Home.md",
            "a/b/c.md",
            "Unicode é 日本.md",
            "100% done?.md",
            "hash # and & plus + equals=.md",
            "quote\"'.md",
        ] {
            let address = note_url("abc", name, "tok", None);
            assert_eq!(
                Route::parse(&address),
                Ok(Route::Note {
                    path: name.into(),
                    token: "tok".into()
                }),
                "{address}"
            );
            assert_eq!(address.host_str(), Some("abc"));
        }
    }

    #[test]
    fn a_fragment_is_escaped_for_the_pages_decoder() {
        let address = note_url("abc", "a.md", "t", Some("100% done ^blk"));
        assert_eq!(address.fragment(), Some("100%25%20done%20^blk"));
        assert_eq!(
            percent_decode_str(address.fragment().unwrap())
                .decode_utf8()
                .unwrap(),
            "100% done ^blk"
        );
    }

    fn act(query: &str) -> Result<Act, ActError> {
        Act::parse(&url(&format!("frame-notes://abc/act/{query}")), "abc", "tok")
    }

    #[test]
    fn every_verb_the_page_sends_parses() {
        assert_eq!(
            act("ready?t=tok&revision=7&digest=0123abcd"),
            Ok(Act::Ready {
                revision: 7,
                digest: "0123abcd".into()
            })
        );
        assert_eq!(
            act("open?t=tok&link=Note%23Head&from=x.md&unresolved=0"),
            Ok(Act::Open {
                link: "Note#Head".into()
            })
        );
        assert_eq!(
            act("web?t=tok&url=https%3A%2F%2Fexample.com%2Fa%3Fb%3D1"),
            Ok(Act::Web {
                url: url("https://example.com/a?b=1")
            })
        );
        assert_eq!(
            act("copy?t=tok&text=fn%20main()%20%7B%7D"),
            Ok(Act::Copy {
                text: "fn main() {}".into()
            })
        );
        assert_eq!(
            act("fold?t=tok&key=note%7CTitle%230&collapsed=1&seq=3"),
            Ok(Act::Fold {
                key: "note|Title#0".into(),
                collapsed: true,
                seq: 3
            })
        );
        assert_eq!(
            act("task?t=tok&line=12&checked=0&revision=4"),
            Ok(Act::Task {
                line: 12,
                checked: false,
                revision: 4
            })
        );
        assert_eq!(act("mermaid-allow?t=tok"), Ok(Act::AllowMermaid));
    }

    #[test]
    fn an_act_needs_the_pages_own_token_and_host() {
        // A link written in a note can be followed; it cannot know the token.
        assert_eq!(act("task?line=1&checked=1&revision=1"), Err(ActError::Token));
        assert_eq!(
            act("task?t=other&line=1&checked=1&revision=1"),
            Err(ActError::Token)
        );
        assert_eq!(
            Act::parse(
                &url("frame-notes://elsewhere/act/mermaid-allow?t=tok"),
                "abc",
                "tok"
            ),
            Err(ActError::NotAnAct)
        );
        assert_eq!(
            Act::parse(&url("frame-notes://abc/note/a.md?k=tok"), "abc", "tok"),
            Err(ActError::NotAnAct)
        );
        assert_eq!(
            Act::parse(&url("https://abc/act/mermaid-allow?t=tok"), "abc", "tok"),
            Err(ActError::NotAnAct)
        );
    }

    #[test]
    fn act_parameters_are_typed_and_bounded() {
        assert_eq!(act("explode?t=tok"), Err(ActError::Unknown("explode".into())));
        assert_eq!(act("ready?t=tok"), Err(ActError::Param("revision")));
        assert_eq!(act("ready?t=tok&revision=-1"), Err(ActError::Param("revision")));
        assert_eq!(act("ready?t=tok&revision=1"), Err(ActError::Param("digest")));
        let long = "a".repeat(MAX_DIGEST + 1);
        assert_eq!(
            act(&format!("ready?t=tok&revision=1&digest={long}")),
            Err(ActError::Param("digest"))
        );
        assert_eq!(
            act("task?t=tok&line=99999999999&checked=1&revision=1"),
            Err(ActError::Param("line"))
        );
        assert_eq!(
            act("task?t=tok&line=1&checked=yes&revision=1"),
            Err(ActError::Param("checked"))
        );
        // The web verb opens http(s) and mail only.
        for scheme in ["javascript:alert(1)", "file:///etc/passwd", "frame-notes://abc/act/ready"] {
            let query = format!("web?t=tok&url={}", utf8_percent_encode(scheme, NON_ALPHANUMERIC));
            assert_eq!(act(&query), Err(ActError::Param("url")), "{scheme}");
        }
        let long = "a".repeat(MAX_COPY + 1);
        assert_eq!(act(&format!("copy?t=tok&text={long}")), Err(ActError::Param("text")));
    }
}
