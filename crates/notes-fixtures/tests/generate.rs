//! The generator is deterministic and writes the mix the notes app's benchmarks need.
mod common;

use common::scratch;
use notes_fixtures::generate::{self, Options, tree_digest};
use regex::Regex;
use std::{collections::BTreeSet, fs, path::Path};
use walkdir::WalkDir;

fn make(name: &str, notes: usize, seed: u64) -> (std::path::PathBuf, generate::Summary) {
    let out = scratch(name);
    let summary = generate::generate(&Options { notes, seed }, &out).unwrap();
    (out, summary)
}

fn read_all(dir: &Path, ext: &str) -> Vec<(String, String)> {
    WalkDir::new(dir)
        .sort_by_file_name()
        .into_iter()
        .map(|e| e.unwrap())
        .filter(|e| e.path().extension().is_some_and(|x| x == ext))
        .map(|e| {
            (
                e.path()
                    .strip_prefix(dir)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                fs::read_to_string(e.path()).unwrap(),
            )
        })
        .collect()
}

#[test]
fn same_seed_same_bytes() {
    let (a, sa) = make("gen-same-a", 400, 7);
    let (b, sb) = make("gen-same-b", 400, 7);
    assert_eq!(sa, sb);
    assert_eq!(tree_digest(&a).unwrap(), sa.digest);
    assert_eq!(tree_digest(&b).unwrap(), sa.digest);
    let files = |d: &Path| -> Vec<(String, Vec<u8>)> {
        WalkDir::new(d)
            .sort_by_file_name()
            .into_iter()
            .map(|e| e.unwrap())
            .filter(|e| e.file_type().is_file())
            .map(|e| {
                (
                    e.path()
                        .strip_prefix(d)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    fs::read(e.path()).unwrap(),
                )
            })
            .collect()
    };
    assert_eq!(
        files(&a),
        files(&b),
        "two runs of one seed must agree byte for byte"
    );
}

#[test]
fn another_seed_another_vault() {
    let (_, one) = make("gen-seed-1", 200, 1);
    let (_, two) = make("gen-seed-2", 200, 2);
    assert_ne!(one.digest, two.digest);
}

/// Pins the stream of the locked `rand` and `rand_chacha`. If this fails after a dependency
/// update, the generator's output changed: regenerate the digest on purpose, never to quiet the test.
#[test]
fn golden_digest() {
    let (_, s) = make("gen-golden", 300, 42);
    assert_eq!(
        (s.notes, s.canvases, s.bases, s.attachments),
        (300, 1, 2, 6),
        "the counts are fixed by the options, not by the random stream"
    );
    assert_eq!(
        s.digest, GOLDEN_300_SEED_42,
        "bytes: {}, large notes: {}",
        s.bytes, s.large_notes
    );
}

const GOLDEN_300_SEED_42: &str = "907c39735dfe0b6f84de386508402452ffe7926739bb9257b57106f1079b0ad2";

#[test]
fn refuses_a_used_directory_and_zero_notes() {
    let out = scratch("gen-refuse");
    fs::write(out.join("keep.txt"), "x").unwrap();
    let err = generate::generate(&Options { notes: 10, seed: 1 }, &out).unwrap_err();
    assert!(err.to_string().contains("not empty"), "{err}");
    assert_eq!(
        fs::read_to_string(out.join("keep.txt")).unwrap(),
        "x",
        "nothing is touched"
    );
    let fresh = scratch("gen-zero");
    assert!(generate::generate(&Options { notes: 0, seed: 1 }, &fresh).is_err());
}

#[test]
fn an_existing_empty_directory_is_fine() {
    let out = scratch("gen-empty-dir");
    let s = generate::generate(&Options { notes: 5, seed: 3 }, &out).unwrap();
    assert_eq!(s.notes, 5);
}

/// 1,000 notes carry every feature the parser, index and search have to cope with at scale.
#[test]
fn the_mix_is_realistic() {
    let (out, s) = make("gen-mix", 1000, 11);
    assert_eq!(s.notes, 1000);
    assert!(s.large_notes >= 3, "large notes: {}", s.large_notes);
    let notes = read_all(&out, "md");
    assert_eq!(notes.len(), 1000);

    let folders: BTreeSet<&str> = notes
        .iter()
        .filter_map(|(p, _)| p.rsplit_once('/').map(|(d, _)| d))
        .collect();
    assert!(folders.len() >= 20, "folders: {}", folders.len());
    assert!(
        notes.iter().any(|(p, _)| p.starts_with("journal/")),
        "daily notes"
    );

    let all: String = notes
        .iter()
        .map(|(_, t)| t.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let count = |re: &str| Regex::new(re).unwrap().find_iter(&all).count();
    let wikilinks = count(r"(?m)(?:^|[^!])\[\[");
    let unresolved = count(r"\[\[Missing ");
    assert!(wikilinks > 3000, "wikilinks: {wikilinks}");
    assert!(
        unresolved > 0 && unresolved * 5 < wikilinks,
        "unresolved: {unresolved} of {wikilinks}"
    );
    for (what, re, at_least) in [
        ("embeds of notes", r"!\[\[[^\]|#]+\]\]", 100),
        ("embeds of headings", r"!\[\[[^\]]+#[^\]^]+\]\]", 10),
        ("embeds of images", r"!\[\[img-\d\.png", 50),
        ("sized images", r"!\[\[img-\d\.png\|\d+", 10),
        ("alias links", r"\[\[[^\]|]+\|[^\]]+\]\]", 100),
        ("heading links", r"\[\[[^\]|#]+#[^\]^|]+", 50),
        ("block links", r"\[\[[^\]|#]+#\^", 20),
        ("Markdown links", r"\]\([^)]+\.md\)", 20),
        ("tags in text", r"(?m) #[\w/-]+", 500),
        ("nested tags", r"#(?:project|area)/\w+", 100),
        ("frontmatter blocks", r"(?m)^---$", 1000),
        ("aliases", r"(?m)^aliases:", 50),
        ("dates", r"(?m)^due: \d{4}-\d\d-\d\d", 100),
        ("open tasks", r"(?m)^\s*- \[ \]", 500),
        ("done tasks", r"(?m)^\s*- \[x\]", 100),
        ("other task statuses", r"(?m)^\s*- \[[^ x]\]", 20),
        ("callouts", r"(?m)^> \[!\w+\]", 100),
        ("folded callouts", r"(?m)^> \[!\w+\][-+]", 20),
        ("code blocks", r"(?m)^```\w+", 100),
        ("tables", r"(?m)^\| --- \|", 50),
        ("math", r"\$\$", 20),
        ("footnotes", r"\[\^\d+\]:", 20),
        ("highlights", r"==\w", 50),
        ("comments", r"%%", 20),
        ("block ids", r"(?m) \^b\d+$", 50),
    ] {
        assert!(count(re) >= at_least, "{what}: {} < {at_least}", count(re));
    }

    let biggest = notes.iter().map(|(_, t)| t.len()).max().unwrap();
    assert!(biggest >= 100_000, "the largest note is {biggest} bytes");
    let smallest = notes.iter().map(|(_, t)| t.len()).min().unwrap();
    assert!(smallest < 3_000, "the smallest note is {smallest} bytes");
}

#[test]
fn extras_are_valid() {
    let (out, s) = make("gen-extras", 2500, 5);
    assert_eq!(s.canvases, 2);
    for (path, text) in read_all(&out, "canvas") {
        let v: serde_json::Value =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert!(
            v["nodes"].as_array().is_some_and(|n| !n.is_empty()),
            "{path}"
        );
    }
    for (path, text) in read_all(&out, "base") {
        serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&text)
            .unwrap_or_else(|e| panic!("{path}: {e}"));
    }
    for (path, text) in read_all(&out, "md") {
        if let Some(rest) = text.strip_prefix("---\n") {
            let (yaml, _) = rest
                .split_once("\n---\n")
                .unwrap_or_else(|| panic!("{path}: unclosed frontmatter"));
            serde_yaml_ng::from_str::<serde_yaml_ng::Value>(yaml)
                .unwrap_or_else(|e| panic!("{path}: {e}\n{yaml}"));
        }
    }
}

/// Every note path is unique in its folder, and no two basenames differ only by case, which a
/// case-insensitive volume would merge.
#[test]
fn names_do_not_collide() {
    let (out, _) = make("gen-names", 1500, 9);
    let mut seen = BTreeSet::new();
    for e in WalkDir::new(&out)
        .into_iter()
        .map(|e| e.unwrap())
        .filter(|e| e.file_type().is_file())
    {
        let rel = e
            .path()
            .strip_prefix(&out)
            .unwrap()
            .to_string_lossy()
            .to_lowercase();
        assert!(seen.insert(rel.clone()), "{rel}");
    }
}
