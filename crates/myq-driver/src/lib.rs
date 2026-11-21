//! MySQL/MariaDB connectivity and schema introspection.

mod config;
mod defaults;
mod introspect;
mod er;
mod estimate;
mod overview;
mod script;
mod backend;
mod session;
mod values;

pub use config::{ConnConfig, SslMode};
pub use defaults::{is_mariadb, parse_column_default, ParsedDefault};
pub use er::{er_model, ErColumn, ErModel, ErRelation, ErTable};
pub use estimate::estimate_rows;
pub use overview::{table_overview, ColumnFull, Constraint, ForeignKey, PartitionInfo, TableInfo, TableOverview, TriggerInfo};
pub use introspect::{database_list, database_sizes, database_tables, index_details, object_definition, column_details, introspect, introspect_columns, table_stats, IntrospectOptions, TableStat};
pub use mysql_async::{self, Conn, Pool};
pub use values::{cell_text, hex, is_binary, is_numeric, row_cells};
pub use myq_core::{ColumnDetail, ColumnInfo, DatabaseRow, IndexDetail, ResultSet, StatementResult, TableRow};
pub use myq_sql::{split_statements, Statement};
pub use backend::MySqlBackend;
pub use session::{capabilities, unused_indexes, probe, query_grid, restore_foreign_key_checks, server_options, session_facts, set_foreign_key_checks, Grid, Probe, ServerOptions, SessionFacts};
pub use script::{execute_script, kill_query, MAX_ROWS, execute_script_with_timeout};

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
