//! A copy of an attached file, kept beside the CRM records.

use std::fs;
use std::path::Path;

use super::model::Id;

pub(crate) fn keep(dir: &Path, id: Id, source: &Path) -> Option<String> {
    let original = source.file_name()?.to_string_lossy();
    let safe = sanitize_filename::sanitize(original.as_ref());
    let safe = if safe.is_empty() {
        "file".to_owned()
    } else {
        safe
    };
    let name = format!("{}-{safe}", id.raw());
    let files = dir.join("files");
    fs::create_dir_all(&files).ok()?;
    fs::copy(source, files.join(&name)).ok()?;
    Some(name)
}
