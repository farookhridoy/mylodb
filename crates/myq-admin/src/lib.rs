//! Server-side administration: table definitions and DDL execution, users and privileges.

mod tables;
mod users;

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
}

pub type Result<T> = std::result::Result<T, Error>;
