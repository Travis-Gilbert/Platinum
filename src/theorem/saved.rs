//! What the Agent place remembers between launches: the conversation it last
//! showed and how wide its navigator was. Kept as `theorem.json` in the
//! profile folder; the conversations themselves belong to the authority.

use std::path::{Path, PathBuf};

use gpui_kit::{App, AppContext as _, Global, Subscription};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Record {
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    routing: theorem_surface_contracts::model_routing::ModelRoutingRequest,
    #[serde(default)]
    thread_id: Option<String>,
    /// In rems; `None` until the person first drags the navigator's edge.
    #[serde(default)]
    navigator_width: Option<f32>,
}

struct Saved {
    record: Record,
    /// Where the record is saved; `None` keeps it in memory, as tests do.
    path: Option<PathBuf>,
    _quit: Subscription,
}

impl Global for Saved {}

/// Load the record at `path`, if there is one, and save it when Platinum quits.
pub fn init(path: Option<PathBuf>, cx: &mut App) {
    let record = path.as_deref().map(load).unwrap_or_default();
    let quit = cx.on_app_quit(|cx| {
        let saved = cx.global::<Saved>();
        let path = saved.path.clone();
        let record = saved.record.clone();
        cx.background_spawn(async move {
            if let Some(path) = path {
                persist(&path, &record);
            }
        })
    });
    cx.set_global(Saved {
        record,
        path,
        _quit: quit,
    });
}

/// The project and conversation the Agent place last showed.
pub fn selection(cx: &App) -> (Option<String>, Option<String>) {
    cx.try_global::<Saved>().map_or((None, None), |saved| {
        (
            saved.record.project_id.clone(),
            saved.record.thread_id.clone(),
        )
    })
}

pub fn set_selection(project_id: Option<&str>, thread_id: Option<&str>, cx: &mut App) {
    if cx.has_global::<Saved>() {
        let saved = cx.global_mut::<Saved>();
        saved.record.project_id = project_id.map(str::to_owned);
        saved.record.thread_id = thread_id.map(str::to_owned);
    }
}

/// The navigator's width in rems, if the person ever set one.
pub fn navigator_width(cx: &App) -> Option<f32> {
    cx.try_global::<Saved>()
        .and_then(|saved| saved.record.navigator_width)
}

pub fn set_navigator_width(rems: f32, cx: &mut App) {
    if cx.has_global::<Saved>() {
        cx.global_mut::<Saved>().record.navigator_width = Some(rems);
    }
}

fn load(path: &Path) -> Record {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
            log::warn!("{} is not a Theorem record: {error}", path.display());
            Record::default()
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Record::default(),
        Err(error) => {
            log::warn!("could not read {}: {error}", path.display());
            Record::default()
        }
    }
}

/// Written beside the record and renamed over it, so a quit mid-write never
/// leaves half a file.
fn persist(path: &Path, record: &Record) {
    let partial = path.with_extension("json.partial");
    let written = serde_json::to_vec_pretty(record)
        .map_err(std::io::Error::other)
        .and_then(|bytes| std::fs::write(&partial, bytes))
        .and_then(|()| std::fs::rename(&partial, path));
    if let Err(error) = written {
        log::warn!("could not save {}: {error}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_record_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("theorem.json");
        let record = Record {
            project_id: Some("p1".into()),
            thread_id: Some("t1".into()),
            navigator_width: Some(16.5),
            routing: Default::default(),
        };
        persist(&path, &record);
        assert_eq!(load(&path), record);
    }

    #[test]
    fn a_missing_or_foreign_file_reads_as_nothing_saved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("theorem.json");
        assert_eq!(load(&path), Record::default());
        std::fs::write(&path, b"not json").unwrap();
        assert_eq!(load(&path), Record::default());
    }
}

/// Credentials remain with the authority, never in the profile's model choices.
pub fn routing(cx: &App) -> theorem_surface_contracts::model_routing::ModelRoutingRequest {
    cx.try_global::<Saved>().map(|saved| saved.record.routing.clone()).unwrap_or_default()
}
pub fn set_routing(routing: theorem_surface_contracts::model_routing::ModelRoutingRequest, cx: &mut App) {
    if cx.has_global::<Saved>() {
        cx.global_mut::<Saved>().record.routing = routing;
    }
}
