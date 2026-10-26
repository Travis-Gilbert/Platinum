//! COVERAGE.md names every row of the plan's parity matrix, and what it cites is really there.
mod common;

use common::{crate_dir, vault, visible_files};
use notes_fixtures::reference::Group;
use regex::Regex;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

const PLUGINS: &str = "The ten most-downloaded community plugins";

fn cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split(" | ")
        .map(|c| c.trim().to_string())
        .collect()
}

fn is_header_or_rule(cells: &[String]) -> bool {
    matches!(cells[0].as_str(), "Feature" | "#" | "Row")
        || cells[0].chars().all(|c| "-: ".contains(c))
}

/// `section / feature` for every row of the plan's matrix, in order.
fn plan_rows() -> Vec<String> {
    let plan = fs::read_to_string(crate_dir().join("../../docs/plans/notes-app.md"))
        .expect("docs/plans/notes-app.md");
    let start = plan
        .find("## Parity matrix")
        .expect("the plan has a parity matrix");
    let end = plan
        .find("## Implementation sequence")
        .expect("and an implementation sequence after it");
    let mut section = String::new();
    let mut rows = Vec::new();
    for line in plan[start..end].lines() {
        if let Some(name) = line.strip_prefix("### ") {
            section = name.trim().to_string();
        } else if line.starts_with('|') {
            let c = cells(line);
            if is_header_or_rule(&c) {
                continue;
            }
            let feature = if section == PLUGINS { &c[1] } else { &c[0] };
            rows.push(format!("{section} / {feature}"));
        }
    }
    rows
}

struct Row {
    files: Vec<String>,
    none: bool,
    refs: Vec<String>,
    gap: String,
}

struct Coverage {
    rows: BTreeMap<String, Row>,
    no_fixture: BTreeMap<String, String>,
    text: String,
}

fn coverage() -> Coverage {
    let text = fs::read_to_string(crate_dir().join("COVERAGE.md")).expect("COVERAGE.md");
    let backticked = Regex::new(r"`([^`]+)`").unwrap();
    let (mut top, mut sub) = (String::new(), String::new());
    let (mut rows, mut no_fixture) = (BTreeMap::new(), BTreeMap::new());
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("## ") {
            top = name.trim().to_string();
            sub.clear();
        } else if let Some(name) = line.strip_prefix("### ") {
            sub = name.trim().to_string();
        } else if line.starts_with('|') {
            let c = cells(line);
            if is_header_or_rule(&c) {
                continue;
            }
            match top.as_str() {
                "Parity matrix" => {
                    assert_eq!(c.len(), 4, "{line}");
                    let key = format!("{sub} / {}", c[0]);
                    let row = Row {
                        none: c[1] == "none",
                        files: backticked
                            .captures_iter(&c[1])
                            .map(|m| m[1].to_string())
                            .collect(),
                        refs: c[2].split(',').map(|r| r.trim().to_string()).collect(),
                        gap: c[3].clone(),
                    };
                    assert!(
                        rows.insert(key.clone(), row).is_none(),
                        "{key} appears twice"
                    );
                }
                "Rows no fixture can exercise" => {
                    assert_eq!(c.len(), 2, "{line}");
                    assert!(
                        no_fixture.insert(c[0].clone(), c[1].clone()).is_none(),
                        "{} appears twice",
                        c[0]
                    );
                }
                _ => {}
            }
        }
    }
    Coverage {
        rows,
        no_fixture,
        text,
    }
}

#[test]
fn every_row_of_the_matrix_is_here_and_nothing_else() {
    let plan = plan_rows();
    assert!(
        plan.len() >= 113,
        "the plan has {} rows; the parse is wrong if it is not 113",
        plan.len()
    );
    let planned: BTreeSet<&String> = plan.iter().collect();
    assert_eq!(planned.len(), plan.len(), "a row appears twice in the plan");
    let cov = coverage();
    let covered: BTreeSet<&String> = cov.rows.keys().collect();
    let missing: Vec<_> = planned.difference(&covered).collect();
    let extra: Vec<_> = covered.difference(&planned).collect();
    assert!(
        missing.is_empty(),
        "rows of the plan that COVERAGE.md lacks: {missing:#?}"
    );
    assert!(
        extra.is_empty(),
        "rows in COVERAGE.md that the plan lacks: {extra:#?}"
    );
}

#[test]
fn rows_without_fixtures_are_exactly_the_rows_that_say_why() {
    let cov = coverage();
    let none: BTreeSet<&String> = cov
        .rows
        .iter()
        .filter(|(_, r)| r.none)
        .map(|(k, _)| k)
        .collect();
    let listed: BTreeSet<&String> = cov.no_fixture.keys().collect();
    assert_eq!(
        none, listed,
        "the table's `none` rows and the list of rows no fixture can exercise disagree"
    );
    assert!(!none.is_empty());
    for (row, why) in &cov.no_fixture {
        assert!(
            why.len() >= 20 && why.ends_with('.'),
            "{row}: say why, in a sentence: {why:?}"
        );
    }
    for (key, row) in &cov.rows {
        if row.none {
            assert!(row.files.is_empty() && row.refs == ["-"], "{key}");
        } else {
            assert!(
                !row.files.is_empty(),
                "{key}: a row with fixtures cites them"
            );
            assert!(
                !row.gap.is_empty() && !row.gap.starts_with("no fixture"),
                "{key}: say what is not covered, or `-`"
            );
        }
    }
}

#[test]
fn cited_files_exist() {
    let cov = coverage();
    for (key, row) in &cov.rows {
        for file in &row.files {
            let in_vault = vault().join(file).exists();
            let in_crate = file == "queries.json"
                || file.starts_with("jsoncanvas/")
                || file.starts_with("src/");
            assert!(
                in_vault || (in_crate && crate_dir().join(file).exists()),
                "{key} cites {file}, which is not there"
            );
        }
    }
}

#[test]
fn reference_answers_are_groups_of_the_harness() {
    let groups: BTreeSet<&str> = Group::ALL
        .iter()
        .map(|g| g.name())
        .chain(["ui", "lint"])
        .collect();
    let cov = coverage();
    for (key, row) in cov.rows.iter().filter(|(_, r)| !r.none) {
        assert!(!row.refs.is_empty(), "{key}");
        for r in &row.refs {
            assert!(
                groups.contains(r.as_str()),
                "{key}: `{r}` is not a reference group (vault, links, ..., ui, lint)"
            );
        }
    }
    let used: BTreeSet<&str> = cov
        .rows
        .values()
        .flat_map(|r| r.refs.iter().map(String::as_str))
        .collect();
    for g in Group::ALL {
        assert!(
            used.contains(g.name()),
            "no row says it is answered by the {} group",
            g.name()
        );
    }
}

#[test]
fn every_note_canvas_and_base_is_cited_somewhere() {
    let cov = coverage();
    let uncited: Vec<String> = visible_files(&vault())
        .into_iter()
        .filter(|f| [".md", ".canvas", ".base"].iter().any(|e| f.ends_with(e)))
        .filter(|f| !cov.text.contains(&format!("`{f}`")))
        .collect();
    assert!(
        uncited.is_empty(),
        "fixtures no row cites (a dead fixture, or a row missing a file): {uncited:#?}"
    );
}

/// A cited folder stands for its files; the folders named here must all hold something.
#[test]
fn cited_folders_are_not_empty() {
    let cov = coverage();
    for (key, row) in &cov.rows {
        for dir in row.files.iter().filter(|f| f.ends_with('/')) {
            let n = fs::read_dir(vault().join(dir))
                .unwrap_or_else(|e| panic!("{key}: {dir}: {e}"))
                .count();
            assert!(n >= 1, "{key}: {dir} is empty");
        }
    }
}
