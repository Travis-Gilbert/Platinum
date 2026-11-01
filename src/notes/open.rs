//! The notes app's entry point: the launch arguments that ask for it, and the
//! window that holds the editor and the reading panel.
//!
//! `--notes-vault <dir>` opens a notes window on the vault in that folder, in
//! place of the browser's window; `--notes-note <path>` names the note to
//! show, as a path from the vault's folder, and defaults to the vault's first
//! note.
//!
//! The window's layout is one view, `NotesWindow`: a row of panes, the editor
//! of the note on screen beside the reading panel that shows it. Both work on
//! the vault worker's authority, so a save in one is what the other shows.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use gpui_kit::component::{ActiveTheme as _, Root, TitleBar};
use gpui_kit::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, WindowBounds,
    WindowOptions, div, point, px, size, transparent_black,
};
use url::Url;

use super::Notes;
use super::editor::{NoteEditor, NoteEditorEvent};
use super::host::ops;
use super::host::service::Service;
use super::host::session::Session;
use super::panel::{ReadingEvent, ReadingPanel};
use crate::glass;

/// The launch arguments of the notes app.
#[derive(Debug, Default, clap::Args)]
pub struct Args {
    /// Open the notes app on the vault in this folder.
    #[arg(long = "notes-vault", value_name = "DIR")]
    vault: Option<PathBuf>,
    /// The note to open, as a path from the vault's folder. The vault's first
    /// note when it is not given.
    #[arg(long = "notes-note", value_name = "PATH", requires = "vault")]
    note: Option<String>,
}

/// A request to open the notes app, checked.
#[derive(Debug, PartialEq, Eq)]
pub struct Launch {
    vault: PathBuf,
    note: Option<String>,
}

impl Args {
    /// The notes app's launch, if it was asked for.
    pub fn launch(self) -> anyhow::Result<Option<Launch>> {
        let Some(vault) = self.vault else {
            return Ok(None);
        };
        let vault = vault
            .canonicalize()
            .with_context(|| format!("--notes-vault: {} cannot be opened", vault.display()))?;
        if !vault.is_dir() {
            bail!("--notes-vault: {} is not a folder", vault.display());
        }
        if let Some(note) = &self.note
            && Path::new(note).is_absolute()
        {
            bail!("--notes-note: {note} must be a path from the vault's folder");
        }
        Ok(Some(Launch {
            vault,
            note: self.note,
        }))
    }
}

/// The frame between the panes, the same as around a browser tab's card.
const FRAME: f32 = 6.;

/// The notes window's one view: a row of panes under the title bar, the
/// editor of the note on screen beside its reading view.
pub struct NotesWindow {
    service: Service,
    /// The editing pane, on the note the reading pane shows; `None` while
    /// that pane has no note.
    editor: Option<Editing>,
    /// The reading pane, or why there is none.
    reading: Result<Entity<ReadingPanel>, SharedString>,
    _subscriptions: Vec<Subscription>,
}

/// The editing pane on one note.
struct Editing {
    path: String,
    view: Entity<NoteEditor>,
    _events: Subscription,
}

impl NotesWindow {
    fn new(
        service: Service,
        reading: anyhow::Result<Entity<ReadingPanel>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut subscriptions = Vec::new();
        let mut session = None;
        let reading = match reading {
            Ok(panel) => {
                let watched = panel.read(cx).session().clone();
                subscriptions.push(cx.observe_in(&watched, window, Self::follow));
                session = Some(watched);
                // This window has no browser to open the web in, so a link to
                // it goes to the system's.
                subscriptions.push(cx.subscribe(
                    &panel,
                    |_, _, event: &ReadingEvent, cx| match event {
                        ReadingEvent::OpenWeb(url) => cx.open_url(url.as_str()),
                    },
                ));
                Ok(panel)
            }
            Err(error) => {
                log::error!(target: "notes", "{error:#}");
                Err(format!("The notes could not be opened: {error:#}").into())
            }
        };
        window.set_window_title("Notes");
        let mut this = Self {
            service,
            editor: None,
            reading,
            _subscriptions: subscriptions,
        };
        if let Some(session) = session {
            this.follow(session, window, cx);
        }
        this
    }

    /// The reading pane changed: the window is named for the note on screen,
    /// and the editing pane moves to it. The editor it leaves saves what was
    /// typed in it as it closes.
    fn follow(&mut self, session: Entity<Session>, window: &mut Window, cx: &mut Context<Self>) {
        let (path, title) = {
            let session = session.read(cx);
            (session.path().map(str::to_owned), session.title())
        };
        window.set_window_title(&match path {
            Some(_) => format!("{title} - Notes"),
            None => "Notes".to_owned(),
        });
        if self.editor.as_ref().map(|editing| &editing.path) == path.as_ref() {
            return;
        }
        self.editor = path.map(|path| {
            let service = self.service.clone();
            let view = cx.new(|cx| NoteEditor::new(service, path.clone(), window, cx));
            let _events = cx.subscribe(&view, move |_, _, event: &NoteEditorEvent, cx| {
                follow_link(event, &session, cx);
            });
            Editing {
                path,
                view,
                _events,
            }
        });
        cx.notify();
    }
}

/// A link clicked in the editor goes where the same link clicked on the page
/// goes: a web address to the system's browser, anything else through the
/// reading pane's session, which resolves it in the vault.
fn follow_link(event: &NoteEditorEvent, session: &Entity<Session>, cx: &mut App) {
    let link = match event {
        NoteEditorEvent::OpenLink(link) => {
            if let Ok(url) = Url::parse(link)
                && matches!(url.scheme(), "http" | "https" | "mailto")
            {
                cx.open_url(url.as_str());
                return;
            }
            link
        }
        NoteEditorEvent::OpenWikiLink(link) => link,
        NoteEditorEvent::Synced { .. } => return,
    };
    let link = link.to_string();
    session.update(cx, |session, cx| session.follow(link, cx));
}

impl Render for NotesWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let card = |id: &'static str| {
            div()
                .id(id)
                .flex_1()
                .min_w_0()
                .h_full()
                .rounded(theme.radius_lg)
                .overflow_hidden()
                .bg(theme.background)
        };
        let editor = self
            .editor
            .as_ref()
            .map(|editing| card("notes-editor-pane").child(editing.view.clone()));
        let reading = card("notes-reading-pane");
        let reading = match &self.reading {
            Ok(panel) => reading.child(panel.clone()),
            Err(reason) => reading
                .flex()
                .items_center()
                .justify_center()
                .p_6()
                .text_color(theme.muted_foreground)
                .child(reason.clone()),
        };
        div()
            .id("notes-window")
            .size_full()
            .flex()
            .flex_col()
            .bg(glass::frame(window, cx))
            .text_color(theme.foreground)
            .child(TitleBar::new().border_b_0().bg(transparent_black()))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .gap(px(FRAME))
                    .p(px(FRAME))
                    .pt_0()
                    .children(editor)
                    .child(reading),
            )
    }
}

/// Opens the notes window on `launch`'s vault. `notes::start` has run.
///
/// The window is opened on a note, not before one: Servo drops a page asked
/// for before its web view's first page is up, so the note to show is named
/// first. A launch that names none opens on the vault's first note, which the
/// vault's worker answers, so the window opens a moment after this returns.
pub fn open_window(launch: Launch, cx: &mut App) -> anyhow::Result<()> {
    let Launch { vault, note } = launch;
    let service = cx.global::<Notes>().open(&vault);
    if note.is_some() {
        return open(service, note, cx);
    }
    cx.spawn(async move |cx| {
        // An answer that is an error is shown as one by the window, which asks
        // the vault again for the reason.
        let first = service
            .read(|vault| Ok(ops::first_note(vault)))
            .await
            .ok()
            .flatten();
        let opened = cx.update(|cx| open(service, first, cx));
        if let Err(error) = opened {
            log::error!("{error:#}");
            cx.update(|cx| cx.quit());
        }
    })
    .detach();
    Ok(())
}

fn open(service: Service, first: Option<String>, cx: &mut App) -> anyhow::Result<()> {
    let mut options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(1100.), px(780.)), cx)),
        window_min_size: Some(size(px(480.), px(320.))),
        ..TitleBar::window_options()
    };
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.traffic_light_position = Some(point(px(9.), px(10.)));
    }
    cx.open_window(options, |window, cx| {
        glass::install(window, cx);
        let vaults = cx.global::<Notes>().vaults().clone();
        let reading = ReadingPanel::new(&vaults, service.clone(), first.as_deref(), window, cx);
        if let (Ok(panel), None) = (&reading, &first) {
            // No note to open: the session says why (an empty vault, or one
            // that could not be read).
            panel.update(cx, |panel, cx| panel.show(None, None, cx));
        }
        let view = cx.new(|cx| NotesWindow::new(service, reading, window, cx));
        cx.new(|cx| Root::new(view, window, cx).bg(transparent_black()))
    })?;
    // Launched from a shell, the app is not the active one, and a window
    // that is not in front is not drawn once something covers it.
    cx.activate(true);
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        notes: Args,
    }

    fn launch(arguments: &[&str]) -> anyhow::Result<Option<Launch>> {
        let cli = Cli::try_parse_from(std::iter::once("frame").chain(arguments.iter().copied()))?;
        cli.notes.launch()
    }

    #[test]
    fn without_a_vault_the_browser_opens() {
        assert_eq!(launch(&[]).unwrap(), None);
    }

    #[test]
    fn a_vault_and_a_note_open_the_notes_app() {
        let vault = tempfile::tempdir().unwrap();
        let path = vault.path().to_str().unwrap();
        let launched = launch(&["--notes-vault", path, "--notes-note", "Folder/Note.md"])
            .unwrap()
            .unwrap();
        // The folder as the system names it, so one vault has one address.
        assert_eq!(launched.vault, vault.path().canonicalize().unwrap());
        assert_eq!(launched.note.as_deref(), Some("Folder/Note.md"));
        let first = launch(&["--notes-vault", path]).unwrap().unwrap();
        assert_eq!(first.note, None);
    }

    #[test]
    fn a_note_without_a_vault_is_refused() {
        assert!(launch(&["--notes-note", "Note.md"]).is_err());
    }

    #[test]
    fn a_vault_that_is_not_a_folder_is_refused() {
        let vault = tempfile::tempdir().unwrap();
        let file = vault.path().join("Note.md");
        std::fs::write(&file, "hi").unwrap();
        let error = launch(&["--notes-vault", file.to_str().unwrap()]).unwrap_err();
        assert!(error.to_string().contains("is not a folder"), "{error:#}");
        let missing = vault.path().join("nowhere");
        let error = launch(&["--notes-vault", missing.to_str().unwrap()]).unwrap_err();
        assert!(error.to_string().contains("cannot be opened"), "{error:#}");
    }

    #[test]
    fn a_note_is_a_path_from_the_vault_not_from_the_disk() {
        let vault = tempfile::tempdir().unwrap();
        let error = launch(&[
            "--notes-vault",
            vault.path().to_str().unwrap(),
            "--notes-note",
            "/etc/passwd",
        ])
        .unwrap_err();
        assert!(
            error.to_string().contains("path from the vault"),
            "{error:#}"
        );
    }
}
