//! Where a site keeps its icon. Most sites name it in their home page's
//! `<link rel=icon>`, many only there: `gpui-kit.com/logo.svg`,
//! `lib.rs/favicon.png`. The rest answer at `/favicon.ico`, which is also
//! where a browser looks when a page names none. [`SiteIcon`] reads the home
//! page once per site, through GPUI's asset cache, and gives the address for
//! [`IconImage`] to fetch and decode within explicit limits.
//!
//! SOURCING: GPUI's HTTP client fetches and html5ever parses, as for search
//! snippets (`markup.rs`). Hand-rolled: `favicon-scraper` (MIT) finds icons
//! by fetching each one to measure it with `imagesize`, which cannot measure
//! an SVG, so it drops `gpui-kit.com/logo.svg`; it also reads no `media` and
//! no touch-icon rel. `webicon` (MIT) was last released in 2017, on reqwest
//! 0.4. `site_icons` and `favicon-picker` are GPL-3.0, and Frame is MIT.

use futures::{AsyncReadExt as _, FutureExt as _};
use gpui_kit::{App, Asset, RenderImage, SharedUri};
use html5ever::tendril::TendrilSink as _;
use html5ever::{ParseOpts, local_name, parse_document};
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use std::time::Duration;
use std::{io::Cursor, sync::Arc};
use url::Url;

/// A site's icon address, from the root of the site (`https://lib.rs/`).
pub enum SiteIcon {}

impl Asset for SiteIcon {
    type Source = Url;
    type Output = SharedUri;

    fn load(root: Url, cx: &mut App) -> impl Future<Output = SharedUri> + Send + 'static + use<> {
        let client = cx.http_client();
        let deadline = cx.background_executor().timer(Duration::from_secs(5));
        async move {
            let declared = async {
                let mut response = client.get(root.as_str(), ().into(), true).await.ok()?;
                if !response.status().is_success() {
                    return None;
                }
                // The head is near the start; a page past this is not read.
                let mut page = Vec::new();
                let mut body = response.body_mut().take(PAGE_LIMIT);
                body.read_to_end(&mut page).await.ok()?;
                declared_icon(&String::from_utf8_lossy(&page), &root)
            };
            let declared = match futures::future::select(declared.boxed(), deadline.boxed()).await {
                futures::future::Either::Left((icon, _)) => icon,
                futures::future::Either::Right(_) => None,
            };
            let icon = match declared {
                Some(icon) => icon,
                None => root.join("/favicon.ico").unwrap_or(root),
            };
            String::from(icon).into()
        }
    }
}

/// Bounded favicon bytes and one decoded frame, cached per URL. GPUI's generic
/// image loader has no response limit and retains every animation frame.
pub enum IconImage {}

impl Asset for IconImage {
    type Source = SharedUri;
    type Output = Option<Arc<RenderImage>>;

    fn load(
        uri: SharedUri,
        cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static + use<> {
        let client = cx.http_client();
        let deadline = cx.background_executor().timer(Duration::from_secs(5));
        async move {
            let fetch = async {
                let mut response = client.get(uri.as_ref(), ().into(), true).await.ok()?;
                if !response.status().is_success() {
                    return None;
                }
                let bytes = bounded_icon_body(response.body_mut()).await?;
                decode_icon(&bytes)
            };
            match futures::future::select(fetch.boxed(), deadline.boxed()).await {
                futures::future::Either::Left((icon, _)) => icon,
                futures::future::Either::Right(_) => None,
            }
        }
    }
}

const ICON_LIMIT: u64 = 2 * 1024 * 1024;
const ICON_DIMENSION: u32 = 1024;

async fn bounded_icon_body(body: impl futures::AsyncRead + Unpin) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    body.take(ICON_LIMIT + 1)
        .read_to_end(&mut bytes)
        .await
        .ok()?;
    (bytes.len() as u64 <= ICON_LIMIT).then_some(bytes)
}

fn decode_icon(bytes: &[u8]) -> Option<Arc<RenderImage>> {
    let mut rgba = if let Ok(format) = image::guess_format(bytes) {
        let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(ICON_DIMENSION);
        limits.max_image_height = Some(ICON_DIMENSION);
        limits.max_alloc = Some(8 * 1024 * 1024);
        reader.limits(limits);
        // Decode just the first frame, even for animated GIF/WebP icons.
        reader.decode().ok()?.thumbnail(32, 32).into_rgba8()
    } else {
        // Reuse resvg already in the dependency graph. Its resolver hooks
        // prevent nested images from bypassing limits or reading local files.
        // from_str also rejects compressed SVG input before decompression.
        let options = resvg::usvg::Options {
            image_href_resolver: resvg::usvg::ImageHrefResolver {
                resolve_data: Box::new(|_, _, _| None),
                resolve_string: Box::new(|_, _| None),
            },
            ..Default::default()
        };
        let tree = resvg::usvg::Tree::from_str(std::str::from_utf8(bytes).ok()?, &options).ok()?;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(32, 32)?;
        let scale = (32.0 / tree.size().width()).min(32.0 / tree.size().height());
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        // tiny-skia returns premultiplied pixels; GPUI expects straight BGRA.
        let pixels: Vec<u8> = pixmap
            .pixels()
            .iter()
            .flat_map(|pixel| {
                let pixel = pixel.demultiply();
                [pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()]
            })
            .collect();
        image::RgbaImage::from_raw(32, 32, pixels)?
    };
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(rgba)])))
}

const PAGE_LIMIT: u64 = 512 * 1024;

/// The icon a page names, resolved against `base`. A plain `icon` comes
/// before an Apple touch icon, one for every color scheme before one for a
/// single scheme, and among those the size nearest a 32-pixel one, the size
/// a result's mark is drawn at on a Retina display; a tie goes to the first.
fn declared_icon(page: &str, base: &Url) -> Option<Url> {
    let dom = parse_document(RcDom::default(), ParseOpts::default()).one(page);
    let mut links = Vec::new();
    collect_links(&dom.document, &mut links);
    links
        .into_iter()
        .filter_map(|link| {
            let rel = link.get("rel")?.to_ascii_lowercase();
            let touch = if rel.split_ascii_whitespace().any(|token| token == "icon") {
                false
            } else if rel.contains("apple-touch-icon") {
                true
            } else {
                return None;
            };
            let href = base.join(link.get("href")?).ok()?;
            if !matches!(href.scheme(), "http" | "https") {
                return None;
            }
            let rank = (
                touch,
                link.get("media").is_some(),
                size_cost(link.get("sizes")),
            );
            Some((rank, href))
        })
        .min_by_key(|(rank, _)| *rank)
        .map(|(_, href)| href)
}

/// How far a declared size is from 32 pixels, smaller ones further: they
/// blur when scaled up. `any` (a vector icon) and an undeclared size cost
/// nothing.
fn size_cost(sizes: Option<&str>) -> u32 {
    let width = sizes
        .and_then(|sizes| sizes.split_ascii_whitespace().next())
        .and_then(|size| size.split(['x', 'X']).next()?.parse::<u32>().ok());
    match width {
        None => 0,
        Some(width) if width >= 32 => width - 32,
        Some(width) => 1000 + 32 - width,
    }
}

/// Every `<link>` in the document, as its attributes.
fn collect_links(node: &Handle, links: &mut Vec<Attributes>) {
    let mut pending = vec![node.clone()];
    while let Some(node) = pending.pop() {
        if let NodeData::Element { name, attrs, .. } = &node.data
            && name.local == local_name!("link")
        {
            let attrs = attrs.borrow();
            links.push(Attributes(
                attrs
                    .iter()
                    .map(|attr| (attr.name.local.to_string(), attr.value.to_string()))
                    .collect(),
            ));
        }
        pending.extend(node.children.borrow().iter().rev().cloned());
    }
}

struct Attributes(Vec<(String, String)>);

impl Attributes {
    fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::AppContext as _;

    fn icon(page: &str) -> Option<String> {
        declared_icon(page, &Url::parse("https://lib.rs/").unwrap()).map(String::from)
    }

    #[gpui_kit::test]
    async fn a_stalled_server_falls_back_after_the_deadline(cx: &mut gpui_kit::TestAppContext) {
        let future = cx.update(|cx| {
            cx.set_http_client(gpui_kit::http_client::FakeHttpClient::create(|_| async {
                futures::future::pending().await
            }));
            SiteIcon::load(Url::parse("https://example.test/").unwrap(), cx)
        });
        let task = cx.background_spawn(future);
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_secs(6));
        cx.run_until_parked();
        assert_eq!(task.await.as_ref(), "https://example.test/favicon.ico");
    }

    #[test]
    fn icon_body_rejects_oversized_responses() {
        let bytes = vec![0; ICON_LIMIT as usize + 1];
        assert!(
            futures::executor::block_on(bounded_icon_body(futures::io::Cursor::new(bytes)))
                .is_none()
        );
        assert_eq!(
            futures::executor::block_on(bounded_icon_body(futures::io::Cursor::new(b"icon")))
                .unwrap(),
            b"icon"
        );
    }

    #[gpui_kit::test]
    async fn icon_fetch_has_its_own_deadline(cx: &mut gpui_kit::TestAppContext) {
        let future = cx.update(|cx| {
            cx.set_http_client(gpui_kit::http_client::FakeHttpClient::create(|_| async {
                futures::future::pending().await
            }));
            IconImage::load("https://example.test/icon.png".into(), cx)
        });
        let task = cx.background_spawn(future);
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_secs(6));
        cx.run_until_parked();
        assert!(task.await.is_none());
    }

    #[test]
    fn raster_dimensions_are_bounded_before_decode() {
        for (width, accepted) in [(32, true), (ICON_DIMENSION + 1, false)] {
            let mut bytes = Cursor::new(Vec::new());
            image::DynamicImage::new_rgba8(width, 1)
                .write_to(&mut bytes, image::ImageFormat::Png)
                .unwrap();
            assert_eq!(decode_icon(bytes.get_ref()).is_some(), accepted);
        }
    }

    #[test]
    fn svg_output_is_bounded_and_nested_images_are_disabled() {
        let icon = decode_icon(br#"<svg xmlns="http://www.w3.org/2000/svg" width="1000000" height="1000000"><rect width="1000000" height="1000000" fill="red"/></svg>"#).unwrap();
        assert_eq!(icon.size(0), gpui_kit::size(32.into(), 32.into()));
        assert_eq!(icon.frame_count(), 1);
        assert_eq!(&icon.as_bytes(0).unwrap()[..4], &[0, 0, 255, 255]);
        let nested = decode_icon(br#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><image width="32" height="32" href="file:///private/icon.png"/></svg>"#).unwrap();
        assert!(nested.as_bytes(0).unwrap().iter().all(|byte| *byte == 0));
    }

    #[test]
    fn deeply_nested_links_do_not_recurse_and_local_files_are_rejected() {
        let html = format!(
            "{}<link rel=icon href=/safe.svg>{}",
            "<div>".repeat(10000),
            "</div>".repeat(10000)
        );
        assert_eq!(icon(&html).as_deref(), Some("https://lib.rs/safe.svg"));
        assert_eq!(icon("<link rel=icon href=file:///private/icon.png>"), None);
    }

    #[test]
    fn a_page_names_its_icon_in_its_head() {
        assert_eq!(
            icon(r#"<link rel="icon" href="/logo.svg">"#).as_deref(),
            Some("https://lib.rs/logo.svg")
        );
        assert_eq!(
            icon(r#"<link rel="shortcut icon" href="https://cdn.example/f.ico">"#).as_deref(),
            Some("https://cdn.example/f.ico")
        );
    }

    #[test]
    fn an_icon_for_every_color_scheme_near_32_pixels_wins() {
        let page = r#"
            <link rel=icon href="/favicon.png" sizes=32x32 media="(prefers-color-scheme: light)">
            <link rel=icon href="/favicon-dark.png" sizes=32x32 media="(prefers-color-scheme: dark)">
            <link rel=icon href="/crates-logo.png" sizes=256x256>
            <link rel=icon href="/favicon-16.png" sizes=16x16>
            <link rel=icon href="/favicon.png" sizes=32x32>
        "#;
        assert_eq!(icon(page).as_deref(), Some("https://lib.rs/favicon.png"));
    }

    #[test]
    fn a_touch_icon_serves_only_where_no_icon_is_named() {
        let touch = r#"<link rel="apple-touch-icon" href="/touch.png">"#;
        assert_eq!(icon(touch).as_deref(), Some("https://lib.rs/touch.png"));
        let both = format!(r#"{touch}<link rel="icon" href="/icon.png" sizes="16x16">"#);
        assert_eq!(icon(&both).as_deref(), Some("https://lib.rs/icon.png"));
    }

    #[test]
    fn other_links_name_no_icon() {
        let page = r#"
            <link rel="mask-icon" href="/pinned.svg">
            <link rel="stylesheet" href="/site.css">
            <link rel="icon">
        "#;
        assert_eq!(icon(page), None);
    }
}
