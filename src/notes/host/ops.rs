//! What the host asks of a vault: each function runs on the vault's one
//! thread (`service.rs`) with the `Vault` in hand and returns plain owned
//! data, so nothing of the vault crosses to the UI or to Servo's threads.
//!
//! The vault's files are untrusted. A path that is asked for is checked
//! against the index and against the disk (a symlink that leaves the vault
//! is not followed), a file is bounded, and a type that could run is served
//! as text.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail, ensure};
use frame_notes::{
    FileKind, LinkSyntax, ReconcileResult, Vault, is_hidden, normalize, parse_note, render_note,
    render_preview, stem,
};
use palette::{FromColor as _, Hsl, Srgb};
use percent_encoding::percent_decode_str;
use serde_json::Value;

/// The largest vault file the reading view serves. Responses are whole.
const MAX_FILE: u64 = 64 * 1024 * 1024;
const MAX_STYLE: u64 = 1024 * 1024;
const MAX_SETTINGS: u64 = 256 * 1024;
const SNIPPETS: &str = ".obsidian/snippets";
const MERMAID_MARKER: &str = "mermaid-allowed";

/// A note rendered at one revision of the authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rendered {
    /// The authority revision of the text this was rendered from.
    pub revision: u64,
    pub html: String,
    pub cssclasses: Vec<String>,
    /// Identifies the html and classes, so an unchanged render is not sent
    /// again.
    pub digest: String,
}

/// Everything the reading page's shell needs.
#[derive(Clone, Debug, PartialEq)]
pub struct PageData {
    pub title: String,
    pub rendered: Rendered,
    pub appearance: Appearance,
    /// The person allowed Mermaid diagrams in this vault.
    pub mermaid: bool,
}

/// Which color scheme the vault asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    Light,
    Dark,
    /// Follow the system.
    System,
}

/// The vault's `.obsidian/appearance.json`, as far as the reading view uses
/// it. A missing or malformed file leaves every default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Appearance {
    pub scheme: Scheme,
    /// The accent color's hue (degrees), saturation and lightness (percent).
    pub accent: Option<(u16, u8, u8)>,
    pub font_size: Option<u16>,
    /// Names of the enabled snippets in `.obsidian/snippets/<name>.css`.
    pub snippets: Vec<String>,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            scheme: Scheme::System,
            accent: None,
            font_size: None,
            snippets: Vec::new(),
        }
    }
}

impl Appearance {
    pub fn read(root: &Path) -> Self {
        let Some(bytes) = read_bounded(&root.join(".obsidian/appearance.json"), MAX_SETTINGS)
        else {
            return Self::default();
        };
        let Ok(Value::Object(settings)) = serde_json::from_slice::<Value>(&bytes) else {
            return Self::default();
        };
        let scheme = match settings.get("theme").and_then(Value::as_str) {
            Some("moonstone") => Scheme::Light,
            Some("obsidian") => Scheme::Dark,
            _ => Scheme::System,
        };
        let snippets = settings
            .get("enabledCssSnippets")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|name| plain_name(name) && snippet_file(root, name).is_some())
            .map(str::to_owned)
            .collect();
        Self {
            scheme,
            accent: settings
                .get("accentColor")
                .and_then(Value::as_str)
                .and_then(accent),
            font_size: settings
                .get("baseFontSize")
                .and_then(Value::as_u64)
                .filter(|size| (10..=40).contains(size))
                .map(|size| size as u16),
            snippets,
        }
    }
}

/// `#rrggbb` as the hue, saturation and lightness Obsidian's accent variables
/// take.
fn accent(color: &str) -> Option<(u16, u8, u8)> {
    let digits = color.strip_prefix('#').filter(|digits| digits.len() == 6)?;
    let rgb = u32::from_str_radix(digits, 16).ok()?;
    let [_, r, g, b] = rgb.to_be_bytes();
    let hsl: Hsl = Hsl::from_color(Srgb::new(r, g, b).into_format::<f32>());
    Some((
        hsl.hue.into_positive_degrees().round() as u16 % 360,
        (hsl.saturation * 100.).round() as u8,
        (hsl.lightness * 100.).round() as u8,
    ))
}

/// A name that is one visible path segment.
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && !name.contains(['/', '\\'])
        && !name.chars().any(char::is_control)
}

/// The snippet's file, if it is a regular file that stays in the snippets
/// folder.
fn snippet_file(root: &Path, name: &str) -> Option<PathBuf> {
    let folder = root.join(SNIPPETS).canonicalize().ok()?;
    let file = folder.join(format!("{name}.css")).canonicalize().ok()?;
    (file.parent() == Some(folder.as_path()) && file.is_file()).then_some(file)
}

fn read_bounded(path: &Path, max: u64) -> Option<Vec<u8>> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > max {
        return None;
    }
    std::fs::read(path).ok()
}

/// The source of an enabled snippet; `file` is the address's name, with `.css`.
pub fn snippet(vault: &Vault, file: &str) -> Result<Option<String>> {
    let Some(name) = file.strip_suffix(".css") else {
        return Ok(None);
    };
    let appearance = Appearance::read(vault.root());
    if !appearance.snippets.iter().any(|enabled| enabled == name) {
        return Ok(None);
    }
    Ok(snippet_file(vault.root(), name)
        .and_then(|file| read_bounded(&file, MAX_STYLE))
        .and_then(|bytes| String::from_utf8(bytes).ok()))
}

/// The current text of a note: the authority's, which holds edits not yet
/// on disk, or else the disk's.
fn current_text(vault: &Vault, path: &str) -> Option<String> {
    let authority = vault.authority();
    if let Ok(Some(id)) = authority.resolve("local", path)
        && let Ok(Some(head)) = authority.head(&id)
    {
        return Some(head.text);
    }
    let file = real_path(vault, path)?;
    String::from_utf8(read_bounded(&file, MAX_FILE)?).ok()
}

/// The file at a vault path, if it is a plain visible path whose real
/// location is inside the vault.
fn real_path(vault: &Vault, path: &str) -> Option<PathBuf> {
    if normalize(path).as_deref() != Some(path) || is_hidden(path) {
        return None;
    }
    let root = vault.root().canonicalize().ok()?;
    let file = root.join(path).canonicalize().ok()?;
    file.starts_with(&root).then_some(file)
}

fn digest(html: &str, cssclasses: &[String]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(html.as_bytes());
    for class in cssclasses {
        hasher.update(&[0]);
        hasher.update(class.as_bytes());
    }
    hasher.finalize().to_hex()[..16].to_owned()
}

/// Every note of the vault, excluded ones among them, in path order.
pub fn notes(vault: &Vault) -> Vec<String> {
    vault
        .index()
        .files()
        .values()
        .filter(|file| file.kind == FileKind::Note)
        .map(|file| file.path.clone())
        .collect()
}

/// The note to show when none is named: the vault's `Home.md` if it has one,
/// else its first note.
pub fn first_note(vault: &Vault) -> Option<String> {
    let notes = notes(vault);
    notes
        .iter()
        .find(|path| path.eq_ignore_ascii_case("home.md"))
        .or(notes.first())
        .cloned()
}

/// A note, at the authority's current revision.
pub fn render(vault: &mut Vault, path: &str) -> Result<Rendered> {
    ensure!(
        FileKind::of(path) == FileKind::Note,
        "{path} is not a note the reading view can show"
    );
    let document = vault.open_document(path)?;
    let view = render_note(
        vault.index(),
        vault.settings(),
        path,
        &document.text,
        &|other| current_text(vault, other),
    );
    Ok(Rendered {
        revision: document.revision,
        digest: digest(&view.html, &view.cssclasses),
        html: view.html,
        cssclasses: view.cssclasses,
    })
}

/// A note with what its reading page needs besides its text.
pub fn page(vault: &mut Vault, path: &str) -> Result<PageData> {
    let rendered = render(vault, path)?;
    Ok(PageData {
        title: stem(path.rsplit('/').next().unwrap_or(path)).to_owned(),
        rendered,
        appearance: Appearance::read(vault.root()),
        mermaid: vault.data_dir().join(MERMAID_MARKER).exists(),
    })
}

/// The person trusts this vault's diagrams.
pub fn allow_mermaid(vault: &mut Vault) -> Result<()> {
    std::fs::write(vault.data_dir().join(MERMAID_MARKER), b"")
        .context("remembering that diagrams are allowed")
}

/// What a page preview of `link`, written in `from`, shows.
pub fn preview(vault: &Vault, from: &str, link: &str) -> Option<String> {
    render_preview(vault.index(), vault.settings(), from, link, &|other| {
        current_text(vault, other)
    })
    .map(|view| view.html)
}

/// A vault file that can be served.
pub struct FileData {
    pub bytes: Vec<u8>,
    pub content_type: &'static str,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FileError {
    NotFound,
    TooLarge,
}

/// An attachment of the vault. Only files the index lists are served, and
/// only from inside the vault.
pub fn file(vault: &Vault, path: &str) -> Result<FileData, FileError> {
    if !vault.index().files().contains_key(path) {
        return Err(FileError::NotFound);
    }
    let real = real_path(vault, path).ok_or(FileError::NotFound)?;
    let metadata = std::fs::metadata(&real).map_err(|_| FileError::NotFound)?;
    if !metadata.is_file() {
        return Err(FileError::NotFound);
    }
    if metadata.len() > MAX_FILE {
        return Err(FileError::TooLarge);
    }
    Ok(FileData {
        bytes: std::fs::read(&real).map_err(|_| FileError::NotFound)?,
        content_type: attachment_type(path),
    })
}

/// The type a vault file is served as. What a browser could run (HTML, script,
/// style, XML) is text; an unknown kind is opaque bytes.
pub fn attachment_type(path: &str) -> &'static str {
    let extension = path.rsplit_once('.').map_or("", |(_, extension)| extension);
    match extension.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "ico" => "image/vnd.microsoft.icon",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "ogg" | "oga" => "audio/ogg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "webm" => "video/webm",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "ogv" => "video/ogg",
        "pdf" => "application/pdf",
        "md" | "txt" | "csv" | "json" | "yaml" | "yml" | "canvas" | "base" | "html" | "htm"
        | "xhtml" | "xml" | "js" | "mjs" | "css" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Where a followed link leads.
#[derive(Debug, PartialEq, Eq)]
pub enum Opened {
    /// A note of the vault, with the heading or block the link named.
    Note {
        path: String,
        fragment: Option<String>,
        created: bool,
    },
    /// A file that is not a note: the reading view does not open it.
    Other(String),
}

/// Resolves a link followed in the note `from`. A link to a note that does
/// not exist makes the note, where the vault keeps new notes.
pub fn open_link(vault: &mut Vault, from: &str, link: &str) -> Result<Opened> {
    let (target, fragment) = match link.split_once('#') {
        Some((target, fragment)) => (target, Some(fragment.to_owned())),
        None => (link, None),
    };
    let target = target.trim();
    ensure!(
        !target.is_empty(),
        "a link to the same note needs no navigation"
    );
    let decoded = percent_decode_str(target).decode_utf8_lossy().into_owned();
    let names = vault.index().names();
    let resolved = [target, decoded.as_str()].into_iter().find_map(|name| {
        names
            .resolve(name, from, LinkSyntax::Wiki)
            .map(str::to_owned)
    });
    if let Some(path) = resolved {
        return Ok(if FileKind::of(&path) == FileKind::Note {
            Opened::Note {
                path,
                fragment,
                created: false,
            }
        } else {
            Opened::Other(path)
        });
    }
    let path = vault
        .settings()
        .new_note_path(&decoded, from, LinkSyntax::Wiki)
        .with_context(|| format!("no note can be made for the link {target:?}"))?;
    create_note(vault, &path)?;
    Ok(Opened::Note {
        path,
        fragment: None,
        created: true,
    })
}

/// An empty note at `path`, which has not existed. Its folder is made if it
/// has to be, but never through a link that leaves the vault.
fn create_note(vault: &mut Vault, path: &str) -> Result<()> {
    let root = vault.root().canonicalize()?;
    let file = root.join(path);
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)?;
        ensure!(
            folder.canonicalize()?.starts_with(&root),
            "{path} is outside the vault"
        );
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&file)
        .with_context(|| format!("making {path}"))?;
    vault.refresh()?;
    Ok(())
}

/// What a toggled checkbox came to.
#[derive(Debug, PartialEq, Eq)]
pub enum Toggled {
    /// The note now has the task as the page shows it, at this revision.
    Done { revision: u64 },
    /// The page showed another revision than the note has; nothing changed.
    Stale { current: u64 },
    /// No task starts on that line.
    NoTask,
}

/// Sets the task on `line` (0-based) of the note, which the page showed at
/// `revision`, to `checked`: an edit through the authority, then a save.
pub fn toggle_task(
    vault: &mut Vault,
    path: &str,
    line: u32,
    checked: bool,
    revision: u64,
) -> Result<Toggled> {
    ensure!(FileKind::of(path) == FileKind::Note, "{path} is not a note");
    let document = vault.open_document(path)?;
    if document.revision != revision {
        return Ok(Toggled::Stale {
            current: document.revision,
        });
    }
    let note = parse_note(&document.text);
    let Some(task) = note
        .tasks
        .iter()
        .find(|task| task.status_span.line == line + 1)
    else {
        return Ok(Toggled::NoTask);
    };
    if task.is_done() == checked {
        return Ok(Toggled::Done { revision });
    }
    let range = task.status_span.range();
    ensure!(
        document.text.get(range.clone()).is_some(),
        "the task's status is not on a character boundary"
    );
    let mut text = document.text;
    text.replace_range(range, if checked { "x" } else { " " });
    vault.edit(path, revision, &operation_id()?, &text)?;
    match vault.save(path)? {
        ReconcileResult::Applied { .. } => Ok(Toggled::Done {
            revision: vault.open_document(path)?.revision,
        }),
        other => bail!("{path} changed on disk and was not saved: {other:?}"),
    }
}

fn operation_id() -> Result<String> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).context("an operation id")?;
    Ok(format!(
        "reading-task-{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A vault of the given files in a temporary folder, with its data folder
    /// beside it.
    pub(crate) struct Fixture {
        pub root: tempfile::TempDir,
        pub data: tempfile::TempDir,
        pub vault: Vault,
    }

    pub(crate) fn fixture(files: &[(&str, &str)]) -> Fixture {
        let root = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let file = root.path().join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
        let vault = Vault::open(root.path(), data.path()).unwrap();
        Fixture { root, data, vault }
    }

    #[test]
    fn a_note_renders_at_the_authoritys_revision() {
        let mut f = fixture(&[("Home.md", "# Home\n\nHello *world*.\n")]);
        let first = render(&mut f.vault, "Home.md").unwrap();
        assert!(first.html.contains("Hello"), "{}", first.html);
        assert_eq!(render(&mut f.vault, "Home.md").unwrap(), first);
        let edited = f
            .vault
            .edit("Home.md", first.revision, "t1", "# Home\n\nChanged.\n")
            .unwrap();
        let second = render(&mut f.vault, "Home.md").unwrap();
        assert_eq!(second.revision, edited.revision);
        assert!(second.revision > first.revision);
        assert_ne!(second.digest, first.digest);
    }

    #[test]
    fn the_notes_of_a_vault_are_its_markdown_files_and_the_first_is_home() {
        let f = fixture(&[
            ("b/Zed.md", "z"),
            ("Alpha.md", "a"),
            ("Home.md", "h"),
            ("pic.png", "x"),
            (".obsidian/app.json", "{}"),
        ]);
        assert_eq!(notes(&f.vault), ["Alpha.md", "Home.md", "b/Zed.md"]);
        assert_eq!(first_note(&f.vault).as_deref(), Some("Home.md"));
        let f = fixture(&[("b/Zed.md", "z"), ("Alpha.md", "a")]);
        assert_eq!(first_note(&f.vault).as_deref(), Some("Alpha.md"));
        let f = fixture(&[("pic.png", "x")]);
        assert_eq!(first_note(&f.vault), None);
    }

    #[test]
    fn only_notes_are_rendered() {
        let mut f = fixture(&[("pic.png", "x"), ("Home.md", "hi")]);
        assert!(render(&mut f.vault, "pic.png").is_err());
        assert!(render(&mut f.vault, "../Home.md").is_err());
        assert!(render(&mut f.vault, ".obsidian/app.json").is_err());
    }

    #[test]
    fn attachments_are_served_from_the_index_with_safe_types() {
        let f = fixture(&[
            ("attachments/pic.png", "png bytes"),
            ("page.html", "<script>alert(1)</script>"),
            ("hidden/.secret.png", "secret"),
        ]);
        let png = file(&f.vault, "attachments/pic.png").unwrap();
        assert_eq!(png.content_type, "image/png");
        assert_eq!(png.bytes, b"png bytes");
        // Active content is text, not a page.
        assert_eq!(
            file(&f.vault, "page.html").unwrap().content_type,
            "text/plain; charset=utf-8"
        );
        assert_eq!(attachment_type("x.SVG"), "image/svg+xml");
        assert_eq!(attachment_type("x.js"), "text/plain; charset=utf-8");
        assert_eq!(attachment_type("x.bin"), "application/octet-stream");
        assert_eq!(attachment_type("noextension"), "application/octet-stream");
        for refused in [
            "missing.png",
            "../attachments/pic.png",
            ".obsidian/appearance.json",
            "hidden/.secret.png",
            "attachments",
        ] {
            assert_eq!(
                file(&f.vault, refused).err(),
                Some(FileError::NotFound),
                "{refused}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_vault_is_not_followed() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.png"), "secret").unwrap();
        let mut f = fixture(&[("Home.md", "hi")]);
        std::os::unix::fs::symlink(
            outside.path().join("secret.png"),
            f.root.path().join("link.png"),
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), f.root.path().join("folder")).unwrap();
        f.vault.refresh().unwrap();
        assert_eq!(file(&f.vault, "link.png").err(), Some(FileError::NotFound));
        assert_eq!(
            file(&f.vault, "folder/secret.png").err(),
            Some(FileError::NotFound)
        );
        assert!(current_text(&f.vault, "link.png").is_none());
    }

    #[test]
    fn a_file_over_the_cap_is_refused_not_read() {
        let f = fixture(&[("big.bin", "")]);
        let big = std::fs::File::create(f.root.path().join("big.bin")).unwrap();
        big.set_len(MAX_FILE + 1).unwrap();
        assert_eq!(file(&f.vault, "big.bin").err(), Some(FileError::TooLarge));
    }

    #[test]
    fn appearance_reads_theme_accent_size_and_enabled_snippets() {
        let f = fixture(&[
            (
                ".obsidian/appearance.json",
                r##"{"theme":"obsidian","accentColor":"#7b6cd9","baseFontSize":18,
                    "enabledCssSnippets":["callouts","missing","../x","a/b",".hid"]}"##,
            ),
            (".obsidian/snippets/callouts.css", ".callout { color: red }"),
            (".obsidian/snippets/off.css", "body { color: blue }"),
        ]);
        let appearance = Appearance::read(f.vault.root());
        assert_eq!(appearance.scheme, Scheme::Dark);
        assert_eq!(appearance.font_size, Some(18));
        assert_eq!(appearance.accent, Some((248, 59, 64)));
        assert_eq!(appearance.snippets, ["callouts"]);
        assert_eq!(
            snippet(&f.vault, "callouts.css").unwrap().as_deref(),
            Some(".callout { color: red }")
        );
        // Only an enabled snippet is served.
        assert_eq!(snippet(&f.vault, "off.css").unwrap(), None);
        assert_eq!(snippet(&f.vault, "callouts").unwrap(), None);
        assert_eq!(snippet(&f.vault, "../appearance.json").unwrap(), None);
    }

    #[test]
    fn appearance_defaults_when_missing_or_broken() {
        let f = fixture(&[("Home.md", "hi")]);
        assert_eq!(Appearance::read(f.vault.root()), Appearance::default());
        let f = fixture(&[(".obsidian/appearance.json", "{not json")]);
        assert_eq!(Appearance::read(f.vault.root()), Appearance::default());
        let f = fixture(&[(
            ".obsidian/appearance.json",
            r#"{"theme":"moonstone","baseFontSize":900,"accentColor":"red"}"#,
        )]);
        let appearance = Appearance::read(f.vault.root());
        assert_eq!(appearance.scheme, Scheme::Light);
        assert_eq!((appearance.font_size, appearance.accent), (None, None));
    }

    #[test]
    fn a_task_toggle_edits_and_saves_the_note() {
        let mut f = fixture(&[("Tasks.md", "# Tasks\n\n- [ ] one\n- [x] two\n- [ ] three\n")]);
        let shown = render(&mut f.vault, "Tasks.md").unwrap();
        // Lines are 0-based: "- [ ] one" is line 2.
        let done = toggle_task(&mut f.vault, "Tasks.md", 2, true, shown.revision).unwrap();
        let Toggled::Done { revision } = done else {
            panic!("{done:?}")
        };
        assert!(revision > shown.revision);
        let disk = std::fs::read_to_string(f.root.path().join("Tasks.md")).unwrap();
        assert_eq!(disk, "# Tasks\n\n- [x] one\n- [x] two\n- [ ] three\n");
        // And back, from the revision the page now shows.
        assert!(matches!(
            toggle_task(&mut f.vault, "Tasks.md", 3, false, revision).unwrap(),
            Toggled::Done { .. }
        ));
        let disk = std::fs::read_to_string(f.root.path().join("Tasks.md")).unwrap();
        assert_eq!(disk, "# Tasks\n\n- [x] one\n- [ ] two\n- [ ] three\n");
    }

    #[test]
    fn a_task_toggle_from_an_old_page_or_a_missing_task_changes_nothing() {
        let mut f = fixture(&[("Tasks.md", "- [ ] one\ntext\n")]);
        let shown = render(&mut f.vault, "Tasks.md").unwrap();
        let before = std::fs::read_to_string(f.root.path().join("Tasks.md")).unwrap();
        assert_eq!(
            toggle_task(&mut f.vault, "Tasks.md", 0, true, shown.revision + 5).unwrap(),
            Toggled::Stale {
                current: shown.revision
            }
        );
        assert_eq!(
            toggle_task(&mut f.vault, "Tasks.md", 1, true, shown.revision).unwrap(),
            Toggled::NoTask
        );
        // Already as asked.
        assert_eq!(
            toggle_task(&mut f.vault, "Tasks.md", 0, false, shown.revision).unwrap(),
            Toggled::Done {
                revision: shown.revision
            }
        );
        assert!(toggle_task(&mut f.vault, "../x.md", 0, true, 0).is_err());
        assert_eq!(
            std::fs::read_to_string(f.root.path().join("Tasks.md")).unwrap(),
            before
        );
    }

    #[test]
    fn a_link_opens_its_note_and_an_unresolved_one_makes_it() {
        let mut f = fixture(&[
            ("Home.md", "[[Other]] [[Missing]]"),
            ("folder/Other.md", "other"),
            ("pic.png", "x"),
        ]);
        assert_eq!(
            open_link(&mut f.vault, "Home.md", "Other#Heading").unwrap(),
            Opened::Note {
                path: "folder/Other.md".into(),
                fragment: Some("Heading".into()),
                created: false
            }
        );
        assert_eq!(
            open_link(&mut f.vault, "Home.md", "pic.png").unwrap(),
            Opened::Other("pic.png".into())
        );
        let made = open_link(&mut f.vault, "Home.md", "Missing").unwrap();
        assert_eq!(
            made,
            Opened::Note {
                path: "Missing.md".into(),
                fragment: None,
                created: true
            }
        );
        assert_eq!(
            std::fs::read_to_string(f.root.path().join("Missing.md")).unwrap(),
            ""
        );
        // Now it exists, so the same link just opens it.
        assert_eq!(
            open_link(&mut f.vault, "Home.md", "Missing").unwrap(),
            Opened::Note {
                path: "Missing.md".into(),
                fragment: None,
                created: false
            }
        );
        // A path makes its folders; a name that is not a file name makes nothing.
        assert!(matches!(
            open_link(&mut f.vault, "Home.md", "new/deep/Note").unwrap(),
            Opened::Note { created: true, .. }
        ));
        assert!(f.root.path().join("new/deep/Note.md").is_file());
        for refused in ["../outside", ".obsidian/x", "a:b", "#"] {
            assert!(
                open_link(&mut f.vault, "Home.md", refused).is_err(),
                "{refused}"
            );
        }
        assert!(!f.root.path().parent().unwrap().join("outside.md").exists());
    }

    #[test]
    fn mermaid_is_allowed_once_per_vault_in_its_data_folder() {
        let mut f = fixture(&[("Home.md", "hi")]);
        assert!(!page(&mut f.vault, "Home.md").unwrap().mermaid);
        allow_mermaid(&mut f.vault).unwrap();
        assert!(page(&mut f.vault, "Home.md").unwrap().mermaid);
        assert!(f.data.path().join(MERMAID_MARKER).exists());
        assert!(!f.root.path().join(MERMAID_MARKER).exists());
        assert_eq!(page(&mut f.vault, "Home.md").unwrap().title, "Home");
    }
}
