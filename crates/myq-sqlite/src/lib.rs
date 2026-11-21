//! SQLite as a MyLoDB engine (docs/ENGINE_ABSTRACTION.md, step 5). The file is opened **read-only** in this version, by the
//! open flag and by `PRAGMA query_only`, so nothing here can change it.

mod backend;
mod grid;

pub use backend::SqliteBackend;
pub use grid::{count_rows, fetch_page, table_meta};

/// The one database a SQLite file contains.
pub const MAIN: &str = "main";
