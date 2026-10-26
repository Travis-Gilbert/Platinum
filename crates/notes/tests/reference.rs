//! The recorded CLI is the oracle. Missing recordings and unexplained differences are failures.
use frame_notes::{FileKind, Vault, VaultSettings};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

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
fn sorted(mut value: Value) -> Value {
    match &mut value {
        Value::Array(items) => {
            for item in items.iter_mut() {
                *item = sorted(item.take());
            }
            items.sort_by_key(Value::to_string);
        }
        Value::Object(items) => {
            for item in items.values_mut() {
                *item = sorted(item.take());
            }
        }
        _ => (),
    }
    value
}
fn lines(text: &str) -> Value {
    json!(text.lines().filter(|s| !s.is_empty()).collect::<Vec<_>>())
}
fn counts(values: impl IntoIterator<Item = (String, usize)>, key: &str) -> Value {
    Value::Array(
        values
            .into_iter()
            .map(|(name, count)| json!({key:name,"count":count.to_string()}))
            .collect(),
    )
}

fn listed(value: Value, empty: &str) -> Value {
    if value.as_array().is_some_and(Vec::is_empty) {
        lines(empty)
    } else {
        value
    }
}
fn property_name(kind: frame_notes::PropertyType) -> &'static str {
    use frame_notes::PropertyType::*;
    match kind {
        Unknown => "unknown",
        Text => "text",
        List => "multitext",
        Number => "number",
        Checkbox => "checkbox",
        Date => "date",
        DateTime => "datetime",
        Tags => "tags",
        Aliases => "aliases",
    }
}

/// Scope omissions are explicit; every other unsupported command fails with its recorded payload.
fn omitted(command: &str) -> Option<&'static str> {
    match command {
        "vault" => Some(
            "vault name, size and app window state are not modeled by the headless metadata index",
        ),
        "plugins:enabled" | "bookmarks" | "templates" | "snippets:enabled" => Some(
            "application/plugin configuration is step 5 or 11; no live .obsidian settings reader",
        ),
        _ => None,
    }
}
fn answer(v: &Vault, command: &str, args: &[String]) -> Option<Value> {
    let index = v.index();
    let flag = |s: &str| args.iter().any(|a| a == s);
    let param = |s: &str| args.iter().find_map(|a| a.strip_prefix(&format!("{s}=")));
    let path = param("path");
    let file = path.and_then(|p| index.files().get(p));
    let note = file.and_then(|f| f.note.as_ref());
    let total = |value: Value| {
        if flag("total") {
            json!(value.as_array().map_or(0, Vec::len))
        } else {
            value
        }
    };
    Some(match command {
        "files" => total(json!(index.files().keys().collect::<Vec<_>>())),
        "folders" => {
            let mut folders = index.folders();
            folders.insert("/".into());
            json!(folders)
        },
        "bases" => json!(
            index
                .files()
                .iter()
                .filter(|(_, f)| f.kind == FileKind::Base)
                .map(|(p, _)| p)
                .collect::<Vec<_>>()
        ),
        "orphans" => total(json!(index.orphans())),
        "deadends" => total(json!(index.dead_ends())),
        "links" => listed(json!(
            index
                .outgoing(path?)
                .iter()
                .map(|l| match &l.target {
                    frame_notes::Target::File(p) => p.clone(),
                    frame_notes::Target::Unresolved { .. } => format!("{} (unresolved)", l.link.target),
                })
                .collect::<std::collections::BTreeSet<_>>()
        ), "No links found."),
        "backlinks" => listed(counts(index.backlinks(path?), "file"), "No backlinks found."),
        "unresolved" if flag("total") => json!(index.unresolved().len()),
        "unresolved" => {
            let mut rows = counts(index.unresolved(), "link");
            if flag("verbose") {
                let sources = index.unresolved_links();
                for row in rows.as_array_mut().unwrap() {
                    let name = row["link"].as_str().unwrap();
                    row["sources"] = json!(sources.iter()
                        .filter(|(_, links)| links.contains_key(name))
                        .map(|(path, _)| path.as_str()).collect::<Vec<_>>().join(", "));
                }
            }
            listed(rows, "No unresolved links found.")
        },
        "outline" => listed(json!(
            index
                .outline(path?)
                .iter()
                .map(|h| json!({"heading":h.text,"level":h.level,"line":h.span.line}))
                .collect::<Vec<_>>()
        ), "No headings found."),
        "tags" => {
            let tags = if let Some(note) = note {
                let mut tags = BTreeMap::<String, usize>::new();
                for tag in &note.tags {
                    *tags.entry(tag.name.clone()).or_default() += 1;
                }
                tags
            } else {
                index.tags()
            };
            if flag("total") {
                json!(tags.len())
            } else {
                listed(counts(
                    tags.into_iter().map(|(tag, n)| (format!("#{tag}"), n)),
                    "tag",
                ), "No tags found.")
            }
        }
        "properties" if path.is_some() => match note?.frontmatter.as_ref() {
            Some(f) if !f.properties.is_empty() => json!(f.properties),
            _ => lines("No frontmatter found."),
        },
        "properties" => Value::Array(
            index
                .properties()
                .into_iter()
                .map(|(name, (kind, count))| json!({"name":name,"type":property_name(kind),"count":count}))
                .collect(),
        ),
        "aliases" if path.is_some() => listed(json!(
            note?
                .frontmatter
                .as_ref()
                .map(|f| f.aliases())
                .unwrap_or_default()
        ), "No aliases found."),
        "aliases" => {
            let mut aliases = BTreeMap::<String, Vec<&str>>::new();
            for (path, file) in index.files() {
                if let Some(fm) = file.note.as_ref().and_then(|n| n.frontmatter.as_ref()) {
                    for alias in fm.aliases() {
                        aliases.entry(alias).or_default().push(path);
                    }
                }
            }
            if flag("total") {
                json!(aliases.len())
            } else {
                listed(json!(aliases.into_iter().map(|(alias, paths)| {
                    if flag("verbose") { format!("{alias}\t{}", paths.join(", ")) } else { alias }
                }).collect::<Vec<_>>()), "No aliases found.")
            }
        }
        "tasks" => {
            let tasks = index.tasks().into_iter().filter(|(p,t)| {
                path.is_none_or(|path| path==*p) &&
                if let Some(status) = param("status") { status == t.status.to_string() }
                else if flag("done") { t.is_done() }
                else if flag("todo") { !t.is_done() }
                else { true }
            }).collect::<Vec<_>>();
            if flag("total") {
                json!(tasks.len())
            } else if tasks.is_empty() {
                lines("No tasks found.")
            } else {
                let mut rows = Vec::new();
                let mut verbose = Vec::new();
                let mut previous = None;
                for (path, task) in tasks {
                    let source = fs::read_to_string(v.root().join(path)).unwrap();
                    let raw = source.lines().nth(task.span.line as usize - 1).unwrap().trim();
                    rows.push(json!({"file":path,"line":task.span.line.to_string(),"status":task.status.to_string(),"text":raw}));
                    if previous != Some(path) {
                        verbose.push(path.to_string());
                        previous = Some(path);
                    }
                    verbose.push(format!("{}\t{raw}", task.span.line));
                }
                if flag("verbose") { json!(verbose) } else { json!(rows) }
            }
        }
        "wordcount" => {
            let (words, characters) = index.word_count(path?)?;
            json!([format!("words: {words}"), format!("characters: {characters}")])
        }
        "eval" => {
            let code = param("code")?;
            if code.contains(".resolvedLinks") {
                json!(index.resolved_links())
            } else if code.contains(".unresolvedLinks") {
                json!(index.unresolved_links())
            } else if code.contains("getCache") {
                metadata(v)
            } else if code == "app.vault.getFiles().length" {
                json!(index.files().len())
            } else {
                return None;
            }
        }
        _ => return None,
    })
}

fn metadata(v: &Vault) -> Value {
    let mut files = serde_json::Map::new();
    for (path, file) in v.index().files() {
        let Some(note) = &file.note else {
            continue;
        };
        let text = fs::read_to_string(v.root().join(path)).unwrap();
        let offsets = frame_notes::Offsets::new(&text);
        let pos = |span: frame_notes::Span| {
            let point = |byte| {
                let line = offsets.line_of(byte);
                json!({"line":line-1,"col":offsets.byte_to_utf16(byte)-offsets.byte_to_utf16(offsets.line_start(line)),"offset":offsets.byte_to_utf16(byte).saturating_sub(usize::from(text.starts_with('\u{feff}')))})
            };
            json!({"start":point(span.start),"end":point(span.end)})
        };
        let links = |embed| {
            note.links.iter().filter(|l| l.embed==embed && !l.in_frontmatter).map(|l| json!({"link":format!("{}{}",l.target,l.fragment.as_ref().map(ToString::to_string).unwrap_or_default()),"position":pos(l.span),"original":l.span.slice(&text)})).collect::<Vec<_>>()
        };
        files.insert(path.clone(),json!({
            "headings":note.headings.iter().map(|h|json!({"heading":h.text,"level":h.level,"position":pos(h.span)})).collect::<Vec<_>>(),
            "blocks":note.blocks.iter().map(|b|(b.id.clone(),json!({"id":b.id,"position":pos(b.span)}))).collect::<BTreeMap<_,_>>(),
            "links":links(false),"embeds":links(true),
            "tags":note.tags.iter().filter(|t| t.source==frame_notes::TagSource::Body).map(|t|json!({"tag":format!("#{}",t.name),"position":pos(t.span)})).collect::<Vec<_>>(),
            "listItems":note.tasks.iter().map(|t|json!({"task":t.status.to_string(),"position":pos(t.span)})).collect::<Vec<_>>(),
            "footnotes":note.footnotes.iter().filter(|f|f.kind!=frame_notes::FootnoteKind::Reference).map(|f|json!({"id":f.name,"position":pos(f.span)})).collect::<Vec<_>>(),
            "footnoteRefs":note.footnotes.iter().filter(|f|f.kind==frame_notes::FootnoteKind::Reference).map(|f|json!({"id":f.name,"position":pos(f.span)})).collect::<Vec<_>>(),
            "frontmatter":note.frontmatter.as_ref().filter(|f| f.error.is_none()).map(|f| &f.properties),
            "frontmatterPosition":note.frontmatter.as_ref().filter(|f| f.error.is_none()).map(|f| pos(f.span)),
            "frontmatterLinks":note.links.iter().filter(|l|l.in_frontmatter).map(|l|json!({"link":format!("{}{}",l.target,l.fragment.as_ref().map(ToString::to_string).unwrap_or_default())})).collect::<Vec<_>>()
        }));
    }
    Value::Object(files)
}

// Compare only modeled metadata fields; retain every note path and every entry in those fields.
fn modeled_metadata(expected: Value, actual: &Value) -> Value {
    let Value::Object(files) = expected else {
        return expected;
    };
    Value::Object(
        files
            .into_iter()
            .map(|(path, cache)| {
                let mut out = serde_json::Map::new();
                if let Some(fields) = actual.get(&path).and_then(Value::as_object) {
                    for (key, template) in fields {
                        let value = cache.get(key).cloned().unwrap_or_else(|| match template {
                            Value::Array(_) => json!([]),
                            Value::Object(_) => json!({}),
                            _ => Value::Null,
                        });
                        let value = match key.as_str() {
                            "headings" => project_array(value, &["heading", "level", "position"]),
                            "links" | "embeds" => {
                                project_array(value, &["link", "position", "original"])
                            }
                            "tags" => project_array(value, &["tag", "position"]),
                            "footnotes" | "footnoteRefs" => {
                                project_array(value, &["id", "position"])
                            }
                            "listItems" => {
                                let tasks = match value {
                                    Value::Array(items) => Value::Array(
                                        items
                                            .into_iter()
                                            .filter(|item| item.get("task").is_some())
                                            .collect(),
                                    ),
                                    _ => value,
                                };
                                project_array(tasks, &["task", "position"])
                            }
                            "frontmatterLinks" => project_array(value, &["link"]),
                            _ => value,
                        };
                        out.insert(key.clone(), value);
                    }
                }
                (path, Value::Object(out))
            })
            .collect(),
    )
}
fn project_array(value: Value, keys: &[&str]) -> Value {
    match value {
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| {
                    Value::Object(
                        keys.iter()
                            .filter_map(|key| item.get(*key).map(|v| ((*key).into(), v.clone())))
                            .collect(),
                    )
                })
                .collect(),
        ),
        _ => value,
    }
}

/// Enumerate only the load-order ambiguity explicitly accepted by the plan. All fixed links
/// stay in their original buckets; each eligible occurrence can move exactly once.
fn resolution_variants(v: &Vault) -> (usize, Vec<frame_notes::LinkMap>) {
    let index = v.index();
    let mut variants = vec![index.resolved_links()];
    let mut ties = 0;
    for (source, file) in index.files() {
        for link in &file.links {
            if link.link.syntax == frame_notes::LinkSyntax::Path {
                continue;
            }
            let candidates =
                index
                    .names()
                    .resolution_candidates(&link.link.target, source, link.link.syntax);
            let folder = frame_notes::dir(source).to_lowercase();
            if candidates
                .iter()
                .any(|p| p.to_lowercase().starts_with(&folder))
            {
                continue;
            }
            let Some(first) = candidates.first() else {
                continue;
            };
            let tied = candidates
                .iter()
                .copied()
                .take_while(|p| p.encode_utf16().count() == first.encode_utf16().count())
                .collect::<Vec<_>>();
            if tied.len() < 2 {
                continue;
            }
            let frame_notes::Target::File(original) = &link.target else {
                panic!("resolved candidates must have a destination");
            };
            ties += 1;
            let mut next = std::collections::BTreeSet::new();
            for variant in &variants {
                for candidate in &tied {
                    let mut variant = variant.clone();
                    let row = variant.get_mut(source).unwrap();
                    let count = row.get_mut(original).unwrap();
                    *count -= 1;
                    if *count == 0 {
                        row.remove(original);
                    }
                    *row.entry((*candidate).to_string()).or_default() += 1;
                    next.insert(variant);
                }
            }
            variants = next.into_iter().collect();
        }
    }
    (ties, variants)
}

fn matches_resolution_variant(
    variants: &[frame_notes::LinkMap],
    command: &str,
    args: &[String],
    expected: &Value,
) -> bool {
    variants.iter().any(|map| {
        let value = if command == "eval"
            && args
                .iter()
                .any(|a| a == "code=JSON.stringify(app.metadataCache.resolvedLinks)")
        {
            json!(map)
        } else if command == "backlinks"
            && args.iter().any(|a| a == "counts")
            && args.iter().any(|a| a == "format=json")
        {
            let Some(path) = args.iter().find_map(|a| a.strip_prefix("path=")) else {
                return false;
            };
            listed(
                counts(
                    map.iter().filter_map(|(source, links)| {
                        links.get(path).map(|n| (source.clone(), *n))
                    }),
                    "file",
                ),
                "No backlinks found.",
            )
        } else {
            return false;
        };
        sorted(value) == sorted(expected.clone())
    })
}

#[test]
fn fixture_vault_matches_obsidian_recorded_groups() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../notes-fixtures");
    let reference = fixture.join("reference/fixture-vault");
    let groups = [
        "vault",
        "links",
        "outline",
        "properties",
        "tags",
        "tasks",
        "wordcount",
        "eval",
    ];
    let missing = groups
        .iter()
        .filter(|g| !reference.join(format!("{g}.json")).is_file())
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "Obsidian reference files missing in {}: {missing:?}; record these groups before parity can pass",
        reference.display()
    );
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    copy_tree(&fixture.join("vault"), root.path());
    let mut v = Vault::open(root.path(), data.path()).unwrap();
    // Explicit test configuration matching the oracle; production never reads .obsidian live.
    let mut settings = VaultSettings::from_app_json(
        &fs::read_to_string(root.path().join(".obsidian/app.json")).unwrap(),
    )
    .unwrap();
    settings
        .read_property_types(&fs::read_to_string(root.path().join(".obsidian/types.json")).unwrap())
        .unwrap();
    v.set_settings(settings).unwrap();
    let (ties, variants) = resolution_variants(&v);
    assert!(
        ties > 0,
        "fixture must exercise the accepted load-order ambiguity"
    );
    let mut differences = Vec::new();
    let mut omissions = Vec::new();
    let mut matched = BTreeMap::<&str, usize>::new();
    for group in groups {
        let records: BTreeMap<String, Value> =
            serde_json::from_slice(&fs::read(reference.join(format!("{group}.json"))).unwrap())
                .unwrap();
        assert!(
            !records.is_empty(),
            "reference group {group} has no commands"
        );
        for (call, record) in records {
            let command = record["command"].as_str().unwrap();
            if let Some(reason) = omitted(command) {
                omissions.push(json!({"group":group,"call":call,"reason":reason}));
                continue;
            }
            let args: Vec<String> = serde_json::from_value(record["args"].clone()).unwrap();
            let actual = answer(&v, command, &args);
            let mut expected = record.get("parsed").cloned().unwrap_or_else(|| {
                let stdout = record["stdout"].as_str().unwrap_or_default().trim();
                if args.iter().any(|a| a == "total") || command == "eval" {
                    serde_json::from_str(stdout).unwrap_or_else(|_| json!(stdout))
                } else {
                    lines(stdout)
                }
            });
            if command == "eval"
                && args.iter().any(|a| a.contains("getCache"))
                && let Some(actual) = &actual
            {
                expected = modeled_metadata(expected, actual);
            }
            let ordered = command == "outline"
                || (command == "eval" && args.iter().any(|a| a.contains("getCache")));
            let exact = actual.as_ref().is_some_and(|a| {
                if ordered {
                    *a == expected
                } else {
                    sorted(a.clone()) == sorted(expected.clone())
                }
            });
            let accepted_tie = actual.as_ref().is_some_and(|actual| {
                // A broken backlink query must not be repaired by using resolvedLinks instead.
                matches_resolution_variant(
                    std::slice::from_ref(&v.index().resolved_links()),
                    command,
                    &args,
                    actual,
                ) && matches_resolution_variant(&variants, command, &args, &expected)
            });
            if record["error"].is_string() || !(exact || accepted_tie)
            {
                differences.push(json!({"group":group,"call":call,"expected":expected,"actual":actual,"reason":"Unverified oracle difference: compare value semantics and recorded CLI shape; no parity waiver is applied","reference_error":record["error"]}));
            } else {
                *matched.entry(group).or_default() += 1;
            }
        }
    }
    let report = json!({"accepted_resolution_ties":ties,"resolution_variants":variants.len(),"matched":matched,"differences":differences,"omissions":omissions,
        "metadata_fields_not_compared":["sections","referenceLinks (reference definitions are resolved by comrak, not indexed separately)","listItems non-task entries/id/parent","frontmatterLinks.key/displayText/original","links/embeds.displayText (rendered display labels are not source aliases)"]});
    let output = Path::new(env!("CARGO_TARGET_TMPDIR")).join("notes-reference-differences.json");
    fs::write(&output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    assert!(
        differences.is_empty(),
        "{} Obsidian differences; every payload and reason is in {}",
        differences.len(),
        output.display()
    );
}

#[cfg(test)]
mod adapter_regressions {
    use super::*;

    fn with_vault(test: impl FnOnce(&Vault)) {
        let root = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        fs::write(root.path().join("Empty.md"), "").unwrap();
        fs::write(root.path().join("Source.md"), "---\naliases: [First]\ntags: [test]\n---\n# Heading\n[[Empty]] [[Missing]]\n- [x] Done\n1. [ ] Open\n> - [ ] Quote\n").unwrap();
        let vault = Vault::open(root.path(), data.path()).unwrap();
        test(&vault);
    }
    fn call(vault: &Vault, command: &str, args: &[&str]) -> Value {
        answer(
            vault,
            command,
            &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
        .unwrap()
    }

    #[test]
    fn counts_use_strings_and_backlinks_use_file() {
        with_vault(|v| {
            assert_eq!(
                call(v, "backlinks", &["path=Empty.md"]),
                json!([{"file":"Source.md","count":"1"}])
            );
            assert_eq!(
                call(v, "tags", &["path=Source.md"]),
                json!([{"tag":"#test","count":"1"}])
            );
        });
    }

    #[test]
    fn empty_listings_use_the_recorded_sentences() {
        with_vault(|v| {
            for (command, sentence) in [
                ("links", "No links found."),
                ("backlinks", "No backlinks found."),
                ("outline", "No headings found."),
                ("tags", "No tags found."),
                ("tasks", "No tasks found."),
                ("aliases", "No aliases found."),
                ("properties", "No frontmatter found."),
            ] {
                assert_eq!(
                    call(
                        v,
                        command,
                        &[if command == "backlinks" {
                            "path=Source.md"
                        } else {
                            "path=Empty.md"
                        }]
                    ),
                    json!([sentence]),
                    "{command}"
                );
            }
        });
    }

    #[test]
    fn outline_lines_are_numbers() {
        with_vault(|v| {
            assert_eq!(
                call(v, "outline", &["path=Source.md"]),
                json!([{"heading":"Heading","level":1,"line":5}])
            )
        });
    }

    #[test]
    fn task_rows_keep_raw_lines_and_string_line_numbers() {
        with_vault(|v| {
            assert_eq!(
                call(v, "tasks", &["path=Source.md"]),
                json!([
                    {"file":"Source.md","line":"7","status":"x","text":"- [x] Done"},
                    {"file":"Source.md","line":"8","status":" ","text":"1. [ ] Open"},
                    {"file":"Source.md","line":"9","status":" ","text":"> - [ ] Quote"},
                ])
            )
        });
    }

    #[test]
    fn task_verbose_groups_lines_by_file() {
        with_vault(|v| {
            assert_eq!(
                call(v, "tasks", &["verbose"]),
                json!([
                    "Source.md",
                    "7\t- [x] Done",
                    "8\t1. [ ] Open",
                    "9\t> - [ ] Quote"
                ])
            )
        });
    }

    #[test]
    fn wordcount_uses_labeled_lines() {
        with_vault(|v| {
            assert_eq!(
                call(v, "wordcount", &["path=Empty.md"]),
                json!(["words: 0", "characters: 0"])
            )
        });
    }

    #[test]
    fn folders_include_the_root() {
        with_vault(|v| assert_eq!(call(v, "folders", &[]), json!(["/"])));
    }

    #[test]
    fn unresolved_verbose_includes_sources_and_links_have_a_suffix() {
        with_vault(|v| {
            assert_eq!(
                call(v, "unresolved", &["verbose"]),
                json!([{"link":"Missing","count":"1","sources":"Source.md"}])
            );
            assert_eq!(
                call(v, "links", &["path=Source.md"]),
                json!(["Empty.md", "Missing (unresolved)"])
            );
        });
    }

    #[test]
    fn aliases_verbose_uses_tab_separated_lines() {
        with_vault(|v| {
            assert_eq!(
                call(v, "aliases", &["verbose"]),
                json!(["First\tSource.md"])
            )
        });
    }

    #[test]
    fn property_table_counts_remain_numbers() {
        with_vault(|v| {
            assert_eq!(
                call(v, "properties", &[]),
                json!([
                    {"name":"aliases","type":"aliases","count":1},
                    {"name":"tags","type":"tags","count":1}
                ])
            )
        });
    }

    #[test]
    fn property_names_use_obsidian_spelling() {
        use frame_notes::PropertyType::*;
        assert_eq!(
            [Text, List, Number, Checkbox, Date, DateTime, Tags, Aliases].map(property_name),
            [
                "text",
                "multitext",
                "number",
                "checkbox",
                "date",
                "datetime",
                "tags",
                "aliases"
            ]
        );
    }
}

#[test]
fn load_order_acceptance_moves_only_derived_tied_occurrences() {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    for (path, text) in [
        ("a/Note.md", ""),
        ("b/Note.md", ""),
        ("other/Source.md", "[[Note]] [[a/Note]]"),
    ] {
        let path = root.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let v = Vault::open(root.path(), data.path()).unwrap();
    let (ties, variants) = resolution_variants(&v);
    assert_eq!(ties, 1);
    assert_eq!(variants.len(), 2);
    let args = vec!["code=JSON.stringify(app.metadataCache.resolvedLinks)".into()];
    let mut swapped = v.index().resolved_links();
    swapped.insert(
        "other/Source.md".into(),
        BTreeMap::from([("a/Note.md".into(), 1), ("b/Note.md".into(), 1)]),
    );
    assert!(matches_resolution_variant(
        &variants,
        "eval",
        &args,
        &json!(swapped)
    ));
    for path in ["a/Note.md", "b/Note.md"] {
        assert!(matches_resolution_variant(
            &variants,
            "backlinks",
            &[
                format!("path={path}"),
                "counts".into(),
                "format=json".into()
            ],
            &json!([{"file":"other/Source.md","count":"1"}])
        ));
    }
    swapped
        .get_mut("other/Source.md")
        .unwrap()
        .remove("a/Note.md");
    assert!(!matches_resolution_variant(
        &variants,
        "eval",
        &args,
        &json!(swapped)
    ));
    assert!(!matches_resolution_variant(
        &variants,
        "links",
        &[],
        &json!([])
    ));
}

#[test]
fn local_or_unequal_candidates_are_not_accepted_ties() {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    for (path, text) in [
        ("a/Note.md", "[[Note]]"),
        ("b/Note.md", ""),
        ("long/Note.md", ""),
        ("source.md", "[[Note]]"),
        ("other/Source.md", "[[Unique]]"),
        ("a/Unique.md", ""),
        ("long/Unique.md", ""),
    ] {
        let path = root.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let v = Vault::open(root.path(), data.path()).unwrap();
    assert_eq!(resolution_variants(&v).0, 0);
}

#[test]
fn metadata_uses_bomless_utf16_coordinates_and_omits_invalid_frontmatter_values() {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("Bom.md"),
        "\u{feff}---\nx: 1\n---\n# Title\n",
    )
    .unwrap();
    fs::write(root.path().join("Bad.md"), "---\nx: [broken\n---\n").unwrap();
    let v = Vault::open(root.path(), data.path()).unwrap();
    let cache = metadata(&v);
    assert_eq!(
        cache["Bom.md"]["headings"][0]["position"],
        json!({
            "start":{"line":3,"col":0,"offset":13}, "end":{"line":3,"col":7,"offset":20}
        })
    );
    assert_eq!(cache["Bad.md"]["frontmatter"], Value::Null);
    assert_eq!(cache["Bad.md"]["frontmatterPosition"], Value::Null);
    assert!(
        v.index().files()["Bad.md"]
            .note
            .as_ref()
            .unwrap()
            .frontmatter
            .as_ref()
            .unwrap()
            .error
            .is_some()
    );
}
