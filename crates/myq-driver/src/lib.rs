//! MySQL/MariaDB connectivity and schema introspection.

mod config;
mod introspect;
mod script;
mod values;

pub use config::{ConnConfig, SslMode};
pub use introspect::{introspect, introspect_columns, IntrospectOptions};
pub use mysql_async::{self, Conn, Pool};
pub use values::{cell_text, hex, is_binary, is_numeric, row_cells};
pub use myq_sql::{split_statements, Statement};
pub use script::{execute_script, kill_query, ColumnInfo, ResultSet, StatementResult, MAX_ROWS};

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
