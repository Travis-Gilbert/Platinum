//! Prepare an attachment from one owned copy before deriving any evidence.

use std::fs;
use std::path::Path;

use super::material::{self, Reading};

pub(crate) struct Attachment {
    pub file: String,
    pub reading: Reading,
}

pub(crate) fn prepare(dir: &Path, source: &Path) -> Result<Attachment, String> {
    prepare_copy(dir, source).map_err(|error| format!("Could not attach file: {error}"))
}

fn prepare_copy(dir: &Path, source: &Path) -> anyhow::Result<Attachment> {
    let original = source
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("missing file name"))?;
    let safe = sanitize_filename::sanitize(original.to_string_lossy());
    anyhow::ensure!(!safe.is_empty(), "missing file name");
    let files = dir.join("files");
    fs::create_dir_all(&files)?;
    let directory = tempfile::Builder::new()
        .prefix("attachment-")
        .tempdir_in(&files)?;
    let path = directory.path().join(safe);
    fs::copy(source, &path)?;
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut sidecar = source.as_os_str().to_owned();
        sidecar.push(suffix);
        let sidecar = std::path::PathBuf::from(sidecar);
        if sidecar.is_file() {
            let mut destination = path.as_os_str().to_owned();
            destination.push(suffix);
            fs::copy(sidecar, destination)?;
        }
    }
    // Both saved passages and later displayed rows come from this copy. The
    // original may change or disappear after this point without changing either.
    let reading = material::open(&path).map_err(anyhow::Error::msg)?;
    anyhow::ensure!(
        !reading.passages.is_empty(),
        "That file has nothing to highlight."
    );
    let file = path.strip_prefix(&files)?.to_string_lossy().into_owned();
    let _ = directory.keep();
    Ok(Attachment { file, reading })
}
