//! SQLite as a MyLoDB engine (docs/ENGINE_ABSTRACTION.md, step 5). A file is opened **read-only** (open flag and `PRAGMA query_only`)
//! unless the caller asks for `open_writable`; then SQL and staged grid edits (`apply_changes`) may change it.

mod backend;
mod edit;
mod grid;

pub use backend::SqliteBackend;
pub use edit::{apply_changes, preview_changes};
pub use grid::{count_rows, fetch_page, table_meta};

/// The version of the SQLite library MyLoDB carries.
pub fn sqlite_version() -> &'static str {
    rusqlite::version()
}

/// The one database a SQLite file contains.
pub const MAIN: &str = "main";
