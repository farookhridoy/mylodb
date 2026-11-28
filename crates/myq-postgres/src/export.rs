//! Export from PostgreSQL: the rows of a table (with the grid's filters and sort) or the result of one query, streamed into the same
//! writers the MySQL export uses (CSV, JSON, JSON Lines, SQL INSERT, XML, HTML, XLSX), inside a read-only transaction.

use crate::backend::{explain, CancelToken, PgBackend};
use crate::grid::{order_clause, params, q, where_clause, PgMeta};
use crate::split::split;
use futures_util::StreamExt;
use myq_data::{Dest, ExportColumn, ExportOptions, ExportOutcome, Exporter, PageSpec, Tick};
use std::sync::Arc;
use tokio_postgres::types::Type;
use tokio_postgres::SimpleQueryMessage;

type Result<T> = std::result::Result<T, String>;

const TICK_EVERY: u64 = 1_000;

fn err(e: tokio_postgres::Error) -> String {
    explain(e).0
}

fn numeric(ty: &Type) -> bool {
    [Type::INT2, Type::INT4, Type::INT8, Type::FLOAT4, Type::FLOAT8, Type::NUMERIC, Type::OID].contains(ty)
}

/// Writes rows as they arrive. A binary value is written as `0x…` so the SQL INSERT format can turn it into a binary literal.
async fn stream(b: &PgBackend, sql: &str, columns: Option<Vec<ExportColumn>>, values: Vec<String>, opts: &ExportOptions, dest: Dest<'_>, name: &str, token: Arc<CancelToken>, tick: Tick<'_>) -> Result<ExportOutcome> {
    let client = b.reading().await.map_err(|e| e.0)?;
    token.watch(&client, b.config());
    let outcome = async {
        let opts = ExportOptions { postgres: true, ..opts.clone() };
        let mut count = 0u64;
        let exporter = match columns {
            // a table: its columns are selected as text, and the filter values are bound
            Some(cols) => {
                let mut exporter = Exporter::new(&opts, dest, name, &cols).map_err(|e| e.to_string())?;
                let rows = client.query_raw(sql, params(&values)).await.map_err(err)?;
                let mut rows = std::pin::pin!(rows);
                while let Some(row) = rows.next().await {
                    let row = row.map_err(err)?;
                    let cells: Vec<Option<String>> = (0..cols.len()).map(|i| row.get(i)).collect();
                    exporter.row(&cells).map_err(|e| e.to_string())?;
                    count += 1;
                    if count % TICK_EVERY == 0 && !tick(count) {
                        return Err("cancelled".to_string());
                    }
                }
                exporter
            }
            // a query: preparing it gives the types, the simple protocol every value as text
            None => {
                let prepared = client.prepare(sql).await.map_err(err)?;
                let cols: Vec<ExportColumn> = prepared.columns().iter().map(|c| ExportColumn { name: c.name().to_string(), numeric: numeric(c.type_()), binary: *c.type_() == Type::BYTEA }).collect();
                let mut exporter = Exporter::new(&opts, dest, name, &cols).map_err(|e| e.to_string())?;
                let messages = client.simple_query_raw(sql).await.map_err(err)?;
                let mut messages = std::pin::pin!(messages);
                while let Some(message) = messages.next().await {
                    let SimpleQueryMessage::Row(row) = message.map_err(err)? else { continue };
                    let cells: Vec<Option<String>> = cols
                        .iter()
                        .enumerate()
                        .map(|(i, c)| {
                            let text: &str = row.try_get(i).ok().flatten()?;
                            Some(match text.strip_prefix("\\x") {
                                Some(hex) if c.binary => format!("0x{}", hex.to_ascii_uppercase()),
                                _ => text.to_string(),
                            })
                        })
                        .collect();
                    exporter.row(&cells).map_err(|e| e.to_string())?;
                    count += 1;
                    if count % TICK_EVERY == 0 && !tick(count) {
                        return Err("cancelled".to_string());
                    }
                }
                exporter
            }
        };
        tick(count);
        exporter.finish().map_err(|e| e.to_string())
    }
    .await;
    token.release();
    // an abandoned stream leaves the connection busy: drop it instead of keeping it
    if outcome.is_ok() {
        b.give_back(client).await;
    }
    outcome.map_err(|e| if e.contains("57014") { "cancelled".to_string() } else { e })
}

/// Every row of a table that matches the grid's filters and sort.
pub async fn export_table(b: &PgBackend, m: &PgMeta, spec: &PageSpec, opts: &ExportOptions, dest: Dest<'_>, token: Arc<CancelToken>, tick: Tick<'_>) -> Result<ExportOutcome> {
    let (cond, values) = where_clause(m, &spec.filters, spec.where_sql.as_deref())?;
    let order = order_clause(m, spec)?;
    let list: Vec<String> = m
        .meta
        .columns
        .iter()
        .map(|c| {
            let n = q(&c.name);
            if c.binary { format!("'0x' || upper(encode(t.{n}, 'hex')) AS {n}") } else { format!("t.{n}::text AS {n}") }
        })
        .collect();
    let cols = m
        .meta
        .columns
        .iter()
        .map(|c| ExportColumn { name: c.name.clone(), numeric: matches!(c.data_type.as_str(), "smallint" | "int" | "bigint" | "decimal" | "float" | "double" | "oid"), binary: c.binary })
        .collect();
    let sql = format!("SELECT {} FROM {} AS t{cond}{order}", list.join(", "), m.qualified());
    stream(b, &sql, Some(cols), values, opts, dest, &m.meta.table, token, tick).await
}

/// The result of one read-only statement, with `schema` first in the search path.
pub async fn export_query(b: &PgBackend, schema: Option<&str>, sql: &str, opts: &ExportOptions, dest: Dest<'_>, token: Arc<CancelToken>, tick: Tick<'_>) -> Result<ExportOutcome> {
    let statements = split(sql);
    let [statement] = statements.as_slice() else { return Err("export one statement at a time".into()) };
    let first = statement.text.trim_start_matches('(').split(|c: char| !c.is_ascii_alphabetic()).next().unwrap_or("").to_ascii_uppercase();
    if !matches!(first.as_str(), "SELECT" | "WITH" | "TABLE" | "VALUES") {
        return Err("only SELECT statements can be exported".into());
    }
    // the search path lasts as long as the read-only transaction `stream` opens, so it is set inside the backend's session
    let b = match schema.filter(|s| !s.is_empty()) {
        Some(s) => b.with_schema(s),
        None => b.clone(),
    };
    stream(&b, &statement.text, None, vec![], opts, dest, "query", token, tick).await
}
