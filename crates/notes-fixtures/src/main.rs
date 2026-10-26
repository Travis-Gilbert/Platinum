//! `notes-fixtures generate` writes a deterministic vault.
use anyhow::Result;
use clap::{Parser, Subcommand};
use notes_fixtures::generate::{self, Options};
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Write a deterministic vault: the same seed gives the same bytes.
    Generate {
        #[arg(long, default_value_t = 10_000)]
        notes: usize,
        #[arg(long)]
        seed: u64,
        /// An empty or new directory.
        #[arg(long)]
        out: PathBuf,
    },
}

fn main() -> Result<()> {
    match Args::parse().command {
        Command::Generate { notes, seed, out } => {
            let s = generate::generate(&Options { notes, seed }, &out)?;
            println!(
                "{} notes ({} large), {} canvases, {} bases, {} attachments, {} bytes\ndigest {}",
                s.notes, s.large_notes, s.canvases, s.bases, s.attachments, s.bytes, s.digest
            );
        }
    }
    Ok(())
}
