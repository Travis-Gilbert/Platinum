//! What a note on disk is when the process saving it is killed.
//!
//! `examples/notes_save_kill.rs` is the victim: it edits and saves one note
//! of a copy of the fixtures vault over and over, printing each step. This
//! test starts it, waits for a chosen step, waits a little longer, kills it
//! with SIGKILL (no chance to clean up) and asks a new process to check what
//! is left: the note is exactly the original or exactly a whole round's text,
//! the vault opens again, and the note opens at one of those texts.
//!
//! The kills land at varied points: just after an edit is accepted and the
//! save has not begun, just after a save begins, and at plain delays from
//! the start, each followed by a delay of a fraction of a millisecond to
//! tens of milliseconds. Each kill has a vault of its own, so they run side
//! by side on a few threads. The example must be built first:
//! `cargo build --bin frame --example notes_save_kill`.

use std::fs;
use std::io::{BufRead as _, BufReader};
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Kills made. The request is at least fifty.
const KILLS: u64 = 50;
/// Kills in progress at once; each is mostly a vault opening, which is work.
const SIDE_BY_SIDE: usize = 6;
/// The note the example rewrites: a fixture, so the vault is a real one.
const NOTE: &str = "Home.md";
const SIGKILL: i32 = 9;

fn example() -> PathBuf {
    let tests = std::env::current_exe().unwrap();
    let example = tests
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .join("examples")
        .join("notes_save_kill");
    assert!(
        example.exists(),
        "build the victim first: cargo build --bin frame --example notes_save_kill ({})",
        example.display()
    );
    example
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

/// The line to wait for before the nth kill, and how much longer to wait.
fn moment(kill: u64) -> (String, Duration) {
    let round = 1 + kill / 3 % 4;
    let (line, extra) = match kill % 3 {
        0 => (format!("save {round}"), 0),
        1 => (format!("edit {round}"), 0),
        // From the start, up to 60ms: inside whatever the victim is doing.
        _ => ("ready".to_owned(), kill * 613 % 60_000),
    };
    (line, Duration::from_micros(kill * 7919 % 4000 + extra))
}

/// The victim, which never outlives the test that started it, whatever
/// happens to the test.
struct Victim(Child);

impl Drop for Victim {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[derive(Default)]
struct Found {
    disk_old: u64,
    disk_new: u64,
    opened_old: u64,
    opened_new: u64,
    leftovers: u64,
}

/// Kill number `kill`: start the victim in a new copy of the fixtures, kill it
/// at that kill's moment, and check what it left behind.
fn kill_once(example: &Path, kill: u64, found: &Mutex<Found>) {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/notes-fixtures/vault"),
        root.path(),
    );
    let original = scratch.path().join("original");
    fs::copy(root.path().join(NOTE), &original).unwrap();

    let (line, wait) = moment(kill);
    let mut victim = Victim(
        Command::new(example)
            .arg("run")
            .args([root.path(), data.path()])
            .arg(NOTE)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let mut steps = BufReader::new(victim.0.stdout.take().unwrap()).lines();
    loop {
        match steps.next() {
            Some(Ok(step)) if step == line => break,
            Some(Ok(_)) => {}
            _ => panic!("kill {kill}: the victim ended before {line:?}"),
        }
    }
    std::thread::sleep(wait);
    victim.0.kill().unwrap();
    let status = victim.0.wait().unwrap();
    assert_eq!(status.signal(), Some(SIGKILL), "kill {kill}");

    let checked = Command::new(example)
        .arg("check")
        .args([root.path(), data.path()])
        .arg(NOTE)
        .arg(&original)
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&checked.stdout);
    assert!(
        checked.status.success(),
        "kill {kill} (after {line:?} and {wait:?}): {}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let field = |name: &str| {
        said.split_whitespace()
            .find_map(|part| part.strip_prefix(name)?.strip_prefix('='))
            .unwrap_or_else(|| panic!("kill {kill}: no {name} in {said:?}"))
    };
    let mut found = found.lock().unwrap();
    match field("disk") {
        "old" => found.disk_old += 1,
        _ => found.disk_new += 1,
    }
    match field("opened") {
        "old" => found.opened_old += 1,
        _ => found.opened_new += 1,
    }
    found.leftovers += field("leftovers").parse::<u64>().unwrap();
}

#[test]
fn a_note_survives_sigkill_at_any_point_of_a_save() {
    let example = example();
    let found = Mutex::new(Found::default());
    let next = AtomicU64::new(0);
    std::thread::scope(|scope| {
        for _ in 0..SIDE_BY_SIDE {
            scope.spawn(|| {
                loop {
                    let kill = next.fetch_add(1, Ordering::Relaxed);
                    if kill >= KILLS {
                        break;
                    }
                    kill_once(&example, kill, &found);
                }
            });
        }
    });
    let found = found.into_inner().unwrap();
    println!(
        "{KILLS} kills: on disk {} old and {} new; opened at {} old and {} new; {} leftover temporary files",
        found.disk_old, found.disk_new, found.opened_old, found.opened_new, found.leftovers
    );
    assert_eq!(found.disk_old + found.disk_new, KILLS);
    assert!(
        found.disk_old > 0 && found.disk_new > 0,
        "the kills must land both before and after a save finishes"
    );
}
