use frame_notes::{
    ReconcileConflictReason, ReconcileResult, Vault, VaultSettings, apply_text_edits,
};
use std::{fs, path::Path};

fn setup(files: &[(&str, &str)]) -> (tempfile::TempDir, tempfile::TempDir, Vault) {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    for (path, text) in files {
        fs::create_dir_all(root.path().join(path).parent().unwrap()).unwrap();
        fs::write(root.path().join(path), text).unwrap();
    }
    let vault = Vault::open(root.path(), data.path()).unwrap();
    (root, data, vault)
}

#[test]
fn durable_sessions_save_undo_redo_and_profile_settings() {
    let (root, data, mut v) = setup(&[("a.md", "# Original\n")]);
    assert!(v.authority().describe().durable);
    let id = v.ids().get("a.md").unwrap().to_owned();
    let d = v.open_document("a.md").unwrap();
    let d = v.edit("a.md", d.revision, "edit-1", "# Changed\n").unwrap();
    assert_eq!(v.undo("a.md").unwrap().text, "# Original\n");
    assert_eq!(v.redo("a.md").unwrap().text, d.text);
    v.save("a.md").unwrap();
    v.close("a.md").unwrap();
    v.set_settings(VaultSettings::from_app_json(r#"{"userIgnoreFilters":["a.md"],"newFileLocation":"current","alwaysUpdateLinks":false}"#).unwrap()).unwrap();
    assert!(v.index().files()["a.md"].excluded);
    drop(v);
    fs::create_dir(root.path().join(".obsidian")).unwrap();
    fs::write(root.path().join(".obsidian/app.json"), "{}").unwrap();
    let mut v = Vault::open(root.path(), data.path()).unwrap();
    assert_eq!(v.ids().get("a.md"), Some(id.as_str()));
    assert_eq!(v.open_document("a.md").unwrap().text, "# Changed\n");
    assert!(v.index().files()["a.md"].excluded);
    assert!(!v.settings().always_update_links);
    assert_eq!(v.index().outline("a.md")[0].text, "Changed");
}

#[test]
fn external_disjoint_merge_preserves_undo_and_overlapping_edits_conflict() {
    let (root, _data, mut v) = setup(&[("a.md", "first\nmiddle\nlast\n")]);
    let d = v.open_document("a.md").unwrap();
    v.edit("a.md", d.revision, "local", "first local\nmiddle\nlast\n")
        .unwrap();
    fs::write(root.path().join("a.md"), "first\nmiddle\nlast disk\n").unwrap();
    let changes = v.refresh().unwrap();
    assert!(matches!(changes[0].1, ReconcileResult::Applied { .. }));
    assert_eq!(
        v.open_document("a.md").unwrap().text,
        "first local\nmiddle\nlast disk\n"
    );
    assert_eq!(v.undo("a.md").unwrap().text, "first\nmiddle\nlast disk\n");
    v.redo("a.md").unwrap();
    fs::write(
        root.path().join("a.md"),
        "first external\nmiddle\nlast disk\n",
    )
    .unwrap();
    let changes = v.refresh().unwrap();
    assert!(matches!(
        changes[0].1,
        ReconcileResult::Conflict {
            reason: ReconcileConflictReason::OverlappingEdits,
            ..
        }
    ));
    assert!(matches!(
        v.save("a.md").unwrap(),
        ReconcileResult::Conflict { .. }
    ));
    assert_eq!(
        fs::read_to_string(root.path().join("a.md")).unwrap(),
        "first external\nmiddle\nlast disk\n"
    );
}

#[test]
fn closed_external_change_is_a_reconcile_revision() {
    let (root, _data, mut v) = setup(&[("a.md", "old\n")]);
    fs::write(root.path().join("a.md"), "new\n").unwrap();
    v.refresh().unwrap();
    let id = v.authority().resolve("local", "a.md").unwrap().unwrap();
    let history = v.authority().history(&id, 0).unwrap();
    assert!(
        serde_json::to_string(&history)
            .unwrap()
            .contains("reconcile")
    );
    assert_eq!(v.open_document("a.md").unwrap().text, "new\n");
}

#[test]
fn canvas_and_base_invalid_edits_and_external_merges_are_refused() {
    for (path, valid, invalid) in [
        ("a.canvas", "{\"nodes\":[]}", "{"),
        ("a.base", "views: []", "views: ["),
    ] {
        let (root, _data, mut v) = setup(&[(path, valid)]);
        let d = v.open_document(path).unwrap();
        assert!(v.edit(path, d.revision, "bad", invalid).is_err());
        assert_eq!(v.open_document(path).unwrap().revision, d.revision);
        fs::write(root.path().join(path), invalid).unwrap();
        assert!(matches!(
            v.reconcile(path).unwrap(),
            ReconcileResult::Conflict {
                reason: ReconcileConflictReason::InvalidDocument,
                ..
            }
        ));
        assert_eq!(fs::read_to_string(root.path().join(path)).unwrap(), invalid);
    }
}

#[test]
fn index_queries_hidden_exclusions_and_incremental_resolution() {
    let (root, _data, mut v) = setup(&[
        (
            "a.md",
            "---\naliases: [First]\nscore: 3\n---\n# A\n[[B]] ![[pic.png]] [[Missing]]\n#Tag\n- [x] task\n",
        ),
        ("B.md", "# B\n"),
        (".hidden.md", "# hidden"),
        (".obsidian/app.json", "{}"),
    ]);
    assert_eq!(v.index().files().len(), 2);
    assert_eq!(v.index().backlinks("B.md")["a.md"], 1);
    assert_eq!(v.index().unresolved()["Missing"], 1);
    assert_eq!(v.index().tags()["tag"], 1);
    assert_eq!(v.index().tasks().len(), 1);
    assert_eq!(v.index().properties()["score"].1, 1);
    v.write_attachment("pic.png", b"image").unwrap();
    assert!(!v.index().unresolved().contains_key("pic.png"));
    fs::write(root.path().join("Missing.md"), "new target").unwrap();
    v.refresh().unwrap();
    assert!(!v.index().unresolved().contains_key("Missing"));
    assert_eq!(v.index().backlinks("Missing.md")["a.md"], 1);
    fs::remove_file(root.path().join("Missing.md")).unwrap();
    v.refresh().unwrap();
    assert!(v.ids().get("Missing.md").is_none());
    assert_eq!(v.index().unresolved()["Missing"], 1);
    assert!(v.write_attachment("a.md", b"bypass").is_err());
    assert!(v.open_document("../outside.md").is_err());
}

#[test]
fn explicit_move_preserves_id() {
    let (root, _data, mut v) = setup(&[("a.md", "original")]);
    let id = v.ids().get("a.md").unwrap().to_owned();
    fs::rename(root.path().join("a.md"), root.path().join("b.md")).unwrap();
    v.apply_events(&[rustyred_thg_vfs::VfsEvent::Moved {
        from: rustyred_thg_vfs::VfsPath::new(root.path().join("a.md")),
        to: rustyred_thg_vfs::VfsPath::new(root.path().join("b.md")),
    }])
    .unwrap();
    assert_eq!(v.ids().get("b.md"), Some(id.as_str()));
    assert!(v.ids().get("a.md").is_none());
}

#[test]
fn rename_plan_rewrites_aliases_fragments_embeds_markdown_properties_and_canvas() {
    let note = "---\nrelated: '[[Old#Section|property]]'\n---\n[[Old#Section|alias]] ![[Old#^block|size]] [label](Old.md#Section)\n";
    let canvas = "{\n  \"nodes\": [{\"id\":\"a\", \"type\":\"file\", \"file\":\"Old.md\", \"subpath\":\"#Section\"}, {\"id\":\"b\",\"type\":\"text\",\"text\":\"[[Old|card]]\"}], \"custom\": 42\n}";
    let (root, _data, v) = setup(&[
        ("Old.md", "# Section\ntext ^block"),
        ("Ref.md", note),
        ("Board.canvas", canvas),
    ]);
    let plan = v.plan_rename("Old.md", "folder/New.md").unwrap();
    let find = |path: &str| plan.documents.iter().find(|d| d.path == path).unwrap();
    let rewritten = apply_text_edits(note, &find("Ref.md").edits).unwrap();
    assert!(rewritten.contains("[[New#Section|alias]]"));
    assert!(rewritten.contains("![[New#^block|size]]"));
    assert!(rewritten.contains("[[New#Section|label]]"));
    assert!(rewritten.contains("'[[New#Section|property]]'"));
    let rewritten = apply_text_edits(canvas, &find("Board.canvas").edits).unwrap();
    assert!(rewritten.contains("\"file\":\"folder/New.md\""));
    assert!(rewritten.contains("[[New|card]]"));
    assert!(rewritten.contains("\"custom\": 42\n}"));
    assert_eq!(
        fs::read_to_string(root.path().join("Ref.md")).unwrap(),
        note
    );
    assert!(!root.path().join("folder/New.md").exists());
}

#[test]
fn rename_moved_source_rewrites_relative_outgoing_links_and_can_be_disabled() {
    let (_root, _data, mut v) = setup(&[
        ("a/Source.md", "[target](../Target.md)"),
        ("Target.md", "target"),
    ]);
    let mut settings =
        VaultSettings::from_app_json(r#"{"useMarkdownLinks":true,"newLinkFormat":"relative"}"#)
            .unwrap();
    v.set_settings(settings.clone()).unwrap();
    let plan = v.plan_rename("a/Source.md", "b/deep/Source.md").unwrap();
    assert_eq!(
        apply_text_edits("[target](../Target.md)", &plan.documents[0].edits).unwrap(),
        "[target](../../Target.md)"
    );
    settings.always_update_links = false;
    v.set_settings(settings).unwrap();
    assert!(
        v.plan_rename("a/Source.md", "elsewhere.md")
            .unwrap()
            .documents
            .is_empty()
    );
}

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

#[test]
fn fixture_vault_indexes_and_reopens() {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../notes-fixtures/vault"),
        root.path(),
    );
    let v = Vault::open(root.path(), data.path()).unwrap();
    assert_eq!(
        v.index()
            .files()
            .values()
            .filter(|f| f.kind == frame_notes::FileKind::Note)
            .count(),
        68
    );
    let resolved = v.index().resolved_links();
    drop(v);
    let v = Vault::open(root.path(), data.path()).unwrap();
    assert_eq!(v.index().resolved_links(), resolved);
}

#[test]
fn deletion_recreation_gets_fresh_note_and_authority_identity() {
    let (root, _data, mut v) = setup(&[("a.md", "old")]);
    let original = v.open_document("a.md").unwrap();
    let id = v.ids().get("a.md").unwrap().to_owned();
    fs::remove_file(root.path().join("a.md")).unwrap();
    v.refresh().unwrap();
    assert!(
        v.authority()
            .head(&original.document_id)
            .unwrap()
            .unwrap()
            .retired
    );
    fs::write(root.path().join("a.md"), "new document").unwrap();
    v.refresh().unwrap();
    let next = v.open_document("a.md").unwrap();
    assert_ne!(next.document_id, original.document_id);
    assert_ne!(v.ids().get("a.md").unwrap(), id);
    assert_eq!(next.text, "new document");
}

#[test]
fn rename_plan_includes_unsaved_authority_links_and_revision_guards() {
    let (_root, _data, mut v) = setup(&[("Old.md", "target"), ("Ref.md", "no links yet")]);
    let d = v.open_document("Ref.md").unwrap();
    let d = v
        .edit("Ref.md", d.revision, "unsaved-link", "[[Old|new link]]")
        .unwrap();
    let plan = v.plan_rename("Old.md", "New.md").unwrap();
    let edits = plan.documents.iter().find(|d| d.path == "Ref.md").unwrap();
    assert_eq!(edits.revision, d.revision);
    assert_eq!(edits.document_id, d.document_id);
    assert_eq!(
        apply_text_edits(&d.text, &edits.edits).unwrap(),
        "[[New|new link]]"
    );
}

#[test]
fn empty_folders_are_durable_and_refresh_removes_deleted_folders() {
    let (root, data, mut v) = setup(&[("a.md", "text")]);
    fs::create_dir_all(root.path().join("empty/nested")).unwrap();
    v.refresh().unwrap();
    assert!(v.index().folders().contains("empty/nested"));
    drop(v);
    let mut v = Vault::open(root.path(), data.path()).unwrap();
    assert!(v.index().folders().contains("empty/nested"));
    fs::remove_dir(root.path().join("empty/nested")).unwrap();
    v.refresh().unwrap();
    assert!(!v.index().folders().contains("empty/nested"));
    assert!(v.index().folders().contains("empty"));
}

#[test]
fn open_reports_conflict_and_invalid_external_canvas_does_not_prevent_mount() {
    let (root, data, mut v) = setup(&[("a.canvas", "{\"nodes\":[]}")]);
    fs::write(root.path().join("a.canvas"), "{").unwrap();
    assert!(matches!(
        v.open_document("a.canvas").unwrap().reconciliation,
        Some(ReconcileResult::Conflict {
            reason: ReconcileConflictReason::InvalidDocument,
            ..
        })
    ));
    drop(v);
    let v = Vault::open(root.path(), data.path()).unwrap();
    assert!(v.index().files()["a.canvas"].error.is_some());
}

#[test]
fn repeated_move_event_keeps_id_and_authority_identity() {
    let (root, data, mut v) = setup(&[("a.md", "original")]);
    let original = v.open_document("a.md").unwrap();
    v.close("a.md").unwrap();
    let id = v.ids().get("a.md").unwrap().to_owned();
    fs::rename(root.path().join("a.md"), root.path().join("b.md")).unwrap();
    let event = rustyred_thg_vfs::VfsEvent::Moved {
        from: rustyred_thg_vfs::VfsPath::new(root.path().join("a.md")),
        to: rustyred_thg_vfs::VfsPath::new(root.path().join("b.md")),
    };
    v.apply_events(&[event.clone(), event]).unwrap();
    v.refresh().unwrap();
    assert_eq!(v.ids().get("b.md"), Some(id.as_str()));
    assert_eq!(
        v.open_document("b.md").unwrap().document_id,
        original.document_id
    );
    drop(v);
    let v = Vault::open(root.path(), data.path()).unwrap();
    assert_eq!(v.ids().get("b.md"), Some(id.as_str()));
}

#[test]
fn replay_after_vfs_publication_before_index_keeps_move_identity() {
    use rustyred_thg_core::{
        DiskObjectStore, RedCoreDurability, RedCoreGraphStore, RedCoreOptions,
    };
    let (root, data, v) = setup(&[("a.md", "original")]);
    let id = v.ids().get("a.md").unwrap().to_owned();
    drop(v);
    fs::rename(root.path().join("a.md"), root.path().join("b.md")).unwrap();
    // Simulate the durable boundary immediately after VFS commits and before Vault consumes events.
    let graph = RedCoreGraphStore::open(
        data.path().join("vfs"),
        RedCoreOptions {
            durability: RedCoreDurability::AofAlways,
            ..Default::default()
        },
    )
    .unwrap();
    let mut vfs = rustyred_thg_vfs::Vfs::open_in(
        "frame-notes",
        graph,
        DiskObjectStore::open(data.path().join("blobs")).unwrap(),
    )
    .unwrap();
    vfs.mount(root.path()).unwrap();
    vfs.refresh().unwrap();
    drop(vfs);
    let v = Vault::open(root.path(), data.path()).unwrap();
    assert_eq!(v.ids().get("b.md"), Some(id.as_str()));
    assert!(v.ids().get("a.md").is_none());
}

#[test]
fn invalid_json_after_disjoint_merge_is_refused_without_a_revision() {
    // Removing an array wrapper and adding an element are disjoint line edits.
    // Both sides parse alone; together they yield an invalid object member.
    let base = "{\n  \"a\": [\n    1\n  ]\n}\n";
    let (root, _data, mut v) = setup(&[("a.canvas", base)]);
    let opened = v.open_document("a.canvas").unwrap();
    let local = "{\n  \"a\": [\n    1, 2\n  ]\n}\n";
    let edited = v
        .edit("a.canvas", opened.revision, "array-item", local)
        .unwrap();
    fs::write(root.path().join("a.canvas"), "{\n  \"a\": \n    1\n  \n}\n").unwrap();
    let result = v.reconcile("a.canvas").unwrap();
    assert!(matches!(
        result,
        ReconcileResult::Conflict {
            reason: ReconcileConflictReason::InvalidDocument,
            ..
        }
    ));
    let head = v.authority().head(&edited.document_id).unwrap().unwrap();
    assert_eq!(head.revision, edited.revision);
    assert_eq!(head.text, local);
}

#[test]
fn redcore_incoming_edges_survive_target_edits_and_reopen() {
    use rustyred_thg_core::GraphRead;
    let (root, data, mut v) = setup(&[("a.md", "[[B]]"), ("B.md", "# Old")]);
    let edges = |v: &Vault| {
        v.index()
            .graph()
            .live_edge_records()
            .into_iter()
            .filter(|e| e.edge_type == "LINKS_TO")
            .map(|e| (e.from_id, e.to_id))
            .collect::<Vec<_>>()
    };
    let before = edges(&v);
    assert_eq!(before.len(), 1);
    let d = v.open_document("B.md").unwrap();
    v.edit("B.md", d.revision, "target-heading", "# New")
        .unwrap();
    v.save("B.md").unwrap();
    assert_eq!(edges(&v), before);
    drop(v);
    let v = Vault::open(root.path(), data.path()).unwrap();
    assert_eq!(edges(&v), before);
}

#[test]
fn rename_quotes_yaml_scalar_values_without_changing_comments() {
    let note = "---\nétiquette: '[[Old|friend]]' # keep this comment\nother: \"[[Old]]\"\n---\nBody [[Old]]\n";
    let (_root, _data, v) = setup(&[("Old.md", "target"), ("Ref.md", note)]);
    let plan = v.plan_rename("Old.md", "O'Brien.md").unwrap();
    let edits = &plan
        .documents
        .iter()
        .find(|d| d.path == "Ref.md")
        .unwrap()
        .edits;
    let result = apply_text_edits(note, edits).unwrap();
    assert!(result.contains("\"[[O'Brien|friend]]\" # keep this comment"));
    assert!(result.contains("other: \"[[O'Brien]]\""));
    let parsed = frame_notes::parse_note(&result);
    assert!(parsed.links.iter().all(|link| link.target == "O'Brien"));
    let fm = parsed.frontmatter.unwrap();
    assert!(fm.error.is_none());
    assert_eq!(fm.properties["étiquette"], "[[O'Brien|friend]]");
}

#[test]
fn rebuilding_observes_new_and_removed_files_without_a_prior_refresh() {
    let (root, _data, mut v) = setup(&[("a.md", "[[New]]"), ("Gone.md", "old")]);
    fs::remove_file(root.path().join("Gone.md")).unwrap();
    fs::write(root.path().join("New.md"), "# New").unwrap();
    v.rebuild_index().unwrap();
    assert!(!v.index().files().contains_key("Gone.md"));
    assert_eq!(v.index().backlinks("New.md")["a.md"], 1);
    assert!(v.ids().get("Gone.md").is_none());
}
