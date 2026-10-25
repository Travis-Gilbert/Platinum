//! Where a site keeps its icon. Most sites name it in their home page's
//! `<link rel=icon>`, many only there: `gpui-kit.com/logo.svg`,
//! `lib.rs/favicon.png`. The rest answer at `/favicon.ico`, which is also
//! where a browser looks when a page names none. [`SiteIcon`] reads the home
//! page once per site, through GPUI's asset cache, and gives the address for
//! GPUI's image loader to fetch and decode.
//!
//! SOURCING: GPUI's HTTP client fetches and html5ever parses, as for search
//! snippets (`markup.rs`). Hand-rolled because the crates that pick a site's
//! icon from its page, `site_icons` and `favicon-picker`, are GPL-3.0, and
//! Frame is MIT.

use futures::AsyncReadExt as _;
use gpui_kit::{App, Asset, SharedUri};
use html5ever::tendril::TendrilSink as _;
use html5ever::{ParseOpts, local_name, parse_document};
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use url::Url;

/// A site's icon address, from the root of the site (`https://lib.rs/`).
pub enum SiteIcon {}

impl Asset for SiteIcon {
    type Source = Url;
    type Output = SharedUri;

    fn load(root: Url, cx: &mut App) -> impl Future<Output = SharedUri> + Send + 'static {
        let client = cx.http_client();
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
            let icon = match declared.await {
                Some(icon) => icon,
                None => root.join("/favicon.ico").unwrap_or(root),
            };
            String::from(icon).into()
        }
    }
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
            let rank = (touch, link.get("media").is_some(), size_cost(link.get("sizes")));
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
    for child in node.children.borrow().iter() {
        collect_links(child, links);
    }
}

struct Attributes(Vec<(String, String)>);

impl Attributes {
    fn get(&self, name: &str) -> Option<&str> {
        self.0.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn icon(page: &str) -> Option<String> {
        declared_icon(page, &Url::parse("https://lib.rs/").unwrap()).map(String::from)
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
