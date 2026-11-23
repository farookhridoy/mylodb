//! Staged grid edits for a SQLite table, applied in one transaction. The same `myq_data::Change` the MySQL path takes, so the UI cannot
//! tell the engines apart; the statements are SQLite's (`?` parameters, no `LIMIT` on UPDATE / DELETE, no `DEFAULT` keyword).

use crate::backend::open;
use myq_core::{Dialect, Sqlite};
use myq_data::{ApplyOutcome, Assign, CellValue, Change, ColumnMeta, TableMeta};
use rusqlite::{params_from_iter, types::Value};
use std::path::Path;

type Result<T> = std::result::Result<T, String>;

/// A statement as text fragments and bound values: run with `?` placeholders, or rendered with literals for the preview.
enum Piece {
    Sql(String),
    Param { value: Value, preview: String },
}

struct Stmt(Vec<Piece>);

impl Stmt {
    fn sql(&self) -> String {
        self.0.iter().map(|p| match p { Piece::Sql(s) => s.as_str(), Piece::Param { .. } => "?" }).collect()
    }
    fn params(&self) -> Vec<Value> {
        self.0.iter().filter_map(|p| match p { Piece::Param { value, .. } => Some(value.clone()), _ => None }).collect()
    }
    fn preview(&self) -> String {
        self.0.iter().map(|p| match p { Piece::Sql(s) => s.clone(), Piece::Param { preview, .. } => preview.clone() }).collect()
    }
}

fn sql(s: impl Into<String>) -> Piece {
    Piece::Sql(s.into())
}

fn q(s: &str) -> String {
    Sqlite.quote_ident(s)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect()
}

/// Bytes for a text value written to a BLOB column: `0x…` hex, otherwise the UTF-8 bytes.
fn text_bytes(col: &ColumnMeta, t: &str) -> Vec<u8> {
    if col.binary {
        if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
            if h.len() % 2 == 0 && h.chars().all(|c| c.is_ascii_hexdigit()) {
                return (0..h.len()).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap()).collect();
            }
        }
    }
    t.as_bytes().to_vec()
}

/// The value of one cell. `DEFAULT` is not a SQLite keyword: the callers handle it before they get here.
fn value_piece(col: &ColumnMeta, v: &CellValue) -> Result<Piece> {
    Ok(match v {
        CellValue::Default => return Err(format!("SQLite cannot set `{}` to DEFAULT: type the value", col.name)),
        CellValue::Null => Piece::Param { value: Value::Null, preview: "NULL".into() },
        CellValue::Text(t) if col.binary => {
            let b = text_bytes(col, t);
            Piece::Param { preview: format!("X'{}'", hex(&b)), value: Value::Blob(b) }
        }
        CellValue::Text(t) => Piece::Param { value: Value::Text(t.clone()), preview: Sqlite.quote_string(t) },
        CellValue::File(path) => {
            let b = std::fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))?;
            Piece::Param { preview: format!("<{} bytes from {path}>", b.len()), value: Value::Blob(b) }
        }
    })
}

fn column<'a>(meta: &'a TableMeta, name: &str) -> Result<&'a ColumnMeta> {
    meta.columns.iter().find(|c| c.name == name).ok_or_else(|| format!("column `{name}` does not exist"))
}

fn writable<'a>(meta: &'a TableMeta, name: &str) -> Result<&'a ColumnMeta> {
    let c = column(meta, name)?;
    if c.generated {
        return Err(format!("column `{name}` is generated"));
    }
    Ok(c)
}

/// WHERE for the row key. The supplied key must name exactly the table's `row_key` columns.
fn key_where(meta: &TableMeta, key: &[Assign]) -> Result<Vec<Piece>> {
    let mut names: Vec<&str> = key.iter().map(|k| k.column.as_str()).collect();
    let mut want: Vec<&str> = meta.row_key.iter().map(String::as_str).collect();
    names.sort_unstable();
    want.sort_unstable();
    if names != want {
        return Err(format!("row key must be exactly ({})", meta.row_key.join(", ")));
    }
    let mut out = vec![sql(" WHERE ")];
    for (i, k) in key.iter().enumerate() {
        if i > 0 {
            out.push(sql(" AND "));
        }
        if matches!(k.value, CellValue::Null | CellValue::Default) {
            return Err(format!("key column `{}` has no value", k.column));
        }
        out.push(sql(format!("{} = ", q(&k.column))));
        out.push(value_piece(column(meta, &k.column)?, &k.value)?);
    }
    Ok(out)
}

fn build(meta: &TableMeta, change: &Change) -> Result<Stmt> {
    if let Some(reason) = &meta.read_only_reason {
        return Err(format!("cannot change rows: {reason}"));
    }
    let table = q(&meta.table);
    let mut p = Vec::new();
    match change {
        Change::Update { key, set } => {
            if set.is_empty() {
                return Err("update changes no columns".into());
            }
            p.push(sql(format!("UPDATE {table} SET ")));
            for (i, a) in set.iter().enumerate() {
                if i > 0 {
                    p.push(sql(", "));
                }
                p.push(sql(format!("{} = ", q(&a.column))));
                p.push(value_piece(writable(meta, &a.column)?, &a.value)?);
            }
            p.extend(key_where(meta, key)?);
        }
        Change::Insert { values } => {
            // a column left at DEFAULT is simply not named: SQLite then uses the column's default
            let named: Vec<&Assign> = values.iter().filter(|a| a.value != CellValue::Default).collect();
            for a in values {
                writable(meta, &a.column)?;
            }
            if named.is_empty() {
                p.push(sql(format!("INSERT INTO {table} DEFAULT VALUES")));
            } else {
                let cols: Vec<String> = named.iter().map(|a| q(&a.column)).collect();
                p.push(sql(format!("INSERT INTO {table} ({}) VALUES (", cols.join(", "))));
                for (i, a) in named.iter().enumerate() {
                    if i > 0 {
                        p.push(sql(", "));
                    }
                    p.push(value_piece(column(meta, &a.column)?, &a.value)?);
                }
                p.push(sql(")"));
            }
        }
        Change::Delete { key } => {
            p.push(sql(format!("DELETE FROM {table}")));
            p.extend(key_where(meta, key)?);
        }
    }
    Ok(Stmt(p))
}

fn at(index: usize, message: String) -> String {
    format!("change {}: {message}", index + 1)
}

/// The SQL that `apply_changes` would run, with values rendered as literals.
pub fn preview_changes(meta: &TableMeta, changes: &[Change]) -> Result<Vec<String>> {
    changes.iter().enumerate().map(|(i, c)| build(meta, c).map(|s| s.preview()).map_err(|e| at(i, e))).collect()
}

/// Run all changes in one transaction; any failure rolls everything back. `insert_ids` holds the rowid of each insert.
pub fn apply_changes(path: &Path, meta: &TableMeta, changes: &[Change]) -> Result<ApplyOutcome> {
    // build everything first so a validation error never opens a transaction
    let stmts: Vec<Stmt> = changes.iter().enumerate().map(|(i, c)| build(meta, c).map_err(|e| at(i, e))).collect::<Result<_>>()?;
    let mut conn = open(path, true).map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let mut insert_ids = Vec::new();
    for (i, (change, stmt)) in changes.iter().zip(&stmts).enumerate() {
        let changed = tx.execute(&stmt.sql(), params_from_iter(stmt.params())).map_err(|e| at(i, e.to_string()))?;
        match change {
            Change::Insert { .. } => insert_ids.push(tx.last_insert_rowid().max(0) as u64),
            // SQLite counts a matched row even when the value is unchanged, so 0 always means the row is gone
            Change::Update { .. } | Change::Delete { .. } if changed == 0 => return Err(at(i, "row no longer exists (changed by someone else?)".into())),
            _ => {}
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(ApplyOutcome { applied: changes.len(), insert_ids })
}
