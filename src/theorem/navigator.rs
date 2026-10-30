//! The workbench's navigator: the workspace's projects and the selected
//! project's conversations, newest first, on the tab's pane with no fill of
//! its own (level 0).
//!
//! SOURCING: GPUI Kit's `ListItem`, `Skeleton`, `Tag` and `resize_handle`.

use chrono::{DateTime, Datelike as _, Local};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::InteractiveElementExt as _;
use gpui_kit::component::resizable::{HandleEdge, resize_handle};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    list::ListItem,
    scroll::ScrollableElement as _,
    skeleton::Skeleton,
    spinner::Spinner,
    tag::Tag,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext as _, Axis, Context, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, Styled as _, Window, div, prelude::FluentBuilder as _, px,
    relative, rems,
};
use theorem_surface_contracts::thread::ThreadRunState;
use theoremweb_agent_runtime::{ChatHostPhase, ChatHostProjection, ChatThreadSummary};

use super::driver::Connection;
use super::workbench::{NAVIGATOR_WIDTH, NewConversation, ResizeNavigator, Workbench};
use crate::browser::FRAME;

const ROW: f32 = 1.625;
const SECTION: f32 = 1.5;

/// The selected project's conversations, in the navigator's order: the most
/// recently changed first. With no project selected, every conversation.
pub(super) fn by_recency(projection: &ChatHostProjection) -> Vec<&ChatThreadSummary> {
    let project = projection.selected_project_id.as_deref();
    let mut threads: Vec<_> = projection
        .threads
        .iter()
        .filter(|thread| project.is_none_or(|project| thread.project_id == project))
        .collect();
    threads.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
    threads
}

/// When a conversation last changed, as the navigator groups it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Recency {
    Today,
    Yesterday,
    ThisWeek,
    Earlier,
}

impl Recency {
    pub(super) fn of(updated: DateTime<Local>, now: DateTime<Local>) -> Self {
        let (day, today) = (updated.date_naive(), now.date_naive());
        let days = (today - day).num_days();
        if days <= 0 {
            Recency::Today
        } else if days == 1 {
            Recency::Yesterday
        } else if days < 7 {
            Recency::ThisWeek
        } else {
            Recency::Earlier
        }
    }

    fn label(self) -> &'static str {
        match self {
            Recency::Today => "Today",
            Recency::Yesterday => "Yesterday",
            Recency::ThisWeek => "This week",
            Recency::Earlier => "Earlier",
        }
    }

    /// The time lane's text in this group: the time for the last two days,
    /// the weekday this week, the date before.
    pub(super) fn time(self, updated: DateTime<Local>, now: DateTime<Local>) -> String {
        match self {
            Recency::Today | Recency::Yesterday => updated.format("%H:%M").to_string(),
            Recency::ThisWeek => updated.format("%a").to_string(),
            Recency::Earlier if updated.year() == now.year() => {
                updated.format("%-d %b").to_string()
            }
            Recency::Earlier => updated.format("%-d %b %Y").to_string(),
        }
    }
}

fn local(ms: i64) -> DateTime<Local> {
    DateTime::from_timestamp_millis(ms)
        .unwrap_or_default()
        .with_timezone(&Local)
}

/// A conversation's lifecycle, when it is not the ordinary one.
fn lifecycle_word(status: &str) -> Option<&'static str> {
    match status {
        "archived" => Some("Archived"),
        "new" => Some("New"),
        _ => None,
    }
}

impl Workbench {
    pub(super) fn render_navigator(
        &mut self,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let driver = self.driver().clone();
        let driver = driver.read(cx);
        let projection = driver.projection();
        let connection = driver.connection();
        let loading = connection != Connection::NotConfigured
            && matches!(
                projection.phase,
                ChatHostPhase::Unhydrated | ChatHostPhase::Loading
            );
        let ready = projection.phase == ChatHostPhase::Ready;

        let header = h_flex()
            .h(rems(2.25))
            .flex_shrink_0()
            .pl(px(FRAME))
            .pr_1()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .font_semibold()
                    .child(if projection.workspace_label.is_empty() {
                        SharedString::from("Theorem")
                    } else {
                        SharedString::from(projection.workspace_label.clone())
                    }),
            )
            .when_some(projection.badge.clone(), |header, badge| {
                header.child(Tag::secondary().small().child(badge))
            })
            .child(
                Button::new("theorem-new-conversation")
                    .ghost()
                    .small()
                    .icon(Lucide::SquarePen)
                    .accessibility_label("New conversation")
                    .tooltip_with_action("New conversation", &NewConversation, None)
                    .disabled(!ready)
                    .on_click(cx.listener(|this, _, window, cx| this.new_conversation(window, cx))),
            );

        let body = if loading {
            skeleton_rows().into_any_element()
        } else if ready {
            self.navigator_rows(projection, cx).into_any_element()
        } else {
            div().into_any_element()
        };

        let edge = resize_handle::<ResizeNavigator, ResizeNavigator>(
            "theorem-navigator-edge",
            Axis::Horizontal,
        )
        .inside(HandleEdge::Trailing)
        .on_drag(ResizeNavigator, |drag, _, _, cx| cx.new(|_| drag.clone()))
        .on_double_click(
            cx.listener(|this, _, _, cx| this.set_navigator_width(NAVIGATOR_WIDTH, cx)),
        );

        v_flex()
            .id("theorem-navigator")
            .debug_selector(|| "theorem-navigator".into())
            .relative()
            .w(rems(self.navigator_width))
            .h_full()
            .flex_shrink_0()
            .child(header)
            .child(
                div()
                    .id("theorem-navigator-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(body),
            )
            .child(edge)
    }

    fn navigator_rows(&self, projection: &ChatHostProjection, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let selected_project = projection.selected_project_id.as_deref();
        let selected_thread = projection.selected_thread_id.as_deref();
        let mut rows = v_flex().px(px(FRAME)).pb(px(FRAME)).gap(rems(0.125));

        if !projection.projects.is_empty() {
            rows = rows.child(section("Projects", cx));
            for project in &projection.projects {
                let id = project.project_id.clone();
                let selected = Some(id.as_str()) == selected_project;
                rows = rows.child(
                    row(SharedString::from(format!("theorem-project:{id}")), cx)
                        .selected(selected)
                        .accessibility_label(project.title.clone())
                        .child(
                            div()
                                .truncate()
                                .when(selected, |label| label.font_medium())
                                .child(project.title.clone()),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.select_project(&id, cx))),
                );
            }
        }

        let threads = by_recency(projection);
        if threads.is_empty() {
            rows = rows.child(section("Conversations", cx)).child(
                div()
                    .px_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("No conversations yet"),
            );
            return rows.into_any_element();
        }

        let now = Local::now();
        let mut group = None;
        for thread in threads {
            let updated = local(thread.updated_at_ms);
            let recency = Recency::of(updated, now);
            if group != Some(recency) {
                group = Some(recency);
                rows = rows.child(section(recency.label(), cx));
            }
            let id = thread.thread_id.clone();
            let selected = Some(id.as_str()) == selected_thread;
            // Only the open conversation's run is known as it happens.
            let run = selected.then_some(projection.thread.run_state);
            let word = run
                .and_then(Workbench::run_state_word)
                .or_else(|| lifecycle_word(&thread.status));
            let title = if thread.title.trim().is_empty() {
                "Untitled conversation".to_owned()
            } else {
                thread.title.clone()
            };
            let label = match word {
                Some(word) => format!("{title}, {word}"),
                None => title.clone(),
            };
            rows = rows.child(
                row(SharedString::from(format!("theorem-thread:{id}")), cx)
                    .selected(selected)
                    .accessibility_label(label)
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .gap_1p5()
                            .when(run == Some(ThreadRunState::Streaming), |row| {
                                row.child(Spinner::new().xsmall().color(theme.muted_foreground))
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .when(selected, |label| label.font_medium())
                                    .child(title),
                            )
                            .when_some(word, |row, word| {
                                row.child(
                                    div()
                                        .flex_shrink_0()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(word),
                                )
                            })
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(recency.time(updated, now)),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.select_thread(&id, cx))),
            );
        }
        rows.into_any_element()
    }
}

/// A navigator row: GPUI Kit's list item at the navigator's density, and a
/// stop in the Tab order.
fn row(id: SharedString, cx: &App) -> ListItem {
    let selector = id.to_string();
    ListItem::new(id)
        .debug_selector(move || selector)
        .tab_index(0)
        .h(rems(ROW))
        .py_0()
        .px_2()
        .text_sm()
        .rounded(cx.theme().radius)
        .cursor_pointer()
}

fn section(label: &'static str, cx: &App) -> impl IntoElement {
    div()
        .h(rems(SECTION))
        .flex()
        .items_end()
        .px_2()
        .pb_0p5()
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(label)
}

/// Rows that stand for the conversations while the workspace loads.
fn skeleton_rows() -> impl IntoElement {
    v_flex()
        .px(px(FRAME + 8.))
        .pt_2()
        .gap(rems(0.625))
        .children(
            [0.7, 0.55, 0.8, 0.6, 0.45]
                .map(|fraction| Skeleton::new().h(rems(0.875)).w(relative(fraction))),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone as _;

    fn at(y: i32, m: u32, d: u32, h: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap()
    }

    #[test]
    fn conversations_group_by_how_long_ago_they_changed() {
        let now = at(2026, 10, 29, 15);
        assert_eq!(Recency::of(at(2026, 10, 29, 1), now), Recency::Today);
        assert_eq!(Recency::of(at(2026, 10, 28, 23), now), Recency::Yesterday);
        assert_eq!(Recency::of(at(2026, 10, 23, 9), now), Recency::ThisWeek);
        assert_eq!(Recency::of(at(2026, 10, 22, 9), now), Recency::Earlier);
        // A clock a little ahead of this one is still today.
        assert_eq!(Recency::of(at(2026, 10, 29, 16), now), Recency::Today);
    }

    #[test]
    fn the_time_lane_names_the_time_the_day_or_the_date() {
        let now = at(2026, 10, 29, 15);
        assert_eq!(Recency::Today.time(at(2026, 10, 29, 9), now), "09:00");
        assert_eq!(Recency::ThisWeek.time(at(2026, 10, 26, 9), now), "Mon");
        assert_eq!(Recency::Earlier.time(at(2026, 3, 4, 9), now), "4 Mar");
        assert_eq!(Recency::Earlier.time(at(2025, 3, 4, 9), now), "4 Mar 2025");
    }

    fn thread(id: &str, project: &str, updated_at_ms: i64) -> ChatThreadSummary {
        ChatThreadSummary {
            thread_id: id.into(),
            project_id: project.into(),
            title: id.into(),
            status: "regular".into(),
            updated_at_ms,
        }
    }

    #[test]
    fn the_selected_project_lists_its_own_conversations_newest_first() {
        let mut projection = ChatHostProjection {
            authority: Default::default(),
            phase: ChatHostPhase::Ready,
            workspace_label: String::new(),
            badge: None,
            notice: None,
            projects: Vec::new(),
            threads: vec![
                thread("a", "p1", 1),
                thread("b", "p2", 3),
                thread("c", "p1", 2),
            ],
            selected_project_id: Some("p1".into()),
            selected_thread_id: None,
            thread: Default::default(),
            composer: Default::default(),
        };
        let ids = |projection: &ChatHostProjection| {
            by_recency(projection)
                .into_iter()
                .map(|thread| thread.thread_id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&projection), ["c", "a"]);
        projection.selected_project_id = None;
        assert_eq!(ids(&projection), ["b", "c", "a"]);
    }
}
