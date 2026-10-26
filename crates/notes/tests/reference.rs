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
            .map(|(name, count)| json!({key:name,"count":count}))
            .collect(),
    )
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
        "folders" => json!(index.folders()),
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
        "links" => json!(
            index
                .outgoing(path?)
                .iter()
                .map(|l| match &l.target {
                    frame_notes::Target::File(p) => p.clone(),
                    frame_notes::Target::Unresolved { name, .. } => name.clone(),
                })
                .collect::<std::collections::BTreeSet<_>>()
        ),
        "backlinks" => counts(index.backlinks(path?), "path"),
        "unresolved" if flag("total") => json!(index.unresolved().len()),
        "unresolved" => counts(index.unresolved(), "link"),
        "outline" => json!(
            index
                .outline(path?)
                .iter()
                .map(|h| json!({"heading":h.text,"level":h.level}))
                .collect::<Vec<_>>()
        ),
        "tags" => {
            let tags = if let Some(note) = note {
                let mut tags = BTreeMap::<String, usize>::new();
                for tag in &note.tags {
                    *tags.entry(tag.key()).or_default() += 1;
                }
                tags
            } else {
                index.tags()
            };
            if flag("total") {
                json!(tags.len())
            } else {
                counts(
                    tags.into_iter().map(|(tag, n)| (format!("#{tag}"), n)),
                    "tag",
                )
            }
        }
        "properties" if path.is_some() => json!(note?.frontmatter.as_ref().map(|f| &f.properties)),
        "properties" => Value::Array(
            index
                .properties()
                .into_iter()
                .map(|(name, (kind, count))| json!({"name":name,"type":kind,"count":count}))
                .collect(),
        ),
        "aliases" if path.is_some() => json!(
            note?
                .frontmatter
                .as_ref()
                .map(|f| f.aliases())
                .unwrap_or_default()
        ),
        "aliases" => {
            let values = index
                .files()
                .iter()
                .flat_map(|(path, f)| {
                    f.note
                        .iter()
                        .flat_map(|n| n.frontmatter.iter())
                        .flat_map(move |fm| {
                            fm.aliases()
                                .into_iter()
                                .map(move |alias| json!({"path":path,"alias":alias}))
                        })
                })
                .collect::<Vec<_>>();
            total(json!(values))
        }
        "tasks" => {
            let values=index.tasks().into_iter().filter(|(p,t)| path.is_none_or(|path| path==*p) && (!flag("todo") || t.status==' ') && (!flag("done") || t.status=='x') && param("status").is_none_or(|s| s==t.status.to_string())).map(|(path,t)| json!({"path":path,"line":t.span.line,"status":t.status.to_string(),"text":t.text})).collect::<Vec<_>>();
            total(json!(values))
        }
        "wordcount" => {
            let (words, characters) = index.word_count(path?)?;
            json!({"words":words,"characters":characters})
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
                json!({"line":line-1,"col":offsets.byte_to_utf16(byte)-offsets.byte_to_utf16(offsets.line_start(line)),"offset":offsets.byte_to_utf16(byte)})
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
            "footnotes":note.footnotes.iter().filter(|f|f.kind==frame_notes::FootnoteKind::Definition).map(|f|json!({"id":f.name,"position":pos(f.span)})).collect::<Vec<_>>(),
            "footnoteRefs":note.footnotes.iter().filter(|f|f.kind!=frame_notes::FootnoteKind::Definition).map(|f|json!({"id":f.name,"position":pos(f.span)})).collect::<Vec<_>>(),
            "frontmatter":note.frontmatter.as_ref().map(|f| &f.properties),
            "frontmatterPosition":note.frontmatter.as_ref().map(|f| pos(f.span)),
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
            if record["error"].is_string()
                || actual.as_ref().is_none_or(|a| {
                    if ordered {
                        *a != expected
                    } else {
                        sorted(a.clone()) != sorted(expected.clone())
                    }
                })
            {
                differences.push(json!({"group":group,"call":call,"expected":expected,"actual":actual,"reason":"Unverified oracle difference: compare value semantics and recorded CLI shape; no parity waiver is applied","reference_error":record["error"]}));
            } else {
                *matched.entry(group).or_default() += 1;
            }
        }
    }
    let report = json!({"matched":matched,"differences":differences,"omissions":omissions,
        "metadata_fields_not_compared":["sections","referenceLinks (reference definitions are resolved by comrak, not indexed separately)","listItems non-task entries/id/parent","frontmatterLinks.key/displayText/original","links/embeds.displayText (rendered display labels are not source aliases)"]});
    let output =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/notes-reference-differences.json");
    fs::write(&output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    assert!(
        differences.is_empty(),
        "{} Obsidian differences; every payload and reason is in {}",
        differences.len(),
        output.display()
    );
}
