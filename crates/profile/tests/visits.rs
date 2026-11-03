use frame_profile::ProfileStore;
use rusqlite::Connection;

#[test]
fn repeated_addresses_are_distinct_and_predecessors_do_not_cross_tabs_or_profiles() {
    let root = tempfile::tempdir().unwrap();
    let store = ProfileStore::open(root.path()).unwrap();
    let a = store
        .record_visit_at(
            "window-a:1",
            "https://example.com/",
            "First",
            Some(b"owned preview"),
            123456789,
        )
        .unwrap()
        .unwrap();
    let b = store
        .record_visit("window-a:2", "https://example.com/", "Other tab", None)
        .unwrap()
        .unwrap();
    let c = store
        .record_visit("window-a:1", "https://example.com/", "Again", None)
        .unwrap()
        .unwrap();
    let visits = store.visits(None, 2).unwrap();
    assert_eq!(visits.iter().map(|v| v.id).collect::<Vec<_>>(), vec![c, b]);
    assert_eq!(visits[0].predecessor, Some(a));
    assert_eq!(visits[1].predecessor, None);
    let older = store.visits(Some(b), 2).unwrap();
    assert_eq!(older.len(), 1);
    assert_eq!(older[0].visited, 123456789);
    assert_eq!(older[0].preview.as_deref(), Some(&b"owned preview"[..]));
    assert_eq!(store.suggestions("example", false, 100).unwrap().len(), 1);
    let other = store.imported_profile("visit-test", "Other", "").unwrap();
    assert!(other.visits(None, 100).unwrap().is_empty());
}

#[test]
fn rejects_non_web_visits_and_oversized_previews_without_writing() {
    let root = tempfile::tempdir().unwrap();
    let store = ProfileStore::open(root.path()).unwrap();
    assert!(store.record_visit("a", "file:///secret", "", None).is_err());
    assert!(
        store
            .record_visit(
                "a",
                "https://example.com",
                "",
                Some(&vec![0; 512 * 1024 + 1])
            )
            .is_err()
    );
    assert!(store.visits(None, 100).unwrap().is_empty());
}

#[test]
fn clear_history_forgets_visits_and_previews_and_refuses_queued_ones() {
    let root = tempfile::tempdir().unwrap();
    let store = ProfileStore::open(root.path()).unwrap();
    let before = store
        .record_visit("a", "https://example.com/", "Private", Some(b"pixels"))
        .unwrap()
        .unwrap();
    let previews = store.directory().join("visit-previews");
    assert_eq!(std::fs::read_dir(&previews).unwrap().count(), 1);
    let captured = frame_profile::now();
    store.clear_history().unwrap();
    assert!(store.visits(None, 100).unwrap().is_empty());
    assert!(!previews.exists(), "page previews must not outlive history");
    assert!(store.suggestions("example", false, 10).unwrap().is_empty());
    // A write queued before the clear neither returns nor brings its preview.
    let queued = store
        .record_visit_at("a", "https://example.com/", "", Some(b"pixels"), captured)
        .unwrap();
    assert_eq!(queued, None);
    assert!(store.visits(None, 100).unwrap().is_empty());
    assert!(!previews.exists());
    // Ids are not reused, and a cleared tab starts a new trail.
    let after = store
        .record_visit("a", "https://example.org/", "Later", None)
        .unwrap()
        .unwrap();
    assert!(after > before);
    let visits = store.visits(None, 100).unwrap();
    assert_eq!(visits.len(), 1);
    assert_eq!(visits[0].predecessor, None);
}

#[test]
fn visits_page_newest_first_and_stay_with_their_profile_after_reopen() {
    let root = tempfile::tempdir().unwrap();
    let store = ProfileStore::open(root.path()).unwrap();
    let other = store.imported_profile("source", "Other", "").unwrap();
    other
        .record_visit("a", "https://other.test/", "", None)
        .unwrap();
    let ids: Vec<_> = (0..5)
        .map(|n| {
            store
                .record_visit("a", &format!("https://example.com/{n}"), "", None)
                .unwrap()
                .unwrap()
        })
        .collect();
    drop((store, other));
    let store = ProfileStore::open(root.path()).unwrap();
    let page = |before| {
        store
            .visits(before, 2)
            .unwrap()
            .iter()
            .map(|v| v.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(page(None), vec![ids[4], ids[3]]);
    assert_eq!(page(Some(ids[3])), vec![ids[2], ids[1]]);
    assert_eq!(page(Some(ids[1])), vec![ids[0]]);
    assert!(page(Some(ids[0])).is_empty());
}

#[test]
fn sqlite_visits_migrate_with_previews_and_clearing_removes_the_old_copy() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("profile");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("browser.sqlite3");
    {
        let old = Connection::open(&path).unwrap();
        old.execute_batch("CREATE TABLE visits(id INTEGER PRIMARY KEY,tab TEXT NOT NULL,predecessor INTEGER,url TEXT NOT NULL,title TEXT NOT NULL,visited INTEGER NOT NULL,preview BLOB);").unwrap();
        for (id, predecessor, preview) in
            [(1_i64, None, Some(&b"first"[..])), (2, Some(1_i64), None)]
        {
            old.execute(
                "INSERT INTO visits VALUES(?1,'a',?2,'https://example.com/','Old',?1,?3)",
                rusqlite::params![id, predecessor, preview],
            )
            .unwrap();
        }
    }
    let store = ProfileStore::open(root.path()).unwrap();
    let visits = store.visits(None, 100).unwrap();
    assert_eq!(visits.iter().map(|v| v.id).collect::<Vec<_>>(), vec![2, 1]);
    assert_eq!(visits[1].preview.as_deref(), Some(&b"first"[..]));
    // The trail continues where the old database stopped.
    let next = store
        .record_visit("a", "https://example.org/", "New", None)
        .unwrap()
        .unwrap();
    assert_eq!(next, 3);
    assert_eq!(store.visits(None, 1).unwrap()[0].predecessor, Some(2));
    store.clear_history().unwrap();
    let old = Connection::open(&path).unwrap();
    let rows: i64 = old
        .query_row("SELECT count(*) FROM visits", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0);
}
