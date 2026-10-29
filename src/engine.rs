//! The one Servo instance, shared by every tab: built on the GPUI main
//! thread, woken through GPUI's foreground executor rather than by polling.
//!
//! SOURCING: `ServoBuilder` and Servo's `EventLoopWaker`; GPUI's foreground
//! executor and a `futures` channel carry the wake to the main thread.
//! Ported from Theorem UI's desktop host (`desktop/src/engine.rs`).

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui_kit::{App, AsyncApp, Global};
use servo::protocol_handler::{
    Destination, DoneChannel, FetchContext, NetworkError, ProtocolHandler, ProtocolRegistry,
    Request, RequestMode, Response,
};
use servo::{BuiltinClient, CookieSource, EventLoopWaker, Opts, Preferences, Servo, ServoBuilder};
use url::Url;

/// The URL scheme of the notes app's pages, the only pages given WebGL2.
pub const NOTES_SCHEME: &str = "frame-notes";

/// Check the initiator, never the destination URL or a suppressible referrer.
/// Custom schemes have opaque origins in Servo. Its client metadata preserves
/// the document URL and marks direct embedder loads before entering the network stack.
pub fn notes_request_allowed(request: &Request) -> bool {
    if request.current_url().scheme() != NOTES_SCHEME {
        return false;
    }
    let Some(client) = request.client.as_ref() else {
        return false;
    };
    client
        .document_url
        .as_ref()
        .is_some_and(|url| url.scheme() == NOTES_SCHEME)
        || (client.is_embedder_navigation
            && client.document_url.is_none()
            && !client.is_nested_browsing_context
            && request.mode == RequestMode::Navigate
            && request.destination == Destination::Document
            && request
                .url_list
                .iter()
                .all(|url| url.scheme() == NOTES_SCHEME))
}

/// Wrap every notes handler before registering it, so refused requests cannot
/// reach filesystem or vault code (including existence checks).
struct NotesProtocol(Box<dyn ProtocolHandler>);

impl ProtocolHandler for NotesProtocol {
    fn load<'a>(
        &'a self,
        request: &'a mut Request,
        done: &mut DoneChannel,
        context: &FetchContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send + 'a>> {
        if !notes_request_allowed(request) {
            return Box::pin(std::future::ready(Response::network_error(
                NetworkError::ResourceLoadError("notes request has an untrusted initiator".into()),
            )));
        }
        self.0.load(request, done, context)
    }

    fn privileged_paths(&self) -> &'static [&'static str] {
        self.0.privileged_paths()
    }

    fn is_fetchable(&self) -> bool {
        self.0.is_fetchable()
    }

    fn is_secure(&self) -> bool {
        self.0.is_secure()
    }
}

/// Servo calls `wake` from its own threads; this only posts to the foreground.
/// `pending` folds a burst of wakes into one turn of the event loop.
#[derive(Clone)]
struct ChannelWaker {
    sender: UnboundedSender<()>,
    pending: Arc<AtomicBool>,
}

impl EventLoopWaker for ChannelWaker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(self.clone())
    }

    fn wake(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            let _ = self.sender.unbounded_send(());
        }
    }
}

pub struct Engine {
    pub servo: Servo,
    /// The in-process inspection client. The panel takes it once.
    inspection: Option<BuiltinClient>,
    /// Runs after every turn of Servo's event loop, so the delegates' queued
    /// events reach their views once Servo has let go of them. A callback
    /// that returns false is dropped, as a closed tab's does.
    after_spin: Vec<Box<dyn FnMut(&mut App) -> bool>>,
}

impl Global for Engine {}

impl Engine {
    /// Build Servo with durable storage under `profile_dir` and start the task
    /// that turns its wake-ups into `spin_event_loop` calls.
    pub fn start(profile_dir: &Path, cx: &mut App) {
        Self::start_with_protocols(profile_dir, ProtocolRegistry::default(), cx);
    }

    /// `start`, with the embedder's own URL schemes registered, such as the
    /// notes app's `frame-notes:` pages.
    pub fn start_with_protocols(profile_dir: &Path, mut protocols: ProtocolRegistry, cx: &mut App) {
        // Apply the same boundary to the notes app and to probes, before any
        // handler can inspect a vault path or return a resource.
        if let Some(handler) = protocols.remove(NOTES_SCHEME) {
            protocols
                .register(NOTES_SCHEME, NotesProtocol(handler))
                .expect("the removed notes scheme can be registered again");
        }
        let (sender, receiver) = unbounded();
        let pending = Arc::new(AtomicBool::new(false));
        let mut opts = Opts::default();
        opts.config_dir = Some(profile_dir.to_path_buf());
        // `:has()` is off by default in Servo, and much of the web uses it.
        let preferences = Preferences {
            layout_css_has_selector_enabled: true,
            dom_media_source_enabled: true,
            // Inspection stays in-process. A TCP listener is a separate
            // development option and stays off for ordinary browsing.
            cdp_builtin_enabled: true,
            // WebGL2 stays off for the web at large and is on for the notes
            // app's pages, whose graph (cosmos.gl) needs it.
            dom_webgl2_enabled_schemes: NOTES_SCHEME.into(),
            ..Preferences::default()
        };
        let servo = ServoBuilder::default()
            .opts(opts)
            .preferences(preferences)
            .protocol_registry(protocols)
            .event_loop_waker(Box::new(ChannelWaker {
                sender,
                pending: pending.clone(),
            }))
            .build();
        servo.setup_logging();
        let inspection = servo::take_builtin_client();
        cx.set_global(Engine {
            servo,
            inspection,
            after_spin: Vec::new(),
        });
        // GPUI releases windows before polling quit futures. Drop the final
        // engine handle then, after Page/WebView handles are gone, so Servo's
        // normal shutdown flushes cookies and website storage to this profile.
        cx.on_app_quit(|cx| {
            let engine = cx.remove_global::<Engine>();
            async move {
                drop(engine);
            }
        })
        .detach();
        cx.spawn(async move |cx| spin(receiver, pending, cx).await)
            .detach();
    }

    pub fn servo(cx: &App) -> Servo {
        cx.global::<Engine>().servo.clone()
    }

    /// The panel's inspection client. Only the first caller receives it.
    pub fn take_inspection(&mut self) -> Option<BuiltinClient> {
        self.inspection.take()
    }

    /// The `Cookie` header Servo would send to `url`, for a request Platinum
    /// makes itself: a download.
    pub fn cookie_header(url: &Url, cx: &App) -> Option<String> {
        let engine = cx.try_global::<Engine>()?;
        let cookies = engine
            .servo
            .site_data_manager()
            .cookies_for_url(url.clone(), CookieSource::HTTP);
        let header = cookies
            .iter()
            .map(|cookie| format!("{}={}", cookie.name(), cookie.value()))
            .collect::<Vec<_>>()
            .join("; ");
        (!header.is_empty()).then_some(header)
    }

    /// The user agent Servo sends, so Platinum's own requests look the same.
    pub fn user_agent() -> String {
        servo::prefs::get().user_agent.clone()
    }

    pub fn after_spin(callback: impl FnMut(&mut App) -> bool + 'static, cx: &mut App) {
        cx.global_mut::<Engine>()
            .after_spin
            .push(Box::new(callback));
    }
}

async fn spin(mut wakes: UnboundedReceiver<()>, pending: Arc<AtomicBool>, cx: &mut AsyncApp) {
    while wakes.next().await.is_some() {
        pending.store(false, Ordering::Release);
        cx.update(|cx| {
            let Some(engine) = cx.try_global::<Engine>() else {
                return;
            };
            let servo = engine.servo.clone();
            {
                #[cfg(feature = "performance-check")]
                let _measure = crate::performance::Span::new("servo_spin");
                servo.spin_event_loop();
            }
            // Take the callbacks out so they can reach `cx` mutably.
            let mut callbacks = std::mem::take(&mut cx.global_mut::<Engine>().after_spin);
            callbacks.retain_mut(|callback| callback(cx));
            if cx.try_global::<Engine>().is_none() {
                return;
            }
            let engine = cx.global_mut::<Engine>();
            callbacks.append(&mut engine.after_spin);
            engine.after_spin = callbacks;
        });
    }
}

#[cfg(test)]
mod notes_protocol_tests {
    use super::*;
    use servo::protocol_handler::{
        InsecureRequestsPolicy, Origin, Referrer, RequestBuilder, RequestClient, UrlWithBlobClaim,
    };

    fn request(source: Option<&str>, embedder: bool) -> Request {
        let url = UrlWithBlobClaim::from_url_without_having_claimed_blob(
            "frame-notes://vault/index.html".parse().unwrap(),
        );
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

    #[test]
    fn own_navigation_and_notes_subresources_are_allowed_without_a_referrer() {
        assert!(notes_request_allowed(&request(None, true)));
        let mut request = request(Some("frame-notes://vault/index.html"), false);
        for destination in [
            Destination::IFrame,
            Destination::Image,
            Destination::Script,
            Destination::None,
        ] {
            request.destination = destination;
            request.mode = RequestMode::NoCors;
            assert!(notes_request_allowed(&request));
        }
    }

    #[test]
    fn web_and_opaque_initiators_cannot_probe_notes() {
        for source in [
            None,
            Some("https://example.org/"),
            Some("data:text/html,hello"),
            Some("about:blank"),
        ] {
            let mut request = request(source, false);
            for destination in [
                Destination::Document,
                Destination::IFrame,
                Destination::Image,
                Destination::Script,
                Destination::None,
            ] {
                request.destination = destination;
                for mode in [
                    RequestMode::Navigate,
                    RequestMode::NoCors,
                    RequestMode::CorsMode,
                ] {
                    request.mode = mode;
                    assert!(!notes_request_allowed(&request));
                }
            }
        }
    }

    #[test]
    fn embedder_authority_is_limited_to_direct_top_level_notes_loads() {
        let mut request = request(None, true);
        request.destination = Destination::IFrame;
        assert!(!notes_request_allowed(&request));
        request.destination = Destination::Document;
        request.client.as_mut().unwrap().is_nested_browsing_context = true;
        assert!(!notes_request_allowed(&request));
        request.client.as_mut().unwrap().is_nested_browsing_context = false;
        request.url_list.insert(
            0,
            UrlWithBlobClaim::from_url_without_having_claimed_blob(
                "https://example.org/".parse().unwrap(),
            ),
        );
        assert!(!notes_request_allowed(&request));
        request.client = None;
        assert!(!notes_request_allowed(&request));
        assert!(!notes_request_allowed(&self::request(
            Some("https://example.org/"),
            true
        )));
    }
}
