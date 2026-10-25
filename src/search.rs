//! RustyWeb search service for Frame.
//!
//! `init` opens the durable RedCore session store once and installs it as a
//! GPUI `Global`. `run` performs one search on a background thread and returns
//! Frame-owned plain data ([`SerpResult`]); nothing from `rustyred-web` crosses
//! this module's public boundary.
//!
//! `capture` adds a page the person finished loading to the same store, so the
//! local lanes search what they have read; the next search indexes it.
//!
//! Providers are passed to the async `programmable_search` explicitly, so the
//! process environment is only read (never written). Each search builds its own
//! single-threaded tokio runtime, the same way the substrate's blocking entry
//! point does, because reqwest-based providers need one and GPUI's executor
//! does not provide it.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Instant,
};

use anyhow::{Context as _, anyhow};
use chrono::{DateTime, Local};
use gpui_kit::{App, AppContext as _, Global, Task};
use rustyred_web::{
    SearchProvider,
    programmable::{ProgrammableQueryOptions, SearchSession},
    providers::configured_search_provider_from_env,
    query::EvidenceHit,
};
use theorem_browser_substrate::{
    BrowserCapturePolicy, LoadedPage, RedCoreBrowserSessionStore, durable_browser_session,
    memory_browser_session,
};
use url::Url;

use crate::markup::{self, Marked};

/// Identity of the local browser profile inside the RedCore store.
const SESSION_ID: &str = "frame";

/// Providers that need no key. They are always on unless the environment names
/// an explicit list.
const KEYLESS_PROVIDERS: &[&str] = &["gdelt", "bluesky"];

/// Keyed providers that search the general web. Each is used only when its key
/// is present. `github` and `gitlab` are code-search providers and are left out
/// of the default set (they stay selectable through `RUSTYWEB_SEARCH_PROVIDERS`).
const KEYED_PROVIDERS: &[&str] = &[
    "brave",
    "mojeek",
    "exa",
    "serpapi",
    "perplexity",
    "firecrawl",
    "ourrs",
    "searxng",
    "youtube",
];

/// Providers whose results are general web pages. Without one of these, search
/// reaches only news and social sources.
const GENERAL_WEB_PROVIDERS: &[&str] = &[
    "brave",
    "mojeek",
    "exa",
    "serpapi",
    "perplexity",
    "firecrawl",
    "ourrs",
    "owi",
    "searxng",
];

const PROVIDER_ENV_VARS: &[&str] = &["RUSTYWEB_SEARCH_PROVIDERS", "RUSTY_RED_SEARCH_PROVIDERS"];
const SYNTHESIS_ENV_VAR: &str = "THEOREM_WEB_SYNTHESIS_CONFIG";

/// One search result, ready to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct SerpHit {
    pub title: String,
    pub url: Url,
    /// Host without a leading `www.`.
    pub host: String,
    /// The provider's excerpt as text, with the words it emphasized (the
    /// ones that matched). Markup never reaches the page.
    pub snippet: Marked,
    pub providers: Vec<String>,
    /// `warm`, `fresh` or `federated`, as reported by RustyWeb.
    pub lane: String,
    /// `YYYY-MM-DD` when the provider date could be read, otherwise as given.
    pub published_at: Option<String>,
}

/// A page an answer claim rests on.
#[derive(Clone, Debug, PartialEq)]
pub struct SerpSource {
    pub title: String,
    pub url: Url,
    pub host: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SerpClaim {
    pub text: String,
    pub disputed: bool,
    pub sources: Vec<SerpSource>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SerpAnswer {
    pub claims: Vec<SerpClaim>,
    pub model: String,
}

/// Something that went wrong without failing the search.
#[derive(Clone, Debug, PartialEq)]
pub struct Degradation {
    /// The RustyWeb code, for logs and the example harness.
    pub code: String,
    /// Sentence-case text for the person reading the page.
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SerpResult {
    pub query: String,
    /// True for the explicit AI path.
    pub ask: bool,
    /// Providers that were queried.
    pub providers: Vec<String>,
    /// Local lanes first, then federated and fresh, each in RustyWeb order.
    pub hits: Vec<SerpHit>,
    pub degradations: Vec<Degradation>,
    /// Present only for `ask` searches that produced claims.
    pub answer: Option<SerpAnswer>,
    /// Present only for `ask` searches without an answer: why there is none.
    pub answer_note: Option<String>,
    /// Quiet context for the page footer, such as weak general web coverage.
    pub notes: Vec<String>,
    /// When the search started: the retrieval time of every live result.
    pub searched_at: DateTime<Local>,
    pub elapsed_ms: u64,
}

/// The installed search service.
#[derive(Clone)]
struct SearchService {
    store: Arc<Mutex<RedCoreBrowserSessionStore>>,
    /// Provider names to build for each search (keys already checked at init).
    providers: Arc<[String]>,
    /// Set when the durable store could not be opened and search runs in memory.
    store_note: Option<Arc<str>>,
}

impl Global for SearchService {}

/// Open the durable session store under `data_dir` and install the service.
///
/// Call after the `.env` file is loaded and after `gpui_kit::init`. If the
/// store cannot be opened, search still works from an in-memory store and every
/// result carries a note saying history is not being saved.
pub fn init(data_dir: &std::path::Path, cx: &mut App) {
    let (store, store_note) = match durable_browser_session(data_dir, SESSION_ID) {
        Ok(store) => (store, None),
        Err(error) => {
            log::error!(
                "search: could not open the session store in {}: {error:?}",
                data_dir.display()
            );
            (
                memory_browser_session(SESSION_ID),
                Some(Arc::from(
                    "Search history is not being saved because the history store could not be opened.",
                )),
            )
        }
    };

    let providers = select_providers();
    log::info!("search: providers {providers:?}");
    cx.set_global(SearchService {
        store: Arc::new(Mutex::new(store)),
        providers: providers.into(),
        store_note,
    });
}

/// Run one search off the main thread.
///
/// `ask` is the explicit AI path. A plain search never shows an answer.
pub fn run(query: String, ask: bool, cx: &mut App) -> Task<anyhow::Result<SerpResult>> {
    let Some(service) = cx.try_global::<SearchService>().cloned() else {
        return Task::ready(Err(anyhow!(
            "Search is not ready yet. Restart Frame and try again."
        )));
    };
    cx.background_spawn(async move { service.search(&query, ask) })
}

/// A web page as the person saw it once it finished loading.
pub struct VisitedPage {
    pub url: Url,
    /// The rendered document's markup, after its scripts ran.
    pub html: String,
}

/// Whether a page at `url` belongs in the local index: web pages only, not
/// files, `about:` pages or data URLs.
pub fn capturable(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

/// Add a visited page to the local index off the main thread.
pub fn capture(page: VisitedPage, cx: &mut App) {
    let Some(service) = cx.try_global::<SearchService>().cloned() else {
        return;
    };
    cx.background_spawn(async move {
        if let Err(error) = service.capture(page) {
            log::warn!("search: {error:#}");
        }
    })
    .detach();
}

impl SearchService {
    fn capture(&self, page: VisitedPage) -> anyhow::Result<()> {
        if !capturable(&page.url) {
            return Ok(());
        }
        let mut store = self
            .store
            .lock()
            .map_err(|_| anyhow!("the search store stopped after an earlier failure"))?;
        // Servo does not report the response status; a page that finished
        // loading is kept as the person saw it.
        let loaded = LoadedPage::html(page.url.as_str(), page.html);
        store
            .capture_loaded_page_with_policy(loaded, &BrowserCapturePolicy::default())
            .map_err(|error| anyhow!("could not keep {}: {error}", page.url))?;
        Ok(())
    }

    fn search(&self, query: &str, ask: bool) -> anyhow::Result<SerpResult> {
        let started = Instant::now();
        let searched_at = Local::now();
        let providers: Vec<Arc<dyn SearchProvider>> = self
            .providers
            .iter()
            .filter_map(|name| configured_search_provider_from_env(name))
            .collect();
        let used: Vec<String> = providers.iter().map(|p| p.name().to_string()).collect();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("could not start the search runtime")?;
        let mut store = self.store.lock().map_err(|_| {
            anyhow!("The search store stopped after an earlier failure. Restart Frame.")
        })?;
        let session = runtime
            .block_on(store.programmable_search(
                query,
                // Only Ask runs a model; RustyWeb plans no answer otherwise.
                ProgrammableQueryOptions {
                    synthesize: ask,
                    ..Default::default()
                },
                &providers,
            ))
            .map_err(|error| anyhow!("{} ({})", error.message, error.code))?;
        drop(store);

        let mut result = map_session(&session, ask, used, searched_at);
        result
            .notes
            .extend(self.store_note.iter().map(|n| n.to_string()));
        result.elapsed_ms = started.elapsed().as_millis() as u64;
        Ok(result)
    }
}

/// Provider names to build for each search.
///
/// An explicit `RUSTYWEB_SEARCH_PROVIDERS` (or `RUSTY_RED_SEARCH_PROVIDERS`)
/// list wins. Otherwise: the keyless providers plus every keyed general-web
/// provider whose key is present.
fn select_providers() -> Vec<String> {
    let explicit = PROVIDER_ENV_VARS
        .iter()
        .filter_map(|key| std::env::var(key).ok())
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty());

    let candidates: Vec<String> = match explicit {
        Some(list) => list
            .split(',')
            .map(|name| name.trim().to_ascii_lowercase())
            .filter(|name| !name.is_empty())
            .collect(),
        None => KEYLESS_PROVIDERS
            .iter()
            .chain(KEYED_PROVIDERS)
            .map(|name| name.to_string())
            .collect(),
    };

    let mut seen = HashSet::new();
    let selected: Vec<String> = candidates
        .into_iter()
        .filter(|name| seen.insert(name.clone()))
        .filter(|name| configured_search_provider_from_env(name).is_some())
        .collect();

    if selected.is_empty() {
        // An explicit list whose providers all lack keys would leave nothing to
        // search with. Fall back to the keyless pair.
        log::warn!("search: no configured provider is usable; falling back to gdelt and bluesky");
        return KEYLESS_PROVIDERS.iter().map(|n| n.to_string()).collect();
    }
    selected
}

/// Map one finished RustyWeb session to Frame-owned data.
fn map_session(
    session: &SearchSession,
    ask: bool,
    providers: Vec<String>,
    searched_at: DateTime<Local>,
) -> SerpResult {
    let receipt = &session.response.receipt;
    let hits = map_hits(&session.response.hits);

    // Why each provider failed, from its receipt: the degradation code alone
    // only says that it did.
    let provider_errors: Vec<(String, String)> = session
        .response
        .acquisition
        .iter()
        .flat_map(|acquisition| &acquisition.providers)
        .filter_map(|r| Some((r.provider.clone(), r.error.clone()?)))
        .collect();
    let degradations = receipt
        .degraded
        .iter()
        .filter(|code| !is_synthesis_code(code))
        .map(|code| Degradation {
            code: code.clone(),
            message: describe_degradation(code, &provider_errors),
        })
        .collect();

    let (answer, answer_note) = if ask {
        map_answer(session)
    } else {
        (None, None)
    };

    let mut notes = Vec::new();
    if let Some(note) = weak_web_note(&providers) {
        notes.push(note);
    }

    SerpResult {
        query: session.request.query.clone(),
        ask,
        providers,
        hits,
        degradations,
        answer,
        answer_note,
        notes,
        searched_at,
        elapsed_ms: 0,
    }
}

fn map_answer(session: &SearchSession) -> (Option<SerpAnswer>, Option<String>) {
    let claims: Vec<SerpClaim> = session
        .synthesis
        .iter()
        .flat_map(|artifact| &artifact.claims)
        .map(|claim| SerpClaim {
            text: claim.text.trim().to_string(),
            disputed: claim.disputed,
            sources: claim
                .evidence_ids
                .iter()
                .filter_map(|id| {
                    session
                        .evidence
                        .evidence
                        .iter()
                        .find(|source| &source.evidence_id == id)
                })
                .filter_map(|source| {
                    let url = openable_url(&source.url)?;
                    let title = markup::parse(&source.title).text;
                    Some(SerpSource {
                        host: display_host(&url),
                        title: if title.is_empty() {
                            display_host(&url)
                        } else {
                            title
                        },
                        url,
                    })
                })
                .fold(Vec::<SerpSource>::new(), |mut unique, source| {
                    if !unique.iter().any(|seen| seen.url == source.url) {
                        unique.push(source);
                    }
                    unique
                }),
        })
        .filter(|claim| !claim.text.is_empty())
        .collect();

    if !claims.is_empty() {
        let model = session
            .synthesis
            .as_ref()
            .map(|artifact| artifact.model.clone())
            .unwrap_or_default();
        return (Some(SerpAnswer { claims, model }), None);
    }

    (None, Some(answer_note(&session.response.receipt.degraded)))
}

/// Why an Ask search has no answer, in words that say what to do next.
fn answer_note(codes: &[String]) -> String {
    let has = |code: &str| codes.iter().any(|c| c == code);
    if has("synthesis_model_unavailable") {
        return format!(
            "Answers need a model. Set {SYNTHESIS_ENV_VAR} to a model config file, then restart Frame."
        );
    }
    if has("synthesis_model_configuration_invalid") {
        return format!(
            "The model config at {SYNTHESIS_ENV_VAR} could not be loaded. Check the path and the file, then restart Frame."
        );
    }
    if has("synthesis_evidence_minimum_unmet") {
        return "There is not enough captured page text to answer with citations yet. Open a few of these results so Frame can capture them, then ask again.".to_string();
    }
    if let Some(code) = codes.iter().find(|code| is_synthesis_code(code)) {
        return format!("The answer could not be generated ({}).", humanize(code));
    }
    "No answer could be drawn from these results.".to_string()
}

fn is_synthesis_code(code: &str) -> bool {
    code.starts_with("synthesis_")
}

/// Hits in display order: local lanes first, then the rest, each keeping the
/// order RustyWeb ranked them in. Hits without an openable URL are dropped, and
/// so are repeats of the same URL.
fn map_hits(hits: &[EvidenceHit]) -> Vec<SerpHit> {
    let mut seen = HashSet::new();
    let mut mapped: Vec<SerpHit> = hits
        .iter()
        .filter_map(|hit| {
            let url = openable_url(&hit.url)?;
            if !seen.insert(url.as_str().to_string()) {
                return None;
            }
            let host = display_host(&url);
            // Providers mark titles up too, and some send the URL as the title.
            let title = markup::parse(&hit.title).text;
            Some(SerpHit {
                title: if title.is_empty() || title == url.as_str() {
                    host.clone()
                } else {
                    title
                },
                host,
                url,
                snippet: markup::parse(&hit.snippet),
                providers: hit.providers.clone(),
                lane: hit.lane.clone(),
                published_at: hit
                    .published_at
                    .as_deref()
                    .map(normalize_date)
                    .filter(|date| !date.is_empty()),
            })
        })
        .collect();
    mapped.sort_by_key(|hit| !is_local_lane(&hit.lane));
    mapped
}

/// `warm` (the local index) and `my_web` are local; everything else is fetched.
pub fn is_local_lane(lane: &str) -> bool {
    lane.starts_with("warm") || lane == "my_web"
}

/// Short label for a lane: Local, Web, or what RustyWeb called it.
pub fn lane_label(lane: &str) -> String {
    if is_local_lane(lane) {
        return "Local".to_string();
    }
    match lane {
        // Fetched from the providers during this search.
        "fresh" => "Web".to_string(),
        "federated" => "Federated".to_string(),
        other => capitalize(&humanize(other)),
    }
}

/// Where a lane's results come from, for a result's details.
pub fn lane_origin(lane: &str) -> String {
    if is_local_lane(lane) {
        return "Frame's local index, from pages you opened".to_string();
    }
    match lane {
        "fresh" => "The web, through the search providers".to_string(),
        "federated" => "An index RustyWeb searches with".to_string(),
        other => capitalize(&humanize(other)),
    }
}

fn openable_url(raw: &str) -> Option<Url> {
    let url = Url::parse(raw.trim()).ok()?;
    matches!(url.scheme(), "http" | "https").then_some(url)
}

fn display_host(url: &Url) -> String {
    url.host_str()
        .map(|host| host.strip_prefix("www.").unwrap_or(host))
        .unwrap_or_else(|| url.scheme())
        .to_string()
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `2026-10-24T09:30:00Z` and `20261024T093000Z` become `2026-10-24`. Anything
/// else is returned as given.
fn normalize_date(raw: &str) -> String {
    let text = raw.trim();
    let bytes = text.as_bytes();
    let digits = |range: std::ops::Range<usize>| {
        bytes
            .get(range)
            .is_some_and(|b| b.iter().all(u8::is_ascii_digit))
    };
    if digits(0..4)
        && bytes.get(4) == Some(&b'-')
        && digits(5..7)
        && bytes.get(7) == Some(&b'-')
        && digits(8..10)
    {
        return text[..10].to_string();
    }
    if digits(0..8) && matches!(bytes.get(8), None | Some(b'T')) {
        return format!("{}-{}-{}", &text[..4], &text[4..6], &text[6..8]);
    }
    text.to_string()
}

/// A note when only news and social sources are searched.
fn weak_web_note(providers: &[String]) -> Option<String> {
    let general = providers
        .iter()
        .any(|name| GENERAL_WEB_PROVIDERS.contains(&name.as_str()));
    (!general).then(|| {
        "Only news and social sources are searched. Add a search API key (Brave, Exa, Mojeek, Firecrawl, SerpAPI or Perplexity) to .env and restart Frame for general web results.".to_string()
    })
}

/// Sentence-case text for a RustyWeb degradation code. `provider_errors` pairs
/// a provider name with the error its receipt reported.
fn describe_degradation(code: &str, provider_errors: &[(String, String)]) -> String {
    match code {
        "warm_index_unavailable" => "The local index is not available, so only live results are shown.".to_string(),
        "warm_index_shadow" => "Local search is not ready yet. It fills with the pages you open; until then only live results are shown.".to_string(),
        "dense_query_embedder_configuration_invalid" => {
            "Semantic ranking is off because the embedding model is not configured correctly.".to_string()
        }
        "dense_query_contract_mismatch" => "Semantic ranking is off because the embedding model does not match the index.".to_string(),
        _ => match code.strip_prefix("provider_").and_then(|rest| rest.split_once(':')) {
            Some((status, provider)) => {
                match provider_errors.iter().find(|(name, _)| name == provider) {
                    Some((_, error)) => {
                        let error: String = collapse_whitespace(error).chars().take(120).collect();
                        format!("{provider} did not answer: {}", sentence(&error))
                    }
                    None => format!("{provider} did not answer ({}).", humanize(status)),
                }
            }
            None => capitalize(&humanize(code)),
        },
    }
}

fn humanize(code: &str) -> String {
    code.replace(['_', ':'], " ")
}

/// `text` ending in a full stop, so it reads before a following sentence.
pub fn sentence(text: &str) -> String {
    let text = text.trim();
    if text.ends_with(['.', '!', '?']) {
        text.to_string()
    } else {
        format!("{text}.")
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hit(url: &str, lane: &str, extra: serde_json::Value) -> EvidenceHit {
        let mut value = json!({
            "url": url,
            "title": "A title",
            "snippet": "A  snippet\nwith   gaps",
            "providers": ["gdelt"],
            "lane": lane,
            "score": 1.0,
        });
        value
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().cloned().unwrap_or_default());
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn local_lanes_come_first_and_keep_their_order() {
        let hits = [
            hit("https://a.example/1", "fresh", json!({})),
            hit("https://b.example/2", "warm", json!({})),
            hit("https://c.example/3", "federated", json!({})),
            hit("https://d.example/4", "warm", json!({})),
        ];
        let urls: Vec<_> = map_hits(&hits).iter().map(|h| h.url.to_string()).collect();
        assert_eq!(
            urls,
            [
                "https://b.example/2",
                "https://d.example/4",
                "https://a.example/1",
                "https://c.example/3"
            ]
        );
    }

    #[test]
    fn unopenable_and_repeated_urls_are_dropped() {
        let hits = [
            hit("javascript:alert(1)", "fresh", json!({})),
            hit("not a url", "fresh", json!({})),
            hit("https://a.example/1", "fresh", json!({})),
            hit("https://a.example/1", "warm", json!({})),
        ];
        let mapped = map_hits(&hits);
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].lane, "fresh");
    }

    #[test]
    fn hit_fields_are_cleaned() {
        let mapped = map_hits(&[hit(
            "https://www.example.com/post",
            "fresh",
            json!({"title": "  ", "published_at": "20261024T093000Z"}),
        )]);
        let hit = &mapped[0];
        assert_eq!(hit.host, "example.com");
        assert_eq!(hit.title, "example.com");
        assert_eq!(hit.snippet.text, "A snippet with gaps");
        assert_eq!(hit.published_at.as_deref(), Some("2026-10-24"));
    }

    #[test]
    fn markup_in_titles_and_snippets_becomes_text() {
        let mapped = map_hits(&[hit(
            "https://example.com/tides",
            "fresh",
            json!({
                "title": "<b>Tides</b> &amp; the moon",
                "snippet": "Why the <strong>moon</strong> pulls<script>x()</script>",
            }),
        )]);
        let hit = &mapped[0];
        assert_eq!(hit.title, "Tides & the moon");
        assert_eq!(hit.snippet.text, "Why the moon pulls");
        assert_eq!(&hit.snippet.text[hit.snippet.emphasis[0].clone()], "moon");
    }

    #[test]
    fn dates_are_normalized_or_left_alone() {
        assert_eq!(normalize_date("2026-10-24T09:30:00Z"), "2026-10-24");
        assert_eq!(normalize_date("2026-10-24"), "2026-10-24");
        assert_eq!(normalize_date("20261024"), "2026-10-24");
        assert_eq!(normalize_date("Oct 24, 2026"), "Oct 24, 2026");
        assert_eq!(normalize_date(""), "");
    }

    #[test]
    fn lane_labels() {
        assert_eq!(lane_label("warm"), "Local");
        assert_eq!(lane_label("warm_lexical"), "Local");
        assert_eq!(lane_label("fresh"), "Web");
        assert_eq!(lane_label("federated"), "Federated");
        assert_eq!(lane_label("my_web"), "Local");
        assert_eq!(lane_label("something_new"), "Something new");
    }

    #[test]
    fn degradation_text_names_the_provider() {
        assert_eq!(
            describe_degradation("provider_error:brave", &[]),
            "brave did not answer (error)."
        );
        assert_eq!(
            describe_degradation("provider_rate_limited:exa", &[]),
            "exa did not answer (rate limited)."
        );
        assert_eq!(describe_degradation("something_odd", &[]), "Something odd");
    }

    #[test]
    fn degradation_text_uses_the_receipt_error_when_there_is_one() {
        let errors = vec![("bluesky".to_string(), "HTTP 403 Forbidden".to_string())];
        assert_eq!(
            describe_degradation("provider_error:bluesky", &errors),
            "bluesky did not answer: HTTP 403 Forbidden."
        );
        // Another provider's error is not borrowed.
        assert_eq!(
            describe_degradation("provider_error:gdelt", &errors),
            "gdelt did not answer (error)."
        );
    }

    #[test]
    fn answer_note_says_what_to_do() {
        let codes = |c: &[&str]| c.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(
            answer_note(&codes(&["synthesis_model_unavailable"]))
                .contains("THEOREM_WEB_SYNTHESIS_CONFIG")
        );
        assert!(
            answer_note(&codes(&["synthesis_model_configuration_invalid"]))
                .contains("could not be loaded")
        );
        assert!(
            answer_note(&codes(&["synthesis_evidence_minimum_unmet"]))
                .contains("not enough captured")
        );
        assert!(
            answer_note(&codes(&["synthesis_something_new"])).contains("synthesis something new")
        );
        assert_eq!(
            answer_note(&[]),
            "No answer could be drawn from these results."
        );
    }

    const TIDES: &str = "<html><head><title>Why the moon pulls the tides</title></head>\
        <body><p>The moon's gravity pulls the ocean toward it, raising a bulge of water.</p></body></html>";

    #[test]
    fn a_visited_page_is_found_by_local_search() {
        let service = SearchService {
            store: Arc::new(Mutex::new(memory_browser_session(SESSION_ID))),
            providers: Arc::from(Vec::new()),
            store_note: None,
        };
        let query = "why the moon pulls the tides";
        let before = service.search(query, false).unwrap();
        assert!(
            before
                .degradations
                .iter()
                .any(|d| d.code == "warm_index_shadow")
        );

        for url in ["https://example.com/tides", "file:///Users/me/tides.html"] {
            let url = Url::parse(url).unwrap();
            service
                .capture(VisitedPage {
                    url,
                    html: TIDES.into(),
                })
                .unwrap();
        }

        let after = service.search(query, false).unwrap();
        assert!(
            !after
                .degradations
                .iter()
                .any(|d| d.code.starts_with("warm_index"))
        );
        let local: Vec<_> = after
            .hits
            .iter()
            .filter(|hit| is_local_lane(&hit.lane))
            .map(|hit| hit.url.as_str())
            .collect();
        assert_eq!(local, ["https://example.com/tides"]);
    }

    #[test]
    fn sentence_adds_a_full_stop_once() {
        assert_eq!(sentence("It broke"), "It broke.");
        assert_eq!(sentence("It broke."), "It broke.");
        assert_eq!(sentence(" Why? "), "Why?");
    }

    #[test]
    fn weak_web_note_depends_on_keyed_providers() {
        let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(weak_web_note(&names(&["gdelt", "bluesky"])).is_some());
        assert!(weak_web_note(&names(&["gdelt", "brave"])).is_none());
    }

    #[test]
    fn synthesis_codes_are_kept_out_of_the_footer() {
        assert!(is_synthesis_code("synthesis_model_unavailable"));
        assert!(!is_synthesis_code("provider_error:brave"));
    }
}
