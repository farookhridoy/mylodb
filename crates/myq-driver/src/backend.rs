//! `myq_core::Backend` for MySQL and MariaDB: delegates to the functions that already exist in this crate.

use crate::{column_details, database_list, database_tables, execute_script, index_details, introspect, introspect_columns, pool, ConnConfig, Error, IntrospectOptions, Pool};
use myq_core::{Backend, Node, BackendError, BackendResult, Capabilities, ColumnDetail, DatabaseRow, Dialect, IndexDetail, MySql, StatementResult, TableRow};

fn failed(e: Error) -> BackendError {
    BackendError(e.to_string())
}

/// A MySQL or MariaDB server, reached through its own connection pool.
#[derive(Clone)]
pub struct MySqlBackend {
    pool: Pool,
}

impl MySqlBackend {
    /// No network I/O happens until the first call.
    pub fn new(cfg: &ConnConfig) -> MySqlBackend {
        MySqlBackend { pool: pool(cfg) }
    }

    /// Wrap a pool that already exists (the application's session pool).
    pub fn from_pool(pool: Pool) -> MySqlBackend {
        MySqlBackend { pool }
    }
}

impl Backend for MySqlBackend {
    fn dialect(&self) -> &dyn Dialect {
        &MySql
    }

    async fn capabilities(&self) -> BackendResult<Capabilities> {
        let mut conn = self.pool.get_conn().await.map_err(|e| failed(e.into()))?;
        crate::capabilities(&mut conn).await.map_err(failed)
    }

    async fn index(&self, include_system: bool) -> BackendResult<Vec<Node>> {
        introspect(&self.pool, &IntrospectOptions { include_system, include_columns: false }).await.map_err(failed)
    }

    async fn column_nodes(&self, database: &str) -> BackendResult<Vec<Node>> {
        introspect_columns(&self.pool, database).await.map_err(failed)
    }

    async fn databases(&self) -> BackendResult<Vec<DatabaseRow>> {
        database_list(&self.pool).await.map_err(failed)
    }

    async fn tables(&self, database: &str) -> BackendResult<Vec<TableRow>> {
        database_tables(&self.pool, database).await.map_err(failed)
    }

    async fn columns(&self, database: &str, table: &str) -> BackendResult<Vec<ColumnDetail>> {
        column_details(&self.pool, database, table).await.map_err(failed)
    }

    async fn indexes(&self, database: &str, table: &str) -> BackendResult<Vec<IndexDetail>> {
        index_details(&self.pool, database, table).await.map_err(failed)
    }

    async fn run(&self, database: Option<&str>, sql: &str) -> BackendResult<Vec<StatementResult>> {
        let mut conn = self.pool.get_conn().await.map_err(|e| failed(e.into()))?;
        let results = execute_script(&mut conn, database, sql).await;
        Ok(results)
    }
}
