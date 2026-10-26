//! The reference run: drives Obsidian's CLI over a vault and stores what it answers as JSON.
//!
//! Commands, parameters and output formats are the ones on <https://obsidian.md/help/cli>
//! (`links`, `backlinks`, `unresolved`, `orphans`, `deadends`, `outline`, `aliases`, `properties`,
//! `tags`, `tasks`, `search`, `search:context`, `base:query`, `wordcount`, `eval`, `dev:dom` and a
//! few listings). Search operators are those of <https://obsidian.md/help/plugins/search>, and the
//! `.base` view names come from <https://obsidian.md/help/bases/syntax>. `eval` calls the app API:
//! `app.metadataCache.getCache(path)`, `.resolvedLinks`, `.unresolvedLinks`,
//! `app.vault.getFileByPath`, `WorkspaceLeaf.openFile` and `MarkdownView.getMode`, from
//! <https://docs.obsidian.md/Reference/TypeScript+API/>.
use crate::{
    OBSIDIAN_VERSION,
    launch::Isolation,
    normalize::Normalizer,
    obsidian::{Obsidian, Record, parse_version},
};
use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, btree_map::Entry},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use walkdir::WalkDir;

/// The reading view root observed in the 1.14.5 recording; `--dom-selector` overrides it.
pub const DOM_SELECTOR: &str = ".workspace-leaf.mod-active .markdown-reading-view";
pub const DEFAULT_QUERIES: &str = include_str!("../queries.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum Group {
    Vault,
    Links,
    Outline,
    Properties,
    Tags,
    Tasks,
    Search,
    Bases,
    Wordcount,
    Eval,
    Dom,
}

impl Group {
    pub const ALL: [Group; 11] = [
        Group::Vault,
        Group::Links,
        Group::Outline,
        Group::Properties,
        Group::Tags,
        Group::Tasks,
        Group::Search,
        Group::Bases,
        Group::Wordcount,
        Group::Eval,
        Group::Dom,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Group::Vault => "vault",
            Group::Links => "links",
            Group::Outline => "outline",
            Group::Properties => "properties",
            Group::Tags => "tags",
            Group::Tasks => "tasks",
            Group::Search => "search",
            Group::Bases => "bases",
            Group::Wordcount => "wordcount",
            Group::Eval => "eval",
            Group::Dom => "dom",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Query {
    pub query: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub case: bool,
}

pub fn default_queries() -> Result<Vec<Query>> {
    serde_json::from_str(DEFAULT_QUERIES).context("queries.json")
}

pub struct Config {
    /// The vault on disk, scanned for notes, canvases and bases. In an isolated run the CLI works
    /// on a copy of it (`isolation.vault_copy`).
    pub vault: PathBuf,
    pub out: PathBuf,
    pub obsidian: Obsidian,
    /// Set when the app was started by [`crate::launch`]; recorded in the manifest.
    pub isolation: Option<Isolation>,
    pub queries: Vec<Query>,
    /// Empty means every group.
    pub only: Vec<Group>,
    pub dom_selector: String,
    /// How long the app gets to render a note after it is opened, before its DOM is read.
    pub settle: Duration,
}

#[derive(Debug, Serialize)]
pub struct Failure {
    pub group: &'static str,
    pub call: String,
    pub error: String,
}

#[derive(Debug, Serialize)]
pub struct Gap {
    pub what: &'static str,
    pub why: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Manifest {
    pub obsidian_version: String,
    pub version_output: String,
    pub harness: &'static str,
    pub vault: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isolation: Option<Isolation>,
    pub groups: Vec<&'static str>,
    pub skipped_groups: Vec<&'static str>,
    pub commands: usize,
    /// Actual dev:dom invocations per note, including retries (zero if opening failed).
    pub dom_attempts: BTreeMap<String, usize>,
    pub notes: usize,
    pub canvases: usize,
    pub bases: usize,
    /// On disk but missing from `files`, and listed by `files` but not on disk.
    pub files_only_on_disk: Vec<String>,
    pub files_only_in_obsidian: Vec<String>,
    pub failed: Vec<Failure>,
    pub known_gaps: Vec<Gap>,
}

/// What no run can ask Obsidian, whatever the vault.
pub const KNOWN_GAPS: [Gap; 9] = [
    Gap {
        what: "graph view",
        why: "the CLI has no graph command; derive nodes and edges from eval's resolvedLinks and unresolvedLinks. Filters, groups, forces and the local graph are not queryable",
    },
    Gap {
        what: "unlinked mentions",
        why: "backlinks documents linked backlinks only",
    },
    Gap {
        what: "base views by file",
        why: "base:views lists the current base only, so view names are read from each .base file's YAML",
    },
    Gap {
        what: "JSON output shapes",
        why: "the CLI page names the formats but not their fields; answers are normalized at recording time and parsed only as far as they are JSON",
    },
    Gap {
        what: "canvas contents",
        why: "the CLI has no canvas command; canvases are covered by files, and by their links if resolvedLinks includes them",
    },
    Gap {
        what: "search results view",
        why: "explain, collapse, sort order and copy are UI; only the matches come back",
    },
    Gap {
        what: "Live Preview and Source mode",
        why: "only the reading view's DOM is captured",
    },
    Gap {
        what: "reading view completeness",
        why: "Obsidian renders long notes lazily and embeds asynchronously, so a DOM read after the settle time can miss the end of a note; the supplied recording does not prove complete rendering",
    },
    Gap {
        what: "settings and plugin UI",
        why: "workspace, ribbon, status bar, hotkeys and settings are not in the DOM of a note",
    },
];

#[derive(Default)]
struct Scan {
    notes: Vec<String>,
    canvases: Vec<String>,
    bases: Vec<String>,
    files: Vec<String>,
    statuses: BTreeSet<char>,
    task_notes: BTreeSet<String>,
}

/// Visible files of the vault, as Obsidian sees them: dot folders such as `.obsidian` are skipped.
fn scan(vault: &Path) -> Result<Scan> {
    let task = Regex::new(r"(?m)^\s*(?:[-*+]|\d+[.)])\s\[(.)\]")?;
    let mut scan = Scan::default();
    let walk = WalkDir::new(vault)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'));
    for entry in walk {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(vault)?
            .to_string_lossy()
            .replace('\\', "/");
        match entry.path().extension().and_then(|e| e.to_str()) {
            Some("md") => {
                let text = fs::read_to_string(entry.path()).unwrap_or_default();
                for c in task.captures_iter(&text) {
                    scan.task_notes.insert(rel.clone());
                    scan.statuses.extend(c[1].chars().next());
                }
                scan.notes.push(rel.clone());
            }
            Some("canvas") => scan.canvases.push(rel.clone()),
            Some("base") => scan.bases.push(rel.clone()),
            _ => {}
        }
        scan.files.push(rel);
    }
    Ok(scan)
}

/// The view names of a `.base` file, in order.
fn base_views(vault: &Path, path: &str) -> Result<Vec<String>> {
    let text = fs::read_to_string(vault.join(path))?;
    let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text)?;
    Ok(doc
        .get("views")
        .and_then(|v| v.as_sequence())
        .map(|views| {
            views
                .iter()
                .filter_map(|v| v.get("name")?.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default())
}

struct Recorder<'a> {
    obsidian: &'a Obsidian,
    normalizer: &'a Normalizer,
    records: BTreeMap<String, Record>,
}

impl Recorder<'_> {
    fn run(&self, command: &str, args: &[&str]) -> Record {
        self.normalizer.record(self.obsidian.run(command, args))
    }

    fn call(&mut self, command: &str, args: &[&str]) -> &mut Record {
        let record = self.run(command, args);
        match self.records.entry(record.key()) {
            Entry::Occupied(mut e) => {
                e.insert(record);
                e.into_mut()
            }
            Entry::Vacant(e) => e.insert(record),
        }
    }
}

/// The normalizer stores JSON eval results once, in `parsed`.
fn eval(rec: &mut Recorder, code: &str) {
    rec.call("eval", &[&format!("code={code}")]);
}

fn vault_group(rec: &mut Recorder) {
    for (command, args) in [
        ("vault", &[][..]),
        ("vault", &["info=path"]),
        ("files", &[]),
        ("files", &["total"]),
        ("folders", &[]),
        ("bases", &[]),
        ("plugins:enabled", &["filter=core", "format=json"]),
        ("plugins:enabled", &["filter=community", "format=json"]),
        ("bookmarks", &["format=json"]),
        ("templates", &[]),
        ("snippets:enabled", &[]),
    ] {
        rec.call(command, args);
    }
}

fn links_group(rec: &mut Recorder, scan: &Scan) {
    for (command, args) in [
        ("unresolved", &["verbose", "counts", "format=json"][..]),
        ("unresolved", &["total"]),
        ("orphans", &[]),
        ("orphans", &["total"]),
        ("deadends", &[]),
        ("deadends", &["total"]),
    ] {
        rec.call(command, args);
    }
    for note in &scan.notes {
        let path = format!("path={note}");
        rec.call("links", &[&path]);
        rec.call("backlinks", &[&path, "counts", "format=json"]);
    }
}

fn properties_group(rec: &mut Recorder, scan: &Scan) {
    rec.call("aliases", &["verbose"]);
    rec.call("aliases", &["total"]);
    rec.call("properties", &["counts", "format=json"]);
    for note in &scan.notes {
        let path = format!("path={note}");
        rec.call("properties", &[&path, "format=json"]);
        rec.call("aliases", &[&path]);
    }
}

fn tags_group(rec: &mut Recorder, scan: &Scan) {
    rec.call("tags", &["counts", "format=json"]);
    rec.call("tags", &["counts", "sort=count", "format=json"]);
    rec.call("tags", &["total"]);
    for note in &scan.notes {
        rec.call("tags", &[&format!("path={note}"), "counts", "format=json"]);
    }
}

fn tasks_group(rec: &mut Recorder, scan: &Scan) {
    for args in [
        &["format=json"][..],
        &["verbose"],
        &["todo", "format=json"],
        &["done", "format=json"],
        &["total"],
    ] {
        rec.call("tasks", args);
    }
    for status in scan.statuses.iter().filter(|c| **c != ' ') {
        rec.call("tasks", &[&format!("status={status}"), "format=json"]);
    }
    for note in &scan.task_notes {
        rec.call("tasks", &[&format!("path={note}"), "format=json"]);
    }
}

fn search_group(rec: &mut Recorder, queries: &[Query]) {
    for q in queries {
        let mut args = vec![format!("query={}", q.query)];
        args.extend(q.path.iter().map(|p| format!("path={p}")));
        args.extend(q.limit.iter().map(|n| format!("limit={n}")));
        args.push("format=json".into());
        if q.case {
            args.push("case".into());
        }
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        rec.call("search", &args);
        rec.call("search:context", &args);
        let total: Vec<&str> = args
            .iter()
            .copied()
            .filter(|a| *a != "format=json")
            .chain(["total"])
            .collect();
        rec.call("search", &total);
    }
}

fn bases_group(rec: &mut Recorder, vault: &Path, scan: &Scan, failed: &mut Vec<Failure>) {
    for base in &scan.bases {
        let path = format!("path={base}");
        rec.call("base:query", &[&path, "format=json"]);
        match base_views(vault, base) {
            Ok(views) => {
                for view in views {
                    rec.call(
                        "base:query",
                        &[&path, &format!("view={view}"), "format=json"],
                    );
                }
            }
            Err(e) => failed.push(Failure {
                group: "bases",
                call: format!("views of {base}"),
                error: e.to_string(),
            }),
        }
    }
}

fn eval_group(rec: &mut Recorder) {
    eval(rec, "JSON.stringify(app.metadataCache.resolvedLinks)");
    eval(rec, "JSON.stringify(app.metadataCache.unresolvedLinks)");
    eval(
        rec,
        "JSON.stringify(Object.fromEntries(app.vault.getMarkdownFiles().map(f => [f.path, app.metadataCache.getCache(f.path)])))",
    );
    eval(rec, "app.vault.getFiles().length");
}

#[derive(Serialize)]
struct Dom {
    open: Record,
    dom: Option<Record>,
    attempts: usize,
    /// Earlier failed attempts, retained even if the final call succeeds.
    retries: Vec<Record>,
}

const DOM_ATTEMPTS: usize = 3;
const DOM_RETRY_WAIT: Duration = Duration::from_millis(200);

fn opened_preview(record: &Record) -> bool {
    let text = record.stdout.trim();
    let text = text.strip_prefix("=>").map_or(text, str::trim);
    record.ok()
        && (text == "preview"
            || record.parsed.as_ref().and_then(|v| v.as_str()) == Some("preview")
            || record.eval_value().as_ref().and_then(|v| v.as_str()) == Some("preview"))
}

/// Opens each note in the reading view, waits, and stores the DOM under `dom/`.
fn dom_group(cfg: &Config, scan: &Scan, rec: &Recorder) -> Result<BTreeMap<String, Dom>> {
    let mut pairs = BTreeMap::new();
    for note in &scan.notes {
        let path = serde_json::to_string(note)?;
        let code = format!(
            "code=(async () => {{ const f = app.vault.getFileByPath({path}); if (!f) return 'missing'; const l = app.workspace.getLeaf(false); await l.openFile(f, {{ state: {{ mode: 'preview' }} }}); return l.view.getMode(); }})()"
        );
        let mut open = rec.run("eval", &[&code]);
        if !opened_preview(&open) {
            if open.ok() {
                open.error = Some(format!(
                    "note did not open in preview: {}",
                    open.parsed
                        .as_ref()
                        .map_or_else(|| open.stdout.trim().to_string(), ToString::to_string)
                ));
            }
            pairs.insert(
                note.clone(),
                Dom {
                    open,
                    dom: None,
                    attempts: 0,
                    retries: Vec::new(),
                },
            );
            continue;
        }
        if !cfg.settle.is_zero() {
            std::thread::sleep(cfg.settle);
        }
        let mut retries = Vec::new();
        let mut dom = loop {
            let dom = rec.run("dev:dom", &[&format!("selector={}", cfg.dom_selector)]);
            // 1.14.5's dispatcher emits this before running a handler. A selector miss instead
            // returns "No elements found."; do not retry unrelated errors or hide exhaustion.
            if !dom
                .stdout
                .trim_start()
                .starts_with("Error: Command \"dev:dom\" not found.")
                || retries.len() + 1 == DOM_ATTEMPTS
            {
                break dom;
            }
            retries.push(dom);
            std::thread::sleep(DOM_RETRY_WAIT);
        };
        if dom.ok() && (dom.stdout.trim().is_empty() || dom.stdout.trim() == "No elements found.") {
            dom.error = Some("reading view selector returned no DOM".into());
        }
        let file = format!("dom/{note}.html");
        let to = cfg.out.join(&file);
        fs::create_dir_all(to.parent().context("note without a folder")?)?;
        fs::write(&to, &dom.stdout)?;
        dom.stdout.clear();
        dom.stdout_file = Some(file);
        pairs.insert(
            note.clone(),
            Dom {
                open,
                dom: Some(dom),
                attempts: retries.len() + 1,
                retries,
            },
        );
    }
    Ok(pairs)
}

fn fill(group: Group, cfg: &Config, scan: &Scan, rec: &mut Recorder, failed: &mut Vec<Failure>) {
    match group {
        Group::Vault => vault_group(rec),
        Group::Links => links_group(rec, scan),
        Group::Outline => scan.notes.iter().for_each(|n| {
            rec.call("outline", &[&format!("path={n}"), "format=json"]);
        }),
        Group::Properties => properties_group(rec, scan),
        Group::Tags => tags_group(rec, scan),
        Group::Tasks => tasks_group(rec, scan),
        Group::Search => search_group(rec, &cfg.queries),
        Group::Bases => bases_group(rec, &cfg.vault, scan, failed),
        Group::Wordcount => scan.notes.iter().for_each(|n| {
            rec.call("wordcount", &[&format!("path={n}")]);
        }),
        Group::Eval => eval_group(rec),
        Group::Dom => {}
    }
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

/// Runs the groups and writes `<out>/<group>.json` for each, and `<out>/manifest.json`.
/// Refuses before writing anything unless Obsidian reports exactly [`OBSIDIAN_VERSION`].
pub fn run(cfg: &Config) -> Result<Manifest> {
    let version = cfg.obsidian.run("version", &[]);
    if !version.ok() {
        bail!(
            "`obsidian version` failed: {} {}\nThe CLI needs the 1.12.7+ installer, a registered `obsidian` command and a running app: https://obsidian.md/help/cli#Troubleshooting",
            version.error.unwrap_or_default(),
            version.stderr.trim()
        );
    }
    let found = parse_version(&version.stdout).with_context(|| {
        format!(
            "no version number in `obsidian version`: {:?}",
            version.stdout
        )
    })?;
    if found != OBSIDIAN_VERSION {
        bail!(
            "refusing to run: Obsidian reports {found}, and the reference is recorded against {OBSIDIAN_VERSION} only"
        );
    }
    let normalizer = Normalizer::new(&cfg.vault, cfg.isolation.as_ref())?;
    let version = normalizer.record(version);
    let scan = scan(&cfg.vault)?;
    fs::create_dir_all(&cfg.out)?;
    let (mut groups, mut skipped, mut failed, mut commands) =
        (Vec::new(), Vec::new(), Vec::new(), 1);
    let mut obsidian_files = None;
    let mut dom_attempts = BTreeMap::new();
    for group in Group::ALL {
        if !cfg.only.is_empty() && !cfg.only.contains(&group) {
            skipped.push(group.name());
            continue;
        }
        let name = group.name();
        let mut rec = Recorder {
            obsidian: &cfg.obsidian,
            normalizer: &normalizer,
            records: BTreeMap::new(),
        };
        if group == Group::Dom {
            let pairs = dom_group(cfg, &scan, &rec)?;
            for (note, p) in &pairs {
                commands += 1 + p.attempts;
                dom_attempts.insert(note.clone(), p.attempts);
                for r in std::iter::once(&p.open).chain(p.dom.iter()) {
                    if let Some(error) = &r.error {
                        failed.push(Failure {
                            group: name,
                            call: format!("{} ({note})", r.command),
                            error: error.clone(),
                        });
                    }
                }
            }
            write_json(&cfg.out.join("dom.json"), &pairs)?;
        } else {
            fill(group, cfg, &scan, &mut rec, &mut failed);
            if group == Group::Vault {
                obsidian_files = rec.records.get("files").map(|r| r.stdout.clone());
            }
            commands += rec.records.len();
            failed.extend(rec.records.values().filter(|r| !r.ok()).map(|r| {
                Failure {
                    group: name,
                    call: r.key(),
                    error: format!(
                        "{} {}",
                        r.error.clone().unwrap_or_default(),
                        r.stderr.trim()
                    )
                    .trim()
                    .to_string(),
                }
            }));
            write_json(&cfg.out.join(format!("{name}.json")), &rec.records)?;
        }
        eprintln!("{name}: done");
        groups.push(name);
    }
    let listed: BTreeSet<String> = obsidian_files
        .iter()
        .flat_map(|s| s.lines())
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    let disk: BTreeSet<String> = scan.files.iter().cloned().collect();
    let compare = !listed.is_empty();
    for failure in &mut failed {
        failure.error = normalizer.text(&failure.error);
        failure.call = normalizer.text(&failure.call);
    }
    let manifest = Manifest {
        obsidian_version: found,
        version_output: version.stdout.trim().to_string(),
        harness: concat!(env!("CARGO_PKG_NAME"), " ", env!("CARGO_PKG_VERSION")),
        vault: normalizer.vault_path(&cfg.vault)?,
        isolation: cfg.isolation.as_ref().map(|i| normalizer.isolation(i)),
        groups,
        skipped_groups: skipped,
        commands,
        dom_attempts,
        notes: scan.notes.len(),
        canvases: scan.canvases.len(),
        bases: scan.bases.len(),
        files_only_on_disk: if compare {
            disk.difference(&listed).cloned().collect()
        } else {
            Vec::new()
        },
        files_only_in_obsidian: if compare {
            listed.difference(&disk).cloned().collect()
        } else {
            Vec::new()
        },
        failed,
        known_gaps: KNOWN_GAPS.into_iter().collect(),
    };
    write_json(&cfg.out.join("manifest.json"), &manifest)?;
    Ok(manifest)
}
