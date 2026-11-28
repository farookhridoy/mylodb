//! PostgreSQL behind the engine contract (`myq_core::Backend`): connect, browse, run SQL, page and edit rows, export. A connection is to one database; its
//! **schemas** are what the browser shows where MySQL shows databases.

mod backend;
mod designer;
mod dump;
mod explain;
mod objects;
mod edit;
mod export;
mod grid;
mod split;
mod tls;

pub use backend::{probe, CancelToken, PgBackend, Probe, MAX_ROWS};
pub use designer::{apply_ddl, ddl_statements, read_table_def};
pub use dump::{dump, is_archive, restore, DumpContent, DumpFormat, DumpOptions, DumpResult, RestoreOptions, RestoreResult};
pub use explain::explain;
pub use objects::{drop_object_sql, object_script};
pub use edit::{apply_changes, preview_changes};
pub use export::{export_query, export_table};
pub use grid::{count_rows, estimate_count, fetch_cell, fetch_page, lookup, table_meta, PgMeta};
pub use split::{split, statement_at, Statement};
