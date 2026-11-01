//! The reading view against the DOM Obsidian 1.14.5 recorded for every note of the fixture vault.
//!
//! Each note is rendered with `render_note`, wrapped in the skeleton Obsidian puts around the
//! content (`.markdown-preview-sizer`), and compared with the recording by
//! `notes_fixtures::compare` (see "Comparing DOM structure" in the notes-fixtures README for what
//! is normalized). A note that differs must be listed in `KNOWN` with the reason, and a listed
//! note that starts to match fails the test, so the list only ever shrinks.
//!
//! Out of scope for this headless test, and only for it: what scripts in the page produce.
//! Obsidian's recording holds the output of MathJax (inside `.math`), Prism (the token spans in
//! `code[class*=language-]`), Mermaid and the query renderer (fences replaced in the page), pdf.js
//! (inside `.pdf-embed`) and the Bases viewer (inside `.bases-embed`), and the copy button added
//! to each code block. The renderer writes the containers those scripts start from; the
//! containers, their classes and `data-*` attributes are compared, their contents are not. The
//! page that shows the view runs the same scripts, so they are verified there.
use frame_notes::{Vault, VaultSettings, render_note};
use notes_fixtures::compare::{self, Options, Verdict};
use regex::Regex;
use std::{fs, path::Path};

/// A recorded note that Frame renders differently, with why. Every difference found in the note
/// must contain one of `differences` (in its path or in what the two sides hold), so the note
/// cannot start to differ in another way unnoticed.
struct Known {
    note: &'static str,
    differences: &'static [&'static str],
    reason: &'static str,
}

const KNOWN: &[Known] = &[
    Known {
        note: "callouts/Custom Callout.md.html",
        differences: &["callout-icon"],
        reason: "the icon of a custom callout type is the `--callout-icon` of a CSS rule \
                 (`snippets/callouts.css` gives `custom-purple` lucide-flame); Obsidian reads it \
                 from the computed style, a headless render has no CSS and writes the default \
                 pencil; in the page, reading.js redraws it from the same custom property with \
                 the vendored Lucide, and `notes_check` in Servo matches Obsidian's recording",
    },
    Known {
        note: "callouts/Nested Callouts.md.html",
        differences: &["callout-icon", "node-insert-event"],
        reason: "Obsidian draws a callout inside a folded callout only when it is unfolded: the \
                 recording has the inner one's icon as an empty 16px placeholder and its class \
                 `node-insert-event`; Frame draws it whole, the icon included",
    },
    Known {
        note: "syntax/Footnotes.md.html",
        differences: &["undefined"],
        reason: "for a reference whose definition does not exist Obsidian prints the JavaScript \
                 value `undefined` where the text belongs; Frame keeps the source `[^undefined]`",
    },
    Known {
        note: "syntax/Raw HTML.md.html",
        differences: &["@style", "el-iframe"],
        reason: "raw HTML in a note is untrusted: ammonia drops `style` attributes and `iframe` \
                 elements, which Obsidian keeps (the iframe with a `sandbox` of its own)",
    },
];

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &to.join(entry.file_name()));
        } else {
            fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}

fn options() -> Options {
    let strings = |list: &[&str]| list.iter().map(|s| s.to_string()).collect();
    Options {
        ignore: strings(&["button.copy-code-button"]),
        opaque: strings(&[
            ".math",
            "div.bases-embed",
            "div.el-pre:has(.mermaid-wrapper)",
            "div.el-pre:has(pre.language-mermaid)",
            "div.el-pre:has(.internal-query)",
            "div.el-pre:has(pre.language-query)",
        ]),
        opaque_text: strings(&["code[class*=language-]"]),
        hollow: strings(&["pdf-embed", "bases-embed"]),
        allow_truncated_expected: true,
        max_differences: 200,
        ..Options::default()
    }
}

#[test]
fn every_fixture_note_renders_as_obsidian_does() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../notes-fixtures");
    let expected = fixture.join("reference/fixture-vault/dom");
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    copy_tree(&fixture.join("vault"), root.path());
    let mut vault = Vault::open(root.path(), data.path()).unwrap();
    let mut settings = VaultSettings::from_app_json(
        &fs::read_to_string(root.path().join(".obsidian/app.json")).unwrap(),
    )
    .unwrap();
    settings
        .read_property_types(&fs::read_to_string(root.path().join(".obsidian/types.json")).unwrap())
        .unwrap();
    vault.set_settings(settings).unwrap();

    let read = |path: &str| fs::read_to_string(root.path().join(path)).ok();
    let actual = tempfile::tempdir().unwrap();
    let wrapper = Regex::new(r#"class="markdown-preview-view ([^"]*)""#).unwrap();
    let mut with_cssclasses = 0;
    for entry in walkdir_html(&expected) {
        let note = entry.strip_suffix(".html").unwrap();
        let source =
            read(note).unwrap_or_else(|| panic!("{note} is recorded but not in the vault"));
        let view = render_note(vault.index(), vault.settings(), note, &source, &read);
        // The page puts `cssclasses` on the view's container, so each must be on the recording's.
        let recorded = fs::read_to_string(expected.join(&entry)).unwrap();
        let classes = wrapper
            .captures(&recorded)
            .map_or("", |c| c.get(1).unwrap().as_str());
        for class in &view.cssclasses {
            assert!(
                classes.split_whitespace().any(|c| c == class),
                "{note}: cssclass {class} is not on the recorded view ({classes})"
            );
        }
        with_cssclasses += usize::from(!view.cssclasses.is_empty());
        let out = actual.path().join(&entry);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        fs::write(
            out,
            format!(
                r#"<div class="markdown-reading-view"><div class="markdown-preview-view markdown-rendered"><div class="markdown-preview-sizer markdown-preview-section">{}</div></div></div>"#,
                view.html
            ),
        )
        .unwrap();
    }
    assert!(
        with_cssclasses > 0,
        "no fixture note sets cssclasses: the check above proves nothing"
    );

    let report = compare::compare_dirs(&expected, actual.path(), &options(), None).unwrap();
    println!("{}", report.render());
    let total = report.notes.len();
    let listed = KNOWN.len();
    println!(
        "{} of {total} recorded notes match Obsidian's DOM ({} of them on the recorded prefix of a truncated recording); {listed} are listed as known differences",
        report.matched() + report.prefix_matched(),
        report.prefix_matched(),
    );

    let mut problems = Vec::new();
    for n in &report.notes {
        let known = KNOWN.iter().find(|k| k.note == n.note);
        match (&n.verdict, known) {
            (Verdict::Mismatch { .. }, None) => {
                problems.push(format!(
                    "{} differs from the recording and is not listed in KNOWN",
                    n.note
                ));
            }
            (
                Verdict::Mismatch {
                    differences,
                    omitted,
                },
                Some(k),
            ) => {
                let unlisted: Vec<_> = differences
                    .iter()
                    .map(|d| format!("{d:?}"))
                    .filter(|d| !k.differences.iter().any(|listed| d.contains(listed)))
                    .collect();
                if !unlisted.is_empty() || *omitted > 0 {
                    problems.push(format!(
                        "{} differs, but not only as listed ({:?}: {}): {unlisted:?}, {omitted} more",
                        n.note, k.differences, k.reason
                    ));
                }
            }
            (_, Some(k)) => problems.push(format!(
                "{} now matches: remove it from KNOWN ({})",
                n.note, k.reason
            )),
            _ => {}
        }
    }
    for k in KNOWN {
        if !report.notes.iter().any(|n| n.note == k.note) {
            problems.push(format!("KNOWN lists {}, which has no recording", k.note));
        }
    }
    problems.extend(
        report
            .missing
            .iter()
            .map(|m| format!("{m} was not rendered")),
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The recordings under `dir`, as paths relative to it.
fn walkdir_html(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, base, out);
            } else if path.extension().is_some_and(|e| e == "html") {
                out.push(
                    path.strip_prefix(base)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}
