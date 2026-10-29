//! `notes-fixtures generate` writes a deterministic vault; `notes-fixtures reference` records
//! Obsidian 1.14.5's answers for one; `notes-fixtures compare-dom` compares two recordings of the
//! reading view's DOM.
use anyhow::{Context, Result};
use clap::{ArgGroup, Parser, Subcommand};
use notes_fixtures::{
    compare,
    generate::{self, Options},
    launch,
    obsidian::Obsidian,
    reference::{self, Config, DOM_SELECTOR, Group},
};
use std::{fs, path::PathBuf, time::Duration};

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
    /// Drive Obsidian's CLI over a vault and store its answers as JSON.
    #[command(group(ArgGroup::new("target").required(true).args(["app", "attach"])))]
    Reference {
        /// The vault folder. With --app it is copied and the copy is opened; the CLI runs there.
        #[arg(long)]
        vault: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Start Obsidian from this Obsidian.app on a private HOME and profile, run against a
        /// copy of the vault, and quit it afterwards. The user's own Obsidian is not touched.
        #[arg(long)]
        app: Option<PathBuf>,
        /// Use whichever Obsidian the CLI named by --obsidian reaches. It runs against that
        /// app's own profile, so use it only with an instance you isolated yourself, or a fake.
        #[arg(long)]
        attach: bool,
        /// The Obsidian CLI executable, for --attach.
        #[arg(long, default_value = "obsidian", conflicts_with = "app")]
        obsidian: PathBuf,
        /// Pass `vault=<name>` instead of relying on the working directory.
        #[arg(long, conflicts_with = "app")]
        vault_name: Option<String>,
        /// Search queries as JSON (see queries.json); defaults to the bundled set.
        #[arg(long)]
        queries: Option<PathBuf>,
        /// Run only these groups.
        #[arg(long, value_enum, value_delimiter = ',')]
        only: Vec<Group>,
        /// CSS selector read by `dev:dom` for each note.
        #[arg(long, default_value = DOM_SELECTOR)]
        dom_selector: String,
        /// Milliseconds before each read of a note's DOM, which is read until it stops changing.
        #[arg(long, default_value_t = 500)]
        settle_ms: u64,
        /// With --app: seconds the app gets to bind its socket and open the vault.
        #[arg(long, default_value_t = 90, requires = "app")]
        startup_secs: u64,
        /// With --app: an extra Electron or Chromium flag for the app, e.g. `--app-arg=--force-device-scale-factor=1`.
        #[arg(long = "app-arg", requires = "app", allow_hyphen_values = true)]
        app_args: Vec<String>,
    },
    /// Compare two directories of reading-view DOM recordings, laid out like
    /// `reference/fixture-vault/dom/`, and print each note's differences with their path in the
    /// tree. Exits non-zero when a note differs or has no actual recording.
    CompareDom {
        /// The reference recordings (Obsidian's).
        #[arg(long)]
        expected: PathBuf,
        /// The recordings to check (Frame's).
        #[arg(long)]
        actual: PathBuf,
        /// Compare only recordings whose path contains this text.
        #[arg(long)]
        note: Option<String>,
        /// Drop the elements matching this CSS selector from both sides: what a page script
        /// adds and the renderer does not emit. Repeatable.
        #[arg(long)]
        ignore: Vec<String>,
        /// Compare the elements matching this CSS selector by tag, classes and `data-*` alone
        /// (a script such as MathJax owns their subtree). Repeatable.
        #[arg(long)]
        opaque: Vec<String>,
        /// Compare the elements matching this CSS selector by tag, classes, `data-*` and the
        /// text of their subtree (a script such as Prism wraps the text). Repeatable.
        #[arg(long)]
        opaque_text: Vec<String>,
        /// Let an expected recording end before the actual one: Obsidian renders a long note's
        /// sections as they scroll into view.
        #[arg(long)]
        allow_truncated_expected: bool,
        /// The URL prefix under which the actual side serves vault files.
        #[arg(long, default_value = "/files/")]
        resource_prefix: String,
        /// Differences printed per note.
        #[arg(long, default_value_t = 12)]
        max_differences: usize,
    },
    /// Start an isolated Obsidian, ask it a few read-only questions, print the answers and quit
    /// it: a short check that the launch works and that the CLI reaches the instance.
    Probe {
        /// The Obsidian.app folder.
        #[arg(long)]
        app: PathBuf,
        /// The vault folder; a copy is opened.
        #[arg(long)]
        vault: PathBuf,
        #[arg(long, default_value_t = 90)]
        startup_secs: u64,
        /// An extra Electron or Chromium flag for the app.
        #[arg(long = "app-arg", allow_hyphen_values = true)]
        app_args: Vec<String>,
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
        Command::Reference {
            vault,
            out,
            app,
            attach: _,
            obsidian,
            vault_name,
            queries,
            only,
            dom_selector,
            settle_ms,
            startup_secs,
            app_args,
        } => {
            let vault = vault
                .canonicalize()
                .with_context(|| format!("vault {}", vault.display()))?;
            let queries = match queries {
                Some(path) => serde_json::from_str(
                    &fs::read_to_string(&path).with_context(|| path.display().to_string())?,
                )?,
                None => reference::default_queries()?,
            };
            let instance = app
                .map(|app| {
                    launch::Instance::start(&launch::Options {
                        app,
                        vault: vault.clone(),
                        startup: Duration::from_secs(startup_secs),
                        args: app_args,
                    })
                })
                .transpose()?;
            let cfg = Config {
                obsidian: match &instance {
                    Some(i) => i.client(),
                    None => Obsidian::new(obsidian, &vault).vault_name(vault_name),
                },
                isolation: instance.as_ref().map(launch::Instance::isolation),
                vault,
                out,
                queries,
                only,
                dom_selector,
                settle: Duration::from_millis(settle_ms),
            };
            let result = reference::run(&cfg);
            let quit = instance.map(launch::Instance::quit).transpose();
            let m = result?;
            quit?;
            println!(
                "Obsidian {}: {} commands, {} failed, {} notes; wrote {}",
                m.obsidian_version,
                m.commands,
                m.failed.len(),
                m.notes,
                cfg.out.display()
            );
        }
        Command::CompareDom {
            expected,
            actual,
            note,
            ignore,
            opaque,
            opaque_text,
            allow_truncated_expected,
            resource_prefix,
            max_differences,
        } => {
            let report = compare::compare_dirs(
                &expected,
                &actual,
                &compare::Options {
                    ignore,
                    opaque,
                    opaque_text,
                    allow_truncated_expected,
                    resource_prefix,
                    max_differences,
                },
                note.as_deref(),
            )?;
            print!("{}", report.render());
            if !report.passes() {
                std::process::exit(1);
            }
        }
        Command::Probe {
            app,
            vault,
            startup_secs,
            app_args,
        } => {
            let instance = launch::Instance::start(&launch::Options {
                app,
                vault,
                startup: Duration::from_secs(startup_secs),
                args: app_args,
            })?;
            println!(
                "isolation {}",
                serde_json::to_string_pretty(&instance.isolation())?
            );
            for r in launch::probe(&instance) {
                println!(
                    "\n$ obsidian {}  (exit {:?}{})\n{}{}",
                    r.key(),
                    r.exit_code,
                    r.error
                        .as_deref()
                        .map_or(String::new(), |e| format!(", {e}")),
                    r.stdout,
                    if r.stderr.is_empty() {
                        String::new()
                    } else {
                        format!("stderr: {}", r.stderr)
                    },
                );
            }
            println!("\napp log:\n{}", instance.log());
            instance.quit()?;
        }
    }
    Ok(())
}
