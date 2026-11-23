//! The applications on this Mac, as standing items.
//!
//! An application is a folder whose name ends in `.app`. The folders macOS
//! keeps them in are read two levels deep (`/Applications/Utilities/…`) and
//! never inside an application. A few hundred entries at most; read once
//! off the main thread when the launcher first shows, and again when asked.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::item::{Item, ItemId, Kind, Target};
use crate::words;

/// How far below a folder applications are looked for.
const DEPTH: usize = 2;

/// The folders macOS keeps applications in, for the person whose home is
/// `home`.
pub fn folders(home: Option<&Path>) -> Vec<PathBuf> {
    let mut folders = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
        PathBuf::from("/System/Library/CoreServices/Applications"),
    ];
    if let Some(home) = home {
        folders.push(home.join("Applications"));
    }
    folders
}

/// Every application under `folders`, by name. One that appears twice (the
/// same path through two folders) is listed once. A folder that cannot be
/// read is skipped: it holds nothing the person could have opened anyway.
pub fn scan(folders: &[PathBuf], now: SystemTime) -> Vec<Item> {
    let mut found: Vec<PathBuf> = Vec::new();
    for folder in folders {
        collect(folder, DEPTH, &mut found);
    }
    found.sort();
    found.dedup();
    let mut items: Vec<Item> = found.iter().filter_map(|path| item(path, now)).collect();
    items.sort_by_key(|item| item.title.to_lowercase());
    items
}

fn collect(folder: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let hidden = path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with('.'));
        if hidden || !path.is_dir() {
            continue;
        }
        if path.extension().is_some_and(|extension| extension == "app") {
            found.push(path);
        } else if depth > 1 {
            collect(&path, depth - 1, found);
        }
    }
}

fn item(path: &Path, now: SystemTime) -> Option<Item> {
    let name = path.file_stem()?.to_string_lossy().into_owned();
    let shown = path.to_string_lossy().into_owned();
    let mut item = Item::new(
        ItemId::new("app", &shown),
        Kind::App,
        name,
        Target::Path(path.to_owned()),
    )
    .detail(
        path.parent()
            .map(|folder| folder.to_string_lossy().into_owned())
            .unwrap_or_default(),
    )
    .fact("Path", shown);
    if let Some(modified) = modified_ago(path, now) {
        item = item.fact("Modified", modified);
    }
    Some(item)
}

/// When the file at `path` was last changed, as words counted from `now`.
pub(crate) fn modified_ago(path: &Path, now: SystemTime) -> Option<String> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let since = |time: SystemTime| {
        time.duration_since(SystemTime::UNIX_EPOCH)
            .ok()
            .map(|span| span.as_millis() as i64)
    };
    Some(words::ago(since(now)?, since(modified)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Folder;

    #[test]
    fn applications_are_found_two_levels_deep_and_never_inside_one() {
        let root = Folder::new("apps");
        root.dir("Lumio Dev.app/Contents/Helpers/Inner.app");
        root.dir("Utilities/Terminal.app");
        root.dir("Utilities/Deep/Deeper/Buried.app");
        root.dir(".Hidden.app");
        root.dir("Not An App");
        root.file("Stray.app.txt", "x");

        let items = scan(&[root.path().to_owned()], SystemTime::now());
        let names: Vec<&str> = items.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(names, ["Lumio Dev", "Terminal"]);
        assert_eq!(items[0].kind, Kind::App);
        assert_eq!(
            items[0].target,
            Target::Path(root.path().join("Lumio Dev.app"))
        );
        assert_eq!(items[0].facts[0].name, "Path");
        assert_eq!(items[0].facts[1].name, "Modified");
    }

    #[test]
    fn the_same_application_through_two_folders_is_listed_once() {
        let root = Folder::new("apps-twice");
        root.dir("Flux.app");
        let folder = root.path().to_owned();
        let items = scan(&[folder.clone(), folder], SystemTime::now());
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn a_folder_that_is_not_there_is_skipped() {
        let root = Folder::new("apps-missing");
        root.dir("Mail.app");
        let items = scan(
            &[root.path().join("nowhere"), root.path().to_owned()],
            SystemTime::now(),
        );
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn a_home_adds_its_own_applications_folder() {
        let with = folders(Some(Path::new("/Users/someone")));
        assert!(with.contains(&PathBuf::from("/Users/someone/Applications")));
        assert_eq!(folders(None).len(), with.len() - 1);
    }
}
