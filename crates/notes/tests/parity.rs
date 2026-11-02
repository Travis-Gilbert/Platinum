//! Focused regressions for rules evidenced by the committed Obsidian 1.14.5 recording.
use frame_notes::Vault;
use serde_json::Value;
use std::{fs, path::Path};

fn vault(files: &[(&str, &str)]) -> (tempfile::TempDir, tempfile::TempDir, Vault) {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let path = root.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let vault = Vault::open(root.path(), data.path()).unwrap();
    (root, data, vault)
}

#[test]
fn wordcount_matches_every_recorded_note() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../notes-fixtures");
    let records: Value = serde_json::from_slice(
        &fs::read(fixture.join("reference/fixture-vault/wordcount.json")).unwrap(),
    )
    .unwrap();
    let sources = records
        .as_object()
        .unwrap()
        .values()
        .map(|record| {
            let path = record["args"][0]
                .as_str()
                .unwrap()
                .strip_prefix("path=")
                .unwrap();
            (
                path.to_string(),
                fs::read_to_string(fixture.join("vault").join(path)).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let files = sources
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect::<Vec<_>>();
    let (_root, _data, vault) = vault(&files);
    for record in records.as_object().unwrap().values() {
        let path = record["args"][0]
            .as_str()
            .unwrap()
            .strip_prefix("path=")
            .unwrap();
        let (words, characters) = vault.index().word_count(path).unwrap();
        assert_eq!(
            format!("words: {words}\ncharacters: {characters}\n"),
            record["stdout"].as_str().unwrap(),
            "{path}"
        );
    }
}

#[test]
fn wordcount_excludes_only_closed_leading_frontmatter_and_counts_source() {
    let (_root, _data, vault) = vault(&[
        ("empty.md", "---\n---\n# Hello [[world]]\n"),
        ("broken.md", "---\na: [broken\n---\n# Hello [[world]]\n"),
        ("bom.md", "\u{feff}---\nx: 1\n---\n# Hello [[world]]\n"),
        ("unclosed.md", "---\nx: 1\n"),
        ("unicode.md", "日本 12,345.67 café co-op don't 😀"),
    ]);
    for path in ["empty.md", "broken.md", "bom.md"] {
        assert_eq!(vault.index().word_count(path), Some((2, 18)), "{path}");
    }
    assert_eq!(vault.index().word_count("unclosed.md"), Some((3, 9)));
    assert_eq!(vault.index().word_count("unicode.md"), Some((6, 32)));
}

#[test]
fn deadends_include_files_with_only_unresolved_links() {
    let (_root, _data, vault) = vault(&[
        ("missing.md", "[[Absent]]"),
        ("linked.md", "[[target]]"),
        ("target.md", "Target"),
    ]);
    assert_eq!(vault.index().dead_ends(), ["missing.md", "target.md"]);
}

#[test]
fn done_includes_every_status_except_a_space() {
    for status in [' ', 'x', 'X', '-', '>', '<', '?', '!', '*', '/'] {
        let task = frame_notes::Task {
            status,
            text: String::new(),
            span: frame_notes::Span::default(),
            status_span: frame_notes::Span::default(),
        };
        assert_eq!(task.is_done(), status != ' ');
    }
}

#[test]
fn tags_accept_obsidian_unicode_and_slash_forms_with_exact_spans() {
    let text = "#1/2 #a·b #naïve #emoji-🚀 #🚀 #a/ #2026 word#no \\#escaped `#code`";
    let note = frame_notes::parse_note(text);
    assert_eq!(
        note.tags
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>(),
        ["1/2", "a·b", "naïve", "emoji-🚀", "🚀", "a/"]
    );
    for tag in note.tags {
        assert_eq!(tag.span.slice(text), format!("#{}", tag.name));
    }
}

#[test]
fn frontmatter_tags_are_whole_written_strings() {
    let n = frame_notes::parse_note(
        "---\nTaGs: [Mixed-Case, '#one/two', 123, true, 'two words']\n---\n",
    );
    assert_eq!(
        n.tags.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["Mixed-Case", "one/two"]
    );
    assert!(
        frame_notes::parse_note("---\ntags: a, b\n---\n")
            .tags
            .is_empty()
    );
    assert!(
        frame_notes::parse_note("---\ntag: old\n---\n")
            .tags
            .is_empty()
    );
}

#[test]
fn vault_tags_expand_parents_merge_case_and_skip_exclusions() {
    let (_root, _data, mut v) = vault(&[
        (
            "a.md",
            "#Mixed-Case #Mixed-Case #mixed-case #MIXED-CASE #project/alpha/beta #a/",
        ),
        ("excluded.md", "#excluded"),
    ]);
    v.set_settings(
        frame_notes::VaultSettings::from_app_json(r#"{"userIgnoreFilters":["excluded.md"]}"#)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        v.index().tags(),
        std::collections::BTreeMap::from([
            ("Mixed-Case".into(), 4),
            ("project".into(), 1),
            ("project/alpha".into(), 1),
            ("project/alpha/beta".into(), 1),
            ("a".into(), 1),
        ])
    );
}

#[test]
fn comments_index_links_and_tags_but_not_code_or_math() {
    let text = "%%[[Inline]] #inline%%\n\n%%\n[[Block]] #block\n%%\n\n`[[Code]] #code` $[[Math]]$";
    let n = frame_notes::parse_note(text);
    assert_eq!(
        n.links
            .iter()
            .map(|l| l.target.as_str())
            .collect::<Vec<_>>(),
        ["Inline", "Block"]
    );
    assert_eq!(
        n.tags.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["inline", "block"]
    );
    for l in n.links {
        assert_eq!(l.span.slice(text), format!("[[{}]]", l.target));
    }
}

#[test]
fn footnotes_keep_unused_definitions_and_index_inline_content() {
    use frame_notes::FootnoteKind::*;
    let text = "Ref[^a] again[^a]. Inline^[See [[Target]] #tag].\n\n[^a]: [[Defined]]\n[^unused]: [[Unused]]\n";
    let n = frame_notes::parse_note(text);
    assert_eq!(
        n.links
            .iter()
            .map(|l| l.target.as_str())
            .collect::<Vec<_>>(),
        ["Target", "Defined", "Unused"]
    );
    let inline = n.footnotes.iter().find(|f| f.kind == Inline).unwrap();
    assert_eq!(inline.name, "[inline1");
    assert_eq!(inline.span.slice(text), "See [[Target]] #tag");
    assert!(
        n.footnotes
            .iter()
            .any(|f| f.kind == Definition && f.name == "unused")
    );
    assert_eq!(n.tags[0].span.slice(text), "#tag");
}

#[test]
fn property_links_use_decoded_whole_scalar_values_not_yaml_keys_or_prose() {
    let text = "---\n'[[Key]]': no\na: '[[Wiki]]'\nnested:\n  list:\n    - '[Markdown](Some%20Note.md)'\n    - 'prose [[Not a property link]]'\n    - '![[Not an embed]]'\n---\n";
    let n = frame_notes::parse_note(text);
    assert_eq!(
        n.links
            .iter()
            .map(|l| l.target.as_str())
            .collect::<Vec<_>>(),
        ["Wiki", "Some Note.md"]
    );
    assert!(n.links.iter().all(|l| l.in_frontmatter));
    assert_eq!(n.links[0].span.slice(text), "[[Wiki]]");
    assert_eq!(n.links[1].span.slice(text), "[Markdown](Some%20Note.md)");
}

#[test]
fn resolution_prefers_source_folder_then_utf16_path_length() {
    use frame_notes::{FileIndex, LinkSyntax::Wiki};
    let index = FileIndex::new(["a/Note.md", "b/Note.md", "b/sub/Note.md"]);
    assert_eq!(
        index.resolve("NOTE", "b/Source.md", Wiki),
        Some("b/Note.md")
    );
    assert_eq!(
        index.resolve("Note", "b/Source.md", Wiki),
        Some("b/Note.md")
    );
    assert_eq!(
        index.resolve("Note", "b/sub/Source.md", Wiki),
        Some("b/sub/Note.md")
    );
    assert_eq!(
        index.resolve("a/Note", "b/Source.md", Wiki),
        Some("a/Note.md")
    );
    let index = FileIndex::new(["longfolder/Note.md", "a/b/Note.md"]);
    assert_eq!(
        index.resolve("Note", "Source.md", Wiki),
        Some("a/b/Note.md")
    );
}

#[test]
fn unresolved_names_drop_md_but_source_links_preserve_it() {
    let index = frame_notes::FileIndex::default();
    let n = frame_notes::parse_note("[[Gone.md]] [missing](Other.md) ![[image.png]]");
    for (link, expected) in n.links.iter().zip(["Gone", "Other", "image.png"]) {
        assert!(
            matches!(index.target_of("Source.md", link), frame_notes::Target::Unresolved { name, .. } if name == expected)
        );
    }
    assert_eq!(n.links[0].target, "Gone.md");
}

#[test]
fn canvas_backgrounds_are_file_links() {
    let links = frame_notes::canvas_links(r#"{"nodes":[{"id":"group","type":"group","background":"img.png"},{"id":"text","type":"text","text":"[[Note]]"}]}"#).unwrap();
    assert_eq!(
        links
            .iter()
            .map(|l| l.link.target.as_str())
            .collect::<Vec<_>>(),
        ["img.png", "Note"]
    );
    assert_eq!(links[0].link.syntax, frame_notes::LinkSyntax::Path);
}

#[test]
fn tasks_reject_surrogate_pairs_and_exclude_nested_lists_from_spans() {
    let text = "- [ ] Parent\n  - [x] Child\n- [😀] Not a task\n- [✓] BMP task\n";
    let n = frame_notes::parse_note(text);
    assert_eq!(
        n.tasks.iter().map(|t| t.status).collect::<Vec<_>>(),
        [' ', 'x', '✓']
    );
    assert_eq!(n.tasks[0].span.slice(text), "- [ ] Parent");
    assert_eq!(n.tasks[1].span.slice(text), "- [x] Child");
}

#[test]
fn tasks_include_adjacent_lazy_quote_continuations() {
    let text = "- [ ] Task\n> Quote\n";
    assert_eq!(
        frame_notes::parse_note(text).tasks[0].span.slice(text),
        text.trim_end()
    );
}

#[test]
fn multiple_embed_dividers_and_reserved_uri_escapes_are_preserved() {
    let text = "![[image.png|Alt text|120]] [Heading](Note.md#C%23%20basics)";
    let n = frame_notes::parse_note(text);
    assert_eq!(n.links.len(), 2);
    assert!(n.links[0].embed);
    assert_eq!(n.links[0].span.slice(text), "![[image.png|Alt text|120]]");
    assert_eq!(
        n.links[1].fragment.as_ref().unwrap().to_string(),
        "#C%23 basics"
    );
}

#[test]
fn properties_use_assigned_types_unknown_objects_and_latest_nonnull_values() {
    use frame_notes::PropertyType::*;
    let (_root, _data, mut v) = vault(&[
        (
            "a.md",
            "---\ncreated: 2026-10-25\nnumber: 3\nnested: {key: value}\nlist: [a, null]\n---\n",
        ),
        (
            "b.md",
            "---\ncreated: '{{date}}'\nnumber: not a number\n---\n",
        ),
        ("c.md", "---\ncreated: null\n---\n"),
        ("excluded.md", "---\nprivate: true\n---\n"),
    ]);
    let mut settings =
        frame_notes::VaultSettings::from_app_json(r#"{"userIgnoreFilters":["excluded.md"]}"#)
            .unwrap();
    settings
        .read_property_types(r#"{"types":{"number":"number"}}"#)
        .unwrap();
    v.set_settings(settings).unwrap();
    let props = v.index().properties();
    assert_eq!(props["created"], (Text, 3));
    assert_eq!(props["number"], (Number, 2));
    assert_eq!(props["nested"], (Unknown, 1));
    assert_eq!(props["list"], (List, 1));
    assert!(!props.contains_key("private"));
    assert_eq!(
        frame_notes::PropertyType::infer("x", &serde_json::json!("2026-10-25T14:30")),
        DateTime
    );
    assert_eq!(
        frame_notes::PropertyType::infer("x", &serde_json::json!([3])),
        Unknown
    );
    assert_eq!(
        frame_notes::PropertyType::infer("x", &serde_json::json!("2026-10-25T14:30:00")),
        Text
    );
    assert_eq!(
        frame_notes::PropertyType::infer("x", &serde_json::json!("٢٠٢٦-10-25")),
        Text
    );
}

#[test]
fn resolution_uses_utf16_length_and_exact_vault_paths_before_local_suffixes() {
    use frame_notes::{FileIndex, LinkSyntax};
    let names = FileIndex::new(["😀/Note.md", "aa/Note.md"]);
    assert_eq!(
        names.resolve("Note", "other/Source.md", LinkSyntax::Wiki),
        Some("aa/Note.md")
    );
    let names = FileIndex::new(["Note.md", "a/Note.md"]);
    for syntax in [LinkSyntax::Wiki, LinkSyntax::Markdown] {
        assert_eq!(
            names.resolve("Note", "a/Source.md", syntax),
            Some("Note.md")
        );
        assert_eq!(
            names.resolve("./Note", "a/Source.md", syntax),
            Some("a/Note.md")
        );
    }
}
