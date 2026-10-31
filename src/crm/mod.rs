//! Index, a place on the shelf beside the Agent.
//!
//! The table and the record are the operating surface: objects, saved views,
//! and a company whose name stays put while the center moves between overview,
//! activity, notes, tasks, and evidence. Highlights group the same passages
//! by theme. A summary is kept only when it names the passage it came from.
//! Records are kept in the data directory. An attached file is read the way
//! Glim reads one: a JSON object, a CSV table, a SQLite database, or text.

mod files;
mod material;
mod model;
mod shelf;
mod workspace;

pub use workspace::Crm;
