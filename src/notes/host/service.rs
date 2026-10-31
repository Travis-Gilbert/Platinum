//! The one thread that owns a vault.
//!
//! `frame_notes::Vault` is single-threaded state: the authority's heads, the
//! index and the VFS journal. Servo asks for pages from its own threads and the
//! panel from the UI thread, and neither may block on disk or on a render, so
//! the vault lives on a worker thread of its own and both send it closures.
//! A request is a closure on a bounded queue and an answer is a oneshot, so no
//! lock is shared and the UI thread never waits: it awaits.
//!
//! The queue is bounded. A request that finds it full is refused as `Busy`
//! rather than held back, and a vault that failed to open answers every
//! request with the reason: callers show an honest unavailable state.
//!
//! SOURCING: `notify` is the watcher the VFS builds. The VFS keeps its events
//! to itself and offers no wake-up, so this watches only to know when to call
//! `Vault::refresh`; `tokio::sync::watch` carries the change count to any
//! number of listeners, on any executor.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, PoisonError, RwLock, Weak};

use anyhow::Result;
use frame_notes::{Vault, is_hidden};
use futures::channel::oneshot;
use notify::{EventKind, RecursiveMode, Watcher as _};
use tokio::sync::watch;

use super::shell::Chrome;

/// How many requests may wait for the vault.
const QUEUE: usize = 32;

/// What a request finds: the vault, or why there is none.
pub enum Slot<'a> {
    Ready(&'a mut Vault),
    Unavailable(&'a str),
}

enum Job {
    /// Work for the vault, answering through the closure itself. `write` marks
    /// work that may have changed the vault.
    Run {
        work: Box<dyn FnOnce(Slot) + Send>,
        write: bool,
    },
    /// Files changed on disk: reconcile.
    Refresh,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The queue is full.
    Busy,
    /// The vault's thread has ended.
    Closed,
    /// The vault could not be opened.
    Unavailable(String),
    /// The request itself failed.
    Failed(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => f.write_str("the vault is busy"),
            Self::Closed => f.write_str("the vault is closed"),
            Self::Unavailable(reason) => write!(f, "the vault could not be opened: {reason}"),
            Self::Failed(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for Error {}

struct Inner {
    jobs: SyncSender<Job>,
    /// A refresh is queued, so more file events do not queue another.
    refresh_queued: AtomicBool,
    /// Counts the changes the vault has made or seen.
    changes: watch::Receiver<u64>,
    host: String,
    root: PathBuf,
    /// How many requests have been made of the vault, for tests of what never
    /// reaches it.
    #[cfg(test)]
    submitted: AtomicUsize,
}

/// A handle on a vault's thread. Cheap to clone; the thread ends, and the
/// vault is closed, when the last handle is dropped.
#[derive(Clone)]
pub struct Service {
    inner: Arc<Inner>,
}

/// The `frame-notes:` host of a vault: stable for its location, so a page's
/// address means the same vault after a restart.
pub fn host_of(root: &Path) -> String {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    blake3::hash(root.to_string_lossy().as_bytes()).to_hex()[..16].to_owned()
}

impl Service {
    /// Opens the vault at `root` on a new thread, keeping its derived state in
    /// `data`. This returns at once; requests wait in the queue until the
    /// vault is open.
    pub fn start(root: &Path, data: &Path) -> Self {
        let (jobs, queue) = sync_channel(QUEUE);
        let (changed, changes) = watch::channel(0u64);
        let inner = Arc::new(Inner {
            jobs,
            refresh_queued: AtomicBool::new(false),
            changes,
            host: host_of(root),
            root: root.to_path_buf(),
            #[cfg(test)]
            submitted: AtomicUsize::new(0),
        });
        let watcher = watch_root(root, Arc::downgrade(&inner));
        let (root, data) = (root.to_path_buf(), data.to_path_buf());
        std::thread::Builder::new()
            .name("notes-vault".into())
            .spawn({
                let inner = Arc::downgrade(&inner);
                move || run(&root, &data, queue, inner, changed, watcher)
            })
            .expect("a thread can be spawned");
        Self { inner }
    }

    /// The host of this vault's `frame-notes:` addresses.
    pub fn host(&self) -> &str {
        &self.inner.host
    }

    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    /// How many requests have been made of the vault.
    #[cfg(test)]
    pub fn submitted(&self) -> usize {
        self.inner.submitted.load(Ordering::Acquire)
    }

    /// Changes whenever the vault may have changed: after a write, and after
    /// files changed on disk and were reconciled.
    pub fn changes(&self) -> watch::Receiver<u64> {
        self.inner.changes.clone()
    }

    /// Runs `work` on the vault's thread, for an answer that changes nothing.
    pub async fn read<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut Vault) -> Result<T> + Send + 'static,
    ) -> Result<T, Error> {
        self.enqueue(work, false)?
            .await
            .map_err(|_| Error::Closed)?
    }

    /// Runs `work` on the vault's thread; the vault may change, so listeners
    /// to `changes` are woken after it.
    pub async fn write<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut Vault) -> Result<T> + Send + 'static,
    ) -> Result<T, Error> {
        self.enqueue(work, true)?.await.map_err(|_| Error::Closed)?
    }

    fn enqueue<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut Vault) -> Result<T> + Send + 'static,
        write: bool,
    ) -> Result<oneshot::Receiver<Result<T, Error>>, Error> {
        #[cfg(test)]
        self.inner.submitted.fetch_add(1, Ordering::AcqRel);
        let (reply, answer) = oneshot::channel();
        let job = Job::Run {
            work: Box::new(move |slot| {
                let result = match slot {
                    Slot::Ready(vault) => {
                        work(vault).map_err(|error| Error::Failed(format!("{error:#}")))
                    }
                    Slot::Unavailable(reason) => Err(Error::Unavailable(reason.to_owned())),
                };
                let _ = reply.send(result);
            }),
            write,
        };
        match self.inner.jobs.try_send(job) {
            Ok(()) => Ok(answer),
            Err(TrySendError::Full(_)) => Err(Error::Busy),
            Err(TrySendError::Disconnected(_)) => Err(Error::Closed),
        }
    }
}

/// The vaults that are open, by the host of their `frame-notes:` addresses,
/// and the app's theme their pages take. Vaults open while the app runs and
/// the protocol handler is registered once, so both share this; a lookup takes
/// the lock only to clone a handle.
#[derive(Clone, Default)]
pub struct Vaults {
    services: Arc<RwLock<HashMap<String, Service>>>,
    chrome: Arc<RwLock<Option<Chrome>>>,
}

impl Vaults {
    /// The vault at `root`, opened if it is not open already.
    pub fn open(&self, root: &Path, data: &Path) -> Service {
        self.services
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(host_of(root))
            .or_insert_with(|| Service::start(root, data))
            .clone()
    }

    pub fn get(&self, host: &str) -> Option<Service> {
        self.services
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(host)
            .cloned()
    }

    /// The app's theme, which the pages served from now on take.
    pub fn set_chrome(&self, chrome: Chrome) {
        *self.chrome.write().unwrap_or_else(PoisonError::into_inner) = Some(chrome);
    }

    pub fn chrome(&self) -> Option<Chrome> {
        *self.chrome.read().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Watches the vault's folder and asks for a refresh when something other
/// than a hidden folder changes. A watcher that cannot start leaves the vault
/// to be refreshed by writes only.
fn watch_root(root: &Path, inner: Weak<Inner>) -> Option<notify::RecommendedWatcher> {
    let base = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut watcher =
        match notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            let Ok(event) = event else { return };
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            let visible = event.paths.iter().any(|path| {
                path.strip_prefix(&base)
                    .ok()
                    .and_then(Path::to_str)
                    .is_some_and(|relative| !is_hidden(relative))
            });
            let Some(inner) = inner.upgrade().filter(|_| visible) else {
                return;
            };
            if !inner.refresh_queued.swap(true, Ordering::AcqRel)
                && inner.jobs.try_send(Job::Refresh).is_err()
            {
                inner.refresh_queued.store(false, Ordering::Release);
            }
        }) {
            Ok(watcher) => watcher,
            Err(error) => {
                log::warn!("notes: not watching {}: {error}", root.display());
                return None;
            }
        };
    if let Err(error) = watcher.watch(root, RecursiveMode::Recursive) {
        log::warn!("notes: not watching {}: {error}", root.display());
        return None;
    }
    Some(watcher)
}

fn run(
    root: &Path,
    data: &Path,
    queue: Receiver<Job>,
    inner: Weak<Inner>,
    changed: watch::Sender<u64>,
    _watcher: Option<notify::RecommendedWatcher>,
) {
    let mut vault = Vault::open(root, data).map_err(|error| format!("{error:#}"));
    if let Err(reason) = &vault {
        log::error!("notes: {}: {reason}", root.display());
    }
    let bump = || changed.send_modify(|count| *count += 1);
    for job in queue {
        match job {
            Job::Run { work, write } => {
                work(match &mut vault {
                    Ok(vault) => Slot::Ready(vault),
                    Err(reason) => Slot::Unavailable(reason),
                });
                if write {
                    bump();
                }
            }
            Job::Refresh => {
                if let Some(inner) = inner.upgrade() {
                    // Cleared first: an event during the refresh queues another.
                    inner.refresh_queued.store(false, Ordering::Release);
                }
                if let Ok(vault) = &mut vault {
                    if let Err(error) = vault.refresh() {
                        log::warn!("notes: refresh failed: {error:#}");
                    }
                    bump();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;

    fn vault(files: &[(&str, &str)]) -> (tempfile::TempDir, tempfile::TempDir, Service) {
        let root = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        for (path, text) in files {
            std::fs::write(root.path().join(path), text).unwrap();
        }
        let service = Service::start(root.path(), data.path());
        (root, data, service)
    }

    #[test]
    fn requests_run_on_the_vaults_thread_and_return_owned_answers() {
        let (_root, _data, service) = vault(&[("Home.md", "hi")]);
        let name = block_on(service.read(|_| Ok(std::thread::current().name().map(str::to_owned))))
            .unwrap();
        assert_eq!(name.as_deref(), Some("notes-vault"));
        let text =
            block_on(service.read(|vault| Ok(vault.open_document("Home.md")?.text))).unwrap();
        assert_eq!(text, "hi");
    }

    #[test]
    fn a_failed_request_is_an_answer_not_a_dead_thread() {
        let (_root, _data, service) = vault(&[]);
        let error =
            block_on(service.read(|vault| vault.open_document("Missing.md").map(|d| d.text)))
                .unwrap_err();
        assert!(matches!(error, Error::Failed(_)), "{error:?}");
        assert!(block_on(service.read(|_| Ok(1))).is_ok());
    }

    #[test]
    fn a_vault_that_cannot_open_answers_every_request_with_why() {
        let data = tempfile::tempdir().unwrap();
        let service = Service::start(Path::new("/definitely/not/a/vault"), data.path());
        for _ in 0..2 {
            let error = block_on(service.read(|_| Ok(()))).unwrap_err();
            assert!(matches!(error, Error::Unavailable(_)), "{error:?}");
        }
    }

    #[test]
    fn only_writes_wake_the_listeners() {
        let (_root, _data, service) = vault(&[("Home.md", "hi")]);
        let changes = service.changes();
        block_on(service.read(|_| Ok(()))).unwrap();
        assert_eq!(*changes.borrow(), 0);
        block_on(service.write(|_| Ok(()))).unwrap();
        assert_eq!(*changes.borrow(), 1);
    }

    #[test]
    fn a_full_queue_refuses_instead_of_waiting() {
        let (_root, _data, service) = vault(&[]);
        let (hold, held) = std::sync::mpsc::channel::<()>();
        // Block the thread, then fill the queue behind it.
        let blocker = service
            .enqueue(move |_| Ok(held.recv().is_ok()), false)
            .unwrap();
        let mut accepted = 0;
        let mut refusal = None;
        for _ in 0..QUEUE + 4 {
            match service.enqueue(|_| Ok(()), false) {
                Ok(_) => accepted += 1,
                Err(error) => {
                    refusal = Some(error);
                    break;
                }
            }
        }
        assert_eq!(refusal, Some(Error::Busy));
        assert!(accepted >= QUEUE - 1, "{accepted}");
        drop(hold);
        assert_eq!(block_on(blocker), Ok(Ok(false)));
    }

    #[test]
    fn a_vault_is_opened_once_and_found_by_its_host() {
        let root = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let vaults = Vaults::default();
        let first = vaults.open(root.path(), data.path());
        let second = vaults.open(root.path(), data.path());
        assert!(Arc::ptr_eq(&first.inner, &second.inner));
        let found = vaults.get(first.host()).unwrap();
        assert!(Arc::ptr_eq(&first.inner, &found.inner));
        assert!(vaults.get("0000000000000000").is_none());
    }

    #[test]
    fn a_vaults_host_is_stable_and_differs_between_vaults() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        assert_eq!(host_of(a.path()), host_of(a.path()));
        assert_ne!(host_of(a.path()), host_of(b.path()));
        assert!(
            host_of(a.path())
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        );
    }
}
