//! The launcher with made-up contents, to look at without the application.
//!
//! `LAUNCHER_SHOW` picks what is on screen: `empty`, `find` (the default),
//! `files`, `ask`, `talk` or `working`. `LAUNCHER_LIGHT=1` draws it light.
//!
//!     cargo run -p launcher-view --example launcher_gallery

use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::component::{Theme, ThemeMode, ThemeRegistry};
use gpui_kit::{
    AppContext as _, Bounds, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    Window, WindowBounds, WindowOptions, div, px, size,
};
use launcher::{Cancel, Item, ItemId, Kind, Lane, Session, Source, Target};
use launcher_view::{Attachment, Event, Kept, Launcher, Talk, Turn, Who};

const ASCIINEMA: &str = include_str!("../../../themes/asciinema.json");
const GITHUB_SHARP: &str = include_str!("../../../themes/github-sharp.json");

/// Files that are not there, found at once.
struct Shelf;

impl Source for Shelf {
    fn find(&self, query: &str, _: &Cancel) -> Result<Vec<Item>, String> {
        let files = [
            (
                "Lumio Roadmap.md",
                "~/Documents/Lumio",
                "3 days ago",
                "18.4 KB",
            ),
            (
                "lumio-launch-notes.txt",
                "~/Desktop",
                "2 weeks ago",
                "2.1 KB",
            ),
            ("Lumio brand kit", "~/Documents/Design", "1 month ago", ""),
        ];
        Ok(files
            .into_iter()
            .filter(|(name, ..)| launcher::matching::score(query, name).is_some())
            .map(|(name, folder, modified, size)| {
                let path = format!("{folder}/{name}");
                let kind = if size.is_empty() {
                    Kind::Folder
                } else {
                    Kind::File
                };
                let mut item = Item::new(
                    ItemId::new("file", &path),
                    kind,
                    name,
                    Target::Path(PathBuf::from(&path)),
                )
                .detail(folder)
                .fact("Path", path)
                .fact("Modified", modified);
                if !size.is_empty() {
                    item = item.fact("Size", size).excerpt(
                        "Q3: ship the launcher behind the global shortcut. Search covers \
                         applications, files and commands; asking Theo continues in the \
                         same window, and the conversation opens in the workspace.",
                    );
                }
                item
            })
            .collect())
    }
}

fn app(name: &str, modified: &str) -> Item {
    let path = format!("/Applications/{name}.app");
    Item::new(
        ItemId::new("app", &path),
        Kind::App,
        name,
        Target::Path(PathBuf::from(&path)),
    )
    .detail("/Applications")
    .fact("Path", path)
    .fact("Modified", modified)
}

fn command(name: &str, detail: &str, pinned: bool) -> Item {
    Item::new(
        ItemId::new("command", name),
        Kind::Command,
        name,
        Target::Command(name.to_owned()),
    )
    .detail(detail)
    .pinned(pinned)
}

fn conversation(id: &str, title: &str, when: &str) -> Item {
    Item::new(
        ItemId::new("conversation", id),
        Kind::Conversation,
        title,
        Target::Conversation(id.to_owned()),
    )
    .detail(when)
    .pinned(true)
}

struct Stage {
    launcher: Entity<Launcher>,
}

impl Stage {
    fn new(show: &str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut session = Session::new();
        session.set_standing(
            Lane::Apps,
            vec![
                app("Lumio", "2 days ago"),
                app("Lumio Dev", "5 hours ago"),
                app("Illuminate", "3 weeks ago"),
                app("Mail", "1 month ago"),
                app("Terminal", "1 month ago"),
            ],
        );
        session.set_standing(
            Lane::Commands,
            vec![
                command("New Tab", "Opens a new tab in Platinum", true),
                command(
                    "Open Theo's Workspace",
                    "Opens the Agent place in Platinum",
                    true,
                ),
                command("Show History", "Opens Platinum's history", false),
                command("Show Downloads", "Opens Platinum's downloads", false),
            ],
        );
        session.set_standing(
            Lane::Conversations,
            vec![
                conversation(
                    "t1",
                    "Launcher lifecycle on macOS",
                    "Updated 20 minutes ago",
                ),
                conversation("t2", "Lumio pricing page copy", "Updated yesterday"),
                conversation("t3", "Database rollback plan", "Updated 3 days ago"),
            ],
        );
        let query = match show {
            "empty" | "talk" | "working" => "",
            "ask" => "how does the launcher keep running?",
            _ => "lumio",
        };
        session.expect(Lane::Files);
        session.set_query(query);
        let launcher = cx.new(|cx| {
            let mut launcher = Launcher::new(
                Kept {
                    session,
                    draft: String::new(),
                },
                window,
                cx,
            );
            launcher.add_source(Lane::Files, Arc::new(Shelf), cx);
            if matches!(show, "talk" | "working") {
                launcher.set_talk(
                    Talk {
                        title: Some("Launcher lifecycle on macOS".to_owned()),
                        turns: vec![
                            Turn {
                                who: Who::Person,
                                text: "What changes if Platinum stays resident after its last \
                                       window closes?"
                                    .to_owned(),
                            },
                            Turn {
                                who: Who::Theo,
                                text: "Closing the last window stops being the same as quitting. \
                                       The global shortcut, running tasks and this launcher stay \
                                       available; Quit becomes the explicit way out, and \
                                       reopening the app brings the workspace back."
                                    .to_owned(),
                            },
                        ],
                        attachments: vec![Attachment {
                            id: "a1".to_owned(),
                            name: "Lumio Roadmap.md".to_owned(),
                        }],
                        working: show == "working",
                        refusing: None,
                    },
                    cx,
                );
                launcher.show_talk(window, cx);
            }
            launcher
        });
        cx.subscribe(&launcher, |_, _, event: &Event, cx| {
            println!("{event:?}");
            if *event == Event::Dismiss {
                cx.quit();
            }
        })
        .detach();
        let focus = launcher.clone();
        window.defer(cx, move |window, cx| {
            focus.update(cx, |launcher, cx| launcher.focus(window, cx));
        });
        if show == "files" {
            // Narrow to files, as Tab twice would.
            window.dispatch_action(Box::new(launcher_view::NextScope), cx);
        }
        Self { launcher }
    }
}

impl Render for Stage {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // The desktop the launcher floats over, standing in for it.
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui_kit::rgb(0x3b4252))
            .child(
                div()
                    .w(px(launcher_view::SIZE.0))
                    .h(px(launcher_view::SIZE.1))
                    .child(self.launcher.clone()),
            )
    }
}

fn main() {
    let show = std::env::var("LAUNCHER_SHOW").unwrap_or_else(|_| "find".to_owned());
    let light = std::env::var_os("LAUNCHER_LIGHT").is_some();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            launcher_view::init(cx);
            // Platinum's own themes, so this looks as it will in the browser.
            let registry = ThemeRegistry::global_mut(cx);
            for source in [ASCIINEMA, GITHUB_SHARP] {
                registry.load_themes_from_str(source).expect("theme reads");
            }
            let themes = ThemeRegistry::global(cx).themes();
            let dark = themes.get("Asciinema").cloned();
            let lit = themes.get("GitHub Sharp").cloned();
            Theme::update(cx, |theme| {
                if let Some(dark) = dark {
                    theme.dark_theme = dark;
                }
                if let Some(lit) = lit {
                    theme.light_theme = lit;
                }
            });
            let mode = if light {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            };
            Theme::change(mode, None, cx);
            let bounds = Bounds::centered(None, size(px(900.), px(620.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..WindowOptions::default()
            };
            gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| Stage::new(&show, window, cx))
            })
            .expect("window opens");
        });
}
