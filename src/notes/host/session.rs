//! One reading page's conversation with the host.
//!
//! The page shows a note. What it shows is derived state: the authority (the
//! vault, on its own thread) owns the text and the revision, and the page's
//! rendering of it is replaced whenever the authority moves on. This module
//! owns what is neither: which note the page is on, the epoch of the page
//! load (`Tracker`), the callout folds the person chose (view state, kept per
//! note), and a notice for the person when something did not work.
//!
//! The page talks back through navigation requests: it sets `location` to
//! `frame-notes://<vault>/act/<verb>?t=<token>&...`. `NotesDelegate` answers
//! the request on Servo's turn of the loop, queues the act, and refuses the
//! navigation, so the document is never replaced. It cannot touch GPUI there
//! (it runs inside `spin_event_loop`), so the queue is drained on a task that
//! a channel wakes: `Session::drain`.
//!
//! An act is trusted only with this page load's token, from the top frame,
//! as a script's own navigation. The token is in the page's configuration and
//! nowhere in a note's content, so a link a note carries cannot make one.
//!
//! SOURCING: Servo's `WebViewDelegate` and `NavigationRequest`; GPUI's
//! entities and foreground tasks; `futures` channels carry the wake.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::rc::Rc;

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui_kit::{AppContext, ClipboardItem, Context, Entity, EventEmitter, Task};
use servo::{
    ConsoleLogLevel, CreateNewWebViewRequest, Cursor, EmbedderControl, EmbedderControlId,
    InputEventId, InputEventResult, JSValue, LoadStatus, NavigationRequest, NavigationType,
    WebView, WebViewDelegate,
};
use url::Url;

use super::ops::{self, Opened, Rendered, Toggled};
use super::patch::{Decision, Folds, Tracker};
use super::route::{self, Act, SCHEME};
use super::service::{Error, Service};
use super::shell::Chrome;

/// What the current page load accepts.
pub struct Gate {
    host: String,
    token: String,
    /// The page's own address, so a jump within it is not a navigation away.
    page: Option<Url>,
}

/// What to do with a navigation request.
#[derive(Debug, PartialEq)]
pub enum Disposition {
    /// A message from the page.
    Act(Act),
    /// A jump within the page's own document.
    SameDocument,
    Refuse(String),
}

fn without_fragment(url: &Url) -> Url {
    let mut url = url.clone();
    url.set_fragment(None);
    url
}

/// Sorts a navigation of the reading page. Nothing navigates the page to
/// another document: notes open through acts, and the web opens elsewhere.
pub fn classify(url: &Url, main_frame: bool, kind: NavigationType, gate: &Gate) -> Disposition {
    if url.scheme() != SCHEME || url.host_str() != Some(gate.host.as_str()) {
        return Disposition::Refuse(format!("the reading page does not navigate to {url}"));
    }
    if url.path().starts_with("/act/") {
        if !main_frame || kind != NavigationType::Other {
            return Disposition::Refuse(format!(
                "a message arrived as a {kind:?} navigation, main frame {main_frame}"
            ));
        }
        return match Act::parse(url, &gate.host, &gate.token) {
            Ok(act) => Disposition::Act(act),
            Err(error) => {
                Disposition::Refuse(format!("not a message of this page load: {error:?}"))
            }
        };
    }
    match &gate.page {
        Some(page) if without_fragment(page) == without_fragment(url) => Disposition::SameDocument,
        _ => Disposition::Refuse(format!("the reading page does not navigate to {url}")),
    }
}

/// Something the page's web view told the host, queued for the session.
enum Inbound {
    Act(Act),
    /// The page answered a patch.
    Applied {
        ticket: u64,
        applied: bool,
    },
}

/// The queue between Servo's turn of the loop and the session. Both ends are
/// on the main thread.
struct Inbox {
    queue: RefCell<VecDeque<Inbound>>,
    wake: UnboundedSender<()>,
}

impl Inbox {
    fn push(&self, item: Inbound) {
        self.queue.borrow_mut().push_back(item);
        let _ = self.wake.unbounded_send(());
    }
}

/// The web view's delegate: answers the page's messages and forwards
/// everything else to the embedder's own delegate.
pub struct NotesDelegate {
    inner: Rc<dyn WebViewDelegate>,
    gate: Rc<RefCell<Gate>>,
    inbox: Rc<Inbox>,
}

impl WebViewDelegate for NotesDelegate {
    fn notify_new_frame_ready(&self, webview: WebView) {
        self.inner.notify_new_frame_ready(webview);
    }

    fn notify_cursor_changed(&self, webview: WebView, cursor: Cursor) {
        self.inner.notify_cursor_changed(webview, cursor);
    }

    fn notify_input_event_handled(
        &self,
        webview: WebView,
        id: InputEventId,
        result: InputEventResult,
    ) {
        self.inner.notify_input_event_handled(webview, id, result);
    }

    fn notify_favicon_changed(&self, webview: WebView) {
        self.inner.notify_favicon_changed(webview);
    }

    fn notify_page_title_changed(&self, webview: WebView, title: Option<String>) {
        self.inner.notify_page_title_changed(webview, title);
    }

    fn notify_url_changed(&self, webview: WebView, url: Url) {
        self.inner.notify_url_changed(webview, url);
    }

    fn notify_load_status_changed(&self, webview: WebView, status: LoadStatus) {
        self.inner.notify_load_status_changed(webview, status);
    }

    fn notify_history_changed(&self, webview: WebView, urls: Vec<Url>, current: usize) {
        self.inner.notify_history_changed(webview, urls, current);
    }

    fn notify_status_text_changed(&self, webview: WebView, status: Option<String>) {
        self.inner.notify_status_text_changed(webview, status);
    }

    fn notify_crashed(&self, webview: WebView, reason: String, backtrace: Option<String>) {
        self.inner.notify_crashed(webview, reason, backtrace);
    }

    fn notify_closed(&self, webview: WebView) {
        self.inner.notify_closed(webview);
    }

    fn show_console_message(&self, webview: WebView, level: ConsoleLogLevel, message: String) {
        self.inner.show_console_message(webview, level, message);
    }

    fn request_create_new(&self, parent: WebView, request: CreateNewWebViewRequest) {
        self.inner.request_create_new(parent, request);
    }

    fn show_embedder_control(&self, webview: WebView, control: EmbedderControl) {
        self.inner.show_embedder_control(webview, control);
    }

    fn hide_embedder_control(&self, webview: WebView, control_id: EmbedderControlId) {
        self.inner.hide_embedder_control(webview, control_id);
    }

    fn request_navigation(&self, _: WebView, request: NavigationRequest) {
        let disposition = classify(
            &request.url,
            request.action.is_main_frame,
            request.action.navigation_type,
            &self.gate.borrow(),
        );
        match disposition {
            Disposition::Act(act) => {
                self.inbox.push(Inbound::Act(act));
                request.deny();
            }
            Disposition::SameDocument => request.allow(),
            Disposition::Refuse(reason) => {
                log::warn!(target: "notes", "{reason}");
                request.deny();
            }
        }
    }
}

/// The pieces that exist before the web view does: the web view is built with
/// the delegate, and the session is started with the web view.
pub struct Wiring {
    delegate: Rc<NotesDelegate>,
    gate: Rc<RefCell<Gate>>,
    inbox: Rc<Inbox>,
    wakes: UnboundedReceiver<()>,
    /// The note the web view is built on, once `first_page` has named it.
    first: Option<String>,
}

impl Wiring {
    /// `inner` receives everything but the page's messages.
    pub fn new(host: &str, inner: Rc<dyn WebViewDelegate>) -> Self {
        let (wake, wakes) = unbounded();
        let gate = Rc::new(RefCell::new(Gate {
            host: host.to_owned(),
            token: String::new(),
            page: None,
        }));
        let inbox = Rc::new(Inbox {
            queue: RefCell::default(),
            wake,
        });
        let delegate = Rc::new(NotesDelegate {
            inner,
            gate: gate.clone(),
            inbox: inbox.clone(),
        });
        Self {
            delegate,
            gate,
            inbox,
            wakes,
            first: None,
        }
    }

    pub fn delegate(&self) -> Rc<NotesDelegate> {
        self.delegate.clone()
    }

    /// The address to build the web view on, for the note at `path`; the
    /// session then starts on that page. A page asked for after `build()` can
    /// be lost: until the web view's first page is up, Servo has no browsing
    /// context to load into and drops the request, so the first note is the
    /// builder's (`WebViewBuilder::url`), and every later one the session's.
    pub fn first_page(&mut self, path: &str) -> anyhow::Result<Url> {
        let token =
            new_token().ok_or_else(|| anyhow::anyhow!("no token could be made for the page"))?;
        let url = route::note_url(&self.gate.borrow().host, path, &token, None);
        {
            let mut gate = self.gate.borrow_mut();
            gate.token = token;
            gate.page = Some(url.clone());
        }
        self.first = Some(path.to_owned());
        Ok(url)
    }
}

/// What the panel has to show instead of a page, if there is no page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Finding the note to show.
    Opening,
    Showing,
    /// The vault has no notes.
    Empty,
    /// The vault could not be opened or answered.
    Unavailable(String),
}

/// What the owner of the session has to act on.
pub enum SessionEvent {
    /// A link to the web was followed. The session does not decide where the
    /// web opens.
    OpenWeb(Url),
}

struct Current {
    path: String,
    tracker: Tracker,
}

pub struct Session {
    vault: Service,
    webview: WebView,
    gate: Rc<RefCell<Gate>>,
    inbox: Rc<Inbox>,
    current: Option<Current>,
    /// The folds of the notes the page is not on, kept for when it returns.
    folds: HashMap<String, Folds>,
    status: Status,
    notice: Option<String>,
    /// The app's theme as the page was last told it, told again to every page
    /// load: a page served before a theme change may not have its script yet.
    chrome: Option<Chrome>,
    _tasks: [Task<()>; 2],
}

impl EventEmitter<SessionEvent> for Session {}

impl Session {
    /// Starts the conversation for `webview`, which was built with `wiring`'s
    /// delegate. The page is not loaded until `show`.
    pub fn start(
        wiring: Wiring,
        webview: WebView,
        vault: Service,
        cx: &mut impl AppContext,
    ) -> Entity<Self> {
        let Wiring {
            gate,
            inbox,
            mut wakes,
            first,
            ..
        } = wiring;
        cx.new(|cx: &mut Context<Self>| {
            let inbox_task = cx.spawn(async move |this, cx| {
                while wakes.next().await.is_some() {
                    if this.update(cx, |session, cx| session.drain(cx)).is_err() {
                        break;
                    }
                }
            });
            let mut changes = vault.changes();
            let changes_task = cx.spawn(async move |this, cx| {
                while changes.changed().await.is_ok() {
                    if this.update(cx, |session, cx| session.refresh(cx)).is_err() {
                        break;
                    }
                }
            });
            Self {
                vault,
                webview,
                gate,
                inbox,
                status: if first.is_some() {
                    Status::Showing
                } else {
                    Status::Opening
                },
                current: first.map(|path| Current {
                    path,
                    tracker: Tracker::new(Folds::default()),
                }),
                folds: HashMap::new(),
                notice: None,
                chrome: None,
                _tasks: [inbox_task, changes_task],
            }
        })
    }

    pub fn webview(&self) -> &WebView {
        &self.webview
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    /// Something the person should know, until the next note opens.
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// The vault-relative path of the note the page is on.
    pub fn path(&self) -> Option<&str> {
        self.current.as_ref().map(|current| current.path.as_str())
    }

    /// The note's name as a title.
    pub fn title(&self) -> String {
        match self.path() {
            Some(path) => {
                let name = path.rsplit('/').next().unwrap_or(path);
                Path::new(name)
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or(name)
                    .to_owned()
            }
            None => "Notes".to_owned(),
        }
    }

    /// Opens `path`, or the vault's home note when none is named.
    pub fn show(&mut self, path: Option<String>, fragment: Option<String>, cx: &mut Context<Self>) {
        if let Some(path) = path {
            self.navigate(path, fragment, cx);
            return;
        }
        self.status = Status::Opening;
        cx.notify();
        let vault = self.vault.clone();
        cx.spawn(async move |this, cx| {
            let first = vault.read(|vault| Ok(ops::first_note(vault))).await;
            let _ = this.update(cx, |session, cx| match first {
                Ok(Some(path)) => session.navigate(path, None, cx),
                Ok(None) => {
                    session.status = Status::Empty;
                    cx.notify();
                }
                Err(error) => {
                    session.status = Status::Unavailable(error.to_string());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn navigate(&mut self, path: String, fragment: Option<String>, cx: &mut Context<Self>) {
        let Some(token) = new_token() else {
            self.status = Status::Unavailable("no token could be made for the page".into());
            cx.notify();
            return;
        };
        let next = self.folds.remove(&path).unwrap_or_default();
        match &mut self.current {
            Some(current) => {
                // Nothing the old page says or answers is heard from here on.
                current.tracker.reset();
                let left = current.tracker.swap_folds(next);
                let left_path = std::mem::replace(&mut current.path, path.clone());
                self.folds.insert(left_path, left);
            }
            None => {
                self.current = Some(Current {
                    path: path.clone(),
                    tracker: Tracker::new(next),
                });
            }
        }
        let url = route::note_url(&self.gate.borrow().host, &path, &token, fragment.as_deref());
        {
            let mut gate = self.gate.borrow_mut();
            gate.token = token;
            gate.page = Some(url.clone());
        }
        self.status = Status::Showing;
        self.notice = None;
        self.webview.load(url);
        cx.notify();
    }

    fn drain(&mut self, cx: &mut Context<Self>) {
        let inbound: Vec<Inbound> = self.inbox.queue.borrow_mut().drain(..).collect();
        for item in inbound {
            match item {
                Inbound::Act(act) => self.act(act, cx),
                Inbound::Applied { ticket, applied } => {
                    let again = self
                        .current
                        .as_mut()
                        .is_some_and(|current| current.tracker.applied(ticket, applied));
                    if again {
                        self.refresh(cx);
                    }
                }
            }
        }
    }

    fn act(&mut self, act: Act, cx: &mut Context<Self>) {
        let Some(current) = &mut self.current else {
            return;
        };
        let epoch = current.tracker.ticket();
        let path = current.path.clone();
        match act {
            Act::Ready { revision, digest } => {
                // The script is running: whatever was known of an earlier
                // load of this page no longer holds.
                current.tracker.reset();
                current.tracker.ready(revision, digest);
                let scripts = loaded_scripts(current.tracker.folds(), self.chrome.as_ref());
                for script in scripts {
                    self.run(script);
                }
                self.refresh(cx);
            }
            Act::Open { link } => {
                let vault = self.vault.clone();
                cx.spawn(async move |this, cx| {
                    let opened = vault
                        .write(move |vault| ops::open_link(vault, &path, &link))
                        .await;
                    let _ = this.update(cx, |session, cx| session.opened(opened, epoch, cx));
                })
                .detach();
            }
            Act::Web { url } => cx.emit(SessionEvent::OpenWeb(url)),
            Act::Copy { text } => cx.write_to_clipboard(ClipboardItem::new_string(text)),
            Act::Fold {
                key,
                collapsed,
                seq,
            } => {
                current.tracker.fold(key, collapsed, seq);
            }
            Act::Task {
                line,
                checked,
                revision,
            } => {
                let vault = self.vault.clone();
                cx.spawn(async move |this, cx| {
                    let toggled = vault
                        .write(move |vault| ops::toggle_task(vault, &path, line, checked, revision))
                        .await;
                    let _ = this.update(cx, |session, cx| session.toggled(toggled, epoch, cx));
                })
                .detach();
            }
            Act::AllowMermaid => {
                let vault = self.vault.clone();
                cx.spawn(async move |this, cx| {
                    let allowed = vault.write(|vault| ops::allow_mermaid(vault)).await;
                    let _ = this.update(cx, |session, cx| match allowed {
                        Ok(()) => session.run("frameNotes.allowMermaid()".into()),
                        Err(error) => {
                            session.tell(format!("Diagrams were not allowed: {error}"), cx)
                        }
                    });
                })
                .detach();
            }
        }
    }

    /// A followed link was resolved. An answer for a page that has been
    /// replaced since is dropped.
    fn opened(&mut self, opened: Result<Opened, Error>, epoch: u64, cx: &mut Context<Self>) {
        if self
            .current
            .as_ref()
            .map(|current| current.tracker.ticket())
            != Some(epoch)
        {
            return;
        }
        match opened {
            Ok(Opened::Note { path, fragment, .. }) => self.navigate(path, fragment, cx),
            Ok(Opened::Other(path)) => self.tell(
                format!("{path} is not a note; the reading view shows notes."),
                cx,
            ),
            Err(error) => self.tell(format!("The link did not open: {error}"), cx),
        }
    }

    /// A checkbox was toggled. Whatever came of it, the page shows the
    /// authority's text again: the page's own change is never the record.
    fn toggled(&mut self, toggled: Result<Toggled, Error>, epoch: u64, cx: &mut Context<Self>) {
        let Some(current) = &mut self.current else {
            return;
        };
        if current.tracker.ticket() != epoch {
            return;
        }
        let refused = match toggled {
            Ok(Toggled::Done { .. }) => None,
            Ok(Toggled::Stale { .. }) => {
                Some("The note changed before the task was toggled; nothing was saved.".to_owned())
            }
            Ok(Toggled::NoTask) => Some("That task is no longer in the note.".to_owned()),
            Err(error) => Some(format!("The task was not saved: {error}")),
        };
        if let Some(notice) = refused {
            current.tracker.distrust();
            self.tell(notice, cx);
        }
        self.refresh(cx);
    }

    /// The app's theme changed: the page on screen is given it, and so is
    /// every page loaded from now on.
    pub fn restyle(&mut self, chrome: Chrome) {
        self.chrome = Some(chrome);
        if self.current.is_some() {
            self.run(chrome.script());
        }
    }

    fn tell(&mut self, notice: String, cx: &mut Context<Self>) {
        log::warn!(target: "notes", "{notice}");
        self.notice = Some(notice);
        cx.notify();
    }

    fn run(&self, script: String) {
        self.webview.evaluate_javascript(script, |result| {
            if let Err(error) = result {
                log::debug!(target: "notes", "the page could not run a script: {error:?}");
            }
        });
    }

    /// The note may have changed: render it, and patch the page if it shows
    /// something else.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(current) = &mut self.current else {
            return;
        };
        let Some(ticket) = current.tracker.begin() else {
            return;
        };
        let (vault, path) = (self.vault.clone(), current.path.clone());
        cx.spawn(async move |this, cx| {
            let rendered = vault.read(move |vault| ops::render(vault, &path)).await;
            let _ = this.update(cx, |session, cx| session.rendered(ticket, rendered, cx));
        })
        .detach();
    }

    fn rendered(&mut self, ticket: u64, rendered: Result<Rendered, Error>, cx: &mut Context<Self>) {
        let Some(current) = &mut self.current else {
            return;
        };
        if let Err(error) = &rendered {
            log::warn!(target: "notes", "{} could not be rendered: {error}", current.path);
        }
        match current.tracker.decide(ticket, rendered.as_ref().ok()) {
            Decision::Send(patch) => {
                let inbox = self.inbox.clone();
                self.webview
                    .evaluate_javascript(patch.script(), move |result| {
                        inbox.push(Inbound::Applied {
                            ticket,
                            applied: matches!(result, Ok(JSValue::Boolean(true))),
                        });
                    });
            }
            Decision::Skip { again: true } => self.refresh(cx),
            Decision::Skip { again: false } | Decision::Stale => {}
        }
    }
}

/// What a page that has just loaded is told, in order: the folds the person
/// chose in this note, then the app's theme.
fn loaded_scripts(folds: &Folds, chrome: Option<&Chrome>) -> Vec<String> {
    folds
        .restore_script()
        .into_iter()
        .chain(chrome.map(Chrome::script))
        .collect()
}

/// A page load's token: unguessable by the notes that page shows.
fn new_token() -> Option<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).ok()?;
    Some(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate() -> Gate {
        Gate {
            host: "abc".into(),
            token: "tok".into(),
            page: Some(Url::parse("frame-notes://abc/note/A.md?k=tok").unwrap()),
        }
    }

    fn classified(url: &str, main_frame: bool, kind: NavigationType) -> Disposition {
        classify(&Url::parse(url).unwrap(), main_frame, kind, &gate())
    }

    #[test]
    fn a_script_navigation_with_the_token_is_the_pages_message() {
        assert_eq!(
            classified(
                "frame-notes://abc/act/open?t=tok&link=Other%23Head",
                true,
                NavigationType::Other
            ),
            Disposition::Act(Act::Open {
                link: "Other#Head".into()
            })
        );
    }

    #[test]
    fn a_message_without_this_loads_token_is_refused() {
        for url in [
            "frame-notes://abc/act/open?link=x",
            "frame-notes://abc/act/open?t=old&link=x",
            "frame-notes://abc/act/open?t=&link=x",
        ] {
            assert!(
                matches!(
                    classified(url, true, NavigationType::Other),
                    Disposition::Refuse(_)
                ),
                "{url}"
            );
        }
    }

    #[test]
    fn a_link_a_note_carries_cannot_send_a_message() {
        // A note cannot know the token, and a click is a link activation.
        let link = "frame-notes://abc/act/task?t=tok&line=1&checked=1&revision=1";
        for kind in [
            NavigationType::LinkActivated,
            NavigationType::FormSubmitted,
            NavigationType::Reload,
        ] {
            assert!(
                matches!(classified(link, true, kind), Disposition::Refuse(_)),
                "{kind:?}"
            );
        }
        // Nor can a frame inside the page.
        assert!(matches!(
            classified(link, false, NavigationType::Other),
            Disposition::Refuse(_)
        ));
    }

    #[test]
    fn another_vaults_pages_and_the_web_are_not_navigated_to() {
        for url in [
            "frame-notes://other/act/open?t=tok&link=x",
            "frame-notes://other/note/A.md?k=tok",
            "frame-notes://abc/note/B.md?k=tok",
            "https://example.org/",
            "file:///etc/passwd",
            "javascript:alert(1)",
        ] {
            assert!(
                matches!(
                    classified(url, true, NavigationType::Other),
                    Disposition::Refuse(_)
                ),
                "{url}"
            );
        }
    }

    #[test]
    fn a_jump_within_the_page_and_a_reload_of_it_stay_on_it() {
        assert_eq!(
            classified(
                "frame-notes://abc/note/A.md?k=tok#Heading",
                true,
                NavigationType::LinkActivated
            ),
            Disposition::SameDocument
        );
        assert_eq!(
            classified(
                "frame-notes://abc/note/A.md?k=tok",
                true,
                NavigationType::Reload
            ),
            Disposition::SameDocument
        );
    }

    #[test]
    fn a_page_that_has_not_been_loaded_accepts_no_navigation() {
        let mut gate = gate();
        gate.page = None;
        let url = Url::parse("frame-notes://abc/note/A.md?k=tok").unwrap();
        assert!(matches!(
            classify(&url, true, NavigationType::Other, &gate),
            Disposition::Refuse(_)
        ));
    }

    #[test]
    fn a_loaded_page_is_given_its_folds_then_the_apps_theme() {
        let mut folds = Folds::default();
        folds.record("k".into(), true, 2);
        let chrome = Chrome::sample(true);
        let scripts = loaded_scripts(&folds, Some(&chrome));
        assert_eq!(scripts.len(), 2);
        assert!(scripts[0].starts_with("frameNotes.restore("));
        assert!(scripts[1].starts_with("frameNotes.chrome("));
        // Nothing remembered and no theme: nothing to say.
        assert!(loaded_scripts(&Folds::default(), None).is_empty());
        assert_eq!(loaded_scripts(&Folds::default(), Some(&chrome)).len(), 1);
    }

    struct Nobody;
    impl WebViewDelegate for Nobody {}

    #[test]
    fn the_first_page_is_named_before_the_web_view_exists_and_is_the_one_the_gate_expects() {
        let mut wiring = Wiring::new("abc", Rc::new(Nobody));
        let url = wiring.first_page("folder/Note.md").unwrap();
        assert_eq!(url.scheme(), SCHEME);
        assert_eq!(url.host_str(), Some("abc"));
        let token = url
            .query_pairs()
            .find_map(|(key, value)| (key == "k").then(|| value.into_owned()))
            .expect("the page's address carries its token");
        {
            let gate = wiring.gate.borrow();
            assert_eq!(gate.token, token);
            assert_eq!(gate.page.as_ref(), Some(&url));
        }
        assert_eq!(wiring.first.as_deref(), Some("folder/Note.md"));
        // A page's own jump within it is not a navigation away, as on any page.
        let mut jump = url.clone();
        jump.set_fragment(Some("Heading"));
        assert!(matches!(
            classify(
                &jump,
                true,
                NavigationType::LinkActivated,
                &wiring.gate.borrow()
            ),
            Disposition::SameDocument
        ));
    }

    #[test]
    fn tokens_are_long_and_different_each_time() {
        let (a, b) = (new_token().unwrap(), new_token().unwrap());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }
}
