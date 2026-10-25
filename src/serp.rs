//! A natively rendered search results page.
//!
//! `Serp` runs one RustyWeb search when it is created and draws the outcome:
//! a header, an optional answer (Ask mode only), the results, and quiet notes.
//! It never navigates by itself. Activating a result emits [`SerpEvent::Open`]
//! and the host decides which tab loads the page.

use gpui_kit::base::Link;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    alert::Alert,
    button::Button,
    empty::{
        Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant,
        EmptyTitle,
    },
    h_flex,
    scroll::ScrollableElement as _,
    skeleton::Skeleton,
    spinner::Spinner,
    tag::Tag,
    v_flex,
};
use gpui_kit::{
    App, ClickEvent, Context, EventEmitter, FocusHandle, Focusable, FontWeight, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, Styled, Task, Window, div,
    prelude::FluentBuilder as _, rems,
};
use url::Url;

use crate::search::{self, SerpAnswer, SerpHit, SerpResult, SerpSource, sentence};

/// How the page was asked for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SerpMode {
    /// Results only. No model runs.
    Search,
    /// Results plus an answer drawn from them, when a model is configured.
    Ask,
}

/// What the host should do next.
#[derive(Clone, Debug, PartialEq)]
pub enum SerpEvent {
    /// Load `url`, in a new tab when `new_tab` is set (the platform secondary
    /// modifier was held: command on macOS).
    Open { url: Url, new_tab: bool },
}

enum State {
    Loading,
    Loaded(SerpResult),
    Failed(SharedString),
}

pub struct Serp {
    query: SharedString,
    mode: SerpMode,
    state: State,
    /// Bumped on every search so a slow, superseded search cannot overwrite a
    /// newer one.
    revision: u64,
    focus: FocusHandle,
    /// Held so the search is cancelled if the page is closed first.
    _search: Task<()>,
}

impl EventEmitter<SerpEvent> for Serp {}

impl Focusable for Serp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Serp {
    /// Create the page and start the search immediately.
    pub fn new(
        query: impl Into<SharedString>,
        mode: SerpMode,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = query.into();
        let revision = 1;
        let search = Self::start(&query, mode, revision, cx);
        Self {
            query,
            mode,
            state: State::Loading,
            revision,
            focus: cx.focus_handle(),
            _search: search,
        }
    }

    pub fn query(&self) -> &SharedString {
        &self.query
    }

    /// Tab label.
    pub fn title(&self) -> SharedString {
        self.query.clone()
    }

    /// Search again for the same query.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.revision += 1;
        self.state = State::Loading;
        self._search = Self::start(&self.query, self.mode, self.revision, cx);
        cx.notify();
    }

    fn start(
        query: &SharedString,
        mode: SerpMode,
        revision: u64,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let search = search::run(query.to_string(), mode == SerpMode::Ask, cx);
        cx.spawn(async move |this, cx| {
            let outcome = search.await;
            this.update(cx, |this, cx| this.finish(revision, outcome, cx))
                .ok();
        })
    }

    fn finish(
        &mut self,
        revision: u64,
        outcome: anyhow::Result<SerpResult>,
        cx: &mut Context<Self>,
    ) {
        if revision != self.revision {
            return;
        }
        self.state = match outcome {
            Ok(result) => State::Loaded(result),
            Err(error) => {
                log::error!("serp: search for {:?} failed: {error:#}", self.query);
                State::Failed(format!("{error:#}").into())
            }
        };
        cx.notify();
    }

    /// A title or source link. The page is opened by the host, never here.
    fn web_link(&self, id: SharedString, url: &Url, cx: &Context<Self>) -> Link {
        let target = url.clone();
        Link::new(id)
            .href(url.as_str().to_string())
            .on_activate(cx.listener(move |_, event: &ClickEvent, _, cx| {
                cx.emit(SerpEvent::Open {
                    url: target.clone(),
                    new_tab: event.modifiers().secondary(),
                });
            }))
            .text_color(cx.theme().link)
            .rounded_sm()
            .hover(|style| style.text_color(cx.theme().link_hover).underline())
            .focus_visible(|style| {
                style
                    .bg(cx.theme().accent)
                    .text_color(cx.theme().accent_foreground)
            })
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let subline = match &self.state {
            State::Loading => h_flex()
                .gap_2()
                .child(Spinner::new().small())
                .child("Searching…")
                .into_any_element(),
            State::Loaded(result) => div().child(summary(result)).into_any_element(),
            State::Failed(_) => div().into_any_element(),
        };
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .min_w_0()
                            .text_xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.query.clone()),
                    )
                    .when(self.mode == SerpMode::Ask, |this| {
                        this.child(Tag::info().small().child("Ask"))
                    }),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(subline),
            )
    }

    fn loading(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let line = |width: f32, height: f32| Skeleton::new().w(rems(width)).h(rems(height));
        v_flex()
            .gap_6()
            .children((0..4).map(|_| {
                v_flex()
                    .gap_2()
                    .child(line(10., 0.75))
                    .child(line(26., 1.1))
                    .child(line(40., 0.75))
                    .child(line(32., 0.75))
            }))
            .text_color(cx.theme().muted_foreground)
    }

    fn failed(&self, message: &SharedString, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_3()
            .child(
                Alert::error(
                    "serp-error",
                    format!(
                        "{} Check your connection, then try again.",
                        sentence(message)
                    ),
                )
                .title("Search failed"),
            )
            .child(
                div().child(
                    Button::new("serp-retry")
                        .label("Try again")
                        .icon(IconName::RotateCw)
                        .outline()
                        .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                ),
            )
    }

    fn empty(&self, cx: &mut Context<Self>) -> impl IntoElement {
        Empty::new()
            .header(
                EmptyHeader::new()
                    .media(
                        EmptyMedia::new()
                            .with_variant(EmptyMediaVariant::Icon)
                            .child(Icon::new(IconName::Search)),
                    )
                    .title(EmptyTitle::new().child(format!("No results for “{}”", self.query)))
                    .description(
                        EmptyDescription::new().child("Check the spelling or try different words."),
                    ),
            )
            .content(
                EmptyContent::new().child(
                    Button::new("serp-search-again")
                        .label("Search again")
                        .icon(IconName::RotateCw)
                        .outline()
                        .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                ),
            )
    }

    fn answer(&self, result: &SerpResult, cx: &mut Context<Self>) -> impl IntoElement {
        let sources = unique_sources(result.answer.as_ref());
        let body = match (&result.answer, &result.answer_note) {
            (Some(answer), _) => v_flex()
                .gap_3()
                .children(answer.claims.iter().map(|claim| {
                    let markers = claim
                        .sources
                        .iter()
                        .filter_map(|source| sources.iter().position(|s| s.url == source.url))
                        .map(|ix| format!("[{}]", ix + 1))
                        .collect::<Vec<_>>()
                        .join(" ");
                    h_flex()
                        .items_start()
                        .gap_2()
                        .when(claim.disputed, |this| {
                            this.child(Tag::warning().small().child("Disputed"))
                        })
                        .child(div().min_w_0().child(if markers.is_empty() {
                            claim.text.clone()
                        } else {
                            format!("{} {markers}", claim.text)
                        }))
                }))
                .when(!sources.is_empty(), |this| {
                    this.child(h_flex().flex_wrap().gap_x_3().gap_y_1().text_xs().children(
                        sources.iter().enumerate().map(|(ix, source)| {
                            self.web_link(
                                format!("serp-source:{}", element_key(&source.url)).into(),
                                &source.url,
                                cx,
                            )
                            .accessibility_label(source.title.clone())
                            .child(format!(
                                "[{}] {}",
                                ix + 1,
                                source.host
                            ))
                        }),
                    ))
                })
                .into_any_element(),
            (None, Some(note)) => div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(note.clone())
                .into_any_element(),
            (None, None) => div().into_any_element(),
        };

        v_flex()
            .id("serp-answer")
            .gap_3()
            .p_4()
            .rounded(cx.theme().radius_tokens().lg)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary)
            .child(
                h_flex()
                    .gap_2()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(Icon::new(IconName::Bot).small())
                    .child("Answer")
                    .when_some(
                        result.answer.as_ref().filter(|a| !a.model.is_empty()),
                        |this, answer| {
                            this.child(
                                Tag::secondary()
                                    .outline()
                                    .small()
                                    .child(answer.model.clone()),
                            )
                        },
                    ),
            )
            .child(body)
    }

    fn result_row(&self, hit: &SerpHit, cx: &mut Context<Self>) -> impl IntoElement {
        let key = element_key(&hit.url);
        let local = search::lane_label(&hit.lane) == "Local";
        v_flex()
            .id(SharedString::from(format!("serp-result:{key}")))
            .gap_1()
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(display_url(&hit.url)),
            )
            .child(
                self.web_link(format!("serp-link:{key}").into(), &hit.url, cx)
                    .accessibility_label(format!("{}, {}", hit.title, hit.host))
                    .text_base()
                    .font_weight(FontWeight::MEDIUM)
                    .child(hit.title.clone()),
            )
            .when(!hit.snippet.is_empty(), |this| {
                this.child(div().text_sm().line_clamp(3).child(hit.snippet.clone()))
            })
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_x_2()
                    .gap_y_1()
                    .child(if local {
                        Tag::primary()
                            .outline()
                            .small()
                            .child(search::lane_label(&hit.lane))
                    } else {
                        Tag::secondary()
                            .outline()
                            .small()
                            .child(search::lane_label(&hit.lane))
                    })
                    .children(
                        hit.providers
                            .iter()
                            .map(|provider| Tag::secondary().small().child(provider.clone())),
                    )
                    .when_some(hit.published_at.clone(), |this, date| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(date),
                        )
                    }),
            )
    }

    fn notes(&self, result: &SerpResult, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let lines: Vec<String> = result
            .notes
            .iter()
            .cloned()
            .chain(result.degradations.iter().map(|d| d.message.clone()))
            .collect();
        if lines.is_empty() {
            return None;
        }
        Some(
            v_flex()
                .id("serp-notes")
                .gap_1()
                .pt_4()
                .border_t_1()
                .border_color(cx.theme().border)
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .children(lines.into_iter().map(|line| {
                    h_flex()
                        .items_start()
                        .gap_2()
                        .child(Icon::new(IconName::Info).xsmall())
                        .child(div().min_w_0().child(line))
                })),
        )
    }
}

impl Render for Serp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.state {
            State::Loading => self.loading(cx).into_any_element(),
            State::Failed(message) => self.failed(message, cx).into_any_element(),
            State::Loaded(result) => v_flex()
                .gap_6()
                .when(self.mode == SerpMode::Ask, |this| {
                    this.child(self.answer(result, cx))
                })
                .child(if result.hits.is_empty() {
                    self.empty(cx).into_any_element()
                } else {
                    let mut rows = Vec::with_capacity(result.hits.len());
                    for hit in &result.hits {
                        rows.push(self.result_row(hit, cx).into_any_element());
                    }
                    v_flex().gap_5().children(rows).into_any_element()
                })
                .children(self.notes(result, cx))
                .into_any_element(),
        };

        div()
            .id("serp")
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .track_focus(&self.focus)
            .child(
                v_flex()
                    .size_full()
                    .overflow_y_scrollbar()
                    .id("serp-scroll")
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(rems(46.))
                            .mx_auto()
                            .px_6()
                            .py_6()
                            .gap_6()
                            .child(self.header(cx))
                            .child(body),
                    ),
            )
    }
}

/// `host/path?query`, lowercase host: stable for one page, so it can name its
/// elements.
fn element_key(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.query() {
        Some(query) => format!("{host}{}?{query}", url.path()),
        None => format!("{host}{}", url.path()),
    }
}

/// The URL line above a title: host and path without the scheme.
fn display_url(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    let host = host.strip_prefix("www.").unwrap_or(host);
    let path = url.path().trim_end_matches('/');
    format!("{host}{path}")
}

/// Count, lane mix, providers and time on one line.
fn summary(result: &SerpResult) -> String {
    let count = result.hits.len();
    let mut parts = vec![format!(
        "{count} {}",
        if count == 1 { "result" } else { "results" }
    )];

    let mut lanes: Vec<(String, usize)> = Vec::new();
    for hit in &result.hits {
        let label = search::lane_label(&hit.lane).to_lowercase();
        match lanes.iter_mut().find(|(name, _)| *name == label) {
            Some((_, n)) => *n += 1,
            None => lanes.push((label, 1)),
        }
    }
    if !lanes.is_empty() {
        parts.push(
            lanes
                .iter()
                .map(|(name, n)| format!("{n} {name}"))
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    if !result.providers.is_empty() {
        parts.push(result.providers.join(", "));
    }
    parts.push(format!("{:.1} s", result.elapsed_ms as f64 / 1000.0));
    parts.join(" · ")
}

/// Every source cited by the answer, once, in first-cited order.
fn unique_sources(answer: Option<&SerpAnswer>) -> Vec<SerpSource> {
    let mut unique: Vec<SerpSource> = Vec::new();
    for source in answer
        .iter()
        .flat_map(|a| &a.claims)
        .flat_map(|c| &c.sources)
    {
        if !unique.iter().any(|seen| seen.url == source.url) {
            unique.push(source.clone());
        }
    }
    unique
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::SerpClaim;

    fn hit(url: &str, lane: &str) -> SerpHit {
        let url = Url::parse(url).unwrap();
        SerpHit {
            title: "Title".into(),
            host: url.host_str().unwrap().into(),
            url,
            snippet: String::new(),
            providers: vec![],
            lane: lane.into(),
            published_at: None,
        }
    }

    fn result(hits: Vec<SerpHit>) -> SerpResult {
        SerpResult {
            query: "q".into(),
            ask: false,
            providers: vec!["gdelt".into(), "bluesky".into()],
            hits,
            degradations: vec![],
            answer: None,
            answer_note: None,
            notes: vec![],
            elapsed_ms: 1840,
        }
    }

    #[test]
    fn element_keys_come_from_the_url() {
        let a = Url::parse("https://Example.com/a/b?x=1#frag").unwrap();
        assert_eq!(element_key(&a), "example.com/a/b?x=1");
        let b = Url::parse("https://example.com/a/b").unwrap();
        assert_ne!(element_key(&a), element_key(&b));
    }

    #[test]
    fn display_url_drops_scheme_www_and_trailing_slash() {
        let url = Url::parse("https://www.example.com/docs/").unwrap();
        assert_eq!(display_url(&url), "example.com/docs");
        let root = Url::parse("https://example.com/").unwrap();
        assert_eq!(display_url(&root), "example.com");
    }

    #[test]
    fn summary_counts_lanes_in_order() {
        let r = result(vec![
            hit("https://a.example/1", "warm"),
            hit("https://b.example/2", "fresh"),
            hit("https://c.example/3", "fresh"),
        ]);
        assert_eq!(
            summary(&r),
            "3 results · 1 local, 2 fresh · gdelt, bluesky · 1.8 s"
        );
        let one = result(vec![hit("https://a.example/1", "fresh")]);
        assert!(summary(&one).starts_with("1 result · "));
    }

    #[test]
    fn answer_sources_are_listed_once() {
        let source = |u: &str| SerpSource {
            title: "T".into(),
            url: Url::parse(u).unwrap(),
            host: "h".into(),
        };
        let answer = SerpAnswer {
            model: "m".into(),
            claims: vec![
                SerpClaim {
                    text: "one".into(),
                    disputed: false,
                    sources: vec![source("https://a.example/"), source("https://b.example/")],
                },
                SerpClaim {
                    text: "two".into(),
                    disputed: false,
                    sources: vec![source("https://b.example/"), source("https://c.example/")],
                },
            ],
        };
        let urls: Vec<_> = unique_sources(Some(&answer))
            .iter()
            .map(|s| s.url.to_string())
            .collect();
        assert_eq!(
            urls,
            [
                "https://a.example/",
                "https://b.example/",
                "https://c.example/"
            ]
        );
        assert!(unique_sources(None).is_empty());
    }
}
