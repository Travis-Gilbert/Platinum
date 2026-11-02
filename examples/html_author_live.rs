//! A01's live acceptance: an agent-written page, versioned, previewed in
//! Platinum's Servo and inspected for what a person would see.
//!
//! Everything but the pixels and the layout is `theorem-html-design`: the
//! revisions are `DocumentAuthority` revisions, the findings are its rules, the
//! evidence is its content-addressed record. This example is the engine half:
//! it serves a revision under `frame-preview:`, lays it out at a phone and a
//! desktop width, runs the crate's probe in the page, takes the screenshot, and
//! hands all three back to the crate.
//!
//! Three designs are run:
//!   `harbor`   revision 1 is the fixture with a 390 px nav overflow and a pale
//!              paragraph; two exact edits make revision 2;
//!   `control`  the corrected page published on its own, which must be clean;
//!   `sandbox`  a page that asks for everything a preview refuses. Every HTTP
//!              request the engine makes is answered here with a refusal and
//!              recorded, so the network is closed by the embedder and not only
//!              by the page's Content-Security-Policy.
//!
//! Grade `live_local`: the screenshots come from this machine's Servo, never a
//! fixture image. Exit status 0 only if every check holds.
extern crate gpui_kit as gpui;

#[allow(dead_code)]
#[path = "../src/engine.rs"]
mod engine;
#[allow(dead_code)]
#[path = "../src/preview.rs"]
mod preview;
#[allow(dead_code)]
#[path = "../src/scene.rs"]
mod scene;
// scene.rs's own frame test reads swatches through `crate::site`; only the
// test build needs it, so the example binary keeps its three modules.
#[cfg(test)]
#[allow(dead_code)]
#[path = "../src/site.rs"]
mod site;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use euclid::{Scale, Size2D};
use futures::FutureExt as _;
use futures::channel::oneshot;
use gpui_kit::AsyncApp;
use serde_json::{Value, json};
use servo::protocol_handler::ProtocolRegistry;
use servo::{
    JSValue, LoadStatus, WebResourceLoad, WebResourceResponse, WebView, WebViewBuilder,
    WebViewDelegate,
};
use theorem_html_design::evidence::{self, Evidence, Shot};
use theorem_html_design::finding::blocked_requests;
use theorem_html_design::{
    DesignStore, DiskObjectStore, ExactEdit, Finding, Layout, MemoryAuthority, Outcome, ReleaseError,
    Writer, layout, validate::validate,
};

/// What the fixtures were written to contain. The ids are the crate's: the same
/// defect keeps its id from one revision to the next.
const NAV_OVERFLOW: &str = "horizontal_overflow:html > body > nav#main@390";
const PALE_LEDE: &str = "low_contrast:html > body > main:nth-of-type(1) > p:nth-of-type(1)";

/// The viewport height; the widths are the arguments.
const HEIGHT: u32 = 800;
/// Bounds every wait on the engine, so a stuck page fails the run.
const LIMIT: Duration = Duration::from_secs(20);

#[derive(Parser)]
struct Args {
    /// The crate's `tests/fixtures` directory.
    #[arg(long)]
    fixtures: PathBuf,
    /// Where the screenshots, the findings and `result.json` are written.
    #[arg(long)]
    evidence: PathBuf,
    /// What the engine is, for the evidence record (the script fills in the
    /// Servo revision).
    #[arg(long, default_value = "servo 0.7 (Platinum fork), in-process")]
    engine: String,
    #[arg(long, value_delimiter = ',', default_value = "390,1280")]
    widths: Vec<u32>,
}

/// What the engine told the delegate about the page.
#[derive(Default)]
struct Observed {
    /// Documents that started loading, so a wait is for the next load and not
    /// the previous one's `Complete`.
    starts: Cell<u32>,
    /// Every HTTP request the engine tried to make. All are refused.
    blocked: RefCell<Vec<String>>,
    crashed: RefCell<Option<String>>,
}

impl WebViewDelegate for Observed {
    fn notify_load_status_changed(&self, _: WebView, status: LoadStatus) {
        if status == LoadStatus::Started {
            self.starts.set(self.starts.get() + 1);
        }
    }

    fn notify_new_frame_ready(&self, view: WebView) {
        view.paint();
        view.rendering_context().present();
    }

    fn load_web_resource(&self, _: WebView, load: WebResourceLoad) {
        let request = load.request();
        if preview::embedder_admits(&request.method, &request.url) {
            // Dropped unanswered, the load goes on as the engine meant it.
            return;
        }
        let url = request.url.clone();
        self.blocked.borrow_mut().push(url.to_string());
        load.intercept(WebResourceResponse::new(url).status_code(http::StatusCode::FORBIDDEN))
            .cancel();
    }

    fn notify_crashed(&self, _: WebView, reason: String, _: Option<String>) {
        *self.crashed.borrow_mut() = Some(reason);
    }
}

/// Wait for `future`, or fail naming `what` if the engine does not answer.
async fn within<T>(cx: &AsyncApp, what: &str, future: impl Future<Output = T>) -> anyhow::Result<T> {
    let future = future.fuse();
    let timeout = cx.background_executor().timer(LIMIT).fuse();
    futures::pin_mut!(future, timeout);
    futures::select! {
        value = future => Ok(value),
        () = timeout => Err(anyhow::anyhow!("{what} timed out after {LIMIT:?}")),
    }
}

/// Poll `ready` until it holds.
async fn until(cx: &AsyncApp, what: &str, mut ready: impl FnMut() -> bool) -> anyhow::Result<()> {
    within(cx, what, async {
        while !ready() {
            cx.background_executor().timer(Duration::from_millis(25)).await;
        }
    })
    .await
}

/// Wait until the page has stopped asking for things: the list of refused
/// requests unchanged for a quarter of a second. A request the page makes just
/// after it loads is still the page's request.
async fn quiet(observed: &Observed, cx: &AsyncApp) -> anyhow::Result<()> {
    within(cx, "the page to stop requesting", async {
        let (mut last, mut same) = (usize::MAX, 0);
        while same < 5 {
            cx.background_executor().timer(Duration::from_millis(50)).await;
            let now = observed.blocked.borrow().len();
            if now == last {
                same += 1;
            } else {
                (last, same) = (now, 0);
            }
        }
    })
    .await
}

/// Evaluate `script`, which must produce a string.
async fn evaluate(view: &WebView, script: &str, cx: &AsyncApp) -> anyhow::Result<String> {
    let (send, receive) = oneshot::channel();
    view.evaluate_javascript(script, move |result| {
        let _ = send.send(match result {
            Ok(JSValue::String(text)) => Ok(text),
            other => Err(anyhow::anyhow!("the script produced {other:?}, not a string")),
        });
    });
    within(cx, "script evaluation", receive)
        .await?
        .map_err(|_| anyhow::anyhow!("the engine dropped the script"))?
}

/// The screenshot, as PNG bytes.
async fn capture(view: &WebView, cx: &AsyncApp) -> anyhow::Result<(Vec<u8>, (u32, u32))> {
    let (send, receive) = oneshot::channel();
    view.take_screenshot(None, move |result| {
        let _ = send.send(result.map_err(|error| anyhow::anyhow!("screenshot: {error:?}")));
    });
    let image = within(cx, "screenshot", receive)
        .await?
        .map_err(|_| anyhow::anyhow!("the engine dropped the screenshot"))??;
    let mut png = Vec::new();
    image.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)?;
    Ok((png, image.dimensions()))
}

/// Show `address` at `width` and wait for the document to finish loading.
async fn show(
    view: &WebView,
    observed: &Observed,
    address: url::Url,
    width: u32,
    cx: &AsyncApp,
) -> anyhow::Result<()> {
    view.set_viewport(Size2D::new(width as f32, HEIGHT as f32), Scale::new(1.0));
    let before = observed.starts.get();
    observed.blocked.borrow_mut().clear();
    view.load(address.clone());
    until(cx, "the page to load", || {
        observed.crashed.borrow().is_some()
            || (observed.starts.get() > before
                && view.load_status() == LoadStatus::Complete
                && view.url().as_ref() == Some(&address))
    })
    .await?;
    if let Some(reason) = observed.crashed.borrow().as_deref() {
        anyhow::bail!("Servo crashed: {reason}");
    }
    Ok(())
}

/// One revision of one design, laid out at one width.
struct Inspection {
    design: String,
    revision: u64,
    width: u32,
    findings: Vec<Finding>,
    evidence: Evidence,
    png: Vec<u8>,
}

impl Inspection {
    fn ids(&self) -> BTreeSet<&str> {
        self.findings.iter().map(|f| f.id.as_str()).collect()
    }
}

struct Lab<'a> {
    view: &'a WebView,
    observed: &'a Observed,
    cx: &'a AsyncApp,
    engine: &'a str,
    blobs: &'a DiskObjectStore,
    out: &'a Path,
}

impl Lab<'_> {
    /// Render `design` at `revision` and `width`, check it, record the evidence.
    async fn inspect(
        &self,
        store: &mut DesignStore<MemoryAuthority>,
        design: &str,
        revision: u64,
        width: u32,
        by: &Writer<'_>,
    ) -> anyhow::Result<Inspection> {
        let source = store.source_at(design, revision)?;
        show(self.view, self.observed, preview::url(design, revision), width, self.cx).await?;
        quiet(self.observed, self.cx).await?;

        let probed = evaluate(self.view, layout::PROBE, self.cx).await?;
        let laid_out: Layout = serde_json::from_str(&probed)?;
        anyhow::ensure!(
            laid_out.viewport_width == width,
            "asked for a {width} px viewport and the page laid out at {} px",
            laid_out.viewport_width
        );
        let (png, (pixels, _)) = capture(self.view, self.cx).await?;
        anyhow::ensure!(pixels == width, "the screenshot is {pixels} px wide, not {width}");

        let mut findings = validate(&source);
        findings.extend(layout::check(&laid_out));
        findings.extend(blocked_requests(
            self.observed.blocked.borrow().iter().map(String::as_str),
        ));
        let evidence = store.record_evidence(
            self.blobs,
            design,
            revision,
            &Shot { viewport_width: width, png: &png, engine: self.engine, findings: &findings },
            by,
        )?;
        let name = format!("{design}-r{revision}-w{width}");
        // What is written is what the store holds, read back, not what was taken.
        let stored = evidence::screenshot(self.blobs, &evidence)?;
        anyhow::ensure!(stored == png, "{name}: the stored screenshot is not the one taken");
        std::fs::write(self.out.join(format!("{name}.png")), &stored)?;
        Ok(Inspection { design: design.to_owned(), revision, width, findings, evidence, png })
    }
}

/// `after` against `before`: what the edit fixed, what it introduced, what stayed.
fn compare(before: &Inspection, after: &Inspection) -> Value {
    let (b, a) = (before.ids(), after.ids());
    json!({
        "width": before.width,
        "from_revision": before.revision,
        "to_revision": after.revision,
        "fixed": b.difference(&a).collect::<Vec<_>>(),
        "introduced": a.difference(&b).collect::<Vec<_>>(),
        "unchanged": b.intersection(&a).collect::<Vec<_>>(),
    })
}

/// The sandbox page's report, once its attempts have settled.
async fn sandbox_report(view: &WebView, cx: &AsyncApp) -> anyhow::Result<Value> {
    const SETTLED: &str = "(() => { const s = window.__sandbox; return !!s && Object.keys(s.attempts).length === 6 && \
        Object.values(s.attempts).every((v) => v !== 'unknown') && s.control.data_image !== undefined; })()";
    // The script's own promises settle in well under a second; the ones that do
    // not (a form post has no event) are reported as they stand after a pause.
    let _ = within(cx, "the sandbox page to settle", async {
        for _ in 0..80 {
            if evaluate(view, &format!("String({SETTLED})"), cx).await.is_ok_and(|v| v == "true") {
                break;
            }
            cx.background_executor().timer(Duration::from_millis(50)).await;
        }
    })
    .await;
    let text = evaluate(view, "JSON.stringify(window.__sandbox)", cx).await?;
    Ok(serde_json::from_str(&text)?)
}

/// The sandbox verdict: nothing got through, and the page was not locked out of
/// what it is allowed. `closed_by` says which layer refused each request.
fn sandbox_verdict(report: &Value, blocked: &[String]) -> Value {
    let attempts = report["attempts"].as_object().cloned().unwrap_or_default();
    let violations: Vec<String> = report["violations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v["blocked"].as_str().map(str::to_owned))
        .collect();
    let leaked: Vec<&String> = attempts
        .iter()
        .filter(|(_, outcome)| matches!(outcome.as_str(), Some("loaded" | "answered")))
        .map(|(name, _)| name)
        .collect();
    // The attempts that name an outside URL, each by the part of the URL that
    // identifies it.
    let reached: Vec<Value> = ["probe-fetch", "probe-xhr", "probe-image", "probe-form", "probe-window"]
        .iter()
        .map(|needle| {
            let by_engine = blocked.iter().any(|url| url.contains(needle));
            let by_policy = violations.iter().any(|url| url.contains(needle));
            // `window.open` may be refused before any request exists: the page
            // is then handed no window at all.
            let no_window = *needle == "probe-window" && attempts["window_open"] == "refused";
            json!({
                "request": needle,
                "refused_by_embedder": by_engine,
                "refused_by_policy": by_policy,
                "refused_by_window": no_window,
            })
        })
        .collect();
    let every_request_refused = reached
        .iter()
        .all(|r| {
            r["refused_by_embedder"] == true
                || r["refused_by_policy"] == true
                || r["refused_by_window"] == true
        });
    let control_ok = report["control"]["script_ran"] == true && report["control"]["data_image"] == "loaded";
    json!({
        "attempts": attempts,
        "leaked": leaked,
        "policy_violations": violations,
        "embedder_refused": blocked,
        "requests": reached,
        "control_ok": control_ok,
        "passed": leaked.is_empty() && every_request_refused && control_ok,
    })
}

async fn run(args: Args, mut store: DesignStore<MemoryAuthority>, view: WebView, observed: Rc<Observed>, cx: &AsyncApp, out: &Path) -> anyhow::Result<bool> {
    // The content-addressed store the evidence lands in; the screenshots kept
    // for the record are written back out of it below.
    let blob_dir = tempfile::tempdir()?;
    let blobs = DiskObjectStore::open(blob_dir.path())?;
    let lab = Lab { view: &view, observed: &observed, cx, engine: &args.engine, blobs: &blobs, out };
    let by = Writer { operation_id: "a01-live-inspect", actor: "agent:a01-live", provenance: "html_author_live" };

    // The acceptance pair: revision 1 has the defects, revision 2 is the fix.
    let mut harbor = Vec::new();
    for revision in [1, 2] {
        for &width in &args.widths {
            harbor.push(lab.inspect(&mut store, "harbor", revision, width, &by).await?);
        }
    }
    let control: Vec<Inspection> = {
        let mut all = Vec::new();
        for &width in &args.widths {
            all.push(lab.inspect(&mut store, "control", 1, width, &by).await?);
        }
        all
    };

    let at = |design: &[Inspection], revision: u64, width: u32| -> anyhow::Result<usize> {
        design
            .iter()
            .position(|i| i.revision == revision && i.width == width)
            .ok_or_else(|| anyhow::anyhow!("no inspection of revision {revision} at {width} px"))
    };
    let (phone, desktop) = (args.widths[0], *args.widths.last().expect("at least one width"));
    let ids = |design: &[Inspection], revision, width| -> anyhow::Result<BTreeSet<String>> {
        Ok(design[at(design, revision, width)?].ids().into_iter().map(str::to_owned).collect())
    };
    let set = |ids: &[&str]| ids.iter().map(|id| (*id).to_owned()).collect::<BTreeSet<String>>();

    let defects_found = ids(&harbor, 1, phone)? == set(&[NAV_OVERFLOW, PALE_LEDE])
        && ids(&harbor, 1, desktop)? == set(&[PALE_LEDE]);
    let defects_gone = args.widths.iter().all(|&w| ids(&harbor, 2, w).is_ok_and(|s| s.is_empty()));
    let control_clean = control.iter().all(|i| i.findings.is_empty());

    let comparisons: Vec<Value> = args
        .widths
        .iter()
        .map(|&w| Ok(compare(&harbor[at(&harbor, 1, w)?], &harbor[at(&harbor, 2, w)?])))
        .collect::<anyhow::Result<_>>()?;

    // Evidence is found again by (design, revision, width), and says which
    // source it was taken from.
    let mut evidence_found = true;
    for i in harbor.iter().chain(&control) {
        let again = store.evidence(&i.design, i.revision, i.width)?;
        let source_hash = store.source_at(&i.design, i.revision).map(|s| theorem_html_design::content_hash(&s))?;
        evidence_found &= again.as_ref() == Some(&i.evidence)
            && i.evidence.content_hash == source_hash
            && evidence::screenshot(&blobs, &i.evidence)? == i.png;
    }

    // The sandbox: the page that asks for everything a preview refuses is
    // inspected like any other, then its own report of what it was told is read.
    let probe = lab.inspect(&mut store, "sandbox", 1, desktop, &by).await?;
    let report = sandbox_report(&view, cx).await?;
    let blocked = observed.blocked.borrow().clone();
    let sandbox = sandbox_verdict(&report, &blocked);

    // The gate: warnings go out with a release, errors stop it, and a width
    // nobody inspected is not a width the release was checked at.
    let site = out.join("site");
    let all = args.widths.clone();
    let corrected = std::fs::read_to_string(args.fixtures.join("fixed.html"))?;
    let fixed_goes_out = store.release("harbor", 2, &all, &site.join("harbor-r2")).is_ok_and(|release| {
        release.warnings.is_empty()
            && std::fs::read_to_string(&release.file).is_ok_and(|page| page == corrected)
    });
    let warned_goes_out = store.release("harbor", 1, &all, &site.join("harbor-r1")).is_ok_and(|release| {
        // The record carries each warning whole, not only its id.
        release.warnings.iter().all(|warning| !warning.message.is_empty())
            && release.warnings.iter().map(|warning| warning.id.as_str()).collect::<BTreeSet<_>>()
                == BTreeSet::from([NAV_OVERFLOW, PALE_LEDE])
    });
    let control_goes_out = store.release("control", 1, &all, &site.join("control-r1")).is_ok();
    let sandbox_ids = match store.release("sandbox", 1, &[desktop], &site.join("sandbox-r1")) {
        Err(ReleaseError::Blocked { ids }) => ids,
        _ => Vec::new(),
    };
    let sandbox_blocked = !sandbox_ids.is_empty()
        && sandbox_ids.iter().all(|id| id.starts_with("unfetchable_resource:html@request:"))
        && !site.join("sandbox-r1").exists();
    let unchecked_refused = matches!(
        store.release("harbor", 2, &[desktop + 1], &site.join("harbor-r2-unchecked")),
        Err(ReleaseError::Unchecked { revision: 2, .. })
    ) && !site.join("harbor-r2-unchecked").exists();
    let releases_hold = fixed_goes_out && warned_goes_out && control_goes_out && sandbox_blocked && unchecked_refused;

    let result = json!({
        "grade": "live_local",
        "engine": args.engine,
        "widths": args.widths,
        "defects_found_at_revision_1": defects_found,
        "defects_gone_at_revision_2": defects_gone,
        "control_clean": control_clean,
        "evidence_found_again": evidence_found,
        "finding_set_difference": comparisons,
        "findings": harbor.iter().chain(&control).map(|i| json!({
            "design": i.design, "revision": i.revision, "width": i.width,
            "screenshot": i.evidence.screenshot, "content_hash": i.evidence.content_hash,
            "findings": i.findings,
        })).collect::<Vec<_>>(),
        "sandbox": sandbox,
        "sandbox_findings": probe.findings,
        "release": {
            "corrected_page_released_as_stored": fixed_goes_out,
            "page_with_warnings_released_with_them": warned_goes_out,
            "control_released": control_goes_out,
            "sandbox_blocked_by": sandbox_ids,
            "sandbox_blocked_and_nothing_written": sandbox_blocked,
            "uninspected_width_refused": unchecked_refused,
        },
    });
    let passed = defects_found
        && defects_gone
        && control_clean
        && evidence_found
        && releases_hold
        && sandbox["passed"] == true;
    let mut result = result;
    result["passed"] = json!(passed);
    std::fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
    std::fs::write(out.join("sandbox.json"), serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(passed)
}

/// Publish the fixtures: `harbor` revision 1, two exact edits to revision 2,
/// the corrected page as `control`, and the sandbox probe.
fn publish(fixtures: &Path) -> anyhow::Result<DesignStore<MemoryAuthority>> {
    let read = |name: &str| std::fs::read_to_string(fixtures.join(name));
    let (index, fixed, sandbox) = (read("index.html")?, read("fixed.html")?, read("sandbox.html")?);
    let by = Writer { operation_id: "a01-live-edit", actor: "agent:a01-live", provenance: "html_author_live" };
    let mut store = DesignStore::new(MemoryAuthority::new());

    anyhow::ensure!(store.publish("harbor", &index, 0, &by)?.revision == 1, "harbor starts at revision 1");
    let edits = [
        ExactEdit {
            old_string: "nav { display: flex; background".to_owned(),
            new_string: "nav { display: flex; flex-wrap: wrap; background".to_owned(),
        },
        ExactEdit { old_string: "color: #999;".to_owned(), new_string: "color: #595959;".to_owned() },
    ];
    let edited = store.edit("harbor", 1, &edits, &by)?;
    anyhow::ensure!(edited.revision == 2 && edited.outcome == Outcome::Advanced, "the edits make revision 2");
    anyhow::ensure!(store.source_at("harbor", 2)? == fixed, "the two edits produce the corrected fixture byte for byte");
    anyhow::ensure!(store.source_at("harbor", 1)? == index, "revision 1 is still the original");

    // Publishing what the design already holds commits nothing.
    let again = Writer { operation_id: "a01-live-same", ..by };
    let same = store.publish("harbor", &fixed, 1, &again)?;
    anyhow::ensure!(same.outcome == Outcome::Unchanged && same.revision == 2, "an identical publish is unchanged");

    store.publish("control", &fixed, 0, &Writer { operation_id: "a01-live-control", ..by })?;
    store.publish("sandbox", &sandbox, 0, &Writer { operation_id: "a01-live-sandbox", ..by })?;
    Ok(store)
}

/// AppKit's application termination exits with 0, so an acceptance run must
/// return its own status before asking AppKit to quit.
fn finish(profile: &Path, status: i32) -> ! {
    let _ = std::fs::remove_dir_all(profile);
    std::process::exit(status);
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    anyhow::ensure!(!args.widths.is_empty() && args.widths.iter().all(|&w| w > 0), "give at least one width");
    std::fs::create_dir_all(&args.evidence)?;
    let store = publish(&args.fixtures)?;

    // What the preview scheme serves: every revision of every design.
    let mut pages = BTreeMap::new();
    for design in ["harbor", "control", "sandbox"] {
        for revision in store.history(design)? {
            pages.insert((design.to_owned(), revision.revision), store.source_at(design, revision.revision)?);
        }
    }
    let pages = Arc::new(pages);
    let mut protocols = ProtocolRegistry::default();
    protocols
        .register(
            preview::PREVIEW_SCHEME,
            preview::PreviewProtocol::new(move |design, revision| pages.get(&(design.to_owned(), revision)).cloned()),
        )
        .map_err(|error| anyhow::anyhow!("registering the preview scheme: {error:?}"))?;

    let profile = std::env::temp_dir().join(format!("frame-html-author-{}", std::process::id()));
    std::fs::create_dir_all(&profile)?;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let out = args.evidence.clone();
    let widest = args.widths.iter().copied().max().unwrap_or(1280);
    gpui_kit::application().run(move |cx| {
        engine::Engine::start_with_protocols(&profile, protocols, cx);
        let context = match scene::SceneRenderingContext::new(dpi::PhysicalSize::new(widest, HEIGHT)) {
            Ok(context) => Rc::new(context),
            Err(error) => {
                eprintln!("html author: GPU context failed: {error:?}");
                finish(&profile, 1);
            },
        };
        let servo = engine::Engine::servo(cx);
        let observed = Rc::new(Observed::default());
        let view = WebViewBuilder::new(&servo, context)
            .url(url::Url::parse("about:blank").expect("about:blank parses"))
            .hidpi_scale_factor(Scale::new(1.0))
            .delegate(observed.clone())
            .build();
        view.show();
        cx.spawn(async move |cx| {
            let checked = run(args, store, view, observed, cx, &out).await;
            let status = match &checked {
                Ok(true) => 0,
                Ok(false) => 1,
                Err(error) => {
                    eprintln!("html author: {error:#}");
                    let _ = std::fs::write(
                        out.join("result.json"),
                        json!({"grade": "live_local", "passed": false, "error": format!("{error:#}")}).to_string(),
                    );
                    2
                },
            };
            finish(&profile, status);
        })
        .detach();
    });
    Ok(())
}
