//! What Servo tells Platinum about one tab's web view, queued for that tab's
//! page, plus the one decision made on the spot: where a navigation may go.
//!
//! SOURCING: Servo's `WebViewDelegate`. Adapted from Theorem UI's desktop host
//! (`desktop/src/delegate.rs`): there is one delegate per tab here, and a new
//! window (`window.open`, `target="_blank"`, "Open link in new tab") becomes a
//! Platinum tab instead of going to the system browser.
//! The navigation policy is ours: it is this app's rule, not a library's.
//! So is the list of file types that are downloads: Servo cannot report a
//! download, so a link to an installer or an archive is caught here, before
//! Servo fetches it, and handed to `downloads.rs`.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use dpi::PhysicalSize;
use servo::{
    ConsoleLogLevel, CreateNewWebViewRequest, Cursor, EmbedderControl, EmbedderControlId,
    InputEventId, InputEventResult, LoadStatus, NavigationRequest, WebView, WebViewDelegate,
};
use url::Url;

use crate::scene::SceneRenderingContext;
use crate::search::VisitedPage;
use crate::site::SiteInfo;

/// One thing the page has to act on after Servo's turn of the loop.
pub enum ServoEvent {
    /// Servo painted and presented a frame.
    Frame,
    PasswordFilled(bool),
    FormFilled(bool),
    PasswordRead(Option<frame_profile::credentials::Login>),
    Cursor(Cursor),
    InputHandled(InputEventId, InputEventResult),
    ShowControl(EmbedderControl),
    HideControl(EmbedderControlId),
    /// The title, address, load state, history or hovered link changed; the
    /// page reads the new values back from the web view.
    Changed,
    /// The page finished loading, so its document can be read.
    Loaded,
    /// The finished page's document, read back for the local search index.
    Document(VisitedPage),
    /// What the site panel shows, read on demand. It never reaches the index.
    Site(SiteInfo),
    Crashed(String),
    OpenExternally(Url),
    /// The page opened a new window; it becomes a tab.
    NewWebView(NewWebView),
    /// The page asked to close itself (`window.close()`).
    Closed,
    /// A file to save rather than show.
    Download(Url),
    /// The page's icon changed; the page reads it back from the web view.
    Favicon,
}

/// A web view Servo built for a new window, with everything a tab needs.
pub struct NewWebView {
    pub webview: WebView,
    pub delegate: Rc<Delegate>,
    pub context: Rc<SceneRenderingContext>,
}

#[derive(Default)]
pub struct Delegate {
    events: RefCell<VecDeque<ServoEvent>>,
    status: RefCell<Option<String>>,
    pub(crate) navigation_starts: Cell<u64>,
    pub(crate) frames: Cell<u64>,
    hidden: Cell<bool>,
}

impl Delegate {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    pub(crate) fn set_visible(&self, visible: bool) {
        self.hidden.set(!visible);
    }

    fn present_if_visible(&self, paint_and_present: impl FnOnce()) {
        if self.hidden.get() {
            return;
        }
        paint_and_present();
        self.frames.set(self.frames.get() + 1);
        self.push(ServoEvent::Frame);
    }

    pub fn take_events(&self) -> VecDeque<ServoEvent> {
        std::mem::take(&mut *self.events.borrow_mut())
    }

    /// The link under the pointer, as the page last reported it.
    pub fn status(&self) -> Option<String> {
        self.status.borrow().clone()
    }

    /// A document read back from the page, queued like Servo's own events.
    pub fn document(&self, page: VisitedPage) {
        self.push(ServoEvent::Document(page));
    }

    /// What the site panel shows, read back from the page.
    pub fn site(&self, info: SiteInfo) {
        self.push(ServoEvent::Site(info));
    }

    /// The page turned out to be a file Servo cannot show.
    pub fn download(&self, url: Url) {
        self.push(ServoEvent::Download(url));
    }

    pub(crate) fn push(&self, event: ServoEvent) {
        self.events.borrow_mut().push_back(event);
    }
}

/// Where a navigation may go.
#[derive(Debug, PartialEq)]
pub enum Verdict {
    Allow,
    /// Handed to the system, which knows the person's mail app.
    OpenExternally,
    /// Saved to the Downloads folder instead of shown.
    Download,
    Deny,
}

/// File types that are always saved: Servo shows none of them, and fetching
/// one as a page would read the whole file only to show a placeholder.
/// Anything else Servo cannot show is caught after it loads (`page.rs`).
const DOWNLOADS: &[&str] = &[
    "7z", "apk", "bz2", "csv", "deb", "dmg", "doc", "docx", "epub", "exe", "gz", "ics", "ipa",
    "iso", "jar", "mobi", "msi", "numbers", "odp", "ods", "odt", "pages", "pdf", "pkg", "ppt",
    "pptx", "rar", "rpm", "tar", "tgz", "torrent", "vcf", "xls", "xlsx", "xz", "zip", "zst",
];

pub fn verdict(url: &Url) -> Verdict {
    match url.scheme() {
        "http" | "https" if is_download(url) => Verdict::Download,
        "http" | "https" | "file" | "about" | "blob" | "data" => Verdict::Allow,
        "mailto" => Verdict::OpenExternally,
        _ => Verdict::Deny,
    }
}

/// Whether the address names a file of a type that is always saved.
fn is_download(url: &Url) -> bool {
    let Some(name) = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
    else {
        return false;
    };
    let Some((_, extension)) = name.rsplit_once('.') else {
        return false;
    };
    DOWNLOADS.contains(&extension.to_ascii_lowercase().as_str())
}

impl WebViewDelegate for Delegate {
    fn notify_new_frame_ready(&self, webview: WebView) {
        self.present_if_visible(|| {
            webview.paint();
            webview.rendering_context().present();
        });
    }

    fn notify_cursor_changed(&self, _: WebView, cursor: Cursor) {
        self.push(ServoEvent::Cursor(cursor));
    }

    fn notify_input_event_handled(&self, _: WebView, id: InputEventId, result: InputEventResult) {
        self.push(ServoEvent::InputHandled(id, result));
    }

    fn notify_favicon_changed(&self, _: WebView) {
        self.push(ServoEvent::Favicon);
    }

    fn notify_page_title_changed(&self, _: WebView, _: Option<String>) {
        self.push(ServoEvent::Changed);
    }

    fn notify_url_changed(&self, _: WebView, _: Url) {
        self.push(ServoEvent::Changed);
    }

    fn notify_load_status_changed(&self, _: WebView, status: LoadStatus) {
        self.push(ServoEvent::Changed);
        if status == LoadStatus::Started {
            self.navigation_starts.set(self.navigation_starts.get() + 1);
        }
        if status == LoadStatus::Complete {
            self.push(ServoEvent::Loaded);
        }
    }

    fn notify_history_changed(&self, _: WebView, _: Vec<Url>, _: usize) {
        self.push(ServoEvent::Changed);
    }

    fn notify_status_text_changed(&self, _: WebView, status: Option<String>) {
        *self.status.borrow_mut() = status;
        self.push(ServoEvent::Changed);
    }

    fn notify_crashed(&self, _: WebView, reason: String, backtrace: Option<String>) {
        log::error!("page crashed: {reason}\n{}", backtrace.unwrap_or_default());
        self.push(ServoEvent::Crashed(reason));
    }

    fn notify_closed(&self, _: WebView) {
        self.push(ServoEvent::Closed);
    }

    fn show_console_message(&self, _: WebView, level: ConsoleLogLevel, message: String) {
        match level {
            ConsoleLogLevel::Error => log::error!(target: "page", "{message}"),
            ConsoleLogLevel::Warn => log::warn!(target: "page", "{message}"),
            _ => log::debug!(target: "page", "{message}"),
        }
    }

    fn request_navigation(&self, _: WebView, request: NavigationRequest) {
        match verdict(&request.url) {
            Verdict::Allow => request.allow(),
            Verdict::OpenExternally => {
                self.push(ServoEvent::OpenExternally(request.url.clone()));
                request.deny();
            }
            Verdict::Download => {
                self.push(ServoEvent::Download(request.url.clone()));
                request.deny();
            }
            Verdict::Deny => {
                log::warn!("refused navigation to {}", request.url);
                request.deny();
            }
        }
    }

    fn request_create_new(&self, parent: WebView, request: CreateNewWebViewRequest) {
        // The tab lays itself out once it is shown; until then it renders at
        // its opener's size.
        let opener = parent.size();
        let size = PhysicalSize::new(opener.width.max(1.) as u32, opener.height.max(1.) as u32);
        let context = match SceneRenderingContext::new(size) {
            Ok(context) => Rc::new(context),
            Err(error) => {
                log::error!("new tab's rendering context: {error:?}");
                return;
            }
        };
        let delegate = Delegate::new();
        let webview = request
            .builder(context.clone())
            .hidpi_scale_factor(parent.hidpi_scale_factor())
            .delegate(delegate.clone())
            .build();
        self.push(ServoEvent::NewWebView(NewWebView {
            webview,
            delegate,
            context,
        }));
    }

    fn show_embedder_control(&self, _: WebView, control: EmbedderControl) {
        self.push(ServoEvent::ShowControl(control));
    }

    fn hide_embedder_control(&self, _: WebView, control_id: EmbedderControlId) {
        self.push(ServoEvent::HideControl(control_id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verdict_for(url: &str) -> Verdict {
        verdict(&Url::parse(url).unwrap())
    }

    #[test]
    fn hidden_frame_notifications_do_not_paint_present_or_replace_the_live_frame() {
        let delegate = Delegate::default();
        let surface = Cell::new(0);
        delegate.present_if_visible(|| surface.set(1));
        delegate.take_events();
        delegate.set_visible(false);
        for _ in 0..4 {
            delegate.present_if_visible(|| surface.set(2));
        }
        assert_eq!(surface.get(), 1);
        assert_eq!(delegate.frames.get(), 1);
        assert!(delegate.take_events().is_empty());
        delegate.set_visible(true);
        delegate.present_if_visible(|| surface.set(3));
        assert_eq!(surface.get(), 3);
        assert_eq!(delegate.frames.get(), 2);
        assert!(matches!(delegate.take_events().pop_front(), Some(ServoEvent::Frame)));
    }

    #[test]
    fn the_web_stays_in_the_browser() {
        assert_eq!(verdict_for("https://example.com/"), Verdict::Allow);
        assert_eq!(verdict_for("http://127.0.0.1:9999/"), Verdict::Allow);
        assert_eq!(verdict_for("file:///Users/me/page.html"), Verdict::Allow);
        assert_eq!(verdict_for("about:blank"), Verdict::Allow);
        assert_eq!(verdict_for("data:text/plain,hi"), Verdict::Allow);
    }

    #[test]
    fn mail_goes_to_the_system() {
        assert_eq!(verdict_for("mailto:a@example.com"), Verdict::OpenExternally);
    }

    #[test]
    fn files_are_downloads() {
        assert_eq!(
            verdict_for("https://example.com/releases/Frame-0.1.dmg"),
            Verdict::Download
        );
        assert_eq!(
            verdict_for("https://example.com/a/b.TAR.GZ?token=1"),
            Verdict::Download
        );
        assert_eq!(
            verdict_for("http://example.com/paper.pdf"),
            Verdict::Download
        );
        // Pages, and files Servo shows, are not.
        assert_eq!(
            verdict_for("https://example.com/download.php?id=3"),
            Verdict::Allow
        );
        assert_eq!(verdict_for("https://example.com/photo.png"), Verdict::Allow);
        assert_eq!(verdict_for("https://example.com/zip"), Verdict::Allow);
        assert_eq!(verdict_for("file:///Users/me/a.zip"), Verdict::Allow);
    }

    #[test]
    fn other_schemes_are_refused() {
        assert_eq!(verdict_for("javascript:alert(1)"), Verdict::Deny);
        assert_eq!(verdict_for("zoommtg://join"), Verdict::Deny);
    }
}
