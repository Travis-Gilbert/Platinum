//! What the person typed into the address bar or the navigator: a place to
//! go, or something to search for.
//!
//! SOURCING: `url` parses; deciding which text is an address is this app's
//! rule. A public-suffix list would be stricter about hosts, but a dotted
//! word without spaces is what every browser treats as an address.

use url::Url;

#[derive(Debug, PartialEq)]
pub enum Target {
    Go(Url),
    Search(String),
}

const SCHEMES: &[&str] = &["http", "https", "file", "about", "data"];

pub fn classify(text: &str) -> Option<Target> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Some(query) = text.strip_prefix('?') {
        return Some(Target::Search(query.trim().to_owned()));
    }
    if text.contains(char::is_whitespace) {
        return Some(Target::Search(text.to_owned()));
    }
    if let Ok(url) = Url::parse(text)
        && SCHEMES.contains(&url.scheme())
    {
        return Some(Target::Go(url));
    }
    if looks_like_host(text) {
        let scheme = if is_local(text) { "http" } else { "https" };
        if let Ok(url) = Url::parse(&format!("{scheme}://{text}")) {
            return Some(Target::Go(url));
        }
    }
    Some(Target::Search(text.to_owned()))
}

fn host_part(text: &str) -> &str {
    let end = text.find(['/', '?', '#']).unwrap_or(text.len());
    let authority = &text[..end];
    match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => host,
        _ => authority,
    }
}

fn is_local(text: &str) -> bool {
    let host = host_part(text);
    let literal = host.trim_start_matches('[').trim_end_matches(']');
    host == "localhost"
        || host.ends_with(".localhost")
        || literal.parse::<std::net::IpAddr>().is_ok()
}

fn looks_like_host(text: &str) -> bool {
    let host = host_part(text);
    if is_local(text) {
        return true;
    }
    let Some((_, tld)) = host.rsplit_once('.') else {
        return false;
    };
    tld.len() >= 2
        && tld.chars().all(|c| c.is_ascii_alphabetic())
        && host.split('.').all(|label| {
            !label.is_empty() && label.chars().all(|c| c.is_alphanumeric() || c == '-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn go(text: &str) -> String {
        match classify(text) {
            Some(Target::Go(url)) => url.to_string(),
            other => panic!("{text}: {other:?}"),
        }
    }

    fn search(text: &str) -> String {
        match classify(text) {
            Some(Target::Search(query)) => query,
            other => panic!("{text}: {other:?}"),
        }
    }

    #[test]
    fn addresses_open() {
        assert_eq!(go("https://servo.org"), "https://servo.org/");
        assert_eq!(go("servo.org"), "https://servo.org/");
        assert_eq!(
            go("en.wikipedia.org/wiki/Servo"),
            "https://en.wikipedia.org/wiki/Servo"
        );
        assert_eq!(go("localhost:3000"), "http://localhost:3000/");
        assert_eq!(go("127.0.0.1:8080/x"), "http://127.0.0.1:8080/x");
        assert_eq!(go("about:blank"), "about:blank");
        assert_eq!(go("file:///tmp/a.html"), "file:///tmp/a.html");
    }

    #[test]
    fn everything_else_searches() {
        assert_eq!(search("rust gpui servo"), "rust gpui servo");
        assert_eq!(search("servo"), "servo");
        assert_eq!(search("v0.7"), "v0.7");
        assert_eq!(search("?servo.org"), "servo.org");
        assert_eq!(search("javascript:alert(1)"), "javascript:alert(1)");
    }

    #[test]
    fn nothing_typed_is_nothing() {
        assert_eq!(classify("   "), None);
    }
}

/// The result's action is explicit; Ask never participates in suggestions.
#[derive(Clone, Debug, PartialEq)]
pub enum SuggestionAction {
    Visit(Url),
    Search(String),
    SwitchTab(u64),
}
#[derive(Clone, Debug)]
pub struct Suggestion {
    pub title: String,
    pub detail: String,
    pub action: SuggestionAction,
}
impl Suggestion {
    pub fn primary(text: &str) -> Option<Self> {
        match classify(text)? {
            Target::Go(url) => Some(Self {
                title: format!("Visit {}", url),
                detail: url.to_string(),
                action: SuggestionAction::Visit(url),
            }),
            Target::Search(query) => Some(Self {
                title: format!("Search the web for {query}"),
                detail: "RustyWeb".into(),
                action: SuggestionAction::Search(query),
            }),
        }
    }
    pub fn library(entry: frame_profile::LibraryEntry) -> Option<Self> {
        let url = Url::parse(&entry.url).ok()?;
        let kind = if entry.folder.is_empty() {
            "History"
        } else {
            "Bookmark"
        };
        let title = if entry.title.is_empty() {
            entry.url.clone()
        } else {
            entry.title
        };
        Some(Self {
            title: format!("Visit · {kind} · {title}"),
            detail: entry.url,
            action: SuggestionAction::Visit(url),
        })
    }
}

/// Primary navigation/search stays first; a known tab wins over another visit
/// to the same address. Preserve distinct tabs even when their URLs match.
pub fn append_library(
    results: &mut Vec<Suggestion>,
    entries: impl IntoIterator<Item = frame_profile::LibraryEntry>,
) {
    for entry in entries {
        if results.iter().any(|s| s.detail == entry.url) {
            continue;
        }
        if let Some(suggestion) = Suggestion::library(entry) {
            results.push(suggestion);
        }
        if results.len() >= 10 {
            break;
        }
    }
}

#[cfg(test)]
mod suggestion_tests {
    use super::*;
    #[test]
    fn exact_address_stays_first_and_visits_are_deduplicated() {
        let mut entries = vec![Suggestion::primary("servo.org").unwrap()];
        append_library(
            &mut entries,
            [frame_profile::LibraryEntry {
                url: "https://servo.org/".into(),
                title: "Servo".into(),
                folder: "Bookmarks".into(),
            }],
        );
        assert_eq!(entries.len(), 1);
        assert!(
            matches!(&entries[0].action,SuggestionAction::Visit(url) if url.as_str()=="https://servo.org/")
        );
        assert!(matches!(
            Suggestion::primary("?servo.org").unwrap().action,
            SuggestionAction::Search(_)
        ));
    }
}
