//! A natively rendered search results page, laid out the way people know
//! from web search: the scopes, then each result as its site, where it
//! leads, its title and its excerpt, with space between results rather than
//! rules. Ask puts an answer drawn from the results above them. How many
//! results came back and how long the search took is a quiet line after the
//! last one.
//!
//! Where a result came from (Frame's local index or the web, which
//! providers, when it was published and when it was retrieved) is behind its
//! ⋮ button, in About this result, with the ways to open it.
//!
//! The page works from the keyboard: Up and Down move through the results,
//! Enter opens one, ⌘Enter opens it in a new tab. It never navigates by
//! itself. Opening emits [`SerpEvent::Open`] and the host decides which tab
//! loads the page.
//!
//! Nothing here rests on color alone: the selected row has a fill and a bar
//! at its leading edge, a site is marked by its initials, and a local result
//! carries a drive icon and the word Local. A web result carries no word:
//! the web is what a search finds unless it says otherwise.
//!
//! SOURCING: GPUI Kit's `List`, `TabBar`, `Popover`, `Avatar`,
//! `DescriptionList`, `Alert`, `Empty`, `Skeleton` and `Button`; sites are
//! named by Servo's public suffix list (`net_traits::pub_domains`), snippets
//! are parsed by html5ever (`markup.rs`), dates are chrono's.

use chrono::{DateTime, Local, NaiveDate};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::Link;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, IndexPath, Sizable as _,
    alert::Alert,
    avatar::Avatar,
    button::{Button, ButtonVariants as _},
    description_list::DescriptionList,
    empty::{
        Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant,
        EmptyTitle,
    },
    h_flex,
    list::{List, ListDelegate, ListItem, ListState},
    popover::{Popover, PopoverState},
    scroll::ScrollableElement as _,
    skeleton::Skeleton,
    tab::{Tab, TabBar},
    tag::Tag,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, EventEmitter, FocusHandle,
    Focusable, FontWeight, HighlightStyle, InteractiveElement, IntoElement, ParentElement, Render,
    Rems, ScrollStrategy, SharedString, Styled, StyledText, Task, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px, relative, rems,
};
use percent_encoding::percent_decode_str;
use url::Url;

use crate::search::{self, SerpAnswer, SerpHit, SerpResult, SerpSource, sentence};

/// Every row is this tall: the list measures one and lays out the rest.
const ROW_HEIGHT: Rems = rems(8.75);

/// The line after the last result.
const FOOTER_HEIGHT: Rems = rems(3.5);

/// The results' distance from the page's leading edge, as a share of its
/// width: a wide page holds them in from the edge, a narrow one barely.
const GUTTER: f32 = 0.05;

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

/// Which results the list shows.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Scope {
    #[default]
    All,
    Local,
    Web,
}

impl Scope {
    const ALL: [Scope; 3] = [Scope::All, Scope::Local, Scope::Web];

    fn label(self) -> &'static str {
        match self {
            Scope::All => "All",
            Scope::Local => "Local",
            Scope::Web => "Web",
        }
    }

    fn includes(self, hit: &SerpHit) -> bool {
        match self {
            Scope::All => true,
            Scope::Local => search::is_local_lane(&hit.lane),
            Scope::Web => !search::is_local_lane(&hit.lane),
        }
    }
}

pub struct Serp {
    query: SharedString,
    mode: SerpMode,
    state: State,
    /// Bumped on every search so a slow, superseded search cannot overwrite a
    /// newer one.
    revision: u64,
    scope: Scope,
    focus: FocusHandle,
    list: Entity<ListState<Results>>,
    /// Held so the search is cancelled if the page is closed first.
    _search: Task<()>,
}

impl EventEmitter<SerpEvent> for Serp {}

impl Focusable for Serp {
    /// The results once there are some, so the arrow keys work at once.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if self.has_rows(cx) {
            self.list.focus_handle(cx)
        } else {
            self.focus.clone()
        }
    }
}

impl Serp {
    /// Create the page and start the search immediately.
    pub fn new(
        query: impl Into<SharedString>,
        mode: SerpMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut serp = Self::build(query.into(), mode, window, cx);
        serp._search = Self::start(&serp.query, mode, serp.revision, window, cx);
        serp
    }

    fn build(
        query: SharedString,
        mode: SerpMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let results = Results {
            serp: cx.weak_entity(),
            hits: Vec::new(),
            selected: None,
            searched_at: None,
            summary: None,
        };
        let list = cx.new(|cx| ListState::new(results, window, cx));
        Self {
            query,
            mode,
            state: State::Loading,
            revision: 1,
            scope: Scope::All,
            focus: cx.focus_handle(),
            list,
            _search: Task::ready(()),
        }
    }

    /// A page showing `result`, with no search behind it.
    #[cfg(test)]
    pub fn loaded(result: SerpResult, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mode = if result.ask {
            SerpMode::Ask
        } else {
            SerpMode::Search
        };
        let mut serp = Self::build(result.query.clone().into(), mode, window, cx);
        serp.state = State::Loaded(result);
        serp.show_scope(window, cx);
        serp
    }

    pub fn query(&self) -> &SharedString {
        &self.query
    }

    /// Tab label.
    pub fn title(&self) -> SharedString {
        self.query.clone()
    }

    /// Search again for the same query.
    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.revision += 1;
        self.state = State::Loading;
        self.show_scope(window, cx);
        self._search = Self::start(&self.query, self.mode, self.revision, window, cx);
        cx.notify();
    }

    fn start(
        query: &SharedString,
        mode: SerpMode,
        revision: u64,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let search = search::run(query.to_string(), mode == SerpMode::Ask, cx);
        cx.spawn_in(window, async move |this, cx| {
            let outcome = search.await;
            this.update_in(cx, |this, window, cx| {
                this.finish(revision, outcome, window, cx)
            })
            .ok();
        })
    }

    fn finish(
        &mut self,
        revision: u64,
        outcome: anyhow::Result<SerpResult>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if revision != self.revision {
            return;
        }
        let had_focus = self.focus.is_focused(window);
        self.state = match outcome {
            Ok(result) => State::Loaded(result),
            Err(error) => {
                log::error!("serp: search for {:?} failed: {error:#}", self.query);
                State::Failed(format!("{error:#}").into())
            }
        };
        self.scope = Scope::All;
        self.show_scope(window, cx);
        // The keyboard follows the page into its results.
        if had_focus && self.has_rows(cx) {
            self.list.update(cx, |list, cx| list.focus(window, cx));
        }
        cx.notify();
    }

    fn has_rows(&self, cx: &App) -> bool {
        !self.list.read(cx).delegate().hits.is_empty()
    }

    /// Fill the list with the results in the current scope, the first one
    /// selected.
    fn show_scope(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (hits, searched_at, summary) = match &self.state {
            State::Loaded(result) => (
                result
                    .hits
                    .iter()
                    .filter(|hit| self.scope.includes(hit))
                    .cloned()
                    .collect(),
                Some(result.searched_at),
                Some(summary(result).into()),
            ),
            _ => (Vec::new(), None, None),
        };
        let first = (!hits.is_empty()).then(|| IndexPath::new(0));
        self.list.update(cx, |list, cx| {
            let results = list.delegate_mut();
            results.hits = hits;
            results.searched_at = searched_at;
            results.summary = summary;
            list.set_selected_index(first, window, cx);
            if let Some(first) = first {
                list.scroll_to_item(first, ScrollStrategy::Top, window, cx);
            }
            cx.notify();
        });
    }

    fn set_scope(&mut self, scope: Scope, window: &mut Window, cx: &mut Context<Self>) {
        if self.scope == scope {
            return;
        }
        self.scope = scope;
        self.show_scope(window, cx);
        self.list.update(cx, |list, cx| list.focus(window, cx));
        cx.notify();
    }

    /// An answer's source link. The page is opened by the host, never here.
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

    /// All, Local and Web, there from the start so the page does not shift
    /// when results arrive. A scope with nothing in it is disabled.
    fn scope_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let hits = match &self.state {
            State::Loaded(result) => result.hits.as_slice(),
            _ => &[],
        };
        let selected = Scope::ALL
            .iter()
            .position(|scope| *scope == self.scope)
            .unwrap_or(0);
        TabBar::new("serp-scope")
            .underline()
            .selected_index(selected)
            .on_click(cx.listener(|this, ix: &usize, window, cx| {
                this.set_scope(Scope::ALL[*ix], window, cx)
            }))
            .children(Scope::ALL.map(|scope| {
                let empty = !hits.iter().any(|hit| scope.includes(hit));
                Tab::new()
                    .label(scope.label())
                    .disabled(scope != Scope::All && empty)
            }))
    }

    /// Placeholder rows in the results' own shape while the search runs.
    fn loading(&self) -> impl IntoElement + use<> {
        let line = |width: f32, height: f32| Skeleton::new().w(rems(width)).h(rems(height));
        v_flex()
            .id("serp-loading")
            .gap_8()
            .pt_3()
            .children((0..4).map(move |_| {
                inset().child(
                    v_flex()
                        .gap_2()
                        .child(
                            h_flex()
                                .gap_3()
                                .child(Skeleton::new().size(px(24.)).rounded_full())
                                .child(v_flex().gap_1().child(line(8., 0.75)).child(line(14., 0.6))),
                        )
                        .child(line(24., 1.25))
                        .child(line(40., 0.75))
                        .child(line(32., 0.75)),
                )
            }))
    }

    fn failed(&self, message: &SharedString, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        v_flex()
            .gap_3()
            .pt_3()
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
                        .on_click(cx.listener(|this, _, window, cx| this.reload(window, cx))),
                ),
            )
    }

    fn empty(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
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
                        .on_click(cx.listener(|this, _, window, cx| this.reload(window, cx))),
                ),
            )
    }

    fn answer(&self, result: &SerpResult, cx: &mut Context<Self>) -> impl IntoElement + use<> {
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

        // Held to a few lines' height, so the results still start on screen.
        div()
            .flex_shrink_0()
            .max_h(rems(16.))
            .overflow_y_scrollbar()
            .id("serp-answer")
            .child(
                v_flex()
                    .gap_3()
                    .p_4()
                    .rounded(cx.theme().radius_tokens().lg)
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
                    .child(body),
            )
    }

    /// The answer, when asked for, then the results.
    fn results(&self, result: &SerpResult, cx: &mut Context<Self>) -> AnyElement {
        let ask = self.mode == SerpMode::Ask;
        v_flex()
            .flex_1()
            .min_h_0()
            .gap_4()
            .when(ask, |this| this.child(self.answer(result, cx)))
            .child(div().flex_1().min_h_0().child(List::new(&self.list).size_full()))
            .into_any_element()
    }
}

impl Render for Serp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.state {
            State::Loading => self.loading().into_any_element(),
            State::Failed(message) => self.failed(message, cx).into_any_element(),
            State::Loaded(result) if result.hits.is_empty() => v_flex()
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .id("serp-empty")
                .child(
                    v_flex()
                        .gap_6()
                        .pb_6()
                        .when(self.mode == SerpMode::Ask, |this| {
                            this.child(self.answer(result, cx))
                        })
                        .child(self.empty(cx)),
                )
                .into_any_element(),
            State::Loaded(result) => self.results(result, cx),
        };

        v_flex()
            .id("serp")
            .track_focus(&self.focus)
            .size_full()
            .px_6()
            .pt_3()
            .text_color(cx.theme().foreground)
            .child(
                v_flex().size_full().pl(relative(GUTTER)).child(
                    v_flex()
                        .h_full()
                        .w_full()
                        .max_w(rems(46.))
                        .gap_4()
                        .child(self.scope_bar(cx))
                        .child(body),
                ),
            )
    }
}

/// The rows the list shows: the loaded results in the current scope.
struct Results {
    serp: WeakEntity<Serp>,
    hits: Vec<SerpHit>,
    selected: Option<usize>,
    /// When the shown search ran: the retrieval time of its live results.
    searched_at: Option<DateTime<Local>>,
    /// The line after the last result.
    summary: Option<SharedString>,
}

impl ListDelegate for Results {
    type Item = ListItem;

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.hits.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let hit = self.hits.get(ix.row)?;
        Some(result_row(
            hit,
            self.selected == Some(ix.row),
            self.serp.clone(),
            self.searched_at?,
            cx,
        ))
    }

    fn render_section_footer(
        &mut self,
        _section: usize,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> {
        let summary = self.summary.clone()?;
        Some(
            inset()
                .h(FOOTER_HEIGHT)
                .items_center()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(summary),
        )
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.selected = ix.map(|ix| ix.row);
        cx.notify();
    }

    fn confirm(&mut self, secondary: bool, _window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let Some(hit) = self.selected.and_then(|row| self.hits.get(row)) else {
            return;
        };
        let url = hit.url.clone();
        self.serp
            .update(cx, |_, cx| {
                cx.emit(SerpEvent::Open {
                    url,
                    new_tab: secondary,
                })
            })
            .ok();
    }
}

/// A row's leading edge: its padding and the selection bar's width, so what
/// is not a result (placeholders, the footer) lines up with the results.
fn inset() -> gpui_kit::Div {
    div()
        .flex()
        .pl_2()
        .gap_3()
        .child(div().w(px(3.)).flex_shrink_0())
}

/// A result's row: its site, where it leads, its title and its excerpt.
/// Clicking it opens the page; its ⋮ button only shows where it came from.
fn result_row(
    hit: &SerpHit,
    selected: bool,
    serp: WeakEntity<Serp>,
    searched_at: DateTime<Local>,
    cx: &App,
) -> ListItem {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let local = search::is_local_lane(&hit.lane);
    let site = site(&hit.url);
    let key = element_key(&hit.url);
    let emphasis = HighlightStyle {
        font_weight: Some(FontWeight::SEMIBOLD),
        ..Default::default()
    };
    let snippet = StyledText::new(hit.snippet.text.clone()).with_highlights(
        hit.snippet
            .emphasis
            .iter()
            .map(|range| (range.clone(), emphasis)),
    );
    let mark = if local {
        Avatar::new()
            .small()
            .placeholder(IconName::HardDrive)
            .text_color(muted)
    } else {
        Avatar::new().small().name(site_mark(&site))
    };
    let about = {
        let hit = hit.clone();
        Popover::new("about")
            .trigger(
                Button::new("about-trigger")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::EllipsisVertical)
                    .accessibility_label("About this result")
                    .tooltip("About this result")
                    // The popover opens on mouse-down, which an accessibility
                    // press only becomes on a button that takes clicks.
                    .on_click(|_, _, _| {}),
            )
            .content(move |_, _, cx| about(&hit, searched_at, &serp, cx))
    };
    ListItem::new(SharedString::from(format!("serp-result:{key}")))
        .group("serp-result")
        .h(ROW_HEIGHT)
        .pl_2()
        .pr_3()
        .py_3()
        .rounded(theme.radius_tokens().lg)
        .accessibility_label(if local {
            format!("{}, {site}, Local", hit.title)
        } else {
            format!("{}, {site}", hit.title)
        })
        .child(
            // Not an `h_flex`: rows stretch, so the selection bar spans the row.
            div()
                .flex()
                .w_full()
                .min_w_0()
                .gap_3()
                .child(
                    div()
                        .w(px(3.))
                        .flex_shrink_0()
                        .rounded_full()
                        .when(selected, |this| this.bg(theme.foreground)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(
                            h_flex()
                                .min_w_0()
                                .gap_3()
                                .child(mark)
                                .child(
                                    v_flex()
                                        .min_w_0()
                                        .child(
                                            h_flex()
                                                .min_w_0()
                                                .gap_1p5()
                                                .text_sm()
                                                .line_height(rems(1.25))
                                                .child(div().min_w_0().truncate().child(site))
                                                .when(local, |this| {
                                                    this.child(
                                                        div()
                                                            .flex_shrink_0()
                                                            .text_color(muted)
                                                            .child("· Local"),
                                                    )
                                                }),
                                        )
                                        .child(
                                            div()
                                                .min_w_0()
                                                .truncate()
                                                .text_xs()
                                                .line_height(rems(1.))
                                                .text_color(muted)
                                                .child(breadcrumb(&hit.url)),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .debug_selector(move || format!("serp-about:{key}"))
                                        .child(about),
                                ),
                        )
                        .child(
                            div()
                                .mt_2()
                                .min_w_0()
                                .truncate()
                                .text_xl()
                                .line_height(rems(1.75))
                                .text_color(theme.link)
                                .group_hover("serp-result", |style| style.underline())
                                .child(hit.title.clone()),
                        )
                        .child(
                            div()
                                .mt_1()
                                .h(rems(2.5))
                                .text_sm()
                                .line_height(rems(1.25))
                                .line_clamp(2)
                                .text_ellipsis()
                                .child(snippet),
                        ),
                ),
        )
}

/// About this result: where it came from, and the ways to open it.
fn about(
    hit: &SerpHit,
    searched_at: DateTime<Local>,
    serp: &WeakEntity<Serp>,
    cx: &mut Context<PopoverState>,
) -> impl IntoElement + use<> {
    let popover = cx.entity();
    let muted = cx.theme().muted_foreground;
    let open = |new_tab: bool| {
        let url = hit.url.clone();
        let serp = serp.clone();
        let popover = popover.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            popover.update(cx, |popover, cx| popover.dismiss(window, cx));
            // The popover is drawn by the list's row, which has to redraw
            // to close it.
            window.refresh();
            serp.update(cx, |_, cx| {
                cx.emit(SerpEvent::Open {
                    url: url.clone(),
                    new_tab,
                })
            })
            .ok();
        }
    };
    v_flex()
        .debug_selector(|| "serp-about".into())
        .w(rems(22.))
        .gap_3()
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(muted)
                .child("About this result"),
        )
        .child(
            v_flex()
                .gap_1()
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .line_clamp(2)
                        .text_ellipsis()
                        .child(hit.title.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis_middle()
                        .child(hit.url.to_string()),
                ),
        )
        .child(
            DescriptionList::horizontal()
                .columns(1)
                .bordered(false)
                .label_width(rems(5.5))
                .small()
                .item("Found in", search::lane_origin(&hit.lane), 1)
                .item("Providers", providers(hit), 1)
                .item("Published", published(hit), 1)
                .item("Retrieved", retrieved(hit, searched_at), 1),
        )
        .child(
            h_flex()
                .flex_wrap()
                .gap_2()
                .child(
                    Button::new("serp-open")
                        .primary()
                        .small()
                        .label("Open")
                        .on_click(open(false)),
                )
                .child(
                    div().debug_selector(|| "serp-open-new-tab".into()).child(
                        Button::new("serp-open-new-tab")
                            .outline()
                            .small()
                            .label("Open in New Tab")
                            .on_click(open(true)),
                    ),
                ),
        )
}

fn providers(hit: &SerpHit) -> String {
    match (hit.providers.is_empty(), search::is_local_lane(&hit.lane)) {
        (false, _) => hit.providers.join(", "),
        (true, true) => "None: Frame kept it".to_string(),
        (true, false) => "Not reported".to_string(),
    }
}

/// The page's own date, as words: `2026-10-24` is "24 Oct 2026".
fn published(hit: &SerpHit) -> String {
    match hit.published_at.as_deref() {
        Some(date) => match NaiveDate::parse_from_str(date, "%Y-%m-%d") {
            Ok(date) => date.format("%-d %b %Y").to_string(),
            Err(_) => date.to_string(),
        },
        None => "Not given".to_string(),
    }
}

/// When Frame or RustyWeb fetched the page, which is not when it was
/// published: live results now, local ones when Frame kept a copy.
fn retrieved(hit: &SerpHit, searched_at: DateTime<Local>) -> String {
    if search::is_local_lane(&hit.lane) {
        "Before this search, when Frame kept a copy".to_string()
    } else if hit.lane == "fresh" {
        format!("During this search, at {}", searched_at.format("%H:%M"))
    } else {
        "Not reported".to_string()
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

/// The site a result is on, by its registrable domain: `rust-lang.org` for
/// `doc.rust-lang.org`. An address that is not a domain stays as it is.
fn site(url: &Url) -> String {
    match url.host() {
        Some(url::Host::Domain(domain)) => net_traits::pub_domains::reg_suffix(domain).to_string(),
        Some(host) => host.to_string(),
        None => url.scheme().to_string(),
    }
}

/// What a site's mark is drawn from: its name without the public suffix,
/// `rust-lang` for `rust-lang.org`.
fn site_mark(site: &str) -> String {
    let suffix = net_traits::pub_domains::pub_suffix(site);
    site.strip_suffix(suffix)
        .map(|name| name.trim_end_matches('.'))
        .filter(|name| !name.is_empty())
        .unwrap_or(site)
        .to_string()
}

/// Where a result leads, the way web search writes it:
/// `https://doc.rust-lang.org › book › ch01-00`.
fn breadcrumb(url: &Url) -> String {
    let mut crumbs = format!("{}://{}", url.scheme(), url.host_str().unwrap_or_default());
    for segment in url.path_segments().into_iter().flatten() {
        if segment.is_empty() {
            continue;
        }
        crumbs.push_str(" › ");
        crumbs.push_str(&percent_decode_str(segment).decode_utf8_lossy());
    }
    crumbs
}

/// Count, lane mix, search time and duration on one line.
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
    parts.push(format!("searched at {}", result.searched_at.format("%H:%M")));
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
pub(crate) mod tests {
    use super::*;
    use crate::markup;
    use crate::search::SerpClaim;
    use chrono::TimeZone as _;
    use gpui_kit::component::Root;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext, size};
    use std::cell::RefCell;
    use std::rc::Rc;

    pub(crate) fn hit(url: &str, lane: &str) -> SerpHit {
        let url = Url::parse(url).unwrap();
        SerpHit {
            title: "Title".into(),
            host: url.host_str().unwrap().into(),
            url,
            snippet: markup::parse("An <b>excerpt</b>"),
            providers: vec![],
            lane: lane.into(),
            published_at: None,
        }
    }

    pub(crate) fn result(hits: Vec<SerpHit>) -> SerpResult {
        SerpResult {
            query: "q".into(),
            ask: false,
            providers: vec!["gdelt".into(), "bluesky".into()],
            hits,
            degradations: vec![],
            answer: None,
            answer_note: None,
            notes: vec![],
            searched_at: Local.with_ymd_and_hms(2026, 10, 24, 17, 31, 0).unwrap(),
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
    fn a_result_is_named_by_its_site_and_led_to_by_a_breadcrumb() {
        let url = Url::parse("https://doc.rust-lang.org/book/ch01%2000/").unwrap();
        assert_eq!(site(&url), "rust-lang.org");
        assert_eq!(site_mark(&site(&url)), "rust-lang");
        assert_eq!(breadcrumb(&url), "https://doc.rust-lang.org › book › ch01 00");

        let uk = Url::parse("https://www.bbc.co.uk/").unwrap();
        assert_eq!(site(&uk), "bbc.co.uk");
        assert_eq!(site_mark(&site(&uk)), "bbc");
        assert_eq!(breadcrumb(&uk), "https://www.bbc.co.uk");

        let ip = Url::parse("http://127.0.0.1:8080/a").unwrap();
        assert_eq!(site(&ip), "127.0.0.1");
    }

    #[test]
    fn summary_counts_lanes_and_says_when_it_searched() {
        let r = result(vec![
            hit("https://a.example/1", "warm"),
            hit("https://b.example/2", "fresh"),
            hit("https://c.example/3", "fresh"),
        ]);
        assert_eq!(
            summary(&r),
            "3 results · 1 local, 2 web · searched at 17:31 · 1.8 s"
        );
        let one = result(vec![hit("https://a.example/1", "fresh")]);
        assert!(summary(&one).starts_with("1 result · "));
    }

    #[test]
    fn publication_and_retrieval_are_told_apart() {
        let at = result(vec![]).searched_at;
        let mut web = hit("https://a.example/1", "fresh");
        web.published_at = Some("2024-10-24".into());
        assert_eq!(published(&web), "24 Oct 2024");
        assert_eq!(retrieved(&web, at), "During this search, at 17:31");

        let mut local = hit("https://b.example/2", "warm");
        local.published_at = Some("last spring".into());
        assert_eq!(published(&local), "last spring");
        assert!(retrieved(&local, at).starts_with("Before this search"));
        assert_eq!(published(&hit("https://c.example/3", "fresh")), "Not given");
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

    /// A results page in a headless window, and every `Open` it asks for.
    fn open_page(
        hits: Vec<SerpHit>,
        cx: &mut TestAppContext,
    ) -> (
        Entity<Serp>,
        Rc<RefCell<Vec<SerpEvent>>>,
        &mut VisualTestContext,
    ) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
        });
        let opened = Rc::new(RefCell::new(Vec::new()));
        let mut page = None;
        let mut subscription = None;
        let window = cx.open_window(size(px(1200.), px(800.)), |window, cx| {
            let serp = cx.new(|cx| Serp::loaded(result(hits), window, cx));
            let seen = opened.clone();
            subscription = Some(cx.subscribe(&serp, move |_, _, event: &SerpEvent, _| {
                seen.borrow_mut().push(event.clone())
            }));
            page = Some(serp.clone());
            Root::new(serp, window, cx)
        });
        std::mem::forget(subscription);
        let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
        cx.update(|window, _| window.activate_window());
        let serp = page.expect("results page");
        cx.update(|window, cx| serp.focus_handle(cx).focus(window, cx));
        cx.run_until_parked();
        (serp, opened, cx)
    }

    fn selected_url(serp: &Entity<Serp>, cx: &mut VisualTestContext) -> Option<String> {
        cx.update(|_, cx| {
            let results = serp.read(cx).list.read(cx).delegate();
            results
                .selected
                .and_then(|row| results.hits.get(row))
                .map(|hit| hit.url.to_string())
        })
    }

    #[gpui_kit::test]
    fn the_keyboard_moves_through_results_and_opens_one(cx: &mut TestAppContext) {
        let (serp, opened, cx) = open_page(
            vec![
                hit("https://a.example/1", "warm"),
                hit("https://b.example/2", "fresh"),
                hit("https://c.example/3", "fresh"),
            ],
            cx,
        );
        assert_eq!(selected_url(&serp, cx).as_deref(), Some("https://a.example/1"));

        cx.simulate_keystrokes("down");
        cx.run_until_parked();
        assert_eq!(selected_url(&serp, cx).as_deref(), Some("https://b.example/2"));

        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        cx.simulate_keystrokes("down cmd-enter");
        cx.run_until_parked();
        assert_eq!(
            *opened.borrow(),
            [
                SerpEvent::Open {
                    url: Url::parse("https://b.example/2").unwrap(),
                    new_tab: false,
                },
                SerpEvent::Open {
                    url: Url::parse("https://c.example/3").unwrap(),
                    new_tab: true,
                },
            ]
        );
    }

    #[gpui_kit::test]
    fn about_this_result_opens_from_its_button_without_opening_the_page(
        cx: &mut TestAppContext,
    ) {
        let (_, opened, cx) = open_page(
            vec![
                hit("https://a.example/1", "fresh"),
                hit("https://b.example/2", "fresh"),
            ],
            cx,
        );
        assert!(cx.debug_bounds("serp-about").is_none());

        let button = cx
            .debug_bounds("serp-about:b.example/2")
            .expect("the second result's button")
            .center();
        cx.simulate_click(button, Modifiers::none());
        cx.run_until_parked();
        assert!(cx.debug_bounds("serp-about").is_some());
        assert!(opened.borrow().is_empty());

        let new_tab = cx
            .debug_bounds("serp-open-new-tab")
            .expect("Open in New Tab")
            .center();
        cx.simulate_click(new_tab, Modifiers::none());
        cx.run_until_parked();
        assert_eq!(
            *opened.borrow(),
            [SerpEvent::Open {
                url: Url::parse("https://b.example/2").unwrap(),
                new_tab: true,
            }]
        );
        assert!(cx.debug_bounds("serp-about").is_none());
    }

    #[gpui_kit::test]
    fn scopes_narrow_the_list_to_local_or_web(cx: &mut TestAppContext) {
        let (serp, _, cx) = open_page(
            vec![
                hit("https://a.example/1", "warm"),
                hit("https://b.example/2", "fresh"),
            ],
            cx,
        );
        cx.update(|window, cx| {
            serp.update(cx, |serp, cx| serp.set_scope(Scope::Web, window, cx))
        });
        cx.run_until_parked();
        assert_eq!(selected_url(&serp, cx).as_deref(), Some("https://b.example/2"));
        let rows = cx.update(|_, cx| serp.read(cx).list.read(cx).delegate().hits.len());
        assert_eq!(rows, 1);
    }
}
