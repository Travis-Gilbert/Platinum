use frame_profile::ProfileStore;

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
        .unwrap();
    let b = store
        .record_visit("window-a:2", "https://example.com/", "Other tab", None)
        .unwrap();
    let c = store
        .record_visit("window-a:1", "https://example.com/", "Again", None)
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
