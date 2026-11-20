//! Table data access: metadata, filtered/sorted/paged reads, staged edits, cell fetch, FK lookup, export.

mod cells;
mod edit;
mod export;
mod meta;
mod query;

pub use cells::{fetch_cell, lookup, CellData, LookupItem, FETCH_CELL_MAX};
pub use edit::{apply_changes, preview_changes, ApplyOutcome, Assign, CellValue, Change};
pub use export::{export_rows, ExportFormat, ExportOptions};
pub use meta::{table_meta, ColumnMeta, ForeignKey, TableMeta};
pub use query::{count_rows, fetch_page, ColumnFilter, Op, Page, PageSpec, Sort, MAX_PAGE, CELL_PREVIEW};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("mysql: {0}")]
    Mysql(#[from] myq_driver::mysql_async::Error),
    #[error("table `{0}` not found")]
    TableNotFound(String),
    #[error("unknown column `{0}`")]
    UnknownColumn(String),
    #[error("not editable: {0}")]
    NotEditable(String),
    #[error("{0}")]
    Invalid(String),
    #[error("change {index}: {message}")]
    Change { index: usize, message: String },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Quote an identifier for MySQL.
pub(crate) fn q(ident: &str) -> String {
    format!("`{}`", ident.replace('`', "``"))
}
