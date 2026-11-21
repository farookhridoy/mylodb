//! The engine-neutral read path (docs/ENGINE_ABSTRACTION.md, step 3): what the application asks an engine for, and the shapes it gets back.
//! A `Backend` implementation owns its connection pool; the rest of the application sees only these types.

use crate::{Capabilities, Dialect};
use serde::Serialize;
use std::future::Future;

/// A failure reported by an engine, in words that can be shown to a person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendError(pub String);

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for BackendError {}

pub type BackendResult<T> = Result<T, BackendError>;

/// A database as the Databases page lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DatabaseRow {
    pub name: String,
    pub collation: String,
    pub charset: String,
    /// `mysql`, `sys`, `information_schema`, `performance_schema`: listed on request, never selectable.
    pub system: bool,
}

/// A table or view as the Structure page lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TableRow {
    pub name: String,
    pub is_view: bool,
    pub engine: Option<String>,
    /// The server's row estimate (exact for views: none).
    pub rows: Option<u64>,
    pub collation: Option<String>,
    /// Data plus index bytes.
    pub bytes: Option<u64>,
    /// Free space inside the table file that `OPTIMIZE TABLE` would give back (phpMyAdmin's "Overhead").
    pub overhead: Option<u64>,
    pub comment: String,
}

/// A column as the object browser shows it: `bigint unsigned`, `varchar(255)`, key and nullability.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ColumnDetail {
    pub name: String,
    /// The full declared type (`COLUMN_TYPE`), e.g. `bigint(20) unsigned` or `varchar(255)`.
    pub column_type: String,
    pub nullable: bool,
    /// `PRI`, `UNI`, `MUL` or empty.
    pub key: String,
    /// e.g. `auto_increment`.
    pub extra: String,
}

/// An index of a table, columns in order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IndexDetail {
    pub name: String,
    pub unique: bool,
    /// `BTREE`, `HASH`, `FULLTEXT`, `SPATIAL`.
    pub kind: String,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ColumnInfo {
    pub name: String,
    pub type_name: String,
    /// BINARY/BLOB columns: cells are hex text, not characters.
    pub binary: bool,
}

/// `None` cells are SQL NULL (distinct from the empty string).
#[derive(Debug, Clone, Serialize)]
pub struct ResultSet {
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<Vec<Option<String>>>,
    pub total_rows: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct StatementResult {
    pub sql: String,
    pub sets: Vec<ResultSet>,
    pub affected_rows: u64,
    pub last_insert_id: Option<u64>,
    pub warnings: u16,
    pub elapsed_ms: u64,
    pub error: Option<String>,
}

/// What the application can ask any engine for. Futures are `Send` so they can run on the async runtime's worker threads.
/// Dispatch is by enum in the application (no `dyn`): see `myq_driver::MySqlBackend`.
#[allow(async_fn_in_trait)]
pub trait Backend {
    /// How this engine writes SQL.
    fn dialect(&self) -> &dyn Dialect;
    /// What this engine (and version) can do.
    fn capabilities(&self) -> impl Future<Output = BackendResult<Capabilities>> + Send;
    /// Databases with their default collation. Sizes are separate, because they can be slow.
    fn databases(&self) -> impl Future<Output = BackendResult<Vec<DatabaseRow>>> + Send;
    /// Tables and views of one database with engine, rows, size and overhead.
    fn tables(&self, database: &str) -> impl Future<Output = BackendResult<Vec<TableRow>>> + Send;
    /// Columns of one table in declared order.
    fn columns(&self, database: &str, table: &str) -> impl Future<Output = BackendResult<Vec<ColumnDetail>>> + Send;
    /// Indexes of one table; the primary key is the index named `PRIMARY`.
    fn indexes(&self, database: &str, table: &str) -> impl Future<Output = BackendResult<Vec<IndexDetail>>> + Send;
    /// Run a script (one or more statements) and report each statement's result. `database` is the default database for the run.
    /// Statement errors are in the results; only a failure to run at all is an `Err`.
    fn run(&self, database: Option<&str>, sql: &str) -> impl Future<Output = BackendResult<Vec<StatementResult>>> + Send;
}
