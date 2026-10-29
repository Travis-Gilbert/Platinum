//! Downloads: files Platinum saves to the Downloads folder instead of showing.
//!
//! Servo has no download support (its `download` attribute is a TODO, and a
//! type it cannot render becomes a one-line placeholder page), so Platinum
//! decides what is a download, in `delegate.rs` and `page.rs` or from the
//! person's "Download Linked File", and fetches it itself, with the page's
//! cookies so that signed-in downloads work.
//!
//! SOURCING: reqwest, already built for RustyWeb's providers, fetches and
//! streams; `content_disposition` reads the server's file name,
//! `sanitize-filename` makes it safe for the disk, `mime_guess` supplies a
//! missing extension and `humansize` writes sizes. GPUI's `open_with_system`
//! and `reveal_path` open and show the result. The list is ours, as are two
//! rules no crate holds: Finder's "report (1).pdf" for a name already taken,
//! and writing to "report.pdf.download" until the file is complete.

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui_kit::{App, AppContext as _, Context, SharedString, Task};
use humansize::{DECIMAL, format_size};
use percent_encoding::percent_decode_str;
use reqwest::header::{CONTENT_DISPOSITION, CONTENT_TYPE, COOKIE, REFERER};
use url::Url;

/// How often a running download reports its progress.
const PROGRESS_EVERY: Duration = Duration::from_millis(150);
/// How long a read may wait before the worker looks for a cancel.
const CANCEL_CHECK: Duration = Duration::from_millis(250);
/// Appended to the file's name until it is complete.
const PARTIAL: &str = "download";

/// What to fetch, and what to send with it.
pub struct Request {
    pub url: Url,
    /// The page the download came from.
    pub referrer: Option<Url>,
    /// The `Cookie` header Servo would send for `url`.
    pub cookies: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DownloadState {
    Running,
    Finished,
    Failed(SharedString),
    Cancelled,
}

pub struct Download {
    pub id: u64,
    pub name: SharedString,
    /// Where the file is, once the server has named it.
    pub path: Option<PathBuf>,
    pub received: u64,
    pub total: Option<u64>,
    pub state: DownloadState,
    cancel: Arc<AtomicBool>,
    _updates: Option<Task<()>>,
}

impl Download {
    pub fn is_running(&self) -> bool {
        self.state == DownloadState::Running
    }

    /// How far along it is, out of 100, when the size is known.
    pub fn percent(&self) -> Option<f32> {
        let total = self.total.filter(|total| *total > 0)?;
        Some((self.received as f64 / total as f64 * 100.).min(100.) as f32)
    }

    /// "12.3 MB of 40 MB", "40 MB", "Failed: ...".
    pub fn status(&self) -> SharedString {
        let size = |bytes: u64| format_size(bytes, DECIMAL);
        match &self.state {
            DownloadState::Running => match self.total {
                Some(total) => format!("{} of {}", size(self.received), size(total)).into(),
                None => size(self.received).into(),
            },
            DownloadState::Finished => size(self.received).into(),
            DownloadState::Failed(reason) => format!("Failed: {reason}").into(),
            DownloadState::Cancelled => "Cancelled".into(),
        }
    }
}

/// What the worker thread reports.
enum Update {
    Named { path: PathBuf, total: Option<u64> },
    Progress(u64),
    Finished(u64),
    Failed(String),
    Cancelled,
}

/// Every download this session, newest first.
pub struct Downloads {
    items: Vec<Download>,
    next_id: u64,
    folder: PathBuf,
}

impl Downloads {
    pub fn new(folder: PathBuf) -> Self {
        Self {
            items: Vec::new(),
            next_id: 0,
            folder,
        }
    }

    /// The person's Downloads folder.
    pub fn default_folder() -> PathBuf {
        dirs::download_dir()
            .or_else(|| dirs::home_dir().map(|home| home.join("Downloads")))
            .unwrap_or_else(std::env::temp_dir)
    }

    pub fn items(&self) -> &[Download] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Overall progress of the running downloads, out of 100, if any is
    /// running; `None` inside when no running download knows its size.
    pub fn running(&self) -> Option<Option<f32>> {
        let running: Vec<&Download> = self.items.iter().filter(|d| d.is_running()).collect();
        if running.is_empty() {
            return None;
        }
        let total: u64 = running.iter().filter_map(|d| d.total).sum();
        let received: u64 = running
            .iter()
            .filter(|d| d.total.is_some())
            .map(|d| d.received)
            .sum();
        Some((total > 0).then(|| (received as f64 / total as f64 * 100.).min(100.) as f32))
    }

    /// Start fetching on GPUI's background executor, where blocking is
    /// allowed, as `search.rs` fetches. Returns the download's id.
    pub fn start(&mut self, request: Request, cx: &mut Context<Self>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        let (updates, mut received) = mpsc::unbounded();
        let name = file_name(&request.url, None, None);
        {
            let folder = self.folder.clone();
            let cancel = cancel.clone();
            cx.background_spawn(async move { run(request, &folder, &cancel, &updates) })
                .detach();
        }
        let task = cx.spawn(async move |downloads, cx| {
            while let Some(update) = received.next().await {
                let applied =
                    downloads.update(cx, |downloads, cx| downloads.apply(id, update, cx));
                if applied.is_err() {
                    break;
                }
            }
        });
        self.items.insert(
            0,
            Download {
                id,
                name: name.into(),
                path: None,
                received: 0,
                total: None,
                state: DownloadState::Running,
                cancel,
                _updates: Some(task),
            },
        );
        cx.notify();
        id
    }

    fn apply(&mut self, id: u64, update: Update, cx: &mut Context<Self>) {
        let Some(download) = self.items.iter_mut().find(|d| d.id == id) else {
            return;
        };
        match update {
            Update::Named { path, total } => {
                if let Some(name) = path.file_name() {
                    download.name = name.to_string_lossy().into_owned().into();
                }
                download.path = Some(path);
                download.total = total;
            }
            Update::Progress(received) => download.received = received,
            Update::Finished(received) => {
                download.received = received;
                download.total = Some(received);
                download.state = DownloadState::Finished;
            }
            Update::Failed(reason) => {
                if download.is_running() {
                    download.state = DownloadState::Failed(reason.into());
                }
            }
            Update::Cancelled => download.state = DownloadState::Cancelled,
        }
        cx.notify();
    }

    pub fn cancel(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(download) = self.items.iter_mut().find(|d| d.id == id)
            && download.is_running()
        {
            download.cancel.store(true, Ordering::Relaxed);
            download.state = DownloadState::Cancelled;
            cx.notify();
        }
    }

    /// Forget the downloads that are no longer running.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.items.retain(Download::is_running);
        cx.notify();
    }

    pub fn open(&self, id: u64, cx: &App) {
        if let Some(path) = self.finished_path(id) {
            cx.open_with_system(path);
        }
    }

    pub fn reveal(&self, id: u64, cx: &App) {
        if let Some(path) = self.finished_path(id) {
            cx.reveal_path(path);
        }
    }

    fn finished_path(&self, id: u64) -> Option<&Path> {
        let download = self.items.iter().find(|d| d.id == id)?;
        (download.state == DownloadState::Finished)
            .then_some(download.path.as_deref())
            .flatten()
    }
}

impl Drop for Downloads {
    fn drop(&mut self) {
        for download in &self.items {
            download.cancel.store(true, Ordering::Relaxed);
        }
    }
}

/// The worker: one small Tokio runtime for reqwest, as `search.rs` builds
/// for each search.
fn run(request: Request, folder: &Path, cancel: &AtomicBool, updates: &mpsc::UnboundedSender<Update>) {
    let outcome = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(anyhow::Error::from)
        .and_then(|runtime| runtime.block_on(fetch(&request, folder, cancel, updates)));
    let update = match outcome {
        Ok(update) => update,
        Err(error) => {
            log::warn!("download {}: {error:#}", request.url);
            Update::Failed(format!("{error:#}"))
        }
    };
    let _ = updates.unbounded_send(update);
}

async fn fetch(
    request: &Request,
    folder: &Path,
    cancel: &AtomicBool,
    updates: &mpsc::UnboundedSender<Update>,
) -> anyhow::Result<Update> {
    let mut client = reqwest::Client::builder();
    if let Some(user_agent) = &request.user_agent {
        client = client.user_agent(user_agent);
    }
    let mut get = client.build()?.get(request.url.clone());
    if let Some(referrer) = &request.referrer {
        get = get.header(REFERER, referrer.as_str());
    }
    if let Some(cookies) = &request.cookies {
        get = get.header(COOKIE, cookies);
    }
    let mut response = get.send().await?.error_for_status()?;
    let header = |name: reqwest::header::HeaderName| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    let name = file_name(
        response.url(),
        header(CONTENT_DISPOSITION).as_deref(),
        header(CONTENT_TYPE).as_deref(),
    );
    let (path, mut partial) = reserve(folder, &name)?;
    let _ = updates.unbounded_send(Update::Named {
        path: path.clone(),
        total: response.content_length(),
    });

    let mut received = 0u64;
    let mut reported = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(Update::Cancelled);
        }
        let chunk = match tokio::time::timeout(CANCEL_CHECK, response.chunk()).await {
            Err(_) => continue,
            Ok(chunk) => chunk?,
        };
        let Some(chunk) = chunk else { break };
        partial.file.write_all(&chunk)?;
        received += chunk.len() as u64;
        if reported.elapsed() >= PROGRESS_EVERY {
            reported = Instant::now();
            let _ = updates.unbounded_send(Update::Progress(received));
        }
    }
    partial.finish(&path)?;
    Ok(Update::Finished(received))
}

/// The file a download is written to until it is complete; removed unless
/// it finishes.
struct Partial {
    path: PathBuf,
    file: File,
    done: bool,
}

impl Partial {
    fn finish(&mut self, path: &Path) -> anyhow::Result<()> {
        self.file.flush()?;
        // Something else may have taken the name meanwhile.
        let path = if path.exists() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            unique_path(path.parent().unwrap_or(Path::new(".")), &name, |p| p.exists())
        } else {
            path.to_owned()
        };
        fs::rename(&self.path, &path)?;
        self.done = true;
        Ok(())
    }
}

impl Drop for Partial {
    fn drop(&mut self) {
        if !self.done {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// A free name in `folder`, with its partial file created so no other
/// download takes it.
fn reserve(folder: &Path, name: &str) -> anyhow::Result<(PathBuf, Partial)> {
    fs::create_dir_all(folder)?;
    loop {
        let path = unique_path(folder, name, |p| p.exists() || partial_path(p).exists());
        let partial = partial_path(&path);
        match OpenOptions::new().write(true).create_new(true).open(&partial) {
            Ok(file) => {
                return Ok((
                    path,
                    Partial {
                        path: partial,
                        file,
                        done: false,
                    },
                ));
            }
            // Another download took it between the check and the create.
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
}

fn partial_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(".");
    name.push(PARTIAL);
    path.with_file_name(name)
}

/// `name` in `folder`, or Finder's "name (1).ext", "name (2).ext", ... when
/// it is taken.
pub fn unique_path(folder: &Path, name: &str, taken: impl Fn(&Path) -> bool) -> PathBuf {
    let path = folder.join(name);
    if !taken(&path) {
        return path;
    }
    let as_path = Path::new(name);
    let stem = as_path
        .file_stem()
        .map_or_else(|| name.to_owned(), |s| s.to_string_lossy().into_owned());
    let extension = as_path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (1..)
        .map(|n| folder.join(format!("{stem} ({n}){extension}")))
        .find(|path| !taken(path))
        .expect("an unbounded range always finds a free name")
}

/// The name to save a download under: the server's, else the address's
/// last part, else its host; made safe for the disk, with an extension from
/// the content type when it has none.
pub fn file_name(url: &Url, disposition: Option<&str>, content_type: Option<&str>) -> String {
    let from_server = disposition
        .map(content_disposition::parse_content_disposition)
        .and_then(|parsed| parsed.filename_full());
    let from_address = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|segment| !segment.is_empty())
        .map(|segment| percent_decode_str(segment).decode_utf8_lossy().into_owned());
    let named = [from_server, from_address]
        .into_iter()
        .flatten()
        .map(safe_name)
        .find(|name| !name.is_empty());
    // A host's dots are not an extension: "example.com" still needs one.
    let has_extension = named
        .as_deref()
        .is_some_and(|name| Path::new(name).extension().is_some());
    let mut name =
        named.unwrap_or_else(|| safe_name(url.host_str().unwrap_or("download")));
    if !has_extension
        && let Some(extension) = content_type.and_then(extension_for)
    {
        name.push('.');
        name.push_str(extension);
    }
    name
}

/// Safe on disk, and not hidden: a leading dot would hide it in Finder.
fn safe_name(name: impl AsRef<str>) -> String {
    sanitize_filename::sanitize(name)
        .trim_start_matches(['.', ' '])
        .trim_end()
        .to_owned()
}

/// The usual extension for a content type, unless it says nothing about the
/// file ("application/octet-stream").
fn extension_for(content_type: &str) -> Option<&'static str> {
    let essence = content_type.split(';').next()?.trim().to_ascii_lowercase();
    if essence.is_empty() || essence == "application/octet-stream" {
        return None;
    }
    mime_guess::get_mime_extensions_str(&essence)?.first().copied()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn the_server_names_the_file_first() {
        assert_eq!(
            file_name(
                &url("https://example.com/get?id=7"),
                Some("attachment; filename=\"Q3 report.pdf\""),
                Some("application/pdf"),
            ),
            "Q3 report.pdf"
        );
        assert_eq!(
            file_name(
                &url("https://example.com/get"),
                Some("attachment; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf"),
                None,
            ),
            "résumé.pdf"
        );
    }

    #[test]
    fn otherwise_the_address_names_it() {
        assert_eq!(
            file_name(&url("https://example.com/files/My%20App.dmg"), None, None),
            "My App.dmg"
        );
        assert_eq!(
            file_name(&url("https://example.com/"), None, Some("application/zip")),
            "example.com.zip"
        );
    }

    #[test]
    fn a_missing_extension_comes_from_the_type() {
        assert_eq!(
            file_name(
                &url("https://example.com/export"),
                None,
                Some("text/csv; charset=utf-8")
            ),
            "export.csv"
        );
        assert_eq!(
            file_name(
                &url("https://example.com/blob"),
                None,
                Some("application/octet-stream")
            ),
            "blob"
        );
    }

    #[test]
    fn names_are_made_safe() {
        assert_eq!(
            file_name(
                &url("https://example.com/x"),
                Some("attachment; filename=\"../../etc/passwd\""),
                None,
            ),
            "etcpasswd"
        );
        assert_eq!(
            file_name(
                &url("https://example.com/x"),
                Some("attachment; filename=\"a:b?.txt\""),
                None,
            ),
            "ab.txt"
        );
    }

    #[test]
    fn a_taken_name_gets_a_number() {
        let folder = Path::new("/downloads");
        let taken: HashSet<PathBuf> = ["report.pdf", "report (1).pdf", "notes"]
            .iter()
            .map(|name| folder.join(name))
            .collect();
        let is_taken = |path: &Path| taken.contains(path);
        assert_eq!(
            unique_path(folder, "photo.jpg", is_taken),
            folder.join("photo.jpg")
        );
        assert_eq!(
            unique_path(folder, "report.pdf", is_taken),
            folder.join("report (2).pdf")
        );
        assert_eq!(
            unique_path(folder, "notes", is_taken),
            folder.join("notes (1)")
        );
    }

    #[test]
    fn a_partial_file_holds_its_name_until_it_is_done() {
        let folder = std::env::temp_dir().join(format!("frame-downloads-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        let (first, mut partial) = reserve(&folder, "a.zip").unwrap();
        let (second, other) = reserve(&folder, "a.zip").unwrap();
        assert_eq!(first, folder.join("a.zip"));
        assert_eq!(second, folder.join("a (1).zip"));
        assert!(folder.join("a.zip.download").exists());

        partial.file.write_all(b"done").unwrap();
        partial.finish(&first).unwrap();
        drop(partial);
        assert_eq!(fs::read(&first).unwrap(), b"done");
        assert!(!folder.join("a.zip.download").exists());

        // An unfinished one leaves nothing behind.
        drop(other);
        assert!(!folder.join("a (1).zip.download").exists());
        assert!(!second.exists());
        let _ = fs::remove_dir_all(&folder);
    }
}
