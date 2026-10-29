//! Explicit local migration utility. Output contains only aggregate results.
use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use frame_profile::{
    ProfileStore,
    credentials::{self, Keychain},
    migration::{self, Categories},
};
use serde_json::json;
use std::path::PathBuf;

#[derive(Clone, ValueEnum)]
enum Browser {
    Dia,
    Firefox,
}
#[derive(Parser)]
struct Args {
    #[arg(long, value_enum)]
    browser: Browser,
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long)]
    without_passwords: bool,
    #[arg(long, conflicts_with = "without_passwords")]
    verify_only: bool,
}
fn main() -> Result<()> {
    let args = Args::parse();
    let browser = match args.browser {
        Browser::Dia => "Dia",
        Browser::Firefox => "Firefox",
    };
    let root = match args.root {
        Some(root) => root,
        None => frame_profile::app_dir(&dirs::data_dir().context("No application data directory")?),
    };
    let catalog = ProfileStore::open(&root)?;
    let sources: Vec<_> = migration::discover()
        .into_iter()
        .filter(|s| s.browser == browser)
        .collect();
    anyhow::ensure!(!sources.is_empty(), "No populated source profiles found");
    for source in sources {
        let target = catalog.imported_profile(
            &source.identity(),
            &format!("{} · {}", source.browser, source.name),
            &source.email,
        )?;
        let vault = Keychain::for_profile(&target);
        let report = if args.verify_only {
            migration::Report::default()
        } else {
            migration::import(
                &target,
                &source,
                Categories {
                    bookmarks: true,
                    history: true,
                    passwords: !args.without_passwords,
                    extensions: true,
                    forms: true,
                    tabs: true,
                },
                &vault,
            )
        };
        let db = target.connect()?;
        let count = |table: &str| -> Result<i64> {
            Ok(db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?)
        };
        let (verified, inaccessible) = if args.without_passwords {
            (0, 0)
        } else {
            credentials::verify_saved(&target, &vault)?
        };
        println!(
            "{}",
            json!({
                "browser": browser, "profile_id": target.profile.id,
                "categories": report.categories.iter().map(|r| json!({
                    "category": r.category, "imported": r.counts.imported,
                    "skipped": r.counts.skipped, "error": r.error,
                })).collect::<Vec<_>>(),
                "totals": { "bookmarks": count("bookmarks")?, "history": count("history")?,
                    "passwords": count("credentials")?, "extensions": count("extensions")?,
                    "forms": target.form_values()?.len(), "tabs": target.session()?.tabs.len() },
                "passwords_verified": verified, "passwords_inaccessible": inaccessible,
                "active_profile_unchanged": true,
            })
        );
    }
    Ok(())
}
