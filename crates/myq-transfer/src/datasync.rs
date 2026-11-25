//! Data compare and sync: find the rows two tables disagree on (by primary key), and write the statements that make the target
//! match the source. The comparison is a pure function over two in-memory tables, so it is tested without a server; reading and
//! applying go through a connection.

use crate::{Error, Result};
use myq_core::q;
use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::{is_binary, is_numeric, row_cells, Conn};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// The most rows read from one side. Beyond this, compare a part (a WHERE condition) or use a dump.
pub const MAX_ROWS: usize = 1_000_000;

#[derive(Debug, Clone)]
pub struct Col {
    pub name: String,
    pub numeric: bool,
    pub binary: bool,
}

pub type Cells = Vec<Option<String>>;

#[derive(Debug, Clone)]
pub struct Table {
    pub cols: Vec<Col>,
    pub rows: Vec<Cells>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CellDiff {
    pub column: String,
    pub source: Option<String>,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Changed {
    pub key: Vec<Option<String>>,
    pub diffs: Vec<CellDiff>,
}

#[derive(Debug, Clone)]
pub struct Diff {
    /// The columns compared (in both tables), in the source's order.
    pub columns: Vec<Col>,
    pub key: Vec<String>,
    pub same: u64,
    pub only_source: Vec<Cells>,
    pub only_target: Vec<Cells>,
    pub changed: Vec<(Cells, Cells)>,
}

fn pos(cols: &[Col], name: &str) -> Option<usize> {
    cols.iter().position(|c| c.name.eq_ignore_ascii_case(name))
}

/// Two cells are the same when their text is, or when both are numbers of equal value (`1` and `1.00`).
fn same_cell(a: &Option<String>, b: &Option<String>, numeric: bool) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => {
            x == y
                || (numeric && matches!((x.trim().parse::<f64>(), y.trim().parse::<f64>()), (Ok(p), Ok(q)) if p == q))
        }
        _ => false,
    }
}

/// Compare two tables on `key` (column names). Only columns present in both are compared; the key must be among them.
pub fn diff(source: &Table, target: &Table, key: &[String]) -> Result<Diff> {
    if key.is_empty() {
        return Err(Error::Invalid("choose the column(s) that identify a row (the primary key)".into()));
    }
    // the common columns, in the source's order
    let common: Vec<(usize, usize, Col)> = source.cols.iter().enumerate().filter_map(|(i, c)| pos(&target.cols, &c.name).map(|j| (i, j, c.clone()))).collect();
    let key_at: Vec<usize> = key
        .iter()
        .map(|k| common.iter().position(|(_, _, c)| c.name.eq_ignore_ascii_case(k)).ok_or_else(|| Error::Invalid(format!("the key column {k} is not in both tables"))))
        .collect::<Result<_>>()?;
    let project_s = |row: &Cells| -> Cells { common.iter().map(|(i, _, _)| row[*i].clone()).collect() };
    let project_t = |row: &Cells| -> Cells { common.iter().map(|(_, j, _)| row[*j].clone()).collect() };
    let key_of = |row: &Cells| -> Vec<Option<String>> { key_at.iter().map(|k| row[*k].clone()).collect() };

    let mut target_rows: HashMap<Vec<Option<String>>, Cells> = HashMap::with_capacity(target.rows.len());
    for r in &target.rows {
        let p = project_t(r);
        if target_rows.insert(key_of(&p), p).is_some() {
            return Err(Error::Invalid("the target has two rows with the same key: choose a key that is unique".into()));
        }
    }
    let mut seen_source: HashSet<Vec<Option<String>>> = HashSet::with_capacity(source.rows.len());
    let (mut same, mut only_source, mut changed) = (0u64, Vec::new(), Vec::new());
    for r in &source.rows {
        let p = project_s(r);
        let k = key_of(&p);
        if !seen_source.insert(k.clone()) {
            return Err(Error::Invalid("the source has two rows with the same key: choose a key that is unique".into()));
        }
        match target_rows.get(&k) {
            None => only_source.push(p),
            Some(t) => {
                if common.iter().enumerate().all(|(i, (_, _, c))| same_cell(&p[i], &t[i], c.numeric)) {
                    same += 1;
                } else {
                    changed.push((p, t.clone()));
                }
            }
        }
    }
    let only_target: Vec<Cells> = target_rows.into_iter().filter(|(k, _)| !seen_source.contains(k)).map(|(_, v)| v).collect();
    let mut only_target = only_target;
    only_target.sort_by(|a, b| key_of(a).cmp(&key_of(b)));
    Ok(Diff { columns: common.into_iter().map(|(_, _, c)| c).collect(), key: key.to_vec(), same, only_source, only_target, changed })
}

impl Diff {
    pub fn changed_view(&self, limit: usize) -> Vec<Changed> {
        let key_at: Vec<usize> = self.key.iter().filter_map(|k| pos(&self.columns, k)).collect();
        self.changed
            .iter()
            .take(limit)
            .map(|(s, t)| Changed {
                key: key_at.iter().map(|k| s[*k].clone()).collect(),
                diffs: self.columns.iter().enumerate().filter(|(i, c)| !same_cell(&s[*i], &t[*i], c.numeric)).map(|(i, c)| CellDiff { column: c.name.clone(), source: s[i].clone(), target: t[i].clone() }).collect(),
            })
            .collect()
    }
}

/// A value as an SQL literal.
pub fn literal(v: &Option<String>, col: &Col) -> String {
    match v {
        None => "NULL".into(),
        Some(s) if col.numeric && s.trim().parse::<f64>().is_ok() => s.trim().to_string(),
        Some(s) if col.binary && s.starts_with("0x") && s[2..].chars().all(|c| c.is_ascii_hexdigit()) => if s.len() == 2 { "''".into() } else { s.clone() },
        Some(s) => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "''")),
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SyncOptions {
    /// Remove the rows only the target has.
    pub delete_extra: bool,
    /// Rows per INSERT statement.
    pub batch: usize,
}

impl Default for SyncOptions {
    fn default() -> Self {
        SyncOptions { delete_extra: false, batch: 200 }
    }
}

/// The statements that turn the target into the source: INSERT the missing rows, UPDATE the changed columns, DELETE the extra rows
/// (only when asked). `target` is `` `db`.`table` ``, already quoted.
pub fn sync_statements(d: &Diff, target: &str, opts: SyncOptions) -> Vec<String> {
    let mut out = Vec::new();
    let cols_sql = d.columns.iter().map(|c| q(&c.name)).collect::<Vec<_>>().join(", ");
    let row_sql = |r: &Cells| format!("({})", r.iter().zip(&d.columns).map(|(v, c)| literal(v, c)).collect::<Vec<_>>().join(", "));
    for chunk in d.only_source.chunks(opts.batch.max(1)) {
        out.push(format!("INSERT INTO {target} ({cols_sql}) VALUES\n  {}", chunk.iter().map(row_sql).collect::<Vec<_>>().join(",\n  ")));
    }
    let key_at: Vec<usize> = d.key.iter().filter_map(|k| pos(&d.columns, k)).collect();
    let where_of = |row: &Cells| key_at.iter().map(|k| match &row[*k] { None => format!("{} IS NULL", q(&d.columns[*k].name)), v => format!("{} = {}", q(&d.columns[*k].name), literal(v, &d.columns[*k])) }).collect::<Vec<_>>().join(" AND ");
    for (s, t) in &d.changed {
        let sets: Vec<String> = d.columns.iter().enumerate().filter(|(i, c)| !same_cell(&s[*i], &t[*i], c.numeric)).map(|(i, c)| format!("{} = {}", q(&c.name), literal(&s[i], c))).collect();
        out.push(format!("UPDATE {target} SET {} WHERE {}", sets.join(", "), where_of(s)));
    }
    if opts.delete_extra {
        for r in &d.only_target {
            out.push(format!("DELETE FROM {target} WHERE {}", where_of(r)));
        }
    }
    out
}

/// The primary key columns of a table, in order (empty when it has none).
pub async fn primary_key(conn: &mut Conn, db: &str, table: &str) -> Result<Vec<String>> {
    let rows: Vec<String> = conn
        .exec("SELECT COLUMN_NAME FROM information_schema.KEY_COLUMN_USAGE WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND CONSTRAINT_NAME = 'PRIMARY' ORDER BY ORDINAL_POSITION", (db, table))
        .await?;
    Ok(rows)
}

/// Read a table (or part of it, with `where_sql`) into memory. Fails past `MAX_ROWS`.
pub async fn read_table(conn: &mut Conn, db: &str, table: &str, where_sql: Option<&str>) -> Result<Table> {
    let w = where_sql.map(str::trim).filter(|w| !w.is_empty()).map(|w| format!(" WHERE {w}")).unwrap_or_default();
    let mut res = conn.query_iter(format!("SELECT * FROM {}.{}{w}", q(db), q(table))).await?;
    let cols: Vec<Col> = res.columns_ref().iter().map(|c| Col { name: c.name_str().to_string(), numeric: is_numeric(c), binary: is_binary(c) }).collect();
    let mut rows = Vec::new();
    while let Some(row) = res.next().await? {
        rows.push(row_cells(row, None).0);
        if rows.len() > MAX_ROWS {
            return Err(Error::Invalid(format!("{db}.{table} has more than {MAX_ROWS} rows to compare: narrow it with a WHERE condition")));
        }
    }
    Ok(Table { cols, rows })
}

/// Run the statements on the target in one transaction (undone on any error). Returns how many ran.
pub async fn apply(conn: &mut Conn, statements: &[String], disable_fk_checks: bool) -> Result<u64> {
    if disable_fk_checks {
        conn.query_drop("SET FOREIGN_KEY_CHECKS = 0").await?;
    }
    let result: Result<u64> = async {
        conn.query_drop("START TRANSACTION").await?;
        let mut n = 0u64;
        for s in statements {
            if let Err(e) = conn.query_drop(s.as_str()).await {
                let _ = conn.query_drop("ROLLBACK").await;
                return Err(Error::Invalid(format!("nothing was changed: {e} (in: {})", s.lines().next().unwrap_or(""))));
            }
            n += 1;
        }
        conn.query_drop("COMMIT").await?;
        Ok(n)
    }
    .await;
    if disable_fk_checks {
        let _ = conn.query_drop("SET FOREIGN_KEY_CHECKS = 1").await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, numeric: bool) -> Col {
        Col { name: name.into(), numeric, binary: false }
    }
    fn row(v: &[Option<&str>]) -> Cells {
        v.iter().map(|x| x.map(String::from)).collect()
    }
    fn table(cols: Vec<Col>, rows: Vec<Cells>) -> Table {
        Table { cols, rows }
    }
    fn key(k: &[&str]) -> Vec<String> {
        k.iter().map(|s| s.to_string()).collect()
    }

    fn pair() -> (Table, Table) {
        let cols = vec![col("id", true), col("name", false), col("total", true)];
        let source = table(cols.clone(), vec![row(&[Some("1"), Some("Ann"), Some("10.50")]), row(&[Some("2"), Some("Bo"), Some("5")]), row(&[Some("3"), Some("Cy"), None]), row(&[Some("4"), Some("O'Neil"), Some("7")])]);
        let target = table(cols, vec![row(&[Some("1"), Some("Ann"), Some("10.5")]), row(&[Some("2"), Some("Bob"), Some("5")]), row(&[Some("5"), Some("Extra"), Some("1")]), row(&[Some("3"), Some("Cy"), Some("9")])]);
        (source, target)
    }

    #[test]
    fn rows_are_sorted_into_same_changed_missing_and_extra() {
        let (s, t) = pair();
        let d = diff(&s, &t, &key(&["id"])).unwrap();
        assert_eq!(d.same, 1, "1 matches (10.50 equals 10.5 as numbers)");
        assert_eq!(d.changed.len(), 2, "2 has another name, 3 has NULL against 9");
        assert_eq!(d.only_source.len(), 1);
        assert_eq!(d.only_source[0][0].as_deref(), Some("4"));
        assert_eq!(d.only_target.len(), 1);
        assert_eq!(d.only_target[0][0].as_deref(), Some("5"));
        let view = d.changed_view(10);
        let two = view.iter().find(|c| c.key == vec![Some("2".to_string())]).unwrap();
        assert_eq!(two.diffs.len(), 1);
        assert_eq!((two.diffs[0].column.as_str(), two.diffs[0].source.as_deref(), two.diffs[0].target.as_deref()), ("name", Some("Bo"), Some("Bob")));
    }

    #[test]
    fn only_common_columns_are_compared_and_a_composite_key_works() {
        let a = table(vec![col("a", false), col("b", false), col("only_a", false)], vec![row(&[Some("x"), Some("1"), Some("p")])]);
        let b = table(vec![col("B", false), col("A", false), col("only_b", false)], vec![row(&[Some("1"), Some("x"), Some("q")])]);
        let d = diff(&a, &b, &key(&["a", "b"])).unwrap();
        assert_eq!(d.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["a", "b"], "names match without regard to case and order");
        assert_eq!((d.same, d.changed.len(), d.only_source.len(), d.only_target.len()), (1, 0, 0, 0));
    }

    #[test]
    fn a_bad_key_is_explained() {
        let (s, t) = pair();
        assert!(diff(&s, &t, &[]).unwrap_err().to_string().contains("choose the column"));
        assert!(diff(&s, &t, &key(&["nope"])).unwrap_err().to_string().contains("not in both tables"));
        let dup = table(s.cols.clone(), vec![row(&[Some("1"), Some("a"), None]), row(&[Some("1"), Some("b"), None])]);
        assert!(diff(&dup, &t, &key(&["id"])).unwrap_err().to_string().contains("source has two rows"));
        assert!(diff(&s, &dup, &key(&["id"])).unwrap_err().to_string().contains("target has two rows"));
    }

    #[test]
    fn the_statements_make_the_target_match_the_source() {
        let (s, t) = pair();
        let d = diff(&s, &t, &key(&["id"])).unwrap();
        let sql = sync_statements(&d, "`b`.`orders`", SyncOptions::default());
        assert_eq!(sql.len(), 3, "one INSERT batch and two UPDATEs; no DELETE unless asked: {sql:?}");
        assert!(sql[0].starts_with("INSERT INTO `b`.`orders` (`id`, `name`, `total`) VALUES") && sql[0].contains("(4, 'O''Neil', 7)"), "{}", sql[0]);
        assert!(sql.iter().any(|s| s == "UPDATE `b`.`orders` SET `name` = 'Bo' WHERE `id` = 2"), "{sql:?}");
        assert!(sql.iter().any(|s| s == "UPDATE `b`.`orders` SET `total` = NULL WHERE `id` = 3"), "{sql:?}");
        let with_delete = sync_statements(&d, "`b`.`orders`", SyncOptions { delete_extra: true, ..Default::default() });
        assert_eq!(with_delete.last().unwrap(), "DELETE FROM `b`.`orders` WHERE `id` = 5");
        // many rows are batched
        let many = table(s.cols.clone(), (0..450).map(|i| row(&[Some(&i.to_string()), Some("n"), Some("1")])).collect());
        let empty = table(s.cols.clone(), vec![]);
        let d2 = diff(&many, &empty, &key(&["id"])).unwrap();
        assert_eq!(sync_statements(&d2, "`t`", SyncOptions { batch: 200, ..Default::default() }).len(), 3);
    }

    #[test]
    fn literals_quote_text_keep_numbers_and_hex_binary_and_null() {
        let t = col("t", false);
        let n = col("n", true);
        let b = Col { name: "b".into(), numeric: false, binary: true };
        assert_eq!(literal(&None, &t), "NULL");
        assert_eq!(literal(&Some("a\\b'c".into()), &t), "'a\\\\b''c'");
        assert_eq!(literal(&Some(" 12.50 ".into()), &n), "12.50");
        assert_eq!(literal(&Some("12abc".into()), &n), "'12abc'", "text that is not a number is quoted even in a numeric column");
        assert_eq!(literal(&Some("0xCAFE".into()), &b), "0xCAFE");
        assert_eq!(literal(&Some("0xZZ".into()), &b), "'0xZZ'");
    }

    #[test]
    fn a_null_key_is_matched_with_is_null() {
        let cols = vec![col("k", false), col("v", false)];
        let s = table(cols.clone(), vec![row(&[None, Some("new")])]);
        let t = table(cols, vec![row(&[None, Some("old")])]);
        let d = diff(&s, &t, &key(&["k"])).unwrap();
        assert_eq!(d.changed.len(), 1);
        assert_eq!(sync_statements(&d, "`t`", SyncOptions::default())[0], "UPDATE `t` SET `v` = 'new' WHERE `k` IS NULL");
    }
}
