use anyhow::Result;
use frame_profile::{
    ProfileStore, Session, SessionTab,
    credentials::{self, Vault},
    migration::{self, Categories, SourceProfile},
};
use rusqlite::Connection;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use tempfile::TempDir;
use zeroize::Zeroizing;

#[derive(Default)]
struct TestVault(RefCell<HashMap<String, Vec<u8>>>);
impl Vault for TestVault {
    fn remove(&self, key: &str) -> Result<()> {
        self.0.borrow_mut().remove(key);
        Ok(())
    }
    fn put(&self, key: &str, value: &[u8]) -> Result<()> {
        self.0.borrow_mut().insert(key.into(), value.into());
        Ok(())
    }
    fn get(&self, key: &str) -> Result<Zeroizing<Vec<u8>>> {
        self.0
            .borrow()
            .get(key)
            .cloned()
            .map(Zeroizing::new)
            .ok_or_else(|| anyhow::anyhow!("Not found"))
    }
}
struct DeniedVault;
impl Vault for DeniedVault {
    fn remove(&self, _: &str) -> Result<()> {
        anyhow::bail!("Access declined")
    }
    fn put(&self, _: &str, _: &[u8]) -> Result<()> {
        anyhow::bail!("Access declined")
    }
    fn get(&self, _: &str) -> Result<Zeroizing<Vec<u8>>> {
        anyhow::bail!("Access declined")
    }
}
fn source(root: &Path) -> SourceProfile {
    SourceProfile {
        format: frame_profile::migration::SourceFormat::Chromium,
        browser: "Chrome".into(),
        name: "Work".into(),
        email: "work@example.test".into(),
        directory: root.into(),
        keychain_service: "Never accessed by tests".into(),
        keychain_account: "test".into(),
    }
}
fn stores() -> (TempDir, ProfileStore) {
    let temp = TempDir::new().unwrap();
    let store = ProfileStore::open(temp.path()).unwrap();
    (temp, store)
}

#[test]
fn discovers_names_accounts_and_rejects_traversal() {
    let source = TempDir::new().unwrap();
    std::fs::create_dir(source.path().join("Default")).unwrap();
    std::fs::write(source.path().join("Local State"),r#"{"profile":{"info_cache":{"Default":{"name":"Work","user_name":"work@example.test"},"../":{"name":"escape"},"/":{"name":"absolute"}}}}"#).unwrap();
    let profiles = migration::discover_at(source.path(), "Chrome", "Chrome").unwrap();
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].name, "Work");
    assert_eq!(profiles[0].email, "work@example.test");
}

#[test]
fn imported_profiles_remain_separate_and_retries_reuse_destination() {
    let (_temp, store) = stores();
    let work = store
        .imported_profile("chrome:work", "Work", "work@example.test")
        .unwrap();
    let personal = store
        .imported_profile("chrome:personal", "Personal", "")
        .unwrap();
    let retry = store
        .imported_profile("chrome:work", "Work", "work@example.test")
        .unwrap();
    assert_eq!(work.profile.id, retry.profile.id);
    assert_ne!(work.directory(), personal.directory());
    work.visit("https://private.example.test/", "Private")
        .unwrap();
    assert!(
        personal
            .suggestions("private", false, 10)
            .unwrap()
            .is_empty()
    );
    assert_ne!(work.search_directory(), personal.search_directory());
    assert_eq!(store.directory(), store.root.join("profile"));
    work.select(work.profile.id).unwrap();
    let reopened = ProfileStore::open(&work.root).unwrap();
    assert_eq!(reopened.profile, work.profile);
    assert!(work.select(99999).is_err());
}

#[test]
fn history_snapshot_includes_committed_wal_and_import_is_idempotent() {
    let (_temp, store) = stores();
    let src = TempDir::new().unwrap();
    let live = Connection::open(src.path().join("History")).unwrap();
    live.execute_batch("PRAGMA journal_mode=WAL;PRAGMA wal_autocheckpoint=0;
      CREATE TABLE urls(url TEXT,title TEXT,last_visit_time INTEGER,visit_count INTEGER,hidden INTEGER);
      INSERT INTO urls VALUES('https://docs.example.test/start','My familiar docs',11644473600000042,8,0);
      INSERT INTO urls VALUES('javascript:alert(1)','unsafe',11644473600000000,1,0);
      INSERT INTO urls VALUES('https://hidden.example.test/','hidden',11644473600000000,1,1);") .unwrap();
    assert!(src.path().join("History-wal").exists());
    let selected = Categories {
        bookmarks: false,
        extensions: false,
        ..Default::default()
    };
    let first = migration::import(&store, &source(src.path()), selected, &TestVault::default());
    assert!(first.categories[0].error.is_none());
    assert_eq!(first.categories[0].counts.imported, 1);
    let second = migration::import(&store, &source(src.path()), selected, &TestVault::default());
    assert_eq!(second.categories[0].counts.imported, 0);
    let hits = store.suggestions("familiar", false, 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].url, "https://docs.example.test/start");
    let (visited, visits): (i64, i64) = store
        .connect()
        .unwrap()
        .query_row("SELECT visited,visits FROM history", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!((visited, visits), (42, 8));
    assert_eq!(
        live.query_row("SELECT count(*) FROM urls", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
}

#[test]
fn bookmarks_include_account_bookmarks_preserve_folders_and_deduplicate() {
    let (_temp, store) = stores();
    let src = TempDir::new().unwrap();
    let bookmarks = r#"{"roots":{"bookmark_bar":{"type":"folder","name":"Work","children":[{"type":"folder","name":"Research","children":[{"type":"url","name":"Notes","url":"https://notes.example.test/"}]}]}}}"#;
    std::fs::write(src.path().join("Bookmarks"), bookmarks).unwrap();
    std::fs::write(src.path().join("AccountBookmarks"), bookmarks).unwrap();
    let categories = Categories {
        history: false,
        extensions: false,
        ..Default::default()
    };
    let report = migration::import(
        &store,
        &source(src.path()),
        categories,
        &TestVault::default(),
    );
    assert!(report.categories[0].error.is_none());
    assert_eq!(report.categories[0].counts.imported, 1);
    assert_eq!(report.categories[0].counts.skipped, 1);
    let hits = store.suggestions("notes", true, 10).unwrap();
    assert_eq!(hits[0].folder, "Work/Research");
    assert!(store.suggestions("notes", false, 10).unwrap().is_empty());
}

#[test]
fn broken_category_does_not_block_other_categories_and_batch_report_is_durable() {
    let (_temp, store) = stores();
    let src = TempDir::new().unwrap();
    std::fs::write(src.path().join("Bookmarks"), "broken json").unwrap();
    std::fs::write(src.path().join("Preferences"),r#"{"extensions":{"settings":{"extension-id":{"manifest":{"name":"A useful tool","version":"2"}}}}}"#).unwrap();
    let categories = Categories {
        history: false,
        ..Default::default()
    };
    let report = migration::import(&store, &source(src.path()), categories, &DeniedVault);
    assert!(report.categories[0].error.is_some());
    assert_eq!(report.categories[1].counts.imported, 1);
    assert_eq!(
        store
            .connect()
            .unwrap()
            .query_row("SELECT count(*) FROM imports", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn password_csv_handles_quotes_unicode_newlines_and_keeps_secrets_out_of_sqlite() {
    let (_temp, store) = stores();
    let file = store.root.join("export.csv");
    let vault = TestVault::default();
    std::fs::write(&file,"name,url,username,password,note\r\nExample,https://accounts.example.test/login,alex@example.test,\"secret,with\"\"quotes\n雪\",ignored\r\nInvalid,javascript:alert(1),x,unsafe,x\r\n").unwrap();
    let counts = migration::password_csv(&store, &file, &vault).unwrap();
    assert_eq!((counts.imported, counts.skipped), (1, 1));
    let accounts = credentials::accounts(&store, "https://accounts.example.test/next").unwrap();
    assert_eq!(accounts, vec!["alex@example.test"]);
    assert_eq!(
        &*credentials::password(&vault, "https://accounts.example.test/next", &accounts[0])
            .unwrap(),
        "secret,with\"quotes\n雪"
    );
    assert!(
        credentials::accounts(&store, "https://evil.example.test/")
            .unwrap()
            .is_empty()
    );
    assert!(credentials::password(&vault, "http://accounts.example.test/", &accounts[0]).is_err());
    migration::password_csv(&store, &file, &vault).unwrap();
    assert_eq!(
        store
            .connect()
            .unwrap()
            .query_row("SELECT count(*) FROM credentials", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    // Flush and inspect the destination's pages. The export remains only where
    // the user chose it; no import copy is written to browser application data.
    store
        .connect()
        .unwrap()
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    let bytes = std::fs::read(store.directory().join("browser.sqlite3")).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("secret,with"));
    assert!(file.exists());
}

#[test]
fn keychain_denial_never_creates_a_phantom_credential() {
    let (_temp, store) = stores();
    assert!(
        credentials::save(
            &store,
            &DeniedVault,
            "https://example.test/",
            "user",
            "secret"
        )
        .is_err()
    );
    assert!(
        credentials::accounts(&store, "https://example.test/")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn chrome_decrypts_external_openssl_vector_and_rejects_unknown_or_corrupt_formats() {
    let key = credentials::derive_chrome_key(b"test-passphrase");
    assert_eq!(
        key.as_ref(),
        &[
            0x15, 0x20, 0xca, 0x2d, 0x2c, 0x5d, 0xce, 0xee, 0xeb, 0xcd, 0x3a, 0x50, 0x81, 0x8a,
            0x46, 0xc7
        ]
    );
    let bytes = [
        0x7e, 0x73, 0x57, 0x55, 0x84, 0x53, 0xf3, 0xe3, 0xc4, 0x25, 0x30, 0x0a, 0x20, 0xc9, 0x96,
        0xd1, 0x74, 0x42, 0xdf, 0xe2, 0x2a, 0x0c, 0xa7, 0x01, 0xd2, 0x94, 0xdc, 0xbb, 0x47, 0xe2,
        0xb7, 0xca,
    ];
    let encrypted = [b"v10".as_slice(), bytes.as_slice()].concat();
    assert_eq!(
        &*credentials::decrypt_chrome(&key, &encrypted).unwrap(),
        "synthetic-password"
    );
    assert!(credentials::decrypt_chrome(&key, b"v20unsupported").is_err());
    assert!(credentials::decrypt_chrome(&key, b"v10corrupt").is_err());
}

#[test]
fn credentials_are_bound_to_exact_scheme_host_port_and_update_in_place() {
    let (_temp, store) = stores();
    let vault = TestVault::default();
    credentials::save(
        &store,
        &vault,
        "https://example.test:8443/login",
        "user",
        "first",
    )
    .unwrap();
    credentials::save(
        &store,
        &vault,
        "https://example.test:8443/next",
        "user",
        "updated",
    )
    .unwrap();
    assert!(credentials::password(&vault, "https://example.test/", "user").is_err());
    assert!(credentials::password(&vault, "https://sub.example.test:8443/", "user").is_err());
    assert_eq!(
        &*credentials::password(&vault, "https://example.test:8443/", "user").unwrap(),
        "updated"
    );
    assert_eq!(
        credentials::accounts(&store, "https://example.test:8443/")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn vault_verification_includes_http_without_allowing_http_fill() {
    let (_temp, store) = stores();
    let vault = TestVault::default();
    credentials::save(
        &store,
        &vault,
        "http://example.test/",
        "user",
        "fixture-secret",
    )
    .unwrap();
    assert_eq!(credentials::verify_saved(&store, &vault).unwrap(), (1, 0));
    assert!(credentials::password(&vault, "http://example.test/", "user").is_err());
    vault.0.borrow_mut().clear();
    assert_eq!(credentials::verify_saved(&store, &vault).unwrap(), (0, 1));
}

#[test]
fn html_bookmarks_use_a_real_html_parser_and_session_survives_reopen() {
    let (_temp, store) = stores();
    let file = store.root.join("bookmarks.html");
    std::fs::write(&file,r#"<!DOCTYPE NETSCAPE-Bookmark-file-1><TITLE>Bookmarks</TITLE><DL><DT><H3>Research</H3><DL><DT><A HREF="https://example.test/?a=1&amp;b=2">Title &amp; notes</A><DT><A HREF="javascript:alert(1)">Bad</A></DL></DL>"#).unwrap();
    let counts = migration::bookmarks_html(&store, &file).unwrap();
    assert_eq!(counts.imported, 1);
    assert_eq!(counts.skipped, 1);
    let hits = store.suggestions("Title & notes", true, 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].folder.contains("Research"));
    let session = Session {
        layout: None,
        tabs: vec![
            SessionTab {
                url: Some("https://example.test/".into()),
                query: None,
            },
            SessionTab {
                url: None,
                query: Some("familiar research".into()),
            },
        ],
        active: 1,
    };
    store.save_session(&session).unwrap();
    assert_eq!(
        ProfileStore::open(&store.root).unwrap().session().unwrap(),
        session
    );
}

#[test]
fn fill_serialization_cannot_turn_password_into_script_and_rechecks_origin() {
    let password = "\"; alert('escape'); //\n</script>";
    let script = credentials::fill_script("https://example.test/login", "user", password).unwrap();
    assert!(script.contains("location.origin !== \"https://example.test\""));
    assert!(script.contains("window !== top"));
    assert!(script.contains(&serde_json::to_string(password).unwrap()));
    assert!(script.contains("form.action"));
    assert!(!script.contains(".submit("));
    assert!(credentials::fill_script("http://insecure.test/", "user", "secret").is_err());
    assert!(
        credentials::fill_script("https://user:secret@example.test/", "user", "secret").is_err()
    );
}

// Synthetic wire fixtures exercise the third-party SNSS decoder without using
// a person's session files. Production contains no custom Pickle parser.
fn nav(tab: i32, url: &str) -> Vec<u8> {
    let mut body = tab.to_le_bytes().to_vec();
    body.extend(0i32.to_le_bytes());
    body.extend((url.len() as i32).to_le_bytes());
    body.extend(url.as_bytes());
    while !body.len().is_multiple_of(4) {
        body.push(0);
    }
    body.extend(0i32.to_le_bytes());
    let mut out = (body.len() as i32).to_le_bytes().to_vec();
    out.extend(body);
    out
}
fn session_bytes(records: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = b"SNSS".to_vec();
    bytes.extend(3i32.to_le_bytes());
    for (id, payload) in records {
        bytes.extend(((payload.len() + 1) as u16).to_le_bytes());
        bytes.push(*id);
        bytes.extend(payload);
    }
    bytes
}
#[test]
fn session_import_uses_newest_file_ignores_closed_tabs_and_is_retry_safe() {
    let (_temp, store) = stores();
    let src = TempDir::new().unwrap();
    let dir = src.path().join("Sessions");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(
        dir.join("Session_100"),
        session_bytes(&[(6, nav(1, "https://old.test/"))]),
    )
    .unwrap();
    std::fs::write(
        dir.join("Tabs_999"),
        session_bytes(&[(1, nav(1, "https://closed.test/"))]),
    )
    .unwrap();
    std::fs::write(
        dir.join("Session_200"),
        session_bytes(&[
            (6, nav(2, "https://open.test/")),
            (6, nav(3, "https://closed.test/")),
            (16, 3i32.to_le_bytes().to_vec()),
        ]),
    )
    .unwrap();
    let categories = Categories {
        bookmarks: false,
        history: false,
        extensions: false,
        tabs: true,
        ..Default::default()
    };
    let report = migration::import(
        &store,
        &source(src.path()),
        categories,
        &TestVault::default(),
    );
    assert!(report.categories[0].error.is_none(), "{}", report.summary());
    assert_eq!(report.categories[0].counts.imported, 1);
    let session = store.session().unwrap();
    assert_eq!(session.tabs.len(), 1);
    assert_eq!(session.tabs[0].url.as_deref(), Some("https://open.test/"));
    migration::import(
        &store,
        &source(src.path()),
        categories,
        &TestVault::default(),
    );
    assert_eq!(store.session().unwrap().tabs.len(), 1);
    // A truncated live tail must fail without replacing the saved session.
    std::fs::write(dir.join("Session_300"), b"SNSS\x03\0\0\0\x20").unwrap();
    let report = migration::import(
        &store,
        &source(src.path()),
        categories,
        &TestVault::default(),
    );
    assert!(report.categories[0].error.is_some());
    assert_eq!(store.session().unwrap(), session);
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "explicit native migration smoke test; uses synthetic source/destination Keychain items"]
fn direct_chrome_password_import_uses_native_keychain_and_a_real_login_database() {
    struct Cleanup {
        store: ProfileStore,
        service: String,
    }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ =
                security_framework::passwords::delete_generic_password(&self.service, "fixture");
            let _ = credentials::delete(
                &self.store,
                &credentials::Keychain::for_profile(&self.store),
                "https://example.test/",
                "fixture-user",
            );
        }
    }
    let (_temp, store) = stores();
    let src = TempDir::new().unwrap();
    let service = format!("Frame.synthetic-chrome.{}", frame_profile::now());
    let _cleanup = Cleanup {
        store: store.clone(),
        service: service.clone(),
    };
    security_framework::passwords::set_generic_password(&service, "fixture", b"test-passphrase")
        .unwrap();
    let db = Connection::open(src.path().join("Login Data")).unwrap();
    db.execute_batch("CREATE TABLE logins(origin_url TEXT,username_value TEXT,password_value BLOB,blacklisted_by_user INTEGER)").unwrap();
    let encrypted = [
        b"v10".as_slice(),
        &[
            0x7e, 0x73, 0x57, 0x55, 0x84, 0x53, 0xf3, 0xe3, 0xc4, 0x25, 0x30, 0x0a, 0x20, 0xc9,
            0x96, 0xd1, 0x74, 0x42, 0xdf, 0xe2, 0x2a, 0x0c, 0xa7, 0x01, 0xd2, 0x94, 0xdc, 0xbb,
            0x47, 0xe2, 0xb7, 0xca,
        ],
    ]
    .concat();
    db.execute(
        "INSERT INTO logins VALUES('https://example.test/login','fixture-user',?1,0)",
        [encrypted],
    )
    .unwrap();
    let mut source = source(src.path());
    source.keychain_service = service;
    source.keychain_account = "fixture".into();
    let selected = Categories {
        bookmarks: false,
        history: false,
        passwords: true,
        extensions: false,
        ..Default::default()
    };
    let vault = credentials::Keychain::for_profile(&store);
    let report = migration::import(&store, &source, selected, &vault);
    assert!(report.categories[0].error.is_none(), "{}", report.summary());
    assert_eq!(report.categories[0].counts.imported, 1);
    assert_eq!(
        &*credentials::password(&vault, "https://example.test/", "fixture-user").unwrap(),
        "synthetic-password"
    );
    let retry = migration::import(&store, &source, selected, &vault);
    assert_eq!(retry.categories[0].counts.imported, 0);
    assert_eq!(retry.categories[0].counts.skipped, 1);
}

#[test]
fn selective_form_import_excludes_payment_and_password_fields() {
    let (_temp, store) = stores();
    let src = TempDir::new().unwrap();
    let db = Connection::open(src.path().join("Web Data")).unwrap();
    db.execute_batch("CREATE TABLE autofill(name TEXT,value TEXT);
      INSERT INTO autofill VALUES('email','work@example.test'),('city','Detroit'),('password','never-import'),('credit_card','never-import');").unwrap();
    let selected = Categories {
        bookmarks: false,
        history: false,
        extensions: false,
        forms: true,
        ..Default::default()
    };
    let report = migration::import(&store, &source(src.path()), selected, &TestVault::default());
    assert_eq!(report.categories[0].counts.imported, 2);
    assert_eq!(report.categories[0].counts.skipped, 2);
    assert_eq!(store.form_values().unwrap().len(), 2);
    assert!(credentials::form_script("https://example.test/", "password", "secret").is_err());
    assert!(
        credentials::form_script("http://example.test/", "email", "work@example.test").is_err()
    );
}
