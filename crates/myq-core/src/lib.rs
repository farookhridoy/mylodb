//! The engine contract of MyLoDB (docs/ENGINE_ABSTRACTION.md, step 2): how an engine writes SQL text (`Dialect`) and what it can do
//! (`Capabilities`). No dependencies on a driver and no I/O, so every crate can use it.
//!
//! Today there is one dialect, MySQL / MariaDB. It owns the *only* implementation of identifier and string quoting: the crates that used
//! to carry their own copy (`myq-schema`, `myq-data`, `myq-admin`, `myq-transfer`, `myq-sql`) call it.

mod backend;
mod capabilities;
mod dialect;
mod objects;

pub use backend::{Backend, BackendError, BackendResult, ColumnDetail, ColumnInfo, DatabaseRow, IndexDetail, ResultSet, StatementResult, TableRow};
pub use capabilities::{Cap, Capabilities, Flavor};
pub use objects::{Kind, Node};
pub use dialect::{lit, q, Dialect, MySql};
