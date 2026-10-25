//! A natively rendered search results page.
//!
//! `Serp` runs one RustyWeb search when it is created and draws the outcome.
//! A result's row gives its title, where it leads and its excerpt, and
//! nothing else. Where it came from (Frame's local index or the web, which
//! providers, when it was published and when it was retrieved) is in the
//! details for the selected result: beside the list on a wide page, under it
//! on a narrow one. Ask adds an answer drawn from the results.
//!
//! The page works from the keyboard: Up and Down move through the results,
//! Enter opens one, ⌘Enter opens it in a new tab. It never navigates by
//! itself. Opening emits [`SerpEvent::Open`] and the host decides which tab
//! loads the page.
//!
//! Nothing here rests on color alone: the selected row has a fill and a bar
//! at its leading edge, Local and Web results carry an icon and a word, and
//! the details are a bordered panel.
//!
//! SOURCING: GPUI Kit's `List`, `TabBar`, `DescriptionList`, `Alert`,
//! `Empty`, `Skeleton` and `Button`; snippets are parsed by html5ever
//! (`markup.rs`), dates are chrono's.

use chrono::NaiveDate;
use gpui_kit::base::Link;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, IndexPath, Sizable as _,
    alert::Alert,
    button::{Button, ButtonVariants as _},
    description_list::DescriptionList,
    empty::{
        Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant,
        EmptyTitle,
    },
    h_flex,
    list::{List, ListDelegate, ListItem, ListState},
    scroll::ScrollableElement as _,
    skeleton::Skeleton,
    spinner::Spinner,
    tab::{Tab, TabBar},
    tag::Tag,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, EventEmitter, FocusHandle,
    Focusable, FontWeight, HighlightStyle, InteractiveElement, IntoElement, ParentElement, Render,
    Rems, ScrollStrategy, SharedString, Styled, StyledText,
    Subscription, Task, WeakEntity, Window, div, prelude::FluentBuilder as _, px, rems,
};
use url::Url;

use crate::search::{self, SerpAnswer, SerpHit, SerpResult, SerpSource, sentence};

/// Every row is this tall: the list measures one and lays out the rest.
const ROW_HEIGHT: Rems = rems(6.5);

/// From this width the details sit beside the results.
const WIDE_AT: f32 = 60.;

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
    /// The details sit beside the list. Measured as the page is laid out.
    wide: bool,
    focus: FocusHandle,
    list: Entity<ListState<Results>>,
    _list_changes: Subscription,
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
        };
        let list = cx.new(|cx| ListState::new(results, window, cx));
        // The details follow the list's selection.
        let list_changes = cx.observe(&list, |_, _, cx| cx.notify());
        Self {
            query,
            mode,
            state: State::Loading,
            revision: 1,
            scope: Scope::All,
            wide: true,
            focus: cx.focus_handle(),
            list,
            _list_changes: list_changes,
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
        let hits: Vec<SerpHit> = match &self.state {
            State::Loaded(result) => result
                .hits
                .iter()
                .filter(|hit| self.scope.includes(hit))
                .cloned()
                .collect(),
            _ => Vec::new(),
        };
        let first = (!hits.is_empty()).then(|| IndexPath::new(0));
        self.list.update(cx, |list, cx| {
            list.delegate_mut().hits = hits;
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

    fn selected_hit(&self, cx: &App) -> Option<SerpHit> {
        let results = self.list.read(cx).delegate();
        results
            .selected
            .and_then(|row| results.hits.get(row))
            .cloned()
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

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
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
                            .truncate()
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

    /// All, Local and Web, when the results have both.
    fn scope_bar(
        &self,
        result: &SerpResult,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let local = result
            .hits
            .iter()
            .filter(|hit| search::is_local_lane(&hit.lane))
            .count();
        let web = result.hits.len() - local;
        if local == 0 || web == 0 {
            return None;
        }
        let selected = Scope::ALL
            .iter()
            .position(|scope| *scope == self.scope)
            .unwrap_or(0);
        Some(
            TabBar::new("serp-scope")
                .segmented()
                .selected_index(selected)
                .on_click(cx.listener(|this, ix: &usize, window, cx| {
                    this.set_scope(Scope::ALL[*ix], window, cx)
                }))
                .children(Scope::ALL.map(|scope| {
                    let count = match scope {
                        Scope::All => result.hits.len(),
                        Scope::Local => local,
                        Scope::Web => web,
                    };
                    Tab::new().label(format!("{} {count}", scope.label()))
                })),
        )
    }

    fn loading(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
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

    fn failed(&self, message: &SharedString, cx: &mut Context<Self>) -> impl IntoElement + use<> {
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

    /// Where the selected result came from, and the ways to open it.
    fn details(
        &self,
        hit: Option<&SerpHit>,
        result: &SerpResult,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let panel = v_flex()
            .id("serp-details")
            .gap_3()
            .p_4()
            .rounded(theme.radius_tokens().lg)
            .border_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(muted)
                    .child("About this result"),
            );
        let Some(hit) = hit else {
            return panel.child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("Select a result to see where it came from."),
            );
        };
        let open = |new_tab: bool| {
            let url = hit.url.clone();
            cx.listener(move |_, _: &ClickEvent, _, cx| {
                cx.emit(SerpEvent::Open {
                    url: url.clone(),
                    new_tab,
                })
            })
        };
        panel
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
                    .item("Retrieved", retrieved(hit, result), 1),
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
                        Button::new("serp-open-new-tab")
                            .outline()
                            .small()
                            .label("Open in New Tab")
                            .on_click(open(true)),
                    ),
            )
    }

    fn notes(&self, result: &SerpResult, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let searched_with = (!result.providers.is_empty())
            .then(|| format!("Searched with {}.", result.providers.join(", ")));
        let lines: Vec<String> = searched_with
            .into_iter()
            .chain(result.notes.iter().cloned())
            .chain(result.degradations.iter().map(|d| d.message.clone()))
            .collect();
        v_flex()
            .id("serp-notes")
            .gap_1()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .children(lines.into_iter().map(|line| {
                h_flex()
                    .items_start()
                    .gap_2()
                    .child(Icon::new(IconName::Info).xsmall())
                    .child(div().min_w_0().child(line))
            }))
    }

    /// The results, their details and the notes, side by side or stacked.
    fn results(&self, result: &SerpResult, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.selected_hit(cx);
        let ask = self.mode == SerpMode::Ask;
        let list = List::new(&self.list).size_full();
        let scope_bar = self.scope_bar(result, cx);
        let details = self.details(selected.as_ref(), result, cx);
        let notes = self.notes(result, cx);
        let answer = ask.then(|| self.answer(result, cx));

        let results = v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .gap_3()
            .children(scope_bar)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(list),
            );

        if self.wide {
            h_flex()
                .flex_1()
                .min_h_0()
                .items_start()
                .gap_6()
                .child(results.h_full())
                .child(
                    v_flex()
                        .w(rems(22.))
                        .flex_shrink_0()
                        .h_full()
                        .overflow_y_scrollbar()
                        .id("serp-aside")
                        .child(
                            v_flex()
                                .gap_4()
                                .pb_6()
                                .children(answer)
                                .child(details)
                                .child(notes),
                        ),
                )
                .into_any_element()
        } else {
            v_flex()
                .flex_1()
                .min_h_0()
                .gap_3()
                .when_some(answer, |this, answer| {
                    this.child(
                        div()
                            .max_h(rems(14.))
                            .overflow_y_scrollbar()
                            .id("serp-answer-scroll")
                            .child(answer),
                    )
                })
                .child(results)
                .child(
                    v_flex()
                        .max_h(rems(18.))
                        .overflow_y_scrollbar()
                        .id("serp-below")
                        .child(v_flex().gap_3().pb_4().child(details).child(notes)),
                )
                .into_any_element()
        }
    }
}

impl Render for Serp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.state {
            State::Loading => self.loading(cx).into_any_element(),
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
                        .child(self.empty(cx))
                        .child(self.notes(result, cx)),
                )
                .into_any_element(),
            State::Loaded(result) => self.results(result, cx),
        };

        let page = cx.weak_entity();
        let wide_at = window.rem_size() * WIDE_AT;
        v_flex()
            .size_full()
            .text_color(cx.theme().foreground)
            .on_children_prepainted(move |bounds, _, cx| {
                let wide = bounds.first().is_some_and(|b| b.size.width >= wide_at);
                if page.upgrade().is_none_or(|page| page.read(cx).wide == wide) {
                    return;
                }
                // A notify while the window draws is dropped; this one lands
                // once the frame is done, and redraws in the other layout.
                let page = page.clone();
                cx.defer(move |cx| {
                    let _ = page.update(cx, |page, cx| {
                        page.wide = wide;
                        cx.notify();
                    });
                });
            })
            .id("serp")
            .track_focus(&self.focus)
            .child(
                v_flex()
                    .size_full()
                    .max_w(rems(72.))
                    .mx_auto()
                    .px_6()
                    .pt_6()
                    .gap_4()
                    .child(self.header(cx))
                    .child(body),
            )
    }
}

/// The rows the list shows: the loaded results in the current scope.
struct Results {
    serp: WeakEntity<Serp>,
    hits: Vec<SerpHit>,
    selected: Option<usize>,
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
        let list = cx.weak_entity();
        Some(result_row(
            hit,
            ix.row,
            self.selected == Some(ix.row),
            list,
            cx,
        ))
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

/// A result's row: where it leads, its title and its excerpt. Clicking it
/// opens the page; the trailing button only selects it, for its details.
fn result_row(
    hit: &SerpHit,
    row: usize,
    selected: bool,
    list: WeakEntity<ListState<Results>>,
    cx: &App,
) -> ListItem {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let local = search::is_local_lane(&hit.lane);
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
    ListItem::new(SharedString::from(format!(
        "serp-result:{}",
        element_key(&hit.url)
    )))
    .h(ROW_HEIGHT)
    .pl_1()
    .pr_2()
    .py_2()
    .border_b_1()
    .border_color(theme.border)
    .accessibility_label(format!("{}, {}", hit.title, hit.host))
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
                    .gap_0p5()
                    .child(
                        h_flex()
                            .min_w_0()
                            .gap_1p5()
                            .text_xs()
                            .line_height(rems(1.))
                            .text_color(muted)
                            .child(
                                Icon::new(if local {
                                    IconName::HardDrive
                                } else {
                                    IconName::Globe
                                })
                                .xsmall(),
                            )
                            .child(div().flex_shrink_0().child(search::lane_label(&hit.lane)))
                            .child(div().flex_shrink_0().child("·"))
                            .child(div().min_w_0().truncate().child(display_url(&hit.url))),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_base()
                            .line_height(rems(1.5))
                            .font_weight(if selected {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::MEDIUM
                            })
                            .child(hit.title.clone()),
                    )
                    .child(
                        div()
                            .h(rems(2.5))
                            .text_sm()
                            .line_height(rems(1.25))
                            .line_clamp(2)
                            .text_ellipsis()
                            .child(snippet),
                    ),
            ),
    )
    .suffix(move |_, _| {
        let list = list.clone();
        Button::new("details")
            .ghost()
            .xsmall()
            .icon(IconName::Info)
            .tab_stop(false)
            .accessibility_label("About this result")
            .tooltip("About this result")
            .on_click(move |_, window, cx| {
                // Select without opening: the row's own click opens.
                cx.stop_propagation();
                list.update(cx, |list, cx| {
                    list.set_selected_index(Some(IndexPath::new(row)), window, cx);
                    cx.notify();
                })
                .ok();
            })
    })
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
fn retrieved(hit: &SerpHit, result: &SerpResult) -> String {
    if search::is_local_lane(&hit.lane) {
        "Before this search, when Frame kept a copy".to_string()
    } else if hit.lane == "fresh" {
        format!(
            "During this search, at {}",
            result.searched_at.format("%H:%M")
        )
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

/// The URL line above a title: host and path without the scheme.
fn display_url(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    let host = host.strip_prefix("www.").unwrap_or(host);
    let path = url.path().trim_end_matches('/');
    format!("{host}{path}")
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
    use chrono::{Local, TimeZone as _};
    use gpui_kit::component::Root;
    use gpui_kit::{TestAppContext, VisualTestContext, size};
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
    fn display_url_drops_scheme_www_and_trailing_slash() {
        let url = Url::parse("https://www.example.com/docs/").unwrap();
        assert_eq!(display_url(&url), "example.com/docs");
        let root = Url::parse("https://example.com/").unwrap();
        assert_eq!(display_url(&root), "example.com");
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
        let r = result(vec![]);
        let mut web = hit("https://a.example/1", "fresh");
        web.published_at = Some("2024-10-24".into());
        assert_eq!(published(&web), "24 Oct 2024");
        assert_eq!(retrieved(&web, &r), "During this search, at 17:31");

        let mut local = hit("https://b.example/2", "warm");
        local.published_at = Some("last spring".into());
        assert_eq!(published(&local), "last spring");
        assert!(retrieved(&local, &r).starts_with("Before this search"));
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
        cx.update(|_, cx| serp.read(cx).selected_hit(cx).map(|hit| hit.url.to_string()))
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
