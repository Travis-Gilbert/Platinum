//! The `frame-notes:` protocol handler: note pages, preview fragments,
//! attachments, snippets and the bundled scripts and styles.
//!
//! Servo calls `load` from its own threads, and the vault lives on another
//! (`service.rs`), so a request becomes a closure sent to the vault's thread
//! and the response is built from what comes back. Nothing here blocks.
//!
//! Every request is untrusted: the engine refuses an initiator that is not
//! the notes app before the handler runs, `load` checks it again so the
//! handler is safe wherever it is registered, and the URL is parsed by
//! `route.rs` into a plain vault path or nothing. A vault's file is served
//! with a type that cannot run, a nosniff header, and (as a document) a
//! content security policy that allows nothing but the notes scheme.

use std::future::{Future, ready};
use std::pin::Pin;

use http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, HeaderName, HeaderValue,
    X_CONTENT_TYPE_OPTIONS,
};
use servo::protocol_handler::{
    DoneChannel, FetchContext, HttpStatus, NetworkError, ProtocolHandler, Request,
    ResourceFetchTiming, Response, ResponseBody,
};
use url::Url;

use super::assets;
use super::ops::{self, FileError};
use super::route::{Refusal, Route};
use super::service::{Error, Service, Vaults};
use super::shell::{self, Chrome};
use crate::engine::notes_request_allowed;

/// What a note's page may load: the notes scheme and nothing else. Inline
/// style is allowed because MathJax writes its styles into the page; script
/// is not, because everything the page runs is a bundled file.
const PAGE_POLICY: &str = "default-src 'none'; script-src frame-notes:; style-src frame-notes: 'unsafe-inline'; \
img-src frame-notes: data: blob:; font-src frame-notes: data:; media-src frame-notes: blob:; \
connect-src frame-notes:; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";

/// What a vault's own file may do if it is opened as a document: nothing.
const FILE_POLICY: &str = "sandbox; default-src 'none'; style-src 'unsafe-inline'";

/// Serves a vault's `frame-notes:` addresses.
pub struct Handler {
    vaults: Vaults,
}

impl Handler {
    pub fn new(vaults: Vaults) -> Self {
        Self { vaults }
    }
}

impl Handler {
    /// The response to `request`. A request whose initiator is not the notes
    /// app is refused here, before any vault code is queued.
    fn answer(&self, request: &Request) -> Pin<Box<dyn Future<Output = Response> + Send>> {
        if !notes_request_allowed(request) {
            return Box::pin(ready(Response::network_error(
                NetworkError::ResourceLoadError("notes request has an untrusted initiator".into()),
            )));
        }
        let address = request.current_url();
        let timing_type = request.timing_type();
        let vaults = self.vaults.clone();
        Box::pin(async move {
            let reply = serve(&vaults, &address.clone().into_url()).await;
            reply.respond(address, ResourceFetchTiming::new(timing_type))
        })
    }
}

impl ProtocolHandler for Handler {
    fn load<'a>(
        &'a self,
        request: &'a mut Request,
        _: &mut DoneChannel,
        _: &FetchContext,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        self.answer(request)
    }

    fn is_fetchable(&self) -> bool {
        true
    }
}

/// A response, before Servo's types.
#[derive(Debug, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub content_type: String,
    pub bytes: Vec<u8>,
    pub kind: Kind,
}

/// What the response is, for the headers that go with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A bundled script, style or font.
    Bundled,
    /// A note's page, or a page saying why there is none.
    Document,
    /// A fragment, a snippet or an attachment of the vault.
    Vault,
    /// An attachment: also sandboxed, should it be opened as a document.
    Attachment,
}

impl Reply {
    fn found(kind: Kind, content_type: &str, bytes: Vec<u8>) -> Self {
        Self {
            status: 200,
            content_type: content_type.to_owned(),
            bytes,
            kind,
        }
    }

    fn refused(status: u16, reason: &str) -> Self {
        let title = match status {
            400 => "Not a valid address",
            404 => "Not found",
            413 => "Too large",
            _ => "Unavailable",
        };
        let reason = reason
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        Self {
            status,
            content_type: "text/html; charset=utf-8".into(),
            bytes: format!("<!doctype html><meta charset=\"utf-8\"><title>{title}</title><p>{title}.</p><p>{reason}</p>")
                .into_bytes(),
            kind: Kind::Document,
        }
    }

    fn respond(self, address: servo::ServoUrl, timing: ResourceFetchTiming) -> Response {
        let mut response = Response::new(address, timing);
        let header = |name: HeaderName, value: &str| {
            HeaderValue::from_str(value).ok().map(|value| (name, value))
        };
        let fixed = |name: HeaderName, value: &'static str| (name, HeaderValue::from_static(value));
        let mut headers = vec![
            header(CONTENT_TYPE, &self.content_type),
            Some(fixed(X_CONTENT_TYPE_OPTIONS, "nosniff")),
            Some(fixed(CACHE_CONTROL, "no-store")),
        ];
        match self.kind {
            Kind::Document => headers.push(Some(fixed(CONTENT_SECURITY_POLICY, PAGE_POLICY))),
            Kind::Attachment => headers.push(Some(fixed(CONTENT_SECURITY_POLICY, FILE_POLICY))),
            Kind::Bundled | Kind::Vault => {}
        }
        for (name, value) in headers.into_iter().flatten() {
            response.headers.insert(name, value);
        }
        response.status = if self.status == 200 {
            HttpStatus::default()
        } else {
            HttpStatus::new_raw(self.status, Vec::new())
        };
        *response.body.lock() = ResponseBody::Done(self.bytes);
        response
    }
}

/// The reply to a request for `url`.
pub async fn serve(vaults: &Vaults, url: &Url) -> Reply {
    let Some(service) = url.host_str().and_then(|host| vaults.get(host)) else {
        return Reply::refused(404, "No such vault.");
    };
    match Route::parse(url) {
        Ok(route) => dispatch(&service, route, vaults.chrome()).await,
        Err(Refusal::Unknown) => Reply::refused(404, "No such address."),
        Err(Refusal::Invalid) => Reply::refused(400, "That is not a path in a vault."),
    }
}

async fn dispatch(service: &Service, route: Route, chrome: Option<Chrome>) -> Reply {
    match route {
        Route::App(path) => match assets::get(&path) {
            Some(asset) => {
                Reply::found(Kind::Bundled, asset.content_type, asset.bytes.into_owned())
            }
            None => Reply::refused(404, "No such file."),
        },
        Route::Note { path, token } => {
            let shown = path.clone();
            match service.read(move |vault| ops::page(vault, &path)).await {
                Ok(page) => Reply::found(
                    Kind::Document,
                    "text/html; charset=utf-8",
                    shell::render(&shown, &token, &page, chrome.as_ref()).into_bytes(),
                ),
                Err(error) => unavailable(&shown, error),
            }
        }
        Route::File(path) => match service.read(move |vault| Ok(ops::file(vault, &path))).await {
            Ok(Ok(file)) => Reply::found(Kind::Attachment, file.content_type, file.bytes),
            Ok(Err(FileError::NotFound)) => Reply::refused(404, "No such file."),
            Ok(Err(FileError::TooLarge)) => Reply::refused(413, "That file is too large to show."),
            Err(error) => unavailable("", error),
        },
        Route::Snippet(file) => match service.read(move |vault| ops::snippet(vault, &file)).await {
            Ok(Some(css)) => Reply::found(Kind::Vault, "text/css; charset=utf-8", css.into_bytes()),
            Ok(None) => Reply::refused(404, "No such snippet."),
            Err(error) => unavailable("", error),
        },
        Route::Preview { from, link } => {
            match service
                .read(move |vault| Ok(ops::preview(vault, &from, &link)))
                .await
            {
                Ok(Some(html)) => {
                    Reply::found(Kind::Vault, "text/html; charset=utf-8", html.into_bytes())
                }
                Ok(None) => Reply::refused(404, "Nothing to preview."),
                Err(error) => unavailable("", error),
            }
        }
    }
}

/// An honest page for a vault or note that cannot be served now.
fn unavailable(path: &str, error: Error) -> Reply {
    match error {
        Error::Busy => Reply::refused(503, "The vault is busy. Try again."),
        Error::Closed | Error::Unavailable(_) => Reply::refused(503, &error.to_string()),
        Error::Failed(reason) if path.is_empty() => Reply::refused(404, &reason),
        Error::Failed(reason) => {
            Reply::refused(404, &format!("{path} could not be opened: {reason}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use futures::executor::block_on;
    use servo::protocol_handler::{
        Destination, InsecureRequestsPolicy, Origin, Referrer, RequestBuilder, RequestClient,
        RequestMode, UrlWithBlobClaim,
    };

    use super::*;

    struct Vault {
        _root: tempfile::TempDir,
        _data: tempfile::TempDir,
        vaults: Vaults,
        service: Service,
    }

    fn vault(files: &[(&str, &str)]) -> Vault {
        let root = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let file = root.path().join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
        let vaults = Vaults::default();
        let service = vaults.open(root.path(), data.path());
        Vault {
            _root: root,
            _data: data,
            vaults,
            service,
        }
    }

    fn get(vault: &Vault, path: &str) -> Reply {
        let url = Url::parse(&format!("frame-notes://{}{path}", vault.service.host())).unwrap();
        block_on(serve(&vault.vaults, &url))
    }

    fn request(address: &str, source: Option<&str>, embedder: bool) -> Request {
        let url = UrlWithBlobClaim::from_url_without_having_claimed_blob(address.parse().unwrap());
        let mut request = RequestBuilder::new(None, url, Referrer::NoReferrer).build();
        request.mode = RequestMode::Navigate;
        request.destination = Destination::Document;
        request.client = Some(RequestClient {
            document_url: source.map(|url| url.parse().unwrap()),
            is_embedder_navigation: embedder,
            origin: Origin::Client,
            preloaded_resources: Default::default(),
            policy_container: Default::default(),
            is_nested_browsing_context: false,
            insecure_requests_policy: InsecureRequestsPolicy::DoNotUpgrade,
            has_trustworthy_ancestor_origin: false,
            is_window: source.is_some(),
        });
        request
    }

    fn loaded(handler: &Handler, request: &Request) -> Response {
        block_on(handler.answer(request))
    }

    fn timing() -> ResourceFetchTiming {
        ResourceFetchTiming::new(request("frame-notes://x/", None, true).timing_type())
    }

    #[test]
    fn a_requests_initiator_is_refused_before_any_vault_code_runs() {
        let vault = vault(&[("Home.md", "hi")]);
        let handler = Handler::new(vault.vaults.clone());
        let address = format!("frame-notes://{}/note/Home.md?k=t", vault.service.host());
        for source in [
            None,
            Some("https://example.org/"),
            Some("data:text/html,hello"),
            Some("about:blank"),
        ] {
            let refused = request(&address, source, false);
            let response = loaded(&handler, &refused);
            assert!(response.is_network_error(), "{source:?}");
        }
        // Nor can the web probe for what is there.
        let probing = request(&address, Some("https://example.org/"), true);
        assert!(loaded(&handler, &probing).is_network_error());
        assert_eq!(vault.service.submitted(), 0);
        // The notes app's own load is served.
        let own = request(&address, None, true);
        assert!(!loaded(&handler, &own).is_network_error());
        assert_eq!(vault.service.submitted(), 1);
    }

    #[test]
    fn a_page_takes_the_app_theme_set_after_the_vault_opened() {
        let vault = vault(&[("Home.md", "hi")]);
        let before = String::from_utf8(get(&vault, "/note/Home.md?k=t").bytes).unwrap();
        assert!(!before.contains("--frame-background-primary"));
        vault.vaults.set_chrome(Chrome::sample(true));
        let after = String::from_utf8(get(&vault, "/note/Home.md?k=t").bytes).unwrap();
        assert!(
            after.contains("--frame-background-primary:hsla(180.0,25.0%,50.0%,1.000)"),
            "{after}"
        );
        assert!(after.contains("<body class=\"theme-dark\">"));
    }

    #[test]
    fn a_note_is_a_document_with_a_policy_and_no_sniffing() {
        let vault = vault(&[("Home.md", "# Home\n\nhello")]);
        let reply = get(&vault, "/note/Home.md?k=tok");
        assert_eq!(reply.status, 200);
        assert_eq!(reply.content_type, "text/html; charset=utf-8");
        assert_eq!(reply.kind, Kind::Document);
        let page = String::from_utf8(reply.bytes).unwrap();
        assert!(
            page.contains("\"token\":\"tok\"") && page.contains("hello"),
            "{page}"
        );
        let response = Reply {
            status: 200,
            content_type: "text/html; charset=utf-8".into(),
            bytes: Vec::new(),
            kind: Kind::Document,
        }
        .respond("frame-notes://x/note/a?k=t".parse().unwrap(), timing());
        assert_eq!(response.headers[X_CONTENT_TYPE_OPTIONS], "nosniff");
        let policy = response.headers[CONTENT_SECURITY_POLICY].to_str().unwrap();
        assert!(
            policy.contains("default-src 'none'") && policy.contains("script-src frame-notes:")
        );
        assert!(!policy.contains("unsafe-eval") && !policy.contains("http"));
    }

    #[test]
    fn the_bundle_is_served_with_the_types_a_page_needs() {
        let vault = vault(&[]);
        let script = get(&vault, "/app/reading.js");
        assert_eq!(
            (script.status, script.content_type.as_str()),
            (200, "text/javascript; charset=utf-8")
        );
        let style = get(&vault, "/app/reading.css");
        assert_eq!(style.content_type, "text/css; charset=utf-8");
        assert_eq!(
            get(&vault, "/app/vendor/mathjax/LICENSE").content_type,
            "text/plain; charset=utf-8"
        );
        assert_eq!(get(&vault, "/app/nothing.js").status, 404);
        assert_eq!(get(&vault, "/app/Cargo.toml").status, 404);
    }

    #[test]
    fn paths_that_leave_the_vault_or_the_bundle_are_refused() {
        let vault = vault(&[("Home.md", "hi")]);
        for path in [
            "/files/../Home.md",
            "/files/%2e%2e/Home.md",
            "/files/a/%2E%2E/%2e%2e/etc/passwd",
            "/files/a%2f..%2fHome.md",
            "/files/a%5c..%5cHome.md",
            "/files/.obsidian/app.json",
            "/files/%2eobsidian/app.json",
            "/files/.theorem/ids.json",
            "/app/../Cargo.toml",
            "/app/%2e%2e/%2e%2e/Cargo.toml",
            "/app/vendor/%2e%2e/%2e%2e/reading.js/../../x",
            "/note/../Home.md?k=t",
            "/note/%2e%2e/Home.md?k=t",
            "/note/Home.md%00.png?k=t",
            "/snippet/..%2fappearance.json",
            "/snippet/a/b",
        ] {
            let reply = get(&vault, path);
            assert!(
                matches!(reply.status, 400 | 404),
                "{path} answered {}",
                reply.status
            );
            assert_eq!(reply.kind, Kind::Document, "{path}");
        }
        assert_eq!(
            get(&vault, "/note/Home.md").status,
            400,
            "a page needs its token"
        );
    }

    #[test]
    fn another_vaults_host_and_unknown_shapes_name_nothing() {
        let vault = vault(&[("Home.md", "hi")]);
        let other = Url::parse("frame-notes://0000000000000000/note/Home.md?k=t").unwrap();
        assert_eq!(block_on(serve(&vault.vaults, &other)).status, 404);
        assert_eq!(get(&vault, "/").status, 404);
        assert_eq!(get(&vault, "/act/ready?t=t").status, 404);
        assert_eq!(get(&vault, "/vault/Home.md").status, 404);
    }

    #[test]
    fn a_vaults_files_are_served_as_what_cannot_run() {
        let vault = vault(&[
            ("attachments/pic.png", "png"),
            (
                "attachments/vector.svg",
                "<svg xmlns='http://www.w3.org/2000/svg'><script>alert(1)</script></svg>",
            ),
            ("attachments/page.html", "<script>alert(1)</script>"),
        ]);
        let png = get(&vault, "/files/attachments/pic.png");
        assert_eq!(
            (png.status, png.content_type.as_str(), png.kind),
            (200, "image/png", Kind::Attachment)
        );
        assert_eq!(
            get(&vault, "/files/attachments/page.html").content_type,
            "text/plain; charset=utf-8"
        );
        assert_eq!(get(&vault, "/files/attachments/missing.png").status, 404);
        // A file the index does not list, such as one in a hidden folder, is not served.
        assert_eq!(get(&vault, "/files/attachments").status, 404);
        let svg = Reply {
            kind: Kind::Attachment,
            ..get(&vault, "/files/attachments/vector.svg")
        };
        let response = svg.respond("frame-notes://x/files/a".parse().unwrap(), timing());
        assert!(
            response.headers[CONTENT_SECURITY_POLICY]
                .to_str()
                .unwrap()
                .starts_with("sandbox")
        );
    }

    #[test]
    fn only_enabled_snippets_are_served() {
        let vault = vault(&[
            (
                ".obsidian/appearance.json",
                r#"{"enabledCssSnippets":["on"]}"#,
            ),
            (".obsidian/snippets/on.css", ".callout{color:red}"),
            (".obsidian/snippets/off.css", ".callout{color:blue}"),
        ]);
        let css = get(&vault, "/snippet/on.css");
        assert_eq!(
            (css.status, css.content_type.as_str()),
            (200, "text/css; charset=utf-8")
        );
        assert_eq!(css.bytes, b".callout{color:red}");
        assert_eq!(get(&vault, "/snippet/off.css").status, 404);
        assert_eq!(get(&vault, "/snippet/appearance.json").status, 404);
    }

    #[test]
    fn a_vault_that_will_not_open_is_an_honest_page_not_a_hang() {
        let data = tempfile::tempdir().unwrap();
        let vaults = Vaults::default();
        let service = vaults.open(Path::new("/definitely/not/a/vault"), data.path());
        let url = Url::parse(&format!(
            "frame-notes://{}/note/Home.md?k=t",
            service.host()
        ))
        .unwrap();
        let reply = block_on(serve(&vaults, &url));
        assert_eq!(reply.status, 503);
        assert!(
            String::from_utf8(reply.bytes)
                .unwrap()
                .contains("could not be opened")
        );
    }

    #[test]
    fn a_missing_note_says_so_and_escapes_what_it_echoes() {
        let vault = vault(&[]);
        let reply = get(&vault, "/note/%3Cb%3E.md?k=t");
        assert_eq!(reply.status, 404);
        let page = String::from_utf8(reply.bytes).unwrap();
        assert!(!page.contains("<b>"), "{page}");
    }
}
