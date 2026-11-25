//! Moving data in and out: SQL script restore, CSV import and database backup.
//!
//! Long operations report `Progress` and stop promptly when the `cancel` flag is set.

mod backup;
mod csv_import;
mod json_import;
mod sql_run;

pub use backup::{backup, prune_backups, strip_definer, BackupOptions, BackupResult, BackupTarget, DbSelection};
pub use csv_import::{import_csv, infer_schema, preview_csv, sniff_delimiter, CsvFormat, CsvPreview, ImportMode, ImportPlan, ImportResult, InferredColumn, RowError};
pub use json_import::{json_to_csv, JsonConversion, MAX_ARRAY_BYTES, NULL_TEXT};
pub use sql_run::{preview_source, run_source, PreviewStatement, RunOptions, RunResult, ScriptError, SourcePreview};

use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, serde::Serialize)]
pub struct Progress {
    pub phase: String,
    pub done: u64,
    pub total: Option<u64>,
    pub message: String,
}

pub type ProgressFn<'a> = &'a mut (dyn FnMut(Progress) + Send);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("mysql: {0}")]
    Mysql(#[from] myq_driver::mysql_async::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("csv: {0}")]
    Csv(#[from] csv::Error),
    #[error("zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("{0}")]
    Invalid(String),
    #[error("cancelled")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) { Err(Error::Cancelled) } else { Ok(()) }
}

pub(crate) use myq_core::q;
