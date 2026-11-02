//! The preview scheme: one revision of a design's page, served to Servo.
//!
//! An agent-written page is shown at `frame-preview://page/{design}/{revision}/index.html`.
//! The page is inline by construction (`theorem-html-design` refuses a page that
//! names an outside resource), so this handler serves exactly one document and
//! no subresources. Three things keep the page from reaching anything else: the
//! initiator check below, `is_fetchable() == false` (a page cannot `fetch()` a
//! sibling revision), and the Content-Security-Policy on the response. The
//! network is the embedder's to refuse: see `Blocked` in
//! `examples/html_author_live.rs`, which answers every HTTP request the page
//! makes and records it as evidence.
//!
//! SOURCING: Servo's `ProtocolHandler` and `ProtocolRegistry`, registered the
//! way `engine.rs` registers the notes scheme. The initiator check is the notes
//! app's, for this scheme (`engine::notes_request_allowed`); `engine.rs` is not
//! touched, so the two stay independent until a third scheme wants a shared one.
//! This file depends only on Servo, `url`, `percent-encoding` and `http`, so an
//! example can include it with `#[path]`.

use std::future::{Future, ready};
use std::pin::Pin;

use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use servo::protocol_handler::{
    Destination, DoneChannel, FetchContext, HttpStatus, NetworkError, ProtocolHandler, Request,
    RequestMode, ResourceFetchTiming, Response, ResponseBody,
};
use url::Url;

/// The URL scheme of design previews.
pub const PREVIEW_SCHEME: &str = "frame-preview";

/// What a preview page may do. Scripts and styles may be inline (a page is one
/// file); images, fonts and media only from `data:`; nothing may connect, post
/// a form, embed or be embedded, or change its base.
pub const POLICY: &str = "default-src 'none'; script-src 'unsafe-inline'; \
style-src 'unsafe-inline'; img-src data:; font-src data:; media-src data:; \
connect-src 'none'; form-action 'none'; base-uri 'none'; frame-ancestors 'none'";

/// The address of `design_id` at `revision`.
pub fn url(design_id: &str, revision: u64) -> Url {
    let design = utf8_percent_encode(design_id, NON_ALPHANUMERIC);
    Url::parse(&format!(
        "{PREVIEW_SCHEME}://page/{design}/{revision}/index.html"
    ))
    .expect("a preview address is a valid URL")
}

/// The design and revision an address names, or `None` if it is not one.
pub fn parse(address: &Url) -> Option<(String, u64)> {
    if address.scheme() != PREVIEW_SCHEME || address.host_str() != Some("page") {
        return None;
    }
    let mut parts = address.path_segments()?;
    let design = percent_decode_str(parts.next()?).decode_utf8().ok()?;
    let revision = parts.next()?.parse().ok()?;
    (parts.next() == Some("index.html") && parts.next().is_none())
        .then(|| (design.into_owned(), revision))
}

/// Check the initiator, never the destination URL or a suppressible referrer:
/// a preview page may be loaded by the embedder, or requested by another
/// preview document, and by nothing else. See `engine::notes_request_allowed`.
fn allowed(request: &Request) -> bool {
    if request.current_url().scheme() != PREVIEW_SCHEME {
        return false;
    }
    let Some(client) = request.client.as_ref() else {
        return false;
    };
    client
        .document_url
        .as_ref()
        .is_some_and(|url| url.scheme() == PREVIEW_SCHEME)
        || (client.is_embedder_navigation
            && client.document_url.is_none()
            && !client.is_nested_browsing_context
            && request.mode == RequestMode::Navigate
            && request.destination == Destination::Document
            && request
                .url_list
                .iter()
                .all(|url| url.scheme() == PREVIEW_SCHEME))
}

/// Serves `source(design, revision)` as an HTML document.
pub struct PreviewProtocol<F> {
    source: F,
}

impl<F> PreviewProtocol<F>
where
    F: Fn(&str, u64) -> Option<String> + Send + Sync + 'static,
{
    pub const fn new(source: F) -> Self {
        Self { source }
    }
}

fn header(value: &str) -> http::HeaderValue {
    http::HeaderValue::from_str(value).expect("a fixed header value is valid")
}

impl<F> ProtocolHandler for PreviewProtocol<F>
where
    F: Fn(&str, u64) -> Option<String> + Send + Sync + 'static,
{
    fn load<'a>(
        &'a self,
        request: &'a mut Request,
        _: &mut DoneChannel,
        _: &FetchContext,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        if !allowed(request) {
            return Box::pin(ready(Response::network_error(
                NetworkError::ResourceLoadError("preview request has an untrusted initiator".into()),
            )));
        }
        let address = request.current_url();
        let page =
            parse(address.as_url()).and_then(|(design, revision)| (self.source)(&design, revision));
        let response = match page {
            Some(page) => {
                let mut response =
                    Response::new(address, ResourceFetchTiming::new(request.timing_type()));
                *response.body.lock() = ResponseBody::Done(page.into_bytes());
                response
                    .headers
                    .insert(http::header::CONTENT_TYPE, header("text/html;charset=utf-8"));
                response
                    .headers
                    .insert(http::header::CONTENT_SECURITY_POLICY, header(POLICY));
                response
                    .headers
                    .insert(http::header::X_CONTENT_TYPE_OPTIONS, header("nosniff"));
                response.status = HttpStatus::default();
                response
            },
            None => Response::network_error(NetworkError::ResourceLoadError(format!(
                "no such preview: {address}"
            ))),
        };
        Box::pin(ready(response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_round_trips_a_design_with_awkward_characters() {
        for design in ["harbor", "harbor/cafe", "a b?c#d", "ünï"] {
            let address = url(design, 7);
            assert_eq!(parse(&address), Some((design.to_owned(), 7)), "{address}");
        }
    }

    #[test]
    fn only_a_complete_preview_address_parses() {
        for address in [
            "https://page/harbor/1/index.html",
            "frame-preview://elsewhere/harbor/1/index.html",
            "frame-preview://page/harbor/one/index.html",
            "frame-preview://page/harbor/1/other.html",
            "frame-preview://page/harbor/1/index.html/more",
            "frame-preview://page/harbor/1",
        ] {
            assert_eq!(parse(&Url::parse(address).unwrap()), None, "{address}");
        }
    }
}
