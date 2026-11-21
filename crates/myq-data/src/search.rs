//! Data search: find a value anywhere in a database. One pass per table (a single `COUNT(*)` over all searchable columns), then a per-column
//! breakdown only for the tables that matched, so a miss costs one scan of the table and a hit two.

use crate::{q, Result};
use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::Conn;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct SearchRequest {
    pub needle: String,
    /// `true`: the column equals the text. `false`: the column contains it (`LIKE`, with `%` and `_` in the text taken literally).
    #[serde(default)]
    pub exact: bool,
    /// Also search numbers, dates and other non-text columns (compared as text). Slower: it cannot use indexes.
    #[serde(default)]
    pub all_columns: bool,
    /// Only these tables; `None` searches every base table.
    #[serde(default)]
    pub tables: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TableHit {
    pub table: String,
    /// Rows with a match in at least one column.
    pub rows: u64,
    /// (column, rows where that column matches), only columns with a match.
    pub columns: Vec<(String, u64)>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SearchResult {
    pub hits: Vec<TableHit>,
    pub tables_total: usize,
    pub tables_scanned: usize,
    pub cancelled: bool,
    /// Tables skipped and why (no searchable column, or an error).
    pub skipped: Vec<String>,
}

const TEXT_TYPES: [&str; 9] = ["char", "varchar", "tinytext", "text", "mediumtext", "longtext", "enum", "set", "json"];
const BINARY_TYPES: [&str; 8] = ["binary", "varbinary", "tinyblob", "blob", "mediumblob", "longblob", "geometry", "bit"];

fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// The predicate for one column and its bound value.
fn predicate(column: &str, ty: &str, exact: bool, needle: &str) -> (String, String) {
    let c = if TEXT_TYPES.contains(&ty) { q(column) } else { format!("CAST({} AS CHAR)", q(column)) };
    if exact { (format!("{c} = ?"), needle.to_string()) } else { (format!("{c} LIKE ? ESCAPE '\\\\'"), format!("%{}%", like_escape(needle))) }
}

/// Search `db`. `is_cancelled` is asked between tables (and a killed statement counts as a cancel); `progress(done, total)` after each table.
pub async fn search_database(conn: &mut Conn, db: &str, req: &SearchRequest, is_cancelled: &(dyn Fn() -> bool + Sync), progress: &mut (dyn FnMut(usize, usize) + Send)) -> Result<SearchResult> {
    let needle = req.needle.as_str();
    if needle.is_empty() {
        return Err(crate::Error::Invalid("type something to search for".into()));
    }
    let rows: Vec<(String, String, String)> = conn
        .exec(
            "SELECT c.TABLE_NAME, c.COLUMN_NAME, LOWER(c.DATA_TYPE) FROM information_schema.COLUMNS c
             JOIN information_schema.TABLES t ON t.TABLE_SCHEMA = c.TABLE_SCHEMA AND t.TABLE_NAME = c.TABLE_NAME
             WHERE c.TABLE_SCHEMA = ? AND t.TABLE_TYPE = 'BASE TABLE' ORDER BY c.TABLE_NAME, c.ORDINAL_POSITION",
            (db,),
        )
        .await?;
    // group by table, in name order
    let mut tables: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for (t, c, ty) in rows {
        if req.tables.as_ref().is_some_and(|w| !w.contains(&t)) {
            continue;
        }
        match tables.last_mut() {
            Some((name, cols)) if *name == t => cols.push((c, ty)),
            _ => tables.push((t, vec![(c, ty)])),
        }
    }
    let mut result = SearchResult { tables_total: tables.len(), ..Default::default() };
    for (i, (table, cols)) in tables.iter().enumerate() {
        if is_cancelled() {
            result.cancelled = true;
            break;
        }
        let searchable: Vec<&(String, String)> = cols.iter().filter(|(_, ty)| !BINARY_TYPES.contains(&ty.as_str()) && (req.all_columns || TEXT_TYPES.contains(&ty.as_str()))).collect();
        if searchable.is_empty() {
            result.skipped.push(format!("{table}: no {}column to search", if req.all_columns { "" } else { "text " }));
            result.tables_scanned += 1;
            progress(i + 1, result.tables_total);
            continue;
        }
        let preds: Vec<(String, String)> = searchable.iter().map(|(c, ty)| predicate(c, ty, req.exact, needle)).collect();
        let qualified = format!("{}.{}", q(db), q(table));
        let any = format!("SELECT COUNT(*) FROM {qualified} WHERE {}", preds.iter().map(|p| p.0.as_str()).collect::<Vec<_>>().join(" OR "));
        let params: Vec<String> = preds.iter().map(|p| p.1.clone()).collect();
        let count: std::result::Result<Option<u64>, _> = conn.exec_first(any.as_str(), params.clone()).await;
        match count {
            Ok(Some(n)) if n > 0 => {
                let sums = format!("SELECT {} FROM {qualified}", preds.iter().map(|p| format!("COALESCE(SUM({}), 0)", p.0)).collect::<Vec<_>>().join(", "));
                match conn.exec_first::<myq_driver::mysql_async::Row, _, _>(sums.as_str(), params).await {
                    Ok(Some(row)) => {
                        let columns: Vec<(String, u64)> = searchable.iter().enumerate().map(|(k, (c, _))| (c.clone(), row.get::<Option<u64>, _>(k).flatten().or_else(|| row.get::<Option<i64>, _>(k).flatten().map(|v| v.max(0) as u64)).or_else(|| row.get::<Option<f64>, _>(k).flatten().map(|v| v as u64)).unwrap_or(0))).filter(|(_, n)| *n > 0).collect();
                        result.hits.push(TableHit { table: table.clone(), rows: n, columns });
                    }
                    Ok(None) => result.hits.push(TableHit { table: table.clone(), rows: n, columns: vec![] }),
                    Err(e) => {
                        result.skipped.push(format!("{table}: {e}"));
                        result.hits.push(TableHit { table: table.clone(), rows: n, columns: vec![] });
                    }
                }
            }
            Ok(_) => {}
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("1317") || msg.to_lowercase().contains("interrupted") {
                    result.cancelled = true;
                    break;
                }
                result.skipped.push(format!("{table}: {msg}"));
            }
        }
        result.tables_scanned += 1;
        progress(i + 1, result.tables_total);
    }
    Ok(result)
}
