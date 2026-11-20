use crate::{single_statement, statement_kind, Error, Result, StatementKind};
use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::{execute_script, Conn};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct Stage {
    pub name: String,
    pub ms: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Counter {
    pub name: String,
    pub value: i64,
}

#[derive(Debug, Serialize)]
pub struct ProfileResult {
    /// Wall-clock time of the statement as measured by the client.
    pub elapsed_ms: u64,
    pub rows_returned: u64,
    /// Execution stages with their durations (`SHOW PROFILE`). Empty when the server has none.
    pub stages: Vec<Stage>,
    /// Why there are no stages, when there are none.
    pub stages_note: Option<String>,
    /// Non-zero session status counters caused by the statement (rows read, temp tables, sorts …).
    pub counters: Vec<Counter>,
}

/// Counters worth showing: what the statement read, sorted, buffered and sent.
fn interesting(name: &str) -> bool {
    name.starts_with("Handler_read")
        || matches!(
            name,
            "Handler_write" | "Handler_update" | "Handler_delete" | "Handler_tmp_write" | "Handler_tmp_update"
                | "Created_tmp_tables" | "Created_tmp_disk_tables" | "Created_tmp_files"
                | "Select_scan" | "Select_full_join" | "Select_full_range_join" | "Select_range" | "Select_range_check"
                | "Sort_rows" | "Sort_scan" | "Sort_range" | "Sort_merge_passes"
                | "Innodb_rows_read" | "Innodb_buffer_pool_read_requests" | "Innodb_buffer_pool_reads" | "Innodb_data_reads"
                | "Bytes_sent" | "Bytes_received"
        )
}

async fn status(conn: &mut Conn) -> Result<HashMap<String, i64>> {
    let rows: Vec<(String, String)> = conn.query("SHOW SESSION STATUS").await?;
    Ok(rows.into_iter().filter_map(|(k, v)| v.parse::<i64>().ok().map(|v| (k, v))).collect())
}

/// Stage timings for the statement we just ran, via the (deprecated but widely available) SHOW PROFILE.
async fn stages(conn: &mut Conn, stmt: &str) -> std::result::Result<Vec<Stage>, String> {
    let profiles: Vec<(u64, f64, String)> = conn.query("SHOW PROFILES").await.map_err(|e| e.to_string())?;
    let id = profiles.iter().rev().find(|(_, _, q)| q.trim().trim_end_matches(';').trim() == stmt.trim()).map(|p| p.0);
    let Some(id) = id else { return Err("the server did not record this statement in its profile history".into()) };
    let rows: Vec<(String, f64)> = conn.query(format!("SHOW PROFILE FOR QUERY {id}")).await.map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(|(name, secs)| Stage { name, ms: secs * 1000.0 }).collect())
}

/// Run a read-only statement and report where the time and the work went.
pub async fn profile(conn: &mut Conn, database: Option<&str>, sql: &str) -> Result<ProfileResult> {
    let stmt = single_statement(sql)?;
    if statement_kind(&stmt) != StatementKind::Read {
        return Err(Error::Rejected("only SELECT statements can be profiled (profiling runs the statement)".into()));
    }
    if let Some(db) = database {
        conn.query_drop(format!("USE `{}`", db.replace('`', "``"))).await?;
    }
    let profiling = conn.query_drop("SET profiling = 1").await.is_ok();

    // Reading the status counters changes some counters itself. Measure that cost once (two reads in a
    // row) and subtract it from the statement's measurement.
    let first = status(conn).await?;
    let before = status(conn).await?;

    let mut results = execute_script(conn, None, &stmt).await;
    let r = results.pop().ok_or_else(|| Error::Server("no result".into()))?;
    if let Some(e) = r.error {
        let _ = conn.query_drop("SET profiling = 0").await;
        return Err(Error::Server(e));
    }

    let (stage_list, stages_note) = if profiling {
        match stages(conn, &stmt).await {
            Ok(s) => (s, None),
            Err(e) => (vec![], Some(e)),
        }
    } else {
        (vec![], Some("this server does not support SHOW PROFILE".to_string()))
    };
    let after = status(conn).await?;
    if profiling {
        let _ = conn.query_drop("SET profiling = 0").await;
    }

    let mut counters: Vec<Counter> = after
        .iter()
        .filter(|(k, _)| interesting(k))
        .filter_map(|(k, a)| {
            let b = before.get(k).copied().unwrap_or(0);
            let f = first.get(k).copied().unwrap_or(0);
            let value = (a - b) - (b - f);
            (value > 0).then(|| Counter { name: k.clone(), value })
        })
        .collect();
    counters.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(ProfileResult { elapsed_ms: r.elapsed_ms, rows_returned: r.sets.iter().map(|s| s.total_rows).sum(), stages: stage_list, stages_note, counters })
}
