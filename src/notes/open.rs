//! The notes app's entry point: the launch arguments that ask for it, and the
//! window that holds the reading panel.
//!
//! `--notes-vault <dir>` opens a notes window on the vault in that folder, in
//! place of the browser's window; `--notes-note <path>` names the note to
//! show, as a path from the vault's folder, and defaults to the vault's first
//! note.
//!
//! The window's layout is one view, `NotesWindow`, with the reading panel as
//! one pane in a row of panes, so another pane can stand beside it.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use gpui_kit::component::{ActiveTheme as _, Root, TitleBar};
use gpui_kit::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, WindowBounds,
    WindowOptions, div, point, px, size, transparent_black,
};

use super::Notes;
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

/// The notes window's one view: a row of panes under the title bar.
pub struct NotesWindow {
    /// The reading pane, or why there is none.
    reading: Result<Entity<ReadingPanel>, SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl NotesWindow {
    fn new(
        reading: anyhow::Result<Entity<ReadingPanel>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut subscriptions = Vec::new();
        let reading = match reading {
            Ok(panel) => {
                // The window is named for the note on screen.
                let session = panel.read(cx).session().clone();
                subscriptions.push(cx.observe_in(&session, window, |_, session, window, cx| {
                    let session = session.read(cx);
                    let title = match session.path() {
                        Some(_) => format!("{} - Notes", session.title()),
                        None => "Notes".to_owned(),
                    };
                    window.set_window_title(&title);
                }));
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
        Self {
            reading,
            _subscriptions: subscriptions,
        }
    }
}

impl Render for NotesWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let pane = div()
            .id("notes-reading-pane")
            .flex_1()
            .min_w_0()
            .h_full()
            .rounded(theme.radius_lg)
            .overflow_hidden()
            .bg(theme.background);
        let pane = match &self.reading {
            Ok(panel) => pane.child(panel.clone()),
            Err(reason) => pane
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
                    .child(pane),
            )
    }
}

/// Opens the notes window on `launch`'s vault. `notes::start` has run.
pub fn open_window(launch: Launch, cx: &mut App) -> anyhow::Result<()> {
    let Launch { vault, note } = launch;
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
        let (service, vaults) = {
            let notes = cx.global::<Notes>();
            (notes.open(&vault), notes.vaults().clone())
        };
        let reading = ReadingPanel::new(&vaults, service, window, cx);
        if let Ok(panel) = &reading {
            panel.update(cx, |panel, cx| panel.show(note, None, cx));
        }
        let view = cx.new(|cx| NotesWindow::new(reading, window, cx));
        cx.new(|cx| Root::new(view, window, cx).bg(transparent_black()))
    })?;
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
