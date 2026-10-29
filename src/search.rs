//! RustyWeb search service for Platinum.
//!
//! `init` opens the durable RedCore session store once and installs it as a
//! GPUI `Global`. `run` performs one search on a background thread and returns
//! Platinum-owned plain data ([`SerpResult`]); nothing from `rustyred-web` crosses
//! this module's public boundary.
//!
//! `capture` adds a page the person finished loading to the same store, so the
//! local lanes search what they have read; the next search indexes it. A
//! worker thread embeds each captured page with the shared EmbeddingGemma 2
//! model ([`crate::embedder`]), so the dense lane finds pages by meaning too.
//!
//! After each search a crawl campaign fetches the results the store holds no
//! content for, as many as the query's intent warrants, and adds them to the
//! same store, so the next search on the topic finds them locally. Campaigns
//! run one at a time on their own thread, with the store unlocked while pages
//! download.
//!
//! Each search waits for providers for at most four seconds, or until 100
//! results are in, whichever comes first. The fused results are reranked on
//! OpenRouter when its key is in the login Keychain, and Platinum's rank profile
//! weighs Bluesky down. Searches for images, video or attachments
//! (`SearchOpts::media`) are reranked by a model that also reads their
//! pictures; nothing in Platinum searches media yet.
//!
//! Providers are passed to the async `programmable_search` explicitly, so the
//! process environment is only read (never written). They are built once, and
//! every search runs on one shared tokio runtime, because reqwest-based
//! providers need one and GPUI's executor does not provide it. Building a
//! provider's HTTP client costs up to seconds, and a runtime that outlives each
//! search keeps provider connections open for the next.

use std::{
    collections::HashSet,
    sync::{Arc, LazyLock, Mutex, mpsc},
    time::Instant,
};

use anyhow::{Context as _, anyhow};
use gpui_kit::{App, AppContext as _, Global, Task};
use rustyred_web::{
    COHERE_RERANK_4_PRO, NEMOTRON_RERANK_VL, OpenRouterReranker, Reranker, SearchOpts,
    SearchProvider, VOYAGE_RERANK_3_LITE,
    programmable::{ProfileRef, ProgrammableQueryOptions, RankProfile, RankRule, SearchSession},
    providers::configured_search_provider_from_env,
    query::EvidenceHit,
};
use theorem_browser_substrate::{
    BrowserCapturePolicy, LiveFetchOptions, LoadedPage, RedCoreBrowserSessionStore,
    durable_browser_session, fetch_campaign_pages, memory_browser_session,
};
use url::Url;

/// Identity of the local browser profile inside the RedCore store.
const SESSION_ID: &str = "frame";

/// Providers that need no key. They are always on unless the environment names
/// an explicit list.
const KEYLESS_PROVIDERS: &[&str] = &["bluesky"];

/// How far Platinum's rank profile weighs Bluesky down: a post's score is scaled
/// by 1 - 0.6, so a strong match can still surface.
const BLUESKY_WEIGHT: f64 = -0.6;

/// Login Keychain item holding the OpenRouter key the rerankers use.
const OPENROUTER_KEYCHAIN_SERVICE: &str = "theorem-openrouter";

/// Visited pages embedded per store visit. The store is unlocked while they
/// are embedded.
const EMBED_BATCH: usize = 8;

/// Searches waiting for their crawl campaign. A search made while this many
/// are waiting gets none.
const CAMPAIGN_QUEUE: usize = 4;

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
    pub snippet: String,
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
    pub elapsed_ms: u64,
}

/// The installed search service.
#[derive(Clone)]
struct SearchService {
    store: Arc<Mutex<RedCoreBrowserSessionStore>>,
    /// The providers every search queries.
    providers: Arc<[Arc<dyn SearchProvider>]>,
    /// Set when the durable store could not be opened and search runs in memory.
    store_note: Option<Arc<str>>,
    /// Platinum's rank profile in the store, when it could be kept there.
    profile: Option<ProfileRef>,
    /// Tells the embedding worker a page was captured.
    embed_wake: Option<mpsc::Sender<()>>,
    /// Hands finished searches to the crawl campaign worker.
    campaigns: Option<mpsc::SyncSender<SearchSession>>,
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

    let mut store = store;
    store.set_search_opts(search_opts());
    let profile = match store.adopt_rank_profile(&frame_profile()) {
        Ok(profile) => Some(profile),
        Err(error) => {
            log::warn!(
                "search: Platinum's rank profile could not be kept, so Bluesky is not weighed down: {} ({})",
                error.message,
                error.code
            );
            None
        }
    };
    match openrouter_rerankers() {
        Ok(None) => {
            log::info!("search: no OpenRouter key in the Keychain, so results are not reranked")
        }
        Ok(Some((rerankers, media))) => {
            store.set_rerankers(rerankers);
            store.set_media_rerankers(media);
        }
        Err(error) => log::warn!("search: results are not reranked: {error:#}"),
    }
    let store = Arc::new(Mutex::new(store));

    let (embed_wake, woken) = mpsc::channel();
    let worker_store = store.clone();
    let embed_wake = std::thread::Builder::new()
        .name("frame-embedder".into())
        .spawn(move || embedding_worker(worker_store, woken))
        .map_err(|error| log::warn!("search: semantic ranking is off: {error}"))
        .ok()
        .map(|_| embed_wake);

    let (campaigns, searches) = mpsc::sync_channel(CAMPAIGN_QUEUE);
    let crawler_store = store.clone();
    let crawler_wake = embed_wake.clone();
    let campaigns = std::thread::Builder::new()
        .name("frame-crawler".into())
        .spawn(move || campaign_worker(crawler_store, searches, crawler_wake))
        .map_err(|error| log::warn!("search: results are not crawled: {error}"))
        .ok()
        .map(|_| campaigns);

    let providers = select_providers();
    log::info!(
        "search: providers {:?}",
        providers.iter().map(|p| p.name()).collect::<Vec<_>>()
    );
    cx.set_global(SearchService {
        store,
        providers: providers.into(),
        store_note,
        profile,
        embed_wake,
        campaigns,
    });
}

/// Stop waiting for providers at four seconds or once 100 results are in,
/// whichever comes first, and rank up to 100 of them.
fn search_opts() -> SearchOpts {
    SearchOpts {
        provider_limit: 25,
        limit: 100,
        provider_timeout_ms: 4_000,
        target_candidates: 100,
        ..SearchOpts::default()
    }
}

/// Platinum's rank profile: RustyWeb's default, with Bluesky weighed down and
/// semantic matches allowed to seed graph expansion.
fn frame_profile() -> RankProfile {
    let mut profile = RankProfile::builtin("default").expect("RustyWeb ships a default profile");
    profile.id = "frame".into();
    profile.name = crate::brand::NAME.into();
    profile.rules.push(RankRule::Provider {
        rule_id: "bluesky-down".into(),
        provider: "bluesky".into(),
        weight: BLUESKY_WEIGHT,
    });
    profile.graph.allow_dense_seeds = true;
    profile
}

type Rerankers = Vec<Arc<dyn Reranker>>;

/// OpenRouter rerankers: Voyage Rerank 3 Lite first and Cohere Rerank 4 Pro
/// when Voyage does not answer, then, for media searches only, NVIDIA's
/// Nemotron Rerank VL, which reads each result's picture. None when the
/// Keychain holds no key.
fn openrouter_rerankers() -> anyhow::Result<Option<(Rerankers, Rerankers)>> {
    let Some(key) = keychain_secret(OPENROUTER_KEYCHAIN_SERVICE)? else {
        return Ok(None);
    };
    let reranker = |model: &str| {
        OpenRouterReranker::new(key.clone(), model)
            .map_err(|error| anyhow!("could not set up {model}: {error}"))
    };
    let text = [VOYAGE_RERANK_3_LITE, COHERE_RERANK_4_PRO]
        .into_iter()
        .map(|model| reranker(model).map(|r| Arc::new(r) as Arc<dyn Reranker>))
        .collect::<anyhow::Result<_>>()?;
    let media = vec![Arc::new(reranker(NEMOTRON_RERANK_VL)?.reading_images()) as Arc<dyn Reranker>];
    Ok(Some((text, media)))
}

/// A generic password from the login Keychain, read through the `security`
/// tool. The item's access list lets that tool read it without a prompt; a
/// Security.framework read would be checked against each rebuilt Platinum binary.
fn keychain_secret(service: &str) -> anyhow::Result<Option<String>> {
    let output = std::process::Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", service, "-w"])
        .output()
        .context("could not run the security tool")?;
    if !output.status.success() {
        return Ok(None);
    }
    let secret = String::from_utf8(output.stdout).context("the Keychain item is not text")?;
    let secret = secret.trim();
    Ok((!secret.is_empty()).then(|| secret.to_string()))
}

/// Load the shared embedding model, then keep visited pages embedded: the
/// whole history first, then each capture as it is reported on `woken`.
fn embedding_worker(store: Arc<Mutex<RedCoreBrowserSessionStore>>, woken: mpsc::Receiver<()>) {
    let model = match crate::embedder::load() {
        Ok(model) => model,
        Err(error) => {
            log::warn!("search: semantic ranking is off: {error:#}");
            return;
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            log::warn!("search: semantic ranking is off: {error}");
            return;
        }
    };
    match store.lock() {
        Ok(mut store) => store.set_text_embedder(model.clone()),
        Err(_) => return,
    }
    loop {
        if let Err(error) = embed_pending_pages(&store, &model, &runtime) {
            log::warn!("search: could not embed visited pages: {error:#}");
        }
        // Several captures reported while embedding are one more pass.
        if woken.recv().is_err() {
            return;
        }
        while woken.try_recv().is_ok() {}
    }
}

/// Embed every visited page the model has not embedded yet, a batch at a time,
/// with the store unlocked while the model runs.
fn embed_pending_pages(
    store: &Mutex<RedCoreBrowserSessionStore>,
    model: &crate::embedder::EmbeddingGemma,
    runtime: &tokio::runtime::Runtime,
) -> anyhow::Result<()> {
    let lock = || {
        store
            .lock()
            .map_err(|_| anyhow!("the search store stopped after an earlier failure"))
    };
    let mut embedded = 0;
    loop {
        let pending = lock()?
            .pending_page_embeddings(EMBED_BATCH)
            .map_err(|error| anyhow!("{} ({})", error.message, error.code))?;
        let Some(pending) = pending else { break };
        let vectors = model.embed_background(pending.inputs())?;
        let published = {
            let mut store = lock()?;
            runtime.block_on(store.publish_page_embeddings(pending, vectors))
        }
        .map_err(|error| anyhow!("{} ({})", error.message, error.code))?;
        // Pages changed while they were embedded; their capture wakes the
        // worker again.
        if published == 0 {
            break;
        }
        embedded += published;
    }
    if embedded > 0 {
        log::info!("search: embedded {embedded} visited pages");
    }
    Ok(())
}

/// Run each finished search's crawl campaign, one at a time, and wake the
/// embedding worker for the pages it adds.
fn campaign_worker(
    store: Arc<Mutex<RedCoreBrowserSessionStore>>,
    searches: mpsc::Receiver<SearchSession>,
    embed_wake: Option<mpsc::Sender<()>>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            log::warn!("search: results are not crawled: {error}");
            return;
        }
    };
    let options = LiveFetchOptions::default();
    for session in searches {
        match run_campaign(&store, &session, &runtime, &options) {
            Ok(0) => {}
            Ok(added) => {
                log::info!(
                    "search: crawled {added} results for {:?}",
                    session.request.query
                );
                if let Some(wake) = &embed_wake {
                    let _ = wake.send(());
                }
            }
            Err(error) => log::warn!(
                "search: could not crawl results for {:?}: {error:#}",
                session.request.query
            ),
        }
    }
}

/// Plan, fetch and ingest one search's campaign, with the store unlocked while
/// pages download. Returns the pages added.
fn run_campaign(
    store: &Mutex<RedCoreBrowserSessionStore>,
    session: &SearchSession,
    runtime: &tokio::runtime::Runtime,
    options: &LiveFetchOptions,
) -> anyhow::Result<usize> {
    let lock = || {
        store
            .lock()
            .map_err(|_| anyhow!("the search store stopped after an earlier failure"))
    };
    let plan = lock()?
        .plan_search_campaign(session)
        .map_err(|error| anyhow!("{} ({})", error.message, error.code))?;
    let Some(plan) = plan else { return Ok(0) };
    let fetch = runtime.block_on(fetch_campaign_pages(&plan, options));
    for (url, reason) in &fetch.failures {
        log::debug!("search: did not crawl {url}: {reason}");
    }
    let receipt = lock()?
        .ingest_campaign_pages(&plan, &fetch.pages, &options.guard_policy)
        .map_err(|error| anyhow!("{error}"))?;
    Ok(receipt.map_or(0, |receipt| receipt.page_count))
}

/// Run one search off the main thread.
///
/// `ask` is the explicit AI path. A plain search never shows an answer.
pub fn run(query: String, ask: bool, cx: &mut App) -> Task<anyhow::Result<SerpResult>> {
    let Some(service) = cx.try_global::<SearchService>().cloned() else {
        return Task::ready(Err(anyhow!(
            "Search is not ready yet. Restart Platinum and try again."
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
        drop(store);
        if let Some(wake) = &self.embed_wake {
            let _ = wake.send(());
        }
        Ok(())
    }

    fn search(&self, query: &str, ask: bool) -> anyhow::Result<SerpResult> {
        let started = Instant::now();
        let used: Vec<String> = self
            .providers
            .iter()
            .map(|p| p.name().to_string())
            .collect();
        let runtime = SEARCH_RUNTIME
            .as_ref()
            .map_err(|error| anyhow!("could not start the search runtime: {error}"))?;
        let mut store = self.store.lock().map_err(|_| {
            anyhow!("The search store stopped after an earlier failure. Restart Platinum.")
        })?;
        let locked = started.elapsed();
        let session = runtime
            .block_on(store.programmable_search(
                query,
                // Only Ask runs a model; RustyWeb plans no answer otherwise.
                ProgrammableQueryOptions {
                    synthesize: ask,
                    profile: self.profile.clone(),
                    ..Default::default()
                },
                &self.providers,
            ))
            .map_err(|error| anyhow!("{} ({})", error.message, error.code))?;
        drop(store);
        let receipt = &session.response.receipt;
        let phases = &receipt.phase_latency_ms;
        log::debug!(
            "search: {query:?} in {} ms: store wait {} ms, warm {:.0} ms, fresh {:.0} ms, \
             fuse {:.0} ms, query {:.0} ms, reranker {:?}, lanes {:?}",
            started.elapsed().as_millis(),
            locked.as_millis(),
            phases.warm_ms,
            phases.fresh_ms,
            phases.fuse_ms,
            phases.total_ms,
            receipt.reranker,
            receipt.lane_candidate_counts,
        );

        let mut result = map_session(&session, ask, used);
        result
            .notes
            .extend(self.store_note.iter().map(|n| n.to_string()));
        result.elapsed_ms = started.elapsed().as_millis() as u64;
        if let Some(campaigns) = &self.campaigns
            && campaigns.try_send(session).is_err()
        {
            log::info!("search: the crawler is busy, so results for {query:?} are not crawled");
        }
        Ok(result)
    }
}

/// One runtime for every search, so provider connections stay pooled between
/// searches instead of closing with a per-search runtime.
static SEARCH_RUNTIME: LazyLock<std::io::Result<tokio::runtime::Runtime>> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("frame-search")
        .enable_all()
        .build()
});

/// The providers every search queries.
///
/// An explicit `RUSTYWEB_SEARCH_PROVIDERS` (or `RUSTY_RED_SEARCH_PROVIDERS`)
/// list wins. Otherwise: the keyless providers plus every keyed general-web
/// provider whose key is present.
fn select_providers() -> Vec<Arc<dyn SearchProvider>> {
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
    let selected: Vec<Arc<dyn SearchProvider>> = candidates
        .into_iter()
        .filter(|name| seen.insert(name.clone()))
        .filter_map(|name| configured_search_provider_from_env(&name))
        .collect();

    if selected.is_empty() {
        // An explicit list whose providers all lack keys would leave nothing to
        // search with. Fall back to the keyless providers.
        log::warn!("search: no configured provider is usable; falling back to bluesky");
        return KEYLESS_PROVIDERS
            .iter()
            .filter_map(|name| configured_search_provider_from_env(name))
            .collect();
    }
    selected
}

/// Map one finished RustyWeb session to Platinum-owned data.
fn map_session(session: &SearchSession, ask: bool, providers: Vec<String>) -> SerpResult {
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
    // A reranker that failed over to the next one changed nothing visible.
    let reranked = receipt.reranker.is_some();
    let degradations = receipt
        .degraded
        .iter()
        .filter(|code| !is_synthesis_code(code))
        .filter(|code| !(reranked && code.starts_with("rerank_failed:")))
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
                    Some(SerpSource {
                        host: display_host(&url),
                        title: if source.title.trim().is_empty() {
                            display_host(&url)
                        } else {
                            source.title.trim().to_string()
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
            "Answers need a model. Set {SYNTHESIS_ENV_VAR} to a model config file, then restart Platinum."
        );
    }
    if has("synthesis_model_configuration_invalid") {
        return format!(
            "The model config at {SYNTHESIS_ENV_VAR} could not be loaded. Check the path and the file, then restart Platinum."
        );
    }
    if has("synthesis_evidence_minimum_unmet") {
        return "There is not enough captured page text to answer with citations yet. Open a few of these results so Platinum can capture them, then ask again.".to_string();
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
            let title = hit.title.trim();
            Some(SerpHit {
                title: if title.is_empty() {
                    host.clone()
                } else {
                    title.to_string()
                },
                host,
                url,
                snippet: collapse_whitespace(&hit.snippet),
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
fn is_local_lane(lane: &str) -> bool {
    lane.starts_with("warm") || lane == "my_web"
}

/// Short label for a lane tag.
pub fn lane_label(lane: &str) -> String {
    if is_local_lane(lane) {
        return "Local".to_string();
    }
    match lane {
        "fresh" => "Fresh".to_string(),
        "federated" => "Federated".to_string(),
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

/// A note when only social and video sources are searched.
fn weak_web_note(providers: &[String]) -> Option<String> {
    let general = providers
        .iter()
        .any(|name| GENERAL_WEB_PROVIDERS.contains(&name.as_str()));
    (!general).then(|| {
        "Only social and video sources are searched. Add a search API key (Brave, Exa, Mojeek, Firecrawl, SerpAPI or Perplexity) to .env and restart Platinum for general web results.".to_string()
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
        "dense_query_vector_required" => "Semantic ranking is off until the embedding model has loaded.".to_string(),
        _ if code.starts_with("dense_query_embedding_failed") => {
            "Semantic ranking is off for this search because the embedding model failed.".to_string()
        }
        _ if code.starts_with("rerank_failed:") => format!(
            "Results keep their first-pass order because reranking with {} did not answer.",
            &code["rerank_failed:".len()..]
        ),
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
            "providers": ["brave"],
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
        assert_eq!(hit.snippet, "A snippet with gaps");
        assert_eq!(hit.published_at.as_deref(), Some("2026-10-24"));
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
        assert_eq!(lane_label("fresh"), "Fresh");
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
            describe_degradation("provider_error:exa", &errors),
            "exa did not answer (error)."
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
        <body><p>The moon's gravity pulls the ocean toward it, raising a bulge of water.</p>\
        <a href=\"https://example.com/moon\">More about the moon</a></body></html>";

    #[test]
    fn a_visited_page_is_found_by_local_search() {
        let service = SearchService {
            store: Arc::new(Mutex::new(memory_browser_session(SESSION_ID))),
            providers: Arc::from(Vec::new()),
            store_note: None,
            profile: None,
            embed_wake: None,
            campaigns: None,
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
        assert!(weak_web_note(&names(&["bluesky", "youtube"])).is_some());
        assert!(weak_web_note(&names(&["bluesky", "brave"])).is_none());
    }

    #[test]
    fn rerank_and_embedding_degradations_read_as_sentences() {
        assert_eq!(
            describe_degradation("rerank_failed:voyageai/rerank-3-lite", &[]),
            "Results keep their first-pass order because reranking with voyageai/rerank-3-lite did not answer."
        );
        assert_eq!(
            describe_degradation("dense_query_embedding_failed:the model stopped", &[]),
            "Semantic ranking is off for this search because the embedding model failed."
        );
        assert_eq!(
            describe_degradation("dense_query_vector_required", &[]),
            "Semantic ranking is off until the embedding model has loaded."
        );
    }

    #[test]
    fn frame_profile_weighs_bluesky_down_and_keeps_the_default_otherwise() {
        let profile = frame_profile();
        profile.validate().unwrap();
        let default = RankProfile::builtin("default").unwrap();
        assert_eq!(profile.revision, default.revision);
        assert_eq!(
            profile.rules,
            [RankRule::Provider {
                rule_id: "bluesky-down".into(),
                provider: "bluesky".into(),
                weight: -0.6,
            }]
        );
        assert!(profile.graph.allow_dense_seeds);
    }

    #[test]
    fn searches_stop_at_four_seconds_or_100_results() {
        let opts = search_opts();
        assert_eq!(opts.provider_timeout_ms, 4_000);
        assert_eq!(opts.target_candidates, 100);
        assert_eq!(opts.limit, 100);
    }

    #[test]
    fn gdelt_is_not_a_default_provider() {
        assert!(!KEYLESS_PROVIDERS.contains(&"gdelt"));
        assert!(!KEYED_PROVIDERS.contains(&"gdelt"));
    }

    #[test]
    fn synthesis_codes_are_kept_out_of_the_footer() {
        assert!(is_synthesis_code("synthesis_model_unavailable"));
        assert!(!is_synthesis_code("provider_error:brave"));
    }
}
