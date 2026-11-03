//! A process that saves a note over and over, to be killed in the middle of a
//! save, and the check of what it leaves. The editor tab's kill test
//! (`src/notes/editor/kill.rs`) runs both, many times.
//!
//! ```text
//! notes_save_kill run   <root> <data> <note>
//! notes_save_kill check <root> <data> <note> <original-file>
//! ```
//!
//! `run` opens the vault, prints `ready`, then for round 1, 2, 3 ... prints
//! `edit N`, makes the note round N's text in the vault, prints `save N`,
//! saves it to disk and prints `saved N`. It never ends on its own.
//!
//! `check` runs after the kill, in a new process. It fails unless the note on
//! disk is exactly the original or exactly some round's full text (a half
//! write is neither), the vault opens again, and the note opens at one of
//! those texts. It prints what it found:
//! `disk=old|new:N opened=old|new:N leftovers=K`.

use std::{fs, path::Path, process::ExitCode};

use anyhow::{Result, bail, ensure};
use frame_notes::Vault;

/// How many lines a round's text has at least; big enough that writing it
/// takes long enough to land a kill in.
const LINES: usize = 6_000;

/// Round `round`'s text. Rounds differ in length as well as in content, so a
/// file cut short by a kill can never match another round's.
fn content(round: u64) -> String {
    let lines = LINES + 40 * (round % 7) as usize;
    format!(
        "# Round {round}\n{}",
        "durable text for the kill test\n".repeat(lines)
    )
}

/// The round a text claims to be, from its heading.
fn round_of(text: &str) -> Option<u64> {
    text.strip_prefix("# Round ")?
        .split('\n')
        .next()?
        .parse()
        .ok()
}

/// `old`, or `new:N` if `text` is exactly round N's.
fn which(text: &str, original: &str) -> Option<String> {
    if text == original {
        return Some("old".into());
    }
    let round = round_of(text)?;
    (text == content(round)).then(|| format!("new:{round}"))
}

fn run(root: &Path, data: &Path, note: &str) -> Result<()> {
    let mut vault = Vault::open(root, data)?;
    let mut revision = vault.open_document(note)?.revision;
    println!("ready");
    for round in 1.. {
        println!("edit {round}");
        revision = vault
            .edit(note, revision, &format!("kill-{round}"), &content(round))?
            .revision;
        println!("save {round}");
        vault.save(note)?;
        println!("saved {round}");
    }
    unreachable!("rounds do not end")
}

fn check(root: &Path, data: &Path, note: &str, original: &Path) -> Result<()> {
    let original = fs::read_to_string(original)?;
    let disk = fs::read_to_string(root.join(note))?;
    let Some(on_disk) = which(&disk, &original) else {
        bail!(
            "the note on disk is neither the original nor a whole round ({} bytes)",
            disk.len()
        );
    };
    let mut vault = Vault::open(root, data)?;
    let opened = vault.open_document(note)?;
    let Some(opened_at) = which(&opened.text, &original) else {
        bail!(
            "the note opened at a text that is neither the original nor a whole round ({} bytes)",
            opened.text.len()
        );
    };
    let leftovers = fs::read_dir(root)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp"))
        .count();
    ensure!(
        vault.save(note).is_ok(),
        "the note opened but could not be saved again"
    );
    println!("disk={on_disk} opened={opened_at} leftovers={leftovers}");
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["run", root, data, note] => run(root.as_ref(), data.as_ref(), note),
        ["check", root, data, note, original] => {
            check(root.as_ref(), data.as_ref(), note, original.as_ref())
        }
        _ => Err(anyhow::anyhow!(
            "usage: notes_save_kill run <root> <data> <note> | check <root> <data> <note> <original>"
        )),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}
