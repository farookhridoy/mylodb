//! PostgreSQL behind the engine contract (`myq_core::Backend`): connect, browse, run SQL. A connection is to one database; its
//! **schemas** are what the browser shows where MySQL shows databases.

mod backend;
mod split;
mod tls;

pub use backend::{probe, CancelToken, PgBackend, Probe, MAX_ROWS};
pub use split::{split, statement_at, Statement};
