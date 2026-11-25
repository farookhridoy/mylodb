//! Server-side administration: table definitions and DDL execution, users and privileges.

mod server;
mod tables;
mod usages;
mod users;

pub use server::{
    binary_logs, binlog_events, error_log, kill, log_settings, maintain, processlist, server_info, set_variable, slow_log, status, top_statements, variables, BinaryLog, BinlogEvent, TopStatement,
    CheckOption, KillKind, LogLine, LogSettings, Maintenance, MaintenanceRow, Process, ProcessList, Scope as VarScope, ServerInfo, SlowQuery, Variable,
};
pub use usages::{filter_definitions, find_usages, first_mention, mentions, Usage};
pub use tables::{read_table_def, run_statements, ApplyError};
pub use users::{
    account_sql, drop_account_sql, grant_diff_sql, list_accounts, parse_grants, read_grants, run_sql, Account, AccountEdit, Grants, Scope, ScopeGrant, Sql,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("mysql: {0}")]
    Mysql(#[from] myq_driver::mysql_async::Error),
    #[error("`{0}` is not a base table (views cannot be edited in the designer)")]
    NotATable(String),
    #[error("table `{0}` not found")]
    NotFound(String),
    /// The request itself is not allowed (bad name, unsafe target).
    #[error("{0}")]
    Rejected(String),
    /// The server cannot provide this right now (feature off, older version).
    #[error("{0}")]
    Unavailable(String),
}

pub type Result<T> = std::result::Result<T, Error>;
