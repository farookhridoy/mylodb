//! The query plan (`EXPLAIN (FORMAT JSON)`), and with `analyze` the measured one. Both run in a read-only transaction that is rolled
//! back, so that measuring a statement cannot change anything.

use crate::backend::{explain as server_error, PgBackend};
use crate::split::split;
use myq_core::{ColumnInfo, Postgres, Dialect, ResultSet};
use myq_profile::{ExplainResult, Hint, Severity};
use serde_json::Value;
use tokio_postgres::SimpleQueryMessage;

/// Rows from which reading a whole table is worth a warning.
const BIG: f64 = 1_000.0;

/// A sequential scan found in a plan: the table, and the rows it read when that was measured.
type Scan = (String, Option<f64>);

fn walk(node: &Value, depth: usize, rows: &mut Vec<Vec<Option<String>>>, scans: &mut Vec<Scan>, hints: &mut Vec<Hint>) {
    let text = |k: &str| node.get(k).and_then(Value::as_str).map(str::to_string);
    let number = |k: &str| node.get(k).and_then(Value::as_f64);
    let kind = text("Node Type").unwrap_or_default();
    let table = text("Relation Name");
    let condition = ["Index Cond", "Hash Cond", "Merge Cond", "Join Filter", "Filter"].iter().find_map(|k| text(k).map(|v| format!("{k}: {v}")));
    rows.push(vec![
        Some(format!("{}{kind}", "  ".repeat(depth))),
        table.clone(),
        text("Index Name"),
        number("Plan Rows").map(|n| n.to_string()),
        number("Actual Rows").map(|n| n.to_string()),
        number("Total Cost").map(|n| n.to_string()),
        number("Actual Total Time").map(|n| format!("{:.3}", n * number("Actual Loops").unwrap_or(1.0))),
        condition,
    ]);
    if kind == "Seq Scan" {
        if let Some(t) = table {
            let loops = number("Actual Loops").unwrap_or(1.0);
            scans.push((t, number("Actual Rows").map(|a| (a + number("Rows Removed by Filter").unwrap_or(0.0)) * loops)));
        }
    }
    if text("Sort Space Type").as_deref() == Some("Disk") {
        hints.push(Hint { severity: Severity::Warning, message: "A sort did not fit in memory and used the disk (see work_mem).".into(), table: None });
    }
    for child in node.get("Plans").and_then(Value::as_array).into_iter().flatten() {
        walk(child, depth + 1, rows, scans, hints);
    }
}

pub async fn explain(b: &PgBackend, schema: Option<&str>, sql: &str, analyze: bool) -> Result<ExplainResult, String> {
    let statements = split(sql);
    let [statement] = statements.as_slice() else { return Err("explain one statement at a time".into()) };
    let first = statement.text.trim_start_matches('(').split(|c: char| !c.is_ascii_alphabetic()).next().unwrap_or("").to_ascii_uppercase();
    if first == "EXPLAIN" {
        return Err("leave out EXPLAIN: the button adds it".into());
    }
    if analyze && !matches!(first.as_str(), "SELECT" | "WITH" | "TABLE" | "VALUES") {
        return Err("Analyze runs the statement, so it is offered for SELECT only".into());
    }
    let client = b.reading().await.map_err(|e| e.0)?;
    let outcome = async {
        if let Some(s) = schema.filter(|s| !s.is_empty()) {
            client.batch_execute(&format!("SET LOCAL search_path TO {}, public", Postgres.quote_ident(s))).await.map_err(|e| server_error(e).0)?;
        }
        let options = if analyze { "ANALYZE, BUFFERS, FORMAT JSON" } else { "FORMAT JSON" };
        let query = format!("EXPLAIN ({options}) {}", statement.text);
        // preparing refuses anything that is not exactly one statement
        client.prepare(&query).await.map_err(|e| server_error(e).0)?;
        let messages = client.simple_query(&query).await.map_err(|e| server_error(e).0)?;
        let json = messages.iter().find_map(|m| match m { SimpleQueryMessage::Row(r) => r.get(0).map(str::to_string), _ => None }).ok_or_else(|| "the server returned no plan".to_string())?;
        let doc: Value = serde_json::from_str(&json).map_err(|e| format!("the plan could not be read: {e}"))?;
        let (mut rows, mut scans, mut hints) = (Vec::new(), Vec::new(), Vec::new());
        if let Some(plan) = doc.get(0).and_then(|d| d.get("Plan")) {
            walk(plan, 0, &mut rows, &mut scans, &mut hints);
        }
        for (table, measured) in scans {
            // without a measurement the plan only says how many rows come out: the table's statistics say how many are read
            let read = match measured {
                Some(n) => n,
                None => client
                    .query_opt("SELECT reltuples::float8 FROM pg_catalog.pg_class WHERE oid = pg_catalog.to_regclass($1)", &[&Postgres.quote_ident(&table)])
                    .await
                    .map_err(|e| server_error(e).0)?
                    .map_or(0.0, |r| r.get(0)),
            };
            if read >= BIG {
                hints.push(Hint { severity: Severity::Warning, message: format!("Sequential scan of `{table}` (about {} rows). An index on the columns used to filter or join would avoid reading every row.", read as u64), table: Some(table) });
            }
        }
        Ok::<_, String>((json, rows, hints))
    }
    .await;
    b.give_back(client).await;
    let (json, rows, hints) = outcome?;
    let columns = ["step", "table", "index", "estimated rows", "actual rows", "cost", "time ms", "condition"].iter().map(|n| ColumnInfo { name: n.to_string(), type_name: String::new(), binary: false }).collect();
    let total = rows.len() as u64;
    Ok(ExplainResult { plan: ResultSet { columns, rows, total_rows: total, truncated: false }, json: Some(json), tree: None, hints })
}
