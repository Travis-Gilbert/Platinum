//! The reference harness against a fake `obsidian` executable: it must issue only documented
//! commands, refuse any version but 1.14.5 before writing anything, and record what fails.
mod common;

use common::{Call, Fake, crate_dir, scratch, vault, visible_files};
use notes_fixtures::{
    OBSIDIAN_VERSION,
    reference::{self, Config, DOM_SELECTOR, Group, Manifest, Query},
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
    time::Duration,
};

fn config(fake: &Fake, vault: &Path, out: &Path) -> Config {
    Config {
        vault: vault.to_path_buf(),
        out: out.to_path_buf(),
        obsidian: fake.client(vault),
        isolation: None,
        queries: reference::default_queries().unwrap(),
        only: Vec::new(),
        dom_selector: DOM_SELECTOR.into(),
        settle: Duration::ZERO,
    }
}

fn json(path: &Path) -> Value {
    serde_json::from_str(
        &fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
    )
    .unwrap()
}

/// One full run over the fixture vault, shared by the tests that read its results.
struct Full {
    vault: PathBuf,
    out: PathBuf,
    calls: Vec<Call>,
    manifest: Manifest,
}

fn full() -> &'static Full {
    static FULL: OnceLock<Full> = OnceLock::new();
    FULL.get_or_init(|| {
        let fake = Fake::new("ref-full");
        let vault = vault().canonicalize().unwrap();
        let out = fake.dir.join("out");
        let manifest = reference::run(&config(&fake, &vault, &out)).unwrap();
        Full {
            vault,
            out,
            calls: fake.calls(),
            manifest,
        }
    })
}

// What https://obsidian.md/help/cli documents for each command the harness may use: its `key=value`
// parameters, the values `format=` takes, and its bare-word flags (copied from the page as of
// 2026-10-26). A call outside this table is a call the page does not describe.
type Spec = (
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
    &'static [&'static str],
);
const DOCUMENTED: &[Spec] = &[
    ("version", &[], &[], &[]),
    ("vault", &["info"], &[], &[]),
    ("files", &["folder", "ext"], &[], &["total"]),
    ("folders", &["folder"], &[], &["total"]),
    ("bases", &[], &[], &[]),
    (
        "plugins:enabled",
        &["filter", "format"],
        &["json", "tsv", "csv"],
        &["versions"],
    ),
    (
        "bookmarks",
        &["format"],
        &["json", "tsv", "csv"],
        &["total", "verbose"],
    ),
    ("templates", &[], &[], &["total"]),
    ("snippets:enabled", &[], &[], &[]),
    ("links", &["file", "path"], &[], &["total"]),
    (
        "backlinks",
        &["file", "path", "format"],
        &["json", "tsv", "csv"],
        &["counts", "total"],
    ),
    (
        "unresolved",
        &["format"],
        &["json", "tsv", "csv"],
        &["total", "counts", "verbose"],
    ),
    ("orphans", &[], &[], &["total"]),
    ("deadends", &[], &[], &["total"]),
    (
        "outline",
        &["file", "path", "format"],
        &["tree", "md", "json"],
        &["total"],
    ),
    (
        "aliases",
        &["file", "path"],
        &[],
        &["total", "verbose", "active"],
    ),
    (
        "properties",
        &["file", "path", "name", "sort", "format"],
        &["yaml", "json", "tsv"],
        &["total", "counts", "active"],
    ),
    (
        "tags",
        &["file", "path", "sort", "format"],
        &["json", "tsv", "csv"],
        &["total", "counts", "active"],
    ),
    (
        "tasks",
        &["file", "path", "status", "format"],
        &["json", "tsv", "csv"],
        &["total", "done", "todo", "verbose", "active", "daily"],
    ),
    (
        "search",
        &["query", "path", "limit", "format"],
        &["text", "json"],
        &["total", "case"],
    ),
    (
        "search:context",
        &["query", "path", "limit", "format"],
        &["text", "json"],
        &["case"],
    ),
    (
        "base:query",
        &["file", "path", "view", "format"],
        &["json", "csv", "tsv", "md", "paths"],
        &[],
    ),
    (
        "wordcount",
        &["file", "path"],
        &[],
        &["words", "characters"],
    ),
    ("eval", &["code"], &[], &[]),
    (
        "dev:dom",
        &["selector", "attr", "css"],
        &[],
        &["total", "text", "inner", "all"],
    ),
];

/// Why a call is not one the CLI page describes, or `None` when it is.
fn undocumented(call: &Call) -> Option<String> {
    let Some((_, params, formats, flags)) = DOCUMENTED.iter().find(|s| s.0 == call.command())
    else {
        return Some(format!("command {}", call.command()));
    };
    let mut seen = BTreeSet::new();
    for arg in call.args() {
        let (word, value) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(k, v)| (k, Some(v)));
        let ok = match value {
            // `code=` and `selector=` carry arbitrary text, so a stray `=` in a flag is the thing to catch.
            Some(v) if word == "format" => params.contains(&word) && formats.contains(&v),
            Some(_) => params.contains(&word),
            None => flags.contains(&word),
        };
        if !ok {
            return Some(format!("argument {arg:?}"));
        }
        if !seen.insert(word.to_string()) {
            return Some(format!("{word} given twice"));
        }
    }
    None
}

#[test]
fn the_table_of_documented_calls_rejects_what_it_should() {
    let call = |args: &[&str]| Call {
        cwd: String::new(),
        argv: args.iter().map(|a| a.to_string()).collect(),
    };
    assert_eq!(undocumented(&call(&["links", "path=a.md"])), None);
    assert_eq!(
        undocumented(&call(&[
            "vault=V",
            "backlinks",
            "path=a.md",
            "counts",
            "format=json"
        ])),
        None
    );
    assert!(
        undocumented(&call(&["links", "path=a.md", "format=json"])).is_some(),
        "links has no format"
    );
    assert!(undocumented(&call(&["search", "query=a", "format=yaml"])).is_some());
    assert!(undocumented(&call(&["search", "query=a", "total", "total"])).is_some());
    assert!(undocumented(&call(&["graph"])).is_some());
    assert!(undocumented(&call(&["tasks", "todo=1"])).is_some());
}

#[test]
fn every_call_is_documented() {
    let run = full();
    let bad: Vec<String> = run
        .calls
        .iter()
        .filter_map(|c| undocumented(c).map(|why| format!("{}: {why}", c.line())))
        .collect();
    assert!(
        bad.is_empty(),
        "{} undocumented calls:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

#[test]
fn version_is_asked_first_and_the_vault_is_the_working_directory() {
    let run = full();
    assert_eq!(run.calls[0].argv, ["version"]);
    for call in &run.calls {
        assert_eq!(
            Path::new(&call.cwd).canonicalize().unwrap(),
            run.vault,
            "{}",
            call.line()
        );
        assert!(
            !call.argv[0].starts_with("vault="),
            "no vault name was given: {}",
            call.line()
        );
    }
}

#[test]
fn a_vault_name_comes_first_on_every_call() {
    let fake = Fake::new("ref-vault-name");
    let vault = vault().canonicalize().unwrap();
    let out = fake.dir.join("out");
    let mut cfg = config(&fake, &vault, &out);
    cfg.obsidian = fake.client(&vault).vault_name(Some("My Vault".into()));
    cfg.only = vec![Group::Vault, Group::Tags];
    reference::run(&cfg).unwrap();
    let calls = fake.calls();
    assert!(calls.len() > 10);
    for call in &calls {
        assert_eq!(call.argv[0], "vault=My Vault", "{}", call.line());
        assert!(
            !call.argv[1].contains('='),
            "the command follows: {}",
            call.line()
        );
        assert_eq!(
            call.argv.iter().filter(|a| a.starts_with("vault=")).count(),
            1
        );
    }
}

#[test]
fn the_manifest_says_what_was_run() {
    let run = full();
    let m = &run.manifest;
    assert_eq!(m.obsidian_version, OBSIDIAN_VERSION);
    assert_eq!(m.version_output, "1.14.5 (installer 1.12.7)");
    assert_eq!(m.groups, Group::ALL.map(Group::name));
    assert!(m.skipped_groups.is_empty());
    assert!(m.failed.is_empty(), "{:?}", m.failed);
    assert!(m.stalled.is_empty(), "{:?}", m.stalled);
    assert_eq!(
        m.commands,
        run.calls.len(),
        "every process the harness started is counted once"
    );
    let files = visible_files(&run.vault);
    let ext = |e: &str| files.iter().filter(|f| f.ends_with(e)).count();
    assert_eq!(
        (m.notes, m.canvases, m.bases),
        (ext(".md"), ext(".canvas"), ext(".base"))
    );
    assert!(m.files_only_on_disk.is_empty() && m.files_only_in_obsidian.is_empty());
    assert_eq!(m.known_gaps.len(), reference::KNOWN_GAPS.len());

    let on_disk = json(&run.out.join("manifest.json"));
    assert_eq!(on_disk["obsidian_version"], "1.14.5");
    assert_eq!(on_disk["commands"], m.commands);
    assert!(
        on_disk["known_gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["what"] == "graph view")
    );
}

#[test]
fn each_group_writes_its_file_of_records() {
    let run = full();
    for group in Group::ALL {
        let file = run.out.join(format!("{}.json", group.name()));
        let records = json(&file);
        let records = records
            .as_object()
            .unwrap_or_else(|| panic!("{} is not a map", file.display()));
        assert!(!records.is_empty(), "{} is empty", group.name());
        for (key, r) in records {
            if group == Group::Dom {
                assert_eq!(r["open"]["exit_code"], 0, "{key}");
                assert_eq!(r["dom"]["exit_code"], 0, "{key}");
            } else {
                assert_eq!(r["exit_code"], 0, "{} {key}", group.name());
                assert!(
                    r["command"].is_string() && r["args"].is_array() && r["stdout"].is_string(),
                    "{key}"
                );
            }
        }
    }
}

#[test]
fn records_keep_content_and_parse_json_when_asked() {
    let run = full();
    let links = json(&run.out.join("links.json"));
    assert_eq!(links["links path=Home.md"]["stdout"], "Dead End.md\n");
    assert!(
        links["links path=Home.md"].get("parsed").is_none(),
        "links has no JSON mode"
    );
    let backlinks = &links["backlinks path=Home.md counts format=json"];
    assert_eq!(backlinks["parsed"][0]["file"], "Home.md");
    assert_eq!(
        links["unresolved verbose counts format=json"]["parsed"][0]["link"],
        "Missing"
    );
    assert_eq!(links["orphans total"]["stdout"], "1\n");
    let vault = json(&run.out.join("vault.json"));
    let total: usize = vault["files total"]["stdout"]
        .as_str()
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(total, visible_files(&run.vault).len());
    assert!(
        vault["folders"]["stdout"]
            .as_str()
            .unwrap()
            .lines()
            .any(|l| l == "callouts")
    );
    assert_eq!(
        vault["bases"]["stdout"].as_str().unwrap().lines().count(),
        3
    );
}

#[test]
fn eval_dumps_are_stored_once_as_json() {
    let run = full();
    let eval = json(&run.out.join("eval.json"));
    let resolved = &eval["eval code=JSON.stringify(app.metadataCache.resolvedLinks)"];
    assert_eq!(resolved["parsed"]["Home.md"]["Dead End.md"], 1);
    assert_eq!(
        resolved["stdout"], "",
        "the text is dropped once it is parsed"
    );
    let unresolved = &eval["eval code=JSON.stringify(app.metadataCache.unresolvedLinks)"];
    assert_eq!(unresolved["parsed"]["Home.md"]["Missing"], 1);
    let cache = eval
        .as_object()
        .unwrap()
        .iter()
        .find(|(k, _)| k.contains("getCache"))
        .unwrap()
        .1;
    assert!(
        cache["parsed"]["Home.md"].is_object(),
        "a JSON string holding JSON is unwrapped"
    );
    assert_eq!(eval.as_object().unwrap().len(), 4);
}

#[test]
fn every_note_is_asked_about_per_file() {
    let run = full();
    let lines: BTreeSet<String> = run.calls.iter().map(Call::line).collect();
    let notes: Vec<String> = visible_files(&run.vault)
        .into_iter()
        .filter(|f| f.ends_with(".md"))
        .collect();
    assert!(notes.len() > 60);
    for note in &notes {
        for prefix in [
            "links",
            "backlinks",
            "outline",
            "properties",
            "aliases",
            "tags",
            "wordcount",
        ] {
            assert!(
                lines
                    .iter()
                    .any(|l| l.starts_with(&format!("{prefix} path={note}"))),
                "{prefix} for {note}"
            );
        }
    }
    assert!(lines.contains("backlinks path=Home.md counts format=json"));
    assert!(lines.contains("outline path=links/Headings Target.md format=json"));
    assert!(lines.contains("properties path=properties/All Property Types.md format=json"));
    assert!(lines.contains("tags path=tagging/Tags.md counts format=json"));
}

#[test]
fn tasks_are_asked_for_by_each_status_character() {
    let run = full();
    let lines: BTreeSet<String> = run.calls.iter().map(Call::line).collect();
    for status in [
        'x', 'X', '/', '-', '>', '<', '?', '!', '*', '"', 'l', 'b', 'i', 'S', 'I', 'p', 'c', 'f',
        'k', 'w', 'u', 'd', '😀',
    ] {
        assert!(
            lines.contains(&format!("tasks status={status} format=json")),
            "status {status}"
        );
    }
    assert!(
        !lines.contains("tasks status=  format=json"),
        "a blank status is the open tasks, which `todo` asks for"
    );
    assert!(
        lines.contains("tasks todo format=json")
            && lines.contains("tasks done format=json")
            && lines.contains("tasks verbose")
    );
    assert!(lines.contains("tasks path=tasks/Task Statuses.md format=json"));
    assert!(
        !lines.contains("tasks path=Orphan.md format=json"),
        "a note without tasks is not asked"
    );
}

#[test]
fn base_views_are_read_from_the_yaml() {
    let run = full();
    let lines: BTreeSet<String> = run.calls.iter().map(Call::line).collect();
    let expected = [
        (
            "bases/Notes.base",
            vec!["All notes", "Cards", "With a status"],
        ),
        (
            "bases/Projects.base",
            vec!["Board", "By status", "Not done", "Gallery"],
        ),
        ("bases/Tasks.base", vec!["By tag", "Names"]),
    ];
    for (base, views) in expected {
        assert!(
            lines.contains(&format!("base:query path={base} format=json")),
            "{base}"
        );
        for view in views {
            assert!(
                lines.contains(&format!("base:query path={base} view={view} format=json")),
                "{base} {view}"
            );
        }
    }
    let bases = json(&run.out.join("bases.json"));
    assert_eq!(
        bases["base:query path=bases/Projects.base view=Board format=json"]["parsed"][0]["view"],
        "Board"
    );
}

#[test]
fn searches_come_in_threes_with_the_documented_parameters() {
    let fake = Fake::new("ref-search");
    let vault = vault().canonicalize().unwrap();
    let out = fake.dir.join("out");
    let mut cfg = config(&fake, &vault, &out);
    cfg.only = vec![Group::Search];
    cfg.queries = vec![
        Query {
            query: "two words".into(),
            path: None,
            limit: None,
            case: false,
        },
        Query {
            query: "Apple".into(),
            path: Some("search".into()),
            limit: Some(5),
            case: true,
        },
    ];
    reference::run(&cfg).unwrap();
    let lines: Vec<String> = fake.calls().iter().skip(1).map(Call::line).collect();
    assert_eq!(
        lines,
        [
            "search query=two words format=json",
            "search:context query=two words format=json",
            "search query=two words total",
            "search query=Apple path=search limit=5 format=json case",
            "search:context query=Apple path=search limit=5 format=json case",
            "search query=Apple path=search limit=5 case total",
        ]
    );
    let records = json(&out.join("search.json"));
    assert!(
        records.get("search query=\"two words\" total").is_some(),
        "keys quote values with spaces"
    );
    assert_eq!(
        records["search query=\"two words\" format=json"]["parsed"][0],
        "Home.md"
    );
    assert_eq!(
        records["search:context query=\"two words\" format=json"]["parsed"][0]["line"],
        1
    );
}

#[test]
fn the_bundled_queries_cover_the_search_operators() {
    let queries = reference::default_queries().unwrap();
    assert!(queries.len() >= 30);
    let text: Vec<&str> = queries.iter().map(|q| q.query.as_str()).collect();
    for operator in [
        "file:",
        "path:",
        "content:",
        "match-case:",
        "ignore-case:",
        "tag:#",
        "line:(",
        "block:(",
        "section:(",
        "task:",
        "task-todo:",
        "task-done:",
        " OR ",
        "-",
        "\"",
        "/",
        "[status:",
        "(",
    ] {
        assert!(
            text.iter().any(|q| q.contains(operator)),
            "no query uses {operator}"
        );
    }
    assert!(queries.iter().any(|q| q.case), "the case flag");
    assert!(
        queries.iter().any(|q| q.path.is_some()),
        "the path parameter"
    );
    assert!(
        queries.iter().any(|q| q.limit.is_some()),
        "the limit parameter"
    );
    let unique: BTreeSet<&str> = text.iter().copied().collect();
    assert_eq!(
        unique.len(),
        text.len(),
        "a repeated query would overwrite its own record"
    );
}

#[test]
fn the_reading_view_of_each_note_is_stored() {
    let run = full();
    let dom = json(&run.out.join("dom.json"));
    let notes: Vec<String> = visible_files(&run.vault)
        .into_iter()
        .filter(|f| f.ends_with(".md"))
        .collect();
    assert_eq!(dom.as_object().unwrap().len(), notes.len());
    for note in &notes {
        let entry = &dom[note.as_str()];
        assert!(
            entry["open"]["args"][0]
                .as_str()
                .unwrap()
                .contains(&serde_json::to_string(note).unwrap()),
            "{note}: opened by path"
        );
        assert!(
            entry["open"]["args"][0]
                .as_str()
                .unwrap()
                .contains("mode: 'preview'"),
            "{note}: in the reading view"
        );
        assert_eq!(
            entry["dom"]["stdout"], "",
            "the markup lives in its own file"
        );
        let file = entry["dom"]["stdout_file"].as_str().unwrap();
        assert_eq!(file, format!("dom/{note}.html"));
        let html = fs::read_to_string(run.out.join(file)).unwrap();
        assert!(
            html.contains(&format!(
                "fake dom for {}",
                serde_json::to_string(note).unwrap()
            )),
            "{note}: the DOM read is the one just opened"
        );
        assert!(html.contains(&format!("data-selector=\"{DOM_SELECTOR}\"")));
    }
    // The note is opened before its DOM is read, every time, and an unchanging DOM is read
    // three times.
    let order: Vec<&str> = run
        .calls
        .iter()
        .filter(|c| matches!(c.command(), "eval" | "dev:dom"))
        .map(Call::command)
        .collect();
    let dom_calls = &order[order.len() - 4 * notes.len()..];
    assert!(
        dom_calls
            .chunks(4)
            .all(|p| p == ["eval", "dev:dom", "dev:dom", "dev:dom"])
    );
    assert!(run.manifest.dom_attempts.values().all(|n| *n == 3));
    assert!(notes.iter().all(|n| dom[n.as_str()]["changes"] == 0));
}

#[test]
fn only_runs_the_chosen_groups() {
    let fake = Fake::new("ref-only");
    let vault = vault().canonicalize().unwrap();
    let out = fake.dir.join("out");
    let mut cfg = config(&fake, &vault, &out);
    cfg.only = vec![Group::Tags, Group::Wordcount];
    let m = reference::run(&cfg).unwrap();
    assert_eq!(m.groups, ["tags", "wordcount"]);
    assert_eq!(m.skipped_groups.len(), 9);
    let written: BTreeSet<String> = fs::read_dir(&out)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        written,
        ["manifest.json", "tags.json", "wordcount.json"]
            .map(String::from)
            .into()
    );
    let commands: BTreeSet<String> = fake
        .calls()
        .iter()
        .map(|c| c.command().to_string())
        .collect();
    assert_eq!(
        commands,
        ["version", "tags", "wordcount"].map(String::from).into()
    );
}

#[test]
fn a_failing_command_is_recorded_and_the_run_goes_on() {
    let fake = Fake::new("ref-fail");
    let vault = vault().canonicalize().unwrap();
    let out = fake.dir.join("out");
    let mut cfg = config(&fake, &vault, &out);
    cfg.obsidian = fake.client(&vault).env("FAKE_OBSIDIAN_FAIL", "orphans");
    cfg.only = vec![Group::Links, Group::Tags];
    let m = reference::run(&cfg).unwrap();
    let calls: BTreeSet<&str> = m.failed.iter().map(|f| f.call.as_str()).collect();
    assert_eq!(calls, ["orphans", "orphans total"].into(), "{:?}", m.failed);
    assert!(m.failed.iter().all(|f| f.group == "links"
        && f.error.contains("exited with")
        && f.error.contains("Error: orphans failed")));
    let links = json(&out.join("links.json"));
    assert_eq!(links["orphans"]["exit_code"], 1);
    assert_eq!(links["orphans"]["stderr"], "Error: orphans failed\n");
    assert!(links["orphans"]["error"].is_string());
    assert_eq!(
        links["deadends"]["exit_code"], 0,
        "the commands after it still ran"
    );
    assert!(out.join("tags.json").exists(), "and so did the next group");
    assert_eq!(
        json(&out.join("manifest.json"))["failed"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

/// The real app reports a failed command as `Error: ...` on a successful exit.
#[test]
fn an_error_printed_on_a_clean_exit_is_a_failed_call() {
    let fake = Fake::new("ref-error-text");
    let vault = vault().canonicalize().unwrap();
    let out = fake.dir.join("out");
    let mut cfg = config(&fake, &vault, &out);
    cfg.obsidian = fake.client(&vault).env("FAKE_OBSIDIAN_ERROR", "orphans");
    cfg.only = vec![Group::Links];
    let m = reference::run(&cfg).unwrap();
    assert!(
        m.failed.iter().any(|f| f.call == "orphans"),
        "{:?}",
        m.failed
    );
    let links = json(&out.join("links.json"));
    assert_eq!(links["orphans"]["exit_code"], 0);
    assert!(
        links["orphans"]["error"]
            .as_str()
            .is_some_and(|e| e.contains("Error: orphans could not be run")),
        "{}",
        links["orphans"]
    );
    assert_eq!(links["deadends"]["exit_code"], 0);
}

/// A call the app never answers is killed at its timeout, run twice more, and recorded as failed;
/// the run goes on.
#[test]
fn a_call_that_never_answers_is_killed_and_recorded() {
    let fake = Fake::new("ref-hang");
    let vault = vault().canonicalize().unwrap();
    let out = fake.dir.join("out");
    let mut cfg = config(&fake, &vault, &out);
    cfg.obsidian = fake
        .client(&vault)
        .env("FAKE_OBSIDIAN_HANG", "orphans")
        .timeout(Duration::from_millis(700));
    cfg.only = vec![Group::Links];
    let started = std::time::Instant::now();
    let m = reference::run(&cfg).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "{:?}",
        started.elapsed()
    );
    assert!(
        m.failed.iter().any(|f| f.call == "orphans"),
        "{:?}",
        m.failed
    );
    let links = json(&out.join("links.json"));
    assert!(links["orphans"]["exit_code"].is_null());
    assert!(
        links["orphans"]["error"]
            .as_str()
            .is_some_and(|e| e.contains("no answer within")),
        "{}",
        links["orphans"]
    );
    assert_eq!(links["deadends"]["exit_code"], 0);
    // The fake hangs on every `orphans` call, `orphans total` too: two stalls and a failure each.
    let stalled: Vec<_> = m.stalled.iter().map(|f| (f.group, f.call.as_str())).collect();
    assert_eq!(
        stalled,
        [
            ("links", "orphans"),
            ("links", "orphans"),
            ("links", "orphans total"),
            ("links", "orphans total")
        ]
    );
    let orphans = fake.calls().iter().filter(|c| c.command() == "orphans").count();
    assert_eq!(orphans, 6);
    assert_eq!(m.commands, fake.calls().len());
}

/// A call that stalls once is run again; its answer is recorded and the stalled attempt is kept
/// in the manifest.
#[test]
fn a_call_that_stalls_once_is_run_again() {
    let fake = Fake::new("ref-stall");
    let vault = vault().canonicalize().unwrap();
    let out = fake.dir.join("out");
    let mut cfg = config(&fake, &vault, &out);
    cfg.obsidian = fake
        .client(&vault)
        .env("FAKE_OBSIDIAN_HANG_ONCE", "orphans")
        .timeout(Duration::from_millis(700));
    cfg.only = vec![Group::Links];
    let m = reference::run(&cfg).unwrap();
    assert!(m.failed.is_empty(), "{:?}", m.failed);
    assert_eq!(m.stalled.len(), 1, "{:?}", m.stalled);
    assert_eq!((m.stalled[0].group, m.stalled[0].call.as_str()), ("links", "orphans"));
    assert!(m.stalled[0].error.contains("no answer within"), "{:?}", m.stalled);
    assert_eq!(m.commands, fake.calls().len());
    let links = json(&out.join("links.json"));
    assert_eq!(links["orphans"]["exit_code"], 0);
    assert!(links["orphans"]["error"].is_null());
    let on_disk = json(&out.join("manifest.json"));
    assert_eq!(on_disk["stalled"][0]["call"], "orphans");
    assert!(on_disk["failed"].as_array().unwrap().is_empty());
}

/// Stalled attempts in the DOM group count as commands too.
#[test]
fn a_dom_call_that_stalls_once_is_run_again() {
    let fake = Fake::new("ref-dom-stall");
    let mut cfg = dom_config(&fake);
    cfg.obsidian = fake
        .client(&cfg.vault)
        .env("FAKE_OBSIDIAN_HANG_ONCE", "dev:dom")
        .timeout(Duration::from_millis(700));
    let m = reference::run(&cfg).unwrap();
    assert!(m.failed.is_empty(), "{:?}", m.failed);
    assert_eq!(m.stalled.len(), 1, "{:?}", m.stalled);
    assert_eq!(m.stalled[0].group, "dom");
    assert_eq!(m.commands, 6);
    assert_eq!(m.commands, fake.calls().len());
    assert_eq!(m.dom_attempts["Note.md"], 3);
    assert!(
        fs::read_to_string(cfg.out.join("dom/Note.md.html"))
            .unwrap()
            .contains("fake dom")
    );
}

#[test]
fn a_file_list_that_disagrees_with_the_disk_is_reported() {
    let fake = Fake::new("ref-files");
    let vault = vault().canonicalize().unwrap();
    let out = fake.dir.join("out");
    let mut cfg = config(&fake, &vault, &out);
    cfg.obsidian = fake
        .client(&vault)
        .env("FAKE_OBSIDIAN_FILES_HIDE", "Orphan.md")
        .env("FAKE_OBSIDIAN_FILES_EXTRA", "Invented.md");
    cfg.only = vec![Group::Vault];
    let m = reference::run(&cfg).unwrap();
    assert_eq!(m.files_only_on_disk, ["Orphan.md"]);
    assert_eq!(m.files_only_in_obsidian, ["Invented.md"]);
}

/// The refusal comes before the first write, whatever the reason the version is not right.
#[test]
fn any_other_version_is_refused_and_nothing_is_written() {
    let vault = vault().canonicalize().unwrap();
    for (reported, message) in [
        ("1.12.7 (installer 1.12.7)", "Obsidian reports 1.12.7"),
        ("1.14.4", "Obsidian reports 1.14.4"),
        ("1.14.50", "Obsidian reports 1.14.50"),
        ("1.15.0", "Obsidian reports 1.15.0"),
        ("2.0.0", "Obsidian reports 2.0.0"),
        ("Obsidian", "no version number"),
        ("", "no version number"),
    ] {
        let fake = Fake::new("ref-version");
        let out = fake.dir.join("out");
        let mut cfg = config(&fake, &vault, &out);
        cfg.obsidian = fake.client(&vault).env("FAKE_OBSIDIAN_VERSION", reported);
        let err = reference::run(&cfg).unwrap_err().to_string();
        assert!(err.contains(message), "{reported:?}: {err}");
        if reported.contains('.') {
            assert!(
                err.contains("refusing to run") && err.contains("1.14.5"),
                "{err}"
            );
        }
        assert!(
            !out.exists(),
            "{reported:?}: the output directory must not be created"
        );
        assert_eq!(
            fake.calls().len(),
            1,
            "{reported:?}: only `version` was asked"
        );
    }
}

#[test]
fn a_cli_that_cannot_answer_is_an_error_before_any_write() {
    let vault = vault().canonicalize().unwrap();
    let fake = Fake::new("ref-version-fail");
    let out = fake.dir.join("out");
    let mut cfg = config(&fake, &vault, &out);
    cfg.obsidian = fake.client(&vault).env("FAKE_OBSIDIAN_FAIL", "version");
    let err = reference::run(&cfg).unwrap_err().to_string();
    assert!(
        err.contains("`obsidian version` failed") && err.contains("obsidian.md/help/cli"),
        "{err}"
    );
    assert!(!out.exists());

    let missing = scratch("ref-no-program").join("nothing/obsidian");
    let mut cfg = config(&fake, &vault, &out);
    cfg.obsidian = notes_fixtures::obsidian::Obsidian::new(missing, &vault);
    let err = reference::run(&cfg).unwrap_err().to_string();
    assert!(err.contains("could not run"), "{err}");
    assert!(!out.exists());
}

// The same, through the binary: a fake `obsidian` first on PATH and no `--obsidian` flag.

fn binary(fake: &Fake, args: &[&str], envs: &[(&str, &str)]) -> std::process::Output {
    let path = format!(
        "{}:{}",
        fake.bin().display(),
        std::env::var("PATH").unwrap()
    );
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_notes-fixtures"));
    cmd.args(args)
        .env("PATH", path)
        .env("FAKE_OBSIDIAN_LOG", &fake.log);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

#[test]
fn the_binary_finds_obsidian_on_path() {
    let fake = Fake::new("ref-bin");
    let out = fake.dir.join("out");
    let vault = vault();
    let run = binary(
        &fake,
        &[
            "reference",
            "--attach",
            "--vault",
            vault.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--only",
            "vault,tags,eval",
            "--settle-ms",
            "0",
        ],
        &[],
    );
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "{stderr}");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        stdout.starts_with("Obsidian 1.14.5: ") && stdout.contains(" 0 failed, "),
        "{stdout}"
    );
    assert!(stdout.contains(out.to_str().unwrap()), "{stdout}");
    for done in ["vault: done", "tags: done", "eval: done"] {
        assert!(stderr.contains(done), "{stderr}");
    }
    let calls = fake.calls();
    assert_eq!(calls[0].argv, ["version"]);
    assert!(
        calls
            .iter()
            .all(|c| Path::new(&c.cwd).canonicalize().unwrap() == vault.canonicalize().unwrap())
    );
    assert_eq!(
        json(&out.join("manifest.json"))["obsidian_version"],
        "1.14.5"
    );
}

#[test]
fn the_binary_takes_a_vault_name_queries_and_a_selector() {
    let fake = Fake::new("ref-bin-flags");
    let out = fake.dir.join("out");
    let queries = fake.dir.join("queries.json");
    fs::write(&queries, r#"[{"query": "needle", "limit": 3}]"#).unwrap();
    let obsidian = fake.program();
    let vault = vault();
    let run = binary(
        &fake,
        &[
            "reference",
            "--attach",
            "--vault",
            vault.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--obsidian",
            obsidian.to_str().unwrap(),
            "--vault-name",
            "Fixture",
            "--queries",
            queries.to_str().unwrap(),
            "--only",
            "search,dom",
            "--dom-selector",
            ".custom-root",
            "--settle-ms",
            "0",
        ],
        &[],
    );
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let lines: Vec<String> = fake.calls().iter().map(Call::line).collect();
    assert!(
        lines.contains(&"vault=Fixture search query=needle limit=3 format=json".to_string()),
        "{lines:#?}"
    );
    assert!(lines.contains(&"vault=Fixture dev:dom selector=.custom-root".to_string()));
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.contains("search:context"))
            .count(),
        1,
        "only the queries given"
    );
}

#[test]
fn the_binary_refuses_the_wrong_version() {
    let fake = Fake::new("ref-bin-refuse");
    let out = fake.dir.join("out");
    let vault = vault();
    let run = binary(
        &fake,
        &[
            "reference",
            "--attach",
            "--vault",
            vault.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ],
        &[("FAKE_OBSIDIAN_VERSION", "1.12.7")],
    );
    assert!(!run.status.success());
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        stderr.contains("refusing to run: Obsidian reports 1.12.7"),
        "{stderr}"
    );
    assert!(!out.exists());
    assert_eq!(fake.calls().len(), 1);
}

#[test]
fn the_binary_rejects_a_missing_vault_and_an_unknown_group() {
    let fake = Fake::new("ref-bin-args");
    let out = fake.dir.join("out");
    let run = binary(
        &fake,
        &[
            "reference",
            "--attach",
            "--vault",
            "/nonexistent/vault",
            "--out",
            out.to_str().unwrap(),
        ],
        &[],
    );
    assert!(!run.status.success());
    assert!(String::from_utf8_lossy(&run.stderr).contains("vault /nonexistent/vault"));
    let vault = vault();
    let run = binary(
        &fake,
        &[
            "reference",
            "--attach",
            "--vault",
            vault.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--only",
            "graph",
        ],
        &[],
    );
    assert!(!run.status.success());
    assert!(
        fake.calls().is_empty(),
        "arguments are checked before Obsidian is asked anything"
    );
}

/// The README's claims about the CLI pages and the outputs stay in step with the code.
#[test]
fn the_readme_cites_every_command_the_harness_uses() {
    let readme = fs::read_to_string(crate_dir().join("README.md")).unwrap();
    for (command, ..) in DOCUMENTED {
        let used = full().calls.iter().any(|c| c.command() == *command);
        if used {
            assert!(
                readme.contains(&format!("`{command}")),
                "README does not describe {command}"
            );
        }
    }
    for page in [
        "https://obsidian.md/help/cli",
        "https://obsidian.md/help/plugins/search",
        "https://obsidian.md/help/bases/syntax",
    ] {
        assert!(readme.contains(page), "README does not cite {page}");
    }
    let by_command: BTreeMap<&str, usize> =
        full().calls.iter().fold(BTreeMap::new(), |mut m, c| {
            *m.entry(c.command()).or_default() += 1;
            m
        });
    assert!(by_command.len() >= 20, "{by_command:?}");
}

fn dom_config(fake: &Fake) -> Config {
    let dir = fake.dir.join("vault");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Note.md"), "# Note\n").unwrap();
    let mut cfg = config(fake, &dir, &fake.dir.join("out"));
    cfg.only = vec![Group::Dom];
    cfg
}

#[test]
fn dom_retries_a_missing_command_and_keeps_the_failed_attempts() {
    let fake = Fake::new("ref-dom-retry");
    let mut cfg = dom_config(&fake);
    cfg.obsidian = fake
        .client(&cfg.vault)
        .env("FAKE_OBSIDIAN_DOM_MISSING", "2");
    let m = reference::run(&cfg).unwrap();
    assert!(m.failed.is_empty(), "{:?}", m.failed);
    assert_eq!(m.commands, 7);
    assert_eq!(m.commands, fake.calls().len());
    assert_eq!(m.dom_attempts["Note.md"], 5);
    let dom = json(&cfg.out.join("dom.json"));
    let d = &dom["Note.md"];
    assert_eq!(d["attempts"], 5);
    assert_eq!(d["changes"], 0);
    assert_eq!(d["retries"].as_array().unwrap().len(), 2);
    for retry in d["retries"].as_array().unwrap() {
        assert_eq!(retry["exit_code"], 0);
        assert!(retry["error"].as_str().unwrap().contains("not found"));
        assert!(retry["stdout"].as_str().unwrap().contains("not found"));
    }
    assert!(d["dom"]["error"].is_null());
    assert!(
        fs::read_to_string(cfg.out.join("dom/Note.md.html"))
            .unwrap()
            .contains("fake dom")
    );
    assert_eq!(
        json(&cfg.out.join("manifest.json"))["dom_attempts"]["Note.md"],
        5
    );
}

#[test]
fn dom_is_read_until_it_stops_changing() {
    let fake = Fake::new("ref-dom-changes");
    let mut cfg = dom_config(&fake);
    cfg.obsidian = fake
        .client(&cfg.vault)
        .env("FAKE_OBSIDIAN_DOM_CHANGES", "2");
    let m = reference::run(&cfg).unwrap();
    assert!(m.failed.is_empty(), "{:?}", m.failed);
    // Two answers that differ, then the same one three times.
    assert_eq!(m.dom_attempts["Note.md"], 5);
    assert_eq!(m.commands, 7);
    assert_eq!(m.commands, fake.calls().len());
    let d = &json(&cfg.out.join("dom.json"))["Note.md"];
    assert_eq!(d["changes"], 2);
    assert_eq!(d["retries"], serde_json::json!([]));
    let html = fs::read_to_string(cfg.out.join("dom/Note.md.html")).unwrap();
    assert!(html.contains("fake dom") && !html.contains("rendering"), "{html}");
}

#[test]
fn a_view_that_never_settles_is_recorded_as_it_stands_and_failed() {
    let fake = Fake::new("ref-dom-unsettled");
    let mut cfg = dom_config(&fake);
    cfg.obsidian = fake
        .client(&cfg.vault)
        .env("FAKE_OBSIDIAN_DOM_CHANGES", "99");
    let m = reference::run(&cfg).unwrap();
    assert_eq!(m.dom_attempts["Note.md"], 20);
    assert_eq!(m.commands, fake.calls().len());
    assert_eq!(m.failed.len(), 1, "{:?}", m.failed);
    assert_eq!(m.failed[0].call, "dev:dom (Note.md)");
    assert_eq!(m.failed[0].error, "reading view still changing after 20 reads");
    assert_eq!(json(&cfg.out.join("dom.json"))["Note.md"]["changes"], 19);
    assert!(
        fs::read_to_string(cfg.out.join("dom/Note.md.html"))
            .unwrap()
            .contains("rendering, read 20")
    );
}

#[test]
fn dom_retry_exhaustion_stays_failed_and_continues_to_the_next_note() {
    let fake = Fake::new("ref-dom-exhausted");
    let mut cfg = dom_config(&fake);
    fs::write(cfg.vault.join("Second.md"), "second").unwrap();
    cfg.obsidian = fake
        .client(&cfg.vault)
        .env("FAKE_OBSIDIAN_DOM_MISSING", "99");
    let m = reference::run(&cfg).unwrap();
    assert_eq!(m.commands, 9);
    assert_eq!(m.commands, fake.calls().len());
    assert_eq!(m.failed.len(), 2);
    assert!(
        m.failed
            .iter()
            .all(|f| f.call.starts_with("dev:dom") && f.error.contains("not found"))
    );
    assert!(m.dom_attempts.values().all(|n| *n == 3));
    let dom = json(&cfg.out.join("dom.json"));
    for note in ["Note.md", "Second.md"] {
        assert_eq!(dom[note]["attempts"], 3);
        assert_eq!(dom[note]["retries"].as_array().unwrap().len(), 2);
        assert!(dom[note]["dom"]["error"].is_string());
        assert!(
            fs::read_to_string(cfg.out.join(format!("dom/{note}.html")))
                .unwrap()
                .contains("not found")
        );
    }
}

#[test]
fn dom_rejects_missing_wrong_or_throwing_open_without_reading_stale_dom() {
    for (i, answer) in [
        "=> missing",
        "=> source",
        "=> null",
        "=> {}",
        "=> Promise { <pending> }",
        "Error: failed to open",
        "a log line\nError: failed to open",
    ]
    .iter()
    .enumerate()
    {
        let fake = Fake::new(&format!("ref-dom-open-{i}"));
        let mut cfg = dom_config(&fake);
        cfg.obsidian = fake.client(&cfg.vault).env("FAKE_OBSIDIAN_OPEN", answer);
        let m = reference::run(&cfg).unwrap();
        assert_eq!(m.commands, 2, "{answer}");
        assert_eq!(m.failed.len(), 1, "{answer}");
        assert_eq!(m.failed[0].call, "eval (Note.md)");
        assert_eq!(m.dom_attempts["Note.md"], 0);
        let dom = json(&cfg.out.join("dom.json"));
        assert!(dom["Note.md"]["open"]["error"].is_string());
        assert!(dom["Note.md"]["dom"].is_null());
        assert!(!cfg.out.join("dom/Note.md.html").exists());
        assert!(!fake.calls().iter().any(|c| c.command() == "dev:dom"));
    }
}

#[test]
fn dom_accepts_bare_and_json_preview_answers() {
    for (i, answer) in ["=> preview", "preview", "=> \"preview\"", "\"preview\""]
        .iter()
        .enumerate()
    {
        let fake = Fake::new(&format!("ref-dom-preview-{i}"));
        let mut cfg = dom_config(&fake);
        cfg.obsidian = fake.client(&cfg.vault).env("FAKE_OBSIDIAN_OPEN", answer);
        let m = reference::run(&cfg).unwrap();
        assert!(m.failed.is_empty(), "{answer}: {:?}", m.failed);
        assert_eq!(m.dom_attempts["Note.md"], 3);
        assert_eq!(m.commands, 5);
    }
}

/// An unrelated error ends the reads at once; an empty selection may be a view not yet rendered,
/// so it is read until it stops changing, and fails if it stays empty.
#[test]
fn dom_stops_at_unrelated_errors_and_fails_empty_selections() {
    for (i, (answer, reads)) in [
        ("Error: Invalid selector", 1),
        ("No elements found.", 3),
        ("", 3),
    ]
    .iter()
    .enumerate()
    {
        let fake = Fake::new(&format!("ref-dom-error-{i}"));
        let mut cfg = dom_config(&fake);
        cfg.obsidian = fake.client(&cfg.vault).env("FAKE_OBSIDIAN_DOM", answer);
        let m = reference::run(&cfg).unwrap();
        assert_eq!(m.commands, 2 + reads, "{answer:?}");
        assert_eq!(m.failed.len(), 1, "{answer:?}");
        assert_eq!(m.dom_attempts["Note.md"], *reads, "{answer:?}");
        assert_eq!(
            json(&cfg.out.join("dom.json"))["Note.md"]["retries"],
            serde_json::json!([])
        );
    }
}

#[test]
fn mock_runs_normalize_every_group_and_manifest_before_writing() {
    use notes_fixtures::launch::Isolation;
    let mut outputs = Vec::new();
    for i in 0..2 {
        let fake = Fake::new(&format!("ref-normalize-{i}"));
        let mut cfg = dom_config(&fake);
        fs::write(cfg.vault.join("Note.md"), "# Note\n- [ ] Task\n").unwrap();
        fs::write(
            cfg.vault.join("Notes.base"),
            "views:\n  - name: All\n    type: table\n",
        )
        .unwrap();
        cfg.only.clear();
        let root = format!("/private/var/folders/machine-{i}/T/nfo-run-{i}");
        let copy = format!("{root}/vaults/vault");
        cfg.isolation = Some(Isolation {
            app: "/Applications/Obsidian.app".into(),
            home: format!("{root}/home"),
            user_data_dir: format!("{root}/profile"),
            vault_copy: copy.clone(),
            sandboxed: true,
        });
        cfg.obsidian = fake
            .client(&cfg.vault)
            .env("FAKE_OBSIDIAN_VAULT_PATH", &copy)
            .env(
                "FAKE_OBSIDIAN_DIAGNOSTIC",
                &format!("{copy} {root}/home {root}/profile {root}/tmp"),
            )
            .env(
                "FAKE_OBSIDIAN_DOM",
                &format!(
                    "<img src=\"app://host-{i}{copy}/attachments/pixel.png?{}\">",
                    1793019655954_u64 + i
                ),
            );
        let m = reference::run(&cfg).unwrap();
        assert!(m.failed.is_empty(), "{:?}", m.failed);
        assert_eq!(m.groups.len(), 11);
        assert_eq!(m.commands, fake.calls().len());
        let manifest = json(&cfg.out.join("manifest.json"));
        // Repo-relative when Cargo's target directory is inside the checkout, otherwise a placeholder.
        let repo = crate_dir().join("../..").canonicalize().unwrap();
        let vault_dir = fake.dir.join("vault").canonicalize().unwrap();
        let expected = vault_dir
            .strip_prefix(&repo)
            .map_or("__vault__".into(), |p| p.to_string_lossy().into_owned());
        assert_eq!(manifest["vault"], expected);
        assert_eq!(manifest["isolation"]["home"], "__home__");
        assert_eq!(manifest["isolation"]["user_data_dir"], "__profile__");
        assert_eq!(manifest["isolation"]["vault_copy"], "__vault__");
        assert_eq!(manifest["isolation"]["sandboxed"], true);
        assert_eq!(manifest["isolation"]["app"], "/Applications/Obsidian.app");
        let vault = json(&cfg.out.join("vault.json"));
        assert!(
            vault["vault"]["stdout"]
                .as_str()
                .unwrap()
                .contains("path\t__vault__")
        );
        assert_eq!(vault["vault info=path"]["stdout"], "__vault__\n");
        let mut written = BTreeMap::new();
        for entry in walkdir::WalkDir::new(&cfg.out)
            .into_iter()
            .map(Result::unwrap)
            .filter(|e| e.file_type().is_file())
        {
            let text = fs::read_to_string(entry.path()).unwrap();
            assert!(!text.contains(&root), "{}", entry.path().display());
            assert!(!text.contains(&format!("host-{i}")));
            // A repository-relative test vault name intentionally differs between runs.
            let text = text.replace(&format!("ref-normalize-{i}"), "ref-normalize");
            written.insert(
                entry.path().strip_prefix(&cfg.out).unwrap().to_path_buf(),
                text,
            );
        }
        outputs.push(written);
    }
    assert_eq!(outputs[0], outputs[1]);
}
