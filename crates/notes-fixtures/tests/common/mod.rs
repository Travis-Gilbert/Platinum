#![allow(dead_code)]
//! Shared by the integration tests: paths into the crate and the fake `obsidian` executable.
use notes_fixtures::obsidian::Obsidian;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

pub const SCRIPT: &str = include_str!("../fake_obsidian.sh");

/// Writes `text` to `path` (creating its folders) and makes it executable.
pub fn write_executable(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn vault() -> PathBuf {
    crate_dir().join("vault")
}

/// A scratch directory under the target dir, emptied first, so a failed run can be inspected.
pub fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Files of a vault by relative path (forward slashes), skipping dot folders as Obsidian does.
pub fn visible_files(vault: &Path) -> Vec<String> {
    let walk = WalkDir::new(vault)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'));
    walk.map(|e| e.unwrap())
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            e.path()
                .strip_prefix(vault)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

/// One call the fake received: the directory it ran in and its arguments.
#[derive(Debug, Clone)]
pub struct Call {
    pub cwd: String,
    pub argv: Vec<String>,
}

impl Call {
    /// The command, after an optional leading `vault=<name>`.
    pub fn command(&self) -> &str {
        let at = usize::from(self.argv[0].starts_with("vault="));
        &self.argv[at]
    }

    pub fn args(&self) -> &[String] {
        &self.argv[usize::from(self.argv[0].starts_with("vault=")) + 1..]
    }

    /// `command arg arg`, as it would be typed.
    pub fn line(&self) -> String {
        self.argv.join(" ")
    }
}

/// The fake executable, written out as `<dir>/bin/obsidian`, with its call log.
pub struct Fake {
    pub dir: PathBuf,
    pub log: PathBuf,
}

impl Fake {
    pub fn new(name: &str) -> Self {
        let dir = scratch(name);
        let bin = dir.join("bin");
        fs::create_dir_all(&bin).unwrap();
        write_executable(&bin.join("obsidian"), SCRIPT);
        Self {
            log: dir.join("calls.log"),
            dir,
        }
    }

    /// The directory to put first on `PATH`.
    pub fn bin(&self) -> PathBuf {
        self.dir.join("bin")
    }

    pub fn program(&self) -> PathBuf {
        self.bin().join("obsidian")
    }

    /// A harness client for `vault` that runs the fake by its path.
    pub fn client(&self, vault: &Path) -> Obsidian {
        Obsidian::new(self.program(), vault).env("FAKE_OBSIDIAN_LOG", self.log.to_str().unwrap())
    }

    pub fn calls(&self) -> Vec<Call> {
        let Ok(text) = fs::read_to_string(&self.log) else {
            return Vec::new();
        };
        let lines: Vec<&str> = text.lines().collect();
        lines
            .chunks(2)
            .map(|pair| Call {
                cwd: pair[0].strip_prefix("cwd\u{1f}").unwrap().to_string(),
                argv: pair[1]
                    .strip_prefix("argv\u{1f}")
                    .map_or_else(Vec::new, |a| a.split('\u{1f}').map(String::from).collect()),
            })
            .collect()
    }
}
