//! MySQL/MariaDB connectivity and schema introspection.

mod config;
mod defaults;
mod introspect;
mod estimate;
mod overview;
mod script;
mod values;

pub use config::{ConnConfig, SslMode};
pub use defaults::{is_mariadb, parse_column_default, ParsedDefault};
pub use estimate::estimate_rows;
pub use overview::{table_overview, ColumnFull, Constraint, ForeignKey, PartitionInfo, TableInfo, TableOverview, TriggerInfo};
pub use introspect::{database_list, database_sizes, database_tables, DatabaseRow, TableRow, index_details, object_definition, IndexDetail, column_details, introspect, introspect_columns, table_stats, ColumnDetail, IntrospectOptions, TableStat};
pub use mysql_async::{self, Conn, Pool};
pub use values::{cell_text, hex, is_binary, is_numeric, row_cells};
pub use myq_sql::{split_statements, Statement};
pub use script::{execute_script, kill_query, ColumnInfo, ResultSet, StatementResult, MAX_ROWS, execute_script_with_timeout};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("mysql: {0}")]
    Mysql(#[from] mysql_async::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Open a pool for the given configuration. No network I/O happens until the first query.
pub fn pool(cfg: &ConnConfig) -> Pool {
    Pool::new(cfg.to_opts())
}
