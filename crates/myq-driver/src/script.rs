//! Statement splitting and script execution.

use crate::Result;
use myq_sql::split_statements;
use mysql_async::{prelude::Queryable, Conn, Pool, Row};
use serde::Serialize;
use std::time::Instant;

/// Rows kept per result set; the rest are counted but dropped (the grid phase adds paging).
pub const MAX_ROWS: usize = 10_000;

#[derive(Debug, Clone, Serialize)]
pub struct ColumnInfo {
    pub name: String,
    pub type_name: String,
    /// BINARY/BLOB columns: cells are hex text, not characters.
    pub binary: bool,
}

/// `None` cells are SQL NULL (distinct from the empty string).
#[derive(Debug, Clone, Serialize)]
pub struct ResultSet {
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<Vec<Option<String>>>,
    pub total_rows: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct StatementResult {
    pub sql: String,
    pub sets: Vec<ResultSet>,
    pub affected_rows: u64,
    pub last_insert_id: Option<u64>,
    pub warnings: u16,
    pub elapsed_ms: u64,
    pub error: Option<String>,
}

async fn run_one(conn: &mut Conn, sql: &str) -> StatementResult {
    let started = Instant::now();
    let mut res = StatementResult { sql: sql.to_string(), ..Default::default() };
    let outcome: Result<()> = async {
        let mut qr = conn.query_iter(sql).await?;
        loop {
            let columns: Vec<ColumnInfo> = qr
                .columns()
                .map(|c| c.iter().map(|c| ColumnInfo { name: c.name_str().to_string(), type_name: format!("{:?}", c.column_type()), binary: crate::values::is_binary(c) }).collect())
                .unwrap_or_default();
            let (rows, total) = qr
                .reduce((Vec::<Vec<Option<String>>>::new(), 0u64), |(mut rows, n), row: Row| {
                    if rows.len() < MAX_ROWS {
                        rows.push(crate::values::row_cells(row, None).0);
                    }
                    (rows, n + 1)
                })
                .await?;
            if !columns.is_empty() {
                res.sets.push(ResultSet { columns, truncated: total > rows.len() as u64, total_rows: total, rows });
            }
            res.affected_rows = qr.affected_rows();
            res.last_insert_id = qr.last_insert_id();
            res.warnings = qr.warnings();
            if qr.is_empty() {
                break;
            }
        }
        Ok(())
    }
    .await;
    if let Err(e) = outcome {
        res.error = Some(e.to_string());
    }
    res.elapsed_ms = started.elapsed().as_millis() as u64;
    res
}

/// Run every statement of `sql` in order on `conn`, stopping after the first error.
/// `database` is selected first when given.
pub async fn execute_script(conn: &mut Conn, database: Option<&str>, sql: &str) -> Vec<StatementResult> {
    let mut out = Vec::new();
    if let Some(db) = database {
        let use_sql = format!("USE `{}`", db.replace('`', "``"));
        if let Err(e) = conn.query_drop(use_sql).await {
            return vec![StatementResult { sql: format!("USE {db}"), error: Some(e.to_string()), ..Default::default() }];
        }
    }
    for st in split_statements(sql) {
        let r = run_one(conn, &st.text).await;
        let failed = r.error.is_some();
        out.push(r);
        if failed {
            break;
        }
    }
    out
}

/// Like `execute_script`, but the run may take at most `timeout_secs`: after that the running statement is killed (the same as
/// Cancel). Returns the results and whether the timeout fired. A killed `SELECT SLEEP()` or similar may end without an error, so the
/// last result is marked as stopped either way.
pub async fn execute_script_with_timeout(pool: &Pool, conn: &mut Conn, database: Option<&str>, sql: &str, timeout_secs: Option<u32>) -> (Vec<StatementResult>, bool) {
    let fired = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let watchdog = timeout_secs.map(|secs| {
        let (pool, fired, server_conn) = (pool.clone(), fired.clone(), conn.id());
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(u64::from(secs))).await;
            fired.store(true, std::sync::atomic::Ordering::SeqCst);
            let _ = kill_query(&pool, server_conn).await;
        })
    });
    let mut results = execute_script(conn, database, sql).await;
    if let Some(w) = watchdog {
        w.abort();
    }
    let timed_out = fired.load(std::sync::atomic::Ordering::SeqCst);
    if timed_out {
        let secs = timeout_secs.unwrap_or(0);
        let note = format!("Stopped by the {secs} s query timeout of this connection; the result may be incomplete.");
        match results.last_mut() {
            Some(last) => last.error = Some(match last.error.take() { Some(e) => format!("{note} ({e})"), None => note }),
            None => results.push(StatementResult { error: Some(note), ..Default::default() }),
        }
    }
    (results, timed_out)
}

/// Interrupt the statement running on server connection `connection_id` (`KILL QUERY`).
pub async fn kill_query(pool: &Pool, connection_id: u32) -> Result<()> {
    let mut c = pool.get_conn().await?;
    c.query_drop(format!("KILL QUERY {connection_id}")).await?;
    Ok(())
}
