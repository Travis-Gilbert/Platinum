use frame_profile::{ProfileStore, Session, SessionTab};
use rusqlite::Connection;
use tempfile::TempDir;

fn legacy(root: &std::path::Path) {
    let catalog = Connection::open(root.join("profiles.sqlite3")).unwrap();
    catalog.execute_batch("CREATE TABLE profiles(id INTEGER PRIMARY KEY,name TEXT,email TEXT,source TEXT); INSERT INTO profiles VALUES(1,'Default','',''),(2,'Work','work@example.test','source'); CREATE TABLE settings(key TEXT,value INTEGER); INSERT INTO settings VALUES('active',2);").unwrap();
    let dir = root.join("profiles/2");
    std::fs::create_dir_all(&dir).unwrap();
    let db = Connection::open(dir.join("browser.sqlite3")).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE bookmarks(url TEXT,title TEXT,folder TEXT); INSERT INTO bookmarks VALUES('https://servo.org/','Servo browser','Work'); CREATE TABLE history(url TEXT,title TEXT,visited INTEGER,visits INTEGER); INSERT INTO history VALUES('https://private.example/','Private visit',42,8); CREATE TABLE credentials(origin TEXT,username TEXT); INSERT INTO credentials VALUES('https://servo.org','person'); CREATE TABLE extensions(id TEXT,name TEXT,version TEXT); INSERT INTO extensions VALUES('ext','Extension','1'); CREATE TABLE form_information(name TEXT,value TEXT); INSERT INTO form_information VALUES('city','Detroit'); CREATE TABLE imports(id INTEGER,source TEXT,category TEXT,completed INTEGER,imported INTEGER,skipped INTEGER); INSERT INTO imports VALUES(1,'source','History',42,1,0); CREATE TABLE session(id INTEGER,value TEXT);").unwrap();
    db.execute(
        "INSERT INTO session VALUES(1,?1)",
        [serde_json::to_string(&Session {
            tabs: vec![SessionTab {
                url: Some("https://servo.org/".into()),
                query: None,
            }],
            active: 0,
            layout: Some(serde_json::json!({"dock":"saved"})),
        })
        .unwrap()],
    )
    .unwrap();
}
#[test]
fn migrates_all_records_preserves_identity_and_does_not_reimport() {
    let temp = TempDir::new().unwrap();
    legacy(temp.path());
    let store = ProfileStore::open(temp.path()).unwrap();
    assert_eq!(store.profile.id, 2);
    assert_eq!(store.profile.email, "work@example.test");
    assert_eq!(store.profiles().unwrap().len(), 2);
    assert_eq!(store.suggestions("serv", true, 10).unwrap().len(), 1);
    assert_eq!(
        store.history_metadata("https://private.example/").unwrap(),
        Some((42, 8))
    );
    assert_eq!(
        store.extensions().unwrap(),
        vec![("Extension".into(), "1".into())]
    );
    assert_eq!(
        store.form_values().unwrap(),
        vec![("city".into(), "Detroit".into())]
    );
    assert_eq!(store.credential_labels().unwrap().len(), 1);
    assert_eq!(store.import_count().unwrap(), 1);
    assert_eq!(
        store.session().unwrap().layout,
        Some(serde_json::json!({"dock":"saved"}))
    );
    store.clear_history().unwrap();
    drop(store);
    let reopened = ProfileStore::open(temp.path()).unwrap();
    assert!(
        reopened
            .suggestions("private", false, 10)
            .unwrap()
            .is_empty()
    );
    assert_eq!(reopened.suggestions("servo", true, 10).unwrap().len(), 1);
    let old = Connection::open(temp.path().join("profiles/2/browser.sqlite3")).unwrap();
    assert_eq!(
        old.query_row("SELECT count(*) FROM history", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn shared_writer_sees_concurrent_commits_and_profiles_are_isolated() {
    let temp = TempDir::new().unwrap();
    let store = ProfileStore::open(temp.path()).unwrap();
    let other = ProfileStore::open(temp.path()).unwrap();
    let work = store.imported_profile("work", "Work", "").unwrap();
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let s = store.clone();
            std::thread::spawn(move || s.visit("https://servo.org/", "Servo browser").unwrap())
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(
        other
            .history_metadata("https://servo.org/")
            .unwrap()
            .unwrap()
            .1,
        8
    );
    assert!(work.suggestions("servo", false, 10).unwrap().is_empty());
    assert_eq!(other.suggestions("servo bro", false, 10).unwrap().len(), 1);
    drop(other);
    drop(work);
    drop(store);
    assert_eq!(
        ProfileStore::open(temp.path())
            .unwrap()
            .suggestions("servo", false, 10)
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn failed_migration_does_not_commit_partial_records_and_can_retry() {
    let temp = TempDir::new().unwrap();
    legacy(temp.path());
    let path = temp.path().join("profiles/2/browser.sqlite3");
    let db = Connection::open(&path).unwrap();
    db.execute("UPDATE session SET value='invalid'", [])
        .unwrap();
    assert!(ProfileStore::open(temp.path()).is_err());
    db.execute(
        "UPDATE session SET value=?1",
        [serde_json::to_string(&Session::default()).unwrap()],
    )
    .unwrap();
    drop(db);
    let store = ProfileStore::open(temp.path()).unwrap();
    assert_eq!(store.suggestions("", true, 10).unwrap().len(), 1);
}
#[test]
fn abandoned_import_is_atomic_and_clear_history_has_no_revision_recovery() {
    let temp = TempDir::new().unwrap();
    let store = ProfileStore::open(temp.path()).unwrap();
    {
        let mut batch = store.import_batch().unwrap();
        batch
            .bookmark("https://example.test/", "Uncommitted", "Work")
            .unwrap();
    }
    assert!(store.suggestions("", true, 10).unwrap().is_empty());
    store
        .visit("https://forgotten.test/", "Unique forgotten title")
        .unwrap();
    let (previous_commit, forgotten_id) = {
        let reader = rustyred::Database::open_read_only(temp.path().join("browser.RR")).unwrap();
        let id = reader
            .engine()
            .query_nodes(rustyred::engine::NodeQuery::label("History"))
            .unwrap()[0]
            .id
            .clone();
        (
            reader
                .engine()
                .pin_graph_history()
                .unwrap()
                .head_roots()
                .logical_commit,
            id,
        )
    };
    store.clear_history().unwrap();
    drop(store);
    let db = rustyred::Database::open_read_only(temp.path().join("browser.RR")).unwrap();
    assert!(
        db.fulltext_search("History", "forgotten", 10, None)
            .unwrap()
            .is_empty()
    );
    assert!(
        db.engine()
            .query_nodes(rustyred::engine::NodeQuery::label("History"))
            .unwrap()
            .is_empty()
    );
    use rustyred::engine::graph::capability::GraphRead;
    if let Some(tombstone) = db
        .engine()
        .get_node_record_including_tombstone(&forgotten_id)
    {
        assert!(tombstone.tombstone);
        assert_eq!(tombstone.properties, serde_json::json!({}));
        assert!(tombstone.labels.is_empty());
    }
    let history = db.engine().pin_graph_history().unwrap();
    assert!(history.revision(previous_commit).is_err());
    assert!(matches!(
        history.coverage(),
        rustyred::engine::graph_history::GraphHistoryCoverage::Since {
            prior_history_unavailable: true,
            ..
        }
    ));
}

#[test]
fn queued_visit_before_clear_cannot_resurrect_history() {
    let temp = TempDir::new().unwrap();
    let store = ProfileStore::open(temp.path()).unwrap();
    let before = frame_profile::now();
    store.clear_history().unwrap();
    store
        .visit_at("https://forgotten.test/", "Queued visit", before)
        .unwrap();
    assert!(store.suggestions("", false, 10).unwrap().is_empty());
    store
        .visit_at("https://new.test/", "New visit", frame_profile::now() + 1)
        .unwrap();
    assert_eq!(store.suggestions("", false, 10).unwrap().len(), 1);
}

#[test]
fn legacy_default_profile_migrates_without_an_active_setting() {
    let temp = TempDir::new().unwrap();
    legacy(temp.path());
    Connection::open(temp.path().join("profiles.sqlite3"))
        .unwrap()
        .execute("DELETE FROM settings WHERE key='active'", [])
        .unwrap();
    let directory = temp.path().join("profile");
    std::fs::create_dir_all(&directory).unwrap();
    Connection::open(directory.join("browser.sqlite3")).unwrap().execute_batch(
        "CREATE TABLE bookmarks(url TEXT,title TEXT,folder TEXT); INSERT INTO bookmarks VALUES('https://default.test/','Default bookmark','Saved');"
    ).unwrap();
    let store = ProfileStore::open(temp.path()).unwrap();
    assert_eq!(store.profile.id, 1);
    let bookmarks = store.suggestions("default", true, 10).unwrap();
    assert_eq!(bookmarks.len(), 1);
    assert_eq!(bookmarks[0].folder, "Saved");
    assert!(store.suggestions("servo", true, 10).unwrap().is_empty());
}

#[test]
fn suggestions_rank_exact_urls_and_match_titles_and_literal_paths() {
    let temp = TempDir::new().unwrap();
    let store = ProfileStore::open(temp.path()).unwrap();
    store
        .bookmark("https://servo.org/", "Engine homepage")
        .unwrap();
    store
        .bookmark("https://other.test/docs", "servo.org reference")
        .unwrap();
    store
        .bookmark("https://other.test/a-b?q=one", "Useful notes")
        .unwrap();
    assert_eq!(
        store.suggestions("servo.org", true, 1).unwrap()[0].url,
        "https://servo.org/"
    );
    assert_eq!(
        store.suggestions("  REFERENCE  ", true, 10).unwrap()[0].url,
        "https://other.test/docs"
    );
    assert_eq!(
        store.suggestions("/a-b?q=", true, 10).unwrap()[0].title,
        "Useful notes"
    );
    assert_eq!(store.suggestions("", true, 2).unwrap().len(), 2);
    assert!(store.suggestions("", true, 0).unwrap().is_empty());
    assert!(
        store
            .suggestions("not-a-match", true, 10)
            .unwrap()
            .is_empty()
    );
    store
        .bookmark("https://servo.org/", "Updated homepage")
        .unwrap();
    assert_eq!(store.suggestions("", true, 10).unwrap().len(), 3);
    assert_eq!(
        store.suggestions("updated", true, 10).unwrap()[0].title,
        "Updated homepage"
    );
}

#[test]
fn history_ranks_recency_then_frequency_and_retains_updated_titles() {
    let temp = TempDir::new().unwrap();
    let store = ProfileStore::open(temp.path()).unwrap();
    for (url, at) in [
        ("https://old.test/", 10),
        ("https://frequent.test/", 20),
        ("https://frequent.test/", 20),
        ("https://recent.test/", 20),
    ] {
        store.visit_at(url, "Same title", at).unwrap();
    }
    let urls: Vec<_> = store
        .suggestions("", false, 10)
        .unwrap()
        .into_iter()
        .map(|e| e.url)
        .collect();
    assert_eq!(
        urls,
        [
            "https://frequent.test/",
            "https://recent.test/",
            "https://old.test/"
        ]
    );
    store
        .visit_at("https://recent.test/", "Changed title", 30)
        .unwrap();
    assert_eq!(
        store.history_metadata("https://recent.test/").unwrap(),
        Some((30, 2))
    );
    assert_eq!(
        store.suggestions("changed", false, 1).unwrap()[0].url,
        "https://recent.test/"
    );
}

#[test]
fn invalid_browser_visits_are_ignored_and_bookmarks_rejected() {
    let temp = TempDir::new().unwrap();
    let store = ProfileStore::open(temp.path()).unwrap();
    for url in [
        "not a URL",
        "about:blank",
        "https://user:secret@example.test/",
    ] {
        store.visit(url, "Not recorded").unwrap();
        assert!(store.bookmark(url, "Not saved").is_err());
    }
    assert!(store.suggestions("", false, 10).unwrap().is_empty());
    assert!(store.suggestions("", true, 10).unwrap().is_empty());
}

#[test]
fn credential_labels_update_and_delete_without_crossing_profiles() {
    let temp = TempDir::new().unwrap();
    let store = ProfileStore::open(temp.path()).unwrap();
    let other = store.imported_profile("work", "Work", "").unwrap();
    for profile in [&store, &other] {
        profile
            .save_credential_label("https://example.test", "person")
            .unwrap();
    }
    store
        .save_credential_label("https://example.test", "person")
        .unwrap();
    assert_eq!(store.credential_labels().unwrap().len(), 1);
    store
        .delete_credential_label("https://example.test", "person")
        .unwrap();
    assert!(store.credential_labels().unwrap().is_empty());
    assert_eq!(
        other.credential_labels().unwrap(),
        vec![("https://example.test".into(), "person".into())]
    );
}

#[test]
fn import_batch_deduplicates_pending_and_committed_records_and_merges_history() {
    let temp = TempDir::new().unwrap();
    let store = ProfileStore::open(temp.path()).unwrap();
    for retry in [false, true] {
        let mut batch = store.import_batch().unwrap();
        assert_eq!(
            batch
                .bookmark("https://example.test/", "Bookmark", "Saved")
                .unwrap(),
            usize::from(!retry)
        );
        assert_eq!(
            batch
                .bookmark("https://example.test/", "Bookmark", "Saved")
                .unwrap(),
            0
        );
        assert_eq!(
            batch
                .history("https://example.test/", "History", 20, 5)
                .unwrap(),
            usize::from(!retry)
        );
        assert_eq!(
            batch
                .history("https://example.test/", "History", 20, 5)
                .unwrap(),
            0
        );
        batch.commit().unwrap();
    }
    let mut batch = store.import_batch().unwrap();
    assert_eq!(
        batch
            .history("https://example.test/", "History", 10, 8)
            .unwrap(),
        1
    );
    assert_eq!(
        batch
            .history("https://example.test/", "History", 30, 2)
            .unwrap(),
        1
    );
    assert_eq!(
        batch
            .history("https://example.test/", "History", 29, 7)
            .unwrap(),
        0
    );
    batch.commit().unwrap();
    assert_eq!(
        store.history_metadata("https://example.test/").unwrap(),
        Some((30, 8))
    );
}

#[test]
fn clear_history_rejects_a_visit_at_the_clear_timestamp() {
    let temp = TempDir::new().unwrap();
    let store = ProfileStore::open(temp.path()).unwrap();
    store.clear_history().unwrap();
    let reader = rustyred::Database::open_read_only(temp.path().join("browser.RR")).unwrap();
    let cleared_at = reader
        .engine()
        .query_nodes(rustyred::engine::NodeQuery::label("HistoryCleared"))
        .unwrap()[0]
        .properties["at"]
        .as_i64()
        .unwrap();
    store
        .visit_at("https://old.test/", "Old", cleared_at)
        .unwrap();
    store
        .visit_at("https://new.test/", "New", cleared_at + 1)
        .unwrap();
    let history = store.suggestions("", false, 10).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].url, "https://new.test/");
}

#[test]
fn search_storage_paths_preserve_default_and_imported_profile_identity() {
    let temp = TempDir::new().unwrap();
    let store = ProfileStore::open(temp.path()).unwrap();
    let other = store.imported_profile("work", "Work", "").unwrap();
    assert_eq!(store.search_directory(), temp.path());
    assert_eq!(
        other.search_directory(),
        temp.path()
            .join("profiles")
            .join(other.profile.id.to_string())
            .join("search")
    );
}

#[test]
fn exact_address_beats_a_newer_fragment_visit_with_the_same_search_terms() {
    let temp = TempDir::new().unwrap();
    let store = ProfileStore::open(temp.path()).unwrap();
    store.visit_at("https://servo.org/", "Servo", 10).unwrap();
    store.visit_at("https://servo.org/#", "Servo", 20).unwrap();
    let results = store.suggestions("servo.org", false, 2).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].url, "https://servo.org/");
}

#[test]
fn browser_writes_do_not_leave_control_event_records() {
    let temp = TempDir::new().unwrap();
    let store = ProfileStore::open(temp.path()).unwrap();
    store
        .visit("https://private.test/", "Private visit")
        .unwrap();
    store.bookmark("https://saved.test/", "Saved page").unwrap();
    drop(store);
    let reader = rustyred::Database::open_read_only(temp.path().join("browser.RR")).unwrap();
    assert!(
        reader
            .engine()
            .query_nodes(rustyred::engine::NodeQuery::label("ControlEvent"))
            .unwrap()
            .is_empty()
    );
}
