//! Bounded, validated ZIP input. The zip crate owns headers, codecs and CRCs;
//! these limits bound the content retained for one sidebar import.
use std::fs::File;
use std::io::{Read, Seek};
use std::path::Path;

use anyhow::{Context as _, ensure};

const ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
const ENTRY_BYTES: u64 = 8 * 1024 * 1024;
const TOTAL_BYTES: u64 = 32 * 1024 * 1024;
const ENTRY_COUNT: usize = 2_000;

#[derive(Debug)]
pub(super) struct Entry {
    pub name: String,
    pub folder: bool,
    pub data: Vec<u8>,
}

pub(super) fn read(path: &Path, depth: usize) -> anyhow::Result<Vec<Entry>> {
    let file = File::open(path).context("opening ZIP file")?;
    ensure!(
        file.metadata()?.len() <= ARCHIVE_BYTES,
        "ZIP file exceeds 64 MiB"
    );
    // Snapshot at most the budget, including a sentinel byte if the file grows.
    let mut bytes = Vec::new();
    file.take(ARCHIVE_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= ARCHIVE_BYTES,
        "ZIP file exceeds 64 MiB"
    );
    entries(std::io::Cursor::new(bytes), depth)
}

fn entries(reader: impl Read + Seek, depth: usize) -> anyhow::Result<Vec<Entry>> {
    let mut archive = zip::ZipArchive::new(reader).context("invalid ZIP directory")?;
    ensure!(
        archive.len() <= ENTRY_COUNT,
        "ZIP contains more than 2,000 entries"
    );
    let mut entries = Vec::new();
    let mut total = 0;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .context("unsupported or encrypted ZIP entry")?;
        file.enclosed_name().context("unsafe ZIP entry path")?;
        ensure!(!file.is_symlink(), "ZIP symbolic links are unsupported");
        let name = file.name()?.trim_end_matches('/').to_owned();
        ensure!(Path::new(&name).is_relative(), "absolute ZIP entry path");
        // Count the same components the sidebar inserts, including `.`; Path
        // normalization must not hide recursion from the depth budget.
        ensure!(
            name.split('/').filter(|part| !part.is_empty()).count() <= depth,
            "ZIP entry is nested too deeply"
        );
        let folder = file.is_dir();
        let mut data = Vec::new();
        if !folder {
            let limit = ENTRY_BYTES.min(TOTAL_BYTES - total);
            ensure!(
                file.size() <= limit,
                "ZIP exceeds its 8 MiB entry or 32 MiB total limit"
            );
            // Do not trust declared lengths; reading to EOF also checks CRCs.
            file.by_ref()
                .take(limit + 1)
                .read_to_end(&mut data)
                .context("corrupt ZIP entry")?;
            ensure!(
                data.len() as u64 <= limit,
                "ZIP exceeds its decompressed size limit"
            );
            total += data.len() as u64;
        }
        entries.push(Entry { name, folder, data });
    }
    Ok(entries)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    pub(crate) fn archive(files: &[(&str, &[u8])], method: CompressionMethod) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in files {
            writer
                .start_file(
                    *name,
                    SimpleFileOptions::default().compression_method(method),
                )
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn stored_and_deflated_entries_preserve_bytes() {
        for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
            let bytes = archive(&[("folder/note.md", b"# Note\n")], method);
            let entries = entries(Cursor::new(bytes), 24).unwrap();
            assert_eq!(entries[0].name, "folder/note.md");
            assert_eq!(entries[0].data, b"# Note\n");
        }
    }

    #[test]
    fn corrupt_unsafe_and_deep_archives_are_rejected() {
        let mut bytes = archive(&[("note.md", b"unique content")], CompressionMethod::Stored);
        let at = bytes
            .windows(14)
            .position(|s| s == b"unique content")
            .unwrap();
        bytes[at] ^= 1;
        assert!(
            entries(Cursor::new(bytes), 24).is_err(),
            "CRC corruption must not become an imported file"
        );
        for name in ["../escape.md", "/absolute.md", "a/b/c.md", "a/././b.md"] {
            let bytes = archive(&[(name, b"text")], CompressionMethod::Stored);
            assert!(
                entries(Cursor::new(bytes), 2).is_err(),
                "rejected path: {name}"
            );
        }
        assert!(entries(Cursor::new(b"PK\x03\x04"), 24).is_err());
    }

    #[test]
    fn aggregate_budget_is_enforced_before_importing_a_bomb() {
        let payload = vec![b'x'; ENTRY_BYTES as usize];
        let files: Vec<_> = ["a", "b", "c", "d", "e"]
            .into_iter()
            .map(|name| (name, payload.as_slice()))
            .collect();
        let bytes = archive(&files, CompressionMethod::Deflated);
        assert!(
            bytes.len() < 100_000,
            "fixture must exercise compressed expansion"
        );
        assert!(
            entries(Cursor::new(bytes), 24)
                .unwrap_err()
                .to_string()
                .contains("32 MiB")
        );
    }
}
