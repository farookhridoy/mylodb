//! Query analysis: EXPLAIN with plain-language hints, and a profiler that reports stage timings
//! and what the statement cost the server (session status counters).

mod explain;
mod profile;

pub use explain::{explain, hints_from_rows, ExplainResult, Hint, Severity};
pub use profile::{profile, Counter, ProfileResult, Stage};

use myq_sql::split_statements;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("mysql: {0}")]
    Mysql(#[from] myq_driver::mysql_async::Error),
    #[error("{0}")]
    Rejected(String),
    #[error("{0}")]
    Server(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub use myq_sql::{statement_kind, StatementKind};

/// Exactly one statement, without its trailing semicolon.
pub(crate) fn single_statement(sql: &str) -> Result<String> {
    let stmts = split_statements(sql);
    match stmts.as_slice() {
        [one] => Ok(one.text.clone()),
        [] => Err(Error::Rejected("there is no statement to analyse".into())),
        _ => Err(Error::Rejected("select a single statement (this script has several)".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_statement_rules() {
        assert_eq!(single_statement("SELECT 1;").unwrap(), "SELECT 1");
        assert!(single_statement("SELECT 1; SELECT 2").is_err());
        assert!(single_statement("  -- nothing\n").is_err());
    }
}
