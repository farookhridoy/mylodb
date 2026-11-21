//! The database engine behind a session. Dispatch is by enum (see docs/ENGINE_ABSTRACTION.md): the commands that read through
//! `myq_core::Backend` work for every variant; the commands that still need MySQL ask for `mysql_pool()` and refuse politely otherwise.

use myq_core::{Backend, BackendResult, Capabilities, ColumnDetail, DatabaseRow, Dialect, IndexDetail, Node, StatementResult, TableRow};
use myq_driver::{MySqlBackend, Pool};

#[derive(Clone)]
pub enum Engine {
    MySql(MySqlBackend),
}

impl Engine {
    pub fn mysql(pool: Pool) -> Engine {
        Engine::MySql(MySqlBackend::from_pool(pool))
    }
}

impl Backend for Engine {
    fn dialect(&self) -> &dyn Dialect {
        match self {
            Engine::MySql(b) => b.dialect(),
        }
    }

    async fn capabilities(&self) -> BackendResult<Capabilities> {
        match self {
            Engine::MySql(b) => b.capabilities().await,
        }
    }

    async fn index(&self, include_system: bool) -> BackendResult<Vec<Node>> {
        match self {
            Engine::MySql(b) => b.index(include_system).await,
        }
    }

    async fn column_nodes(&self, database: &str) -> BackendResult<Vec<Node>> {
        match self {
            Engine::MySql(b) => b.column_nodes(database).await,
        }
    }

    async fn databases(&self) -> BackendResult<Vec<DatabaseRow>> {
        match self {
            Engine::MySql(b) => b.databases().await,
        }
    }

    async fn tables(&self, database: &str) -> BackendResult<Vec<TableRow>> {
        match self {
            Engine::MySql(b) => b.tables(database).await,
        }
    }

    async fn columns(&self, database: &str, table: &str) -> BackendResult<Vec<ColumnDetail>> {
        match self {
            Engine::MySql(b) => b.columns(database, table).await,
        }
    }

    async fn indexes(&self, database: &str, table: &str) -> BackendResult<Vec<IndexDetail>> {
        match self {
            Engine::MySql(b) => b.indexes(database, table).await,
        }
    }

    async fn run(&self, database: Option<&str>, sql: &str) -> BackendResult<Vec<StatementResult>> {
        match self {
            Engine::MySql(b) => b.run(database, sql).await,
        }
    }
}
