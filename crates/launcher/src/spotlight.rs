//! Files and folders on this Mac, found through the index macOS already
//! keeps (Spotlight), by asking `mdfind`.
//!
//! Two questions per query: which files are *named* like it, and which
//! *hold* it. Names come first. Each is one `mdfind` run, read line by line
//! and ended as soon as enough paths are in, or when the search is
//! cancelled because the query changed. Nothing is read from a file's
//! contents except a short passage of a plain-text file, to show why it
//! matched.
//!
//! What this is not: Theorem's own file index, which knows passages inside
//! PDFs and spreadsheets. That lane is separate; this one answers at once
//! with what the Mac already knows.

use std::io::{BufRead as _, BufReader, Read as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

use crate::item::{Item, ItemId, Kind, Target};
use crate::{Cancel, Source, apps, matching, words};

/// The most paths taken from one `mdfind` run.
const TAKE: usize = 40;
/// The most rows handed back for one query.
const RETURN: usize = 24;
/// The most of a plain-text file read to find a passage.
const READ: u64 = 256 * 1024;
/// The longest passage shown, in characters.
const PASSAGE: usize = 320;
/// A name match outranks any contents match.
const NAMED: u32 = 1_000;

/// Files and folders through Spotlight.
#[derive(Clone, Debug)]
pub struct Spotlight {
    program: PathBuf,
    /// The folders searched. Empty searches everywhere Spotlight indexes.
    within: Vec<PathBuf>,
}

impl Spotlight {
    /// Searches under `within` with the system's `mdfind`.
    pub fn new(within: Vec<PathBuf>) -> Self {
        Self {
            program: PathBuf::from("/usr/bin/mdfind"),
            within,
        }
    }

    /// Uses another program in `mdfind`'s place. For tests.
    pub fn with_program(mut self, program: impl Into<PathBuf>) -> Self {
        self.program = program.into();
        self
    }

    fn run(&self, query: &str, by_name: bool, cancel: &Cancel) -> Result<Vec<PathBuf>, String> {
        let mut command = Command::new(&self.program);
        for folder in &self.within {
            command.arg("-onlyin").arg(folder);
        }
        if by_name {
            command.arg("-name");
        }
        // After `--`, so a query that starts with a dash is still a query.
        command.arg("--").arg(query);
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Spotlight could not be asked: {error}."))?;
        let Some(output) = child.stdout.take() else {
            let _ = child.kill();
            return Err("Spotlight could not be asked.".to_owned());
        };
        // Kept where `cancel` can end it, which is what unblocks the read.
        cancel.keep(child);
        let mut paths = Vec::new();
        for line in BufReader::new(output).lines() {
            if cancel.is_cancelled() {
                break;
            }
            let Ok(line) = line else {
                break;
            };
            let path = PathBuf::from(line);
            if wanted(&path) {
                paths.push(path);
                if paths.len() >= TAKE {
                    break;
                }
            }
        }
        if let Some(mut child) = cancel.take() {
            // Enough was read, or the query changed: the rest is not wanted.
            let _ = child.kill();
            let _ = child.wait();
        }
        Ok(paths)
    }
}

impl Source for Spotlight {
    fn find(&self, query: &str, cancel: &Cancel) -> Result<Vec<Item>, String> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let now = SystemTime::now();
        let named = self.run(query, true, cancel)?;
        let mut items: Vec<Item> = named
            .iter()
            .filter_map(|path| item(path, query, true, now))
            .collect();
        if !cancel.is_cancelled() {
            // A failure here still leaves the names that were found.
            for path in self.run(query, false, cancel).unwrap_or_default() {
                if !named.contains(&path)
                    && let Some(item) = item(&path, query, false, now)
                {
                    items.push(item);
                }
            }
        }
        if cancel.is_cancelled() {
            return Ok(Vec::new());
        }
        items.sort_by_key(|item| std::cmp::Reverse(item.score));
        items.truncate(RETURN);
        Ok(items)
    }
}

/// Whether a path is one a person would look for: nothing hidden, nothing
/// inside an application or a library folder.
fn wanted(path: &Path) -> bool {
    path.is_absolute()
        && !path.components().any(|part| {
            let part = part.as_os_str().to_string_lossy();
            part.starts_with('.') || part == "Library" || part == "node_modules"
        })
        && !path.ancestors().skip(1).any(|above| {
            above
                .extension()
                .is_some_and(|extension| extension == "app")
        })
}

fn item(path: &Path, query: &str, named: bool, now: SystemTime) -> Option<Item> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let metadata = std::fs::metadata(path).ok()?;
    let folder = metadata.is_dir();
    // Applications have their own lane.
    if folder && path.extension().is_some_and(|extension| extension == "app") {
        return None;
    }
    let shown = path.to_string_lossy().into_owned();
    let mut item = Item::new(
        ItemId::new("file", &shown),
        if folder { Kind::Folder } else { Kind::File },
        name.clone(),
        Target::Path(path.to_owned()),
    )
    .detail(
        path.parent()
            .map(|above| above.to_string_lossy().into_owned())
            .unwrap_or_default(),
    )
    // The path answers too, so a row found by its folder's name still
    // matches while the lane looks again for a longer query.
    .keywords([shown.clone()])
    .fact("Path", shown);
    if let Some(modified) = apps::modified_ago(path, now) {
        item = item.fact("Modified", modified);
    }
    if !folder {
        item = item.fact("Size", words::size(metadata.len()));
        if let Some(passage) = passage(path, query, metadata.len()) {
            item = item.excerpt(passage);
        }
    }
    let name_score = matching::score(query, &name).unwrap_or(0);
    item.score = if named {
        NAMED + name_score
    } else {
        // Found by what it holds: it ranks by how much of that shows.
        name_score.min(NAMED - 1) / 2 + u32::from(item.excerpt.is_some()) * 100
    };
    Some(item)
}

/// A short passage of a plain-text file: the first lines that hold a word
/// of the query, or the file's opening lines when none does. `None` for a
/// file that is not plain text, is empty, or cannot be read.
fn passage(path: &Path, query: &str, length: u64) -> Option<String> {
    if length == 0 || !reads_as_text(path) {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(READ)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.contains(&0) {
        return None;
    }
    let text = String::from_utf8_lossy(&bytes);
    let terms: Vec<String> = query
        .split_whitespace()
        .filter(|term| term.chars().count() >= 3)
        .map(str::to_lowercase)
        .collect();
    let lines: Vec<&str> = text.lines().collect();
    let at = lines
        .iter()
        .position(|line| {
            let line = line.to_lowercase();
            terms.iter().any(|term| line.contains(term))
        })
        .unwrap_or(0);
    let mut passage = String::new();
    for line in lines[at..]
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
    {
        if !passage.is_empty() {
            passage.push('\n');
        }
        passage.push_str(line);
        if passage.chars().count() >= PASSAGE {
            break;
        }
    }
    if passage.chars().count() > PASSAGE {
        passage = passage.chars().take(PASSAGE).collect::<String>() + "…";
    }
    (!passage.is_empty()).then_some(passage)
}

fn reads_as_text(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return false;
    };
    matches!(
        extension.to_lowercase().as_str(),
        "txt"
            | "md"
            | "markdown"
            | "rst"
            | "org"
            | "csv"
            | "tsv"
            | "json"
            | "toml"
            | "yaml"
            | "yml"
            | "xml"
            | "html"
            | "css"
            | "js"
            | "ts"
            | "tsx"
            | "jsx"
            | "rs"
            | "py"
            | "go"
            | "swift"
            | "c"
            | "h"
            | "cpp"
            | "java"
            | "kt"
            | "rb"
            | "sh"
            | "sql"
            | "tex"
            | "log"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Folder;

    /// A stand-in for `mdfind` that answers from two lists: one for `-name`
    /// and one for contents.
    fn stand_in(root: &Folder, named: &[PathBuf], held: &[PathBuf]) -> Spotlight {
        let list = |paths: &[PathBuf]| {
            paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("\n")
        };
        root.file("named.txt", &list(named));
        root.file("held.txt", &list(held));
        let script = format!(
            "#!/bin/sh\nfor arg in \"$@\"; do\n  if [ \"$arg\" = \"-name\" ]; then cat '{0}/named.txt'; echo; exit 0; fi\ndone\ncat '{0}/held.txt'; echo\n",
            root.path().display()
        );
        let program = root.executable("mdfind", &script);
        Spotlight::new(Vec::new()).with_program(program)
    }

    #[test]
    fn names_come_before_contents_and_each_file_once() {
        let _alone = crate::testing::alone();
        let root = Folder::new("spotlight-order");
        let named = root.file("docs/rollback-plan.md", "The plan.\n");
        let held = root.file(
            "docs/meeting.md",
            "Agenda\n\nWe agreed the database rollback waits for the snapshot.\nNext item.\n",
        );
        let spotlight = stand_in(
            &root,
            std::slice::from_ref(&named),
            &[named.clone(), held.clone()],
        );

        let items = spotlight.find("rollback", &Cancel::new()).unwrap();
        let titles: Vec<&str> = items.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(titles, ["rollback-plan.md", "meeting.md"]);
        assert_eq!(items[0].kind, Kind::File);
        assert_eq!(items[0].target, Target::Path(named));
        // The file found by what it holds shows the passage that matched.
        assert_eq!(
            items[1].excerpt.as_deref(),
            Some("We agreed the database rollback waits for the snapshot.\nNext item.")
        );
    }

    #[test]
    fn hidden_library_and_application_insides_are_left_out() {
        let _alone = crate::testing::alone();
        let root = Folder::new("spotlight-wanted");
        let kept = root.file("notes/report.md", "x");
        let hidden = root.file(".cache/report.md", "x");
        let library = root.file("Library/Caches/report.md", "x");
        let inside = root.file("Tool.app/Contents/report.md", "x");
        let app = root.dir("Report Maker.app");
        let spotlight = stand_in(&root, &[hidden, library, inside, app, kept.clone()], &[]);

        let items = spotlight.find("report", &Cancel::new()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].target, Target::Path(kept));
    }

    #[test]
    fn a_folder_is_a_folder_and_has_no_passage() {
        let _alone = crate::testing::alone();
        let root = Folder::new("spotlight-folder");
        let folder = root.dir("Reports 2026");
        let spotlight = stand_in(&root, &[folder], &[]);
        let items = spotlight.find("reports", &Cancel::new()).unwrap();
        assert_eq!(items[0].kind, Kind::Folder);
        assert_eq!(items[0].excerpt, None);
        assert!(!items[0].verbs().contains(&crate::Verb::AskAbout));
    }

    #[test]
    fn a_path_that_is_gone_is_left_out() {
        let _alone = crate::testing::alone();
        let root = Folder::new("spotlight-gone");
        let gone = root.path().join("was-here.md");
        let spotlight = stand_in(&root, &[gone], &[]);
        assert!(spotlight.find("was", &Cancel::new()).unwrap().is_empty());
    }

    #[test]
    fn a_cancelled_search_delivers_nothing() {
        let _alone = crate::testing::alone();
        let root = Folder::new("spotlight-cancel");
        let file = root.file("report.md", "x");
        let spotlight = stand_in(&root, &[file], &[]);
        let cancel = Cancel::new();
        cancel.cancel();
        assert!(spotlight.find("report", &cancel).unwrap().is_empty());
    }

    #[test]
    fn cancelling_ends_a_search_that_is_waiting_on_spotlight() {
        let _alone = crate::testing::alone();
        let root = Folder::new("spotlight-hang");
        // Never answers: only cancelling can end the read.
        let program = root.executable("mdfind", "#!/bin/sh\nexec sleep 30\n");
        let spotlight = Spotlight::new(Vec::new()).with_program(program);
        let cancel = Cancel::new();
        let canceller = cancel.clone();
        let started = std::time::Instant::now();
        let search = std::thread::spawn(move || spotlight.find("report", &cancel));
        std::thread::sleep(std::time::Duration::from_millis(200));
        canceller.cancel();
        let found = search.join().unwrap().unwrap();
        assert!(found.is_empty());
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }

    #[test]
    fn spotlight_missing_is_said_not_hidden() {
        let _alone = crate::testing::alone();
        let spotlight = Spotlight::new(Vec::new()).with_program("/nowhere/mdfind");
        let error = spotlight.find("report", &Cancel::new()).unwrap_err();
        assert!(error.starts_with("Spotlight could not be asked"), "{error}");
    }

    #[test]
    fn the_folders_searched_and_the_query_reach_spotlight_as_arguments() {
        let _alone = crate::testing::alone();
        let root = Folder::new("spotlight-args");
        let program = root.executable(
            "mdfind",
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}/args.txt'\n",
                root.path().display()
            ),
        );
        let spotlight = Spotlight::new(vec![PathBuf::from("/Users/someone")]).with_program(program);
        spotlight.find("-weird query", &Cancel::new()).unwrap();
        let args = std::fs::read_to_string(root.path().join("args.txt")).unwrap();
        assert_eq!(
            args,
            "-onlyin\n/Users/someone\n-name\n--\n-weird query\n-onlyin\n/Users/someone\n--\n-weird query\n"
        );
    }

    #[test]
    fn a_passage_starts_at_the_line_that_matched_or_at_the_top() {
        let _alone = crate::testing::alone();
        let root = Folder::new("spotlight-passage");
        let matched = root.file(
            "a.md",
            "# Title\n\nFirst.\nThe snapshot isolation note.\nAfter.\n",
        );
        assert_eq!(
            passage(&matched, "snapshot", 10).as_deref(),
            Some("The snapshot isolation note.\nAfter.")
        );
        assert_eq!(
            passage(&matched, "zebra", 10).as_deref(),
            Some("# Title\nFirst.\nThe snapshot isolation note.\nAfter.")
        );
        let binary = root.file("b.md", "ab\0cd");
        assert_eq!(passage(&binary, "ab", 5), None);
        let image = root.file("c.png", "not really");
        assert_eq!(passage(&image, "not", 10), None);
    }
}
