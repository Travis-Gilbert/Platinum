//! cargo run --offline -p frame-notes --example vault_bench -- <vault> <empty-data-dir>
use anyhow::{Context, Result, ensure};
use frame_notes::Vault;
use std::{path::Path, time::Instant};

fn size(path: &Path) -> Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let mut bytes = 0;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        bytes += if meta.is_dir() {
            size(&entry.path())?
        } else {
            meta.len()
        };
    }
    Ok(bytes)
}
fn rss() -> Result<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // getrusage initializes the complete structure on success; RUSAGE_SELF has no borrowed pointers.
    ensure!(
        unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } == 0,
        "getrusage failed"
    );
    let usage = unsafe { usage.assume_init() };
    #[cfg(target_os = "macos")]
    let bytes = usage.ru_maxrss as u64;
    #[cfg(not(target_os = "macos"))]
    let bytes = usage.ru_maxrss as u64 * 1024;
    Ok(bytes)
}
fn main() -> Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    let root = Path::new(
        args.get(1)
            .context("usage: vault_bench <vault> <empty-data-dir>")?,
    );
    let data = Path::new(args.get(2).context("data directory required")?);
    ensure!(
        !data.join("index").exists(),
        "cold benchmark requires a fresh data directory"
    );
    let start = Instant::now();
    let vault = Vault::open(root, data)?;
    let cold = start.elapsed();
    let files = vault.index().files().len();
    drop(vault);
    println!("cold_index_seconds={:.6} files={files}", cold.as_secs_f64());
    let start = Instant::now();
    let vault = Vault::open(root, data)?;
    let warm = start.elapsed();
    ensure!(
        vault.index().files().len() == files,
        "warm index file count changed"
    );
    drop(vault);
    println!("warm_reopen_seconds={:.6}", warm.as_secs_f64());
    println!("peak_rss_bytes={}", rss()?);
    for name in ["authority", "index", "vfs", "blobs"] {
        println!("{name}_bytes={}", size(&data.join(name))?);
    }
    Ok(())
}
