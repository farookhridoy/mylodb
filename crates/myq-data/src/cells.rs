use crate::{edit::{Assign, CellValue}, meta::TableMeta, q, Error, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use myq_driver::mysql_async::{prelude::Queryable, Value};
use myq_driver::{cell_text, Conn};
use serde::Serialize;

/// Upper bound for one cell fetched into the UI (the viewer and editors work on this).
pub const FETCH_CELL_MAX: usize = 16 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct CellData {
    pub is_null: bool,
    /// Full length in bytes (before any cut at `FETCH_CELL_MAX`).
    pub len: usize,
    pub truncated: bool,
    /// Raw bytes (text columns: UTF-8), base64-encoded.
    pub base64: Option<String>,
}

/// Load one full cell value, identified by the row key.
pub async fn fetch_cell(conn: &mut Conn, meta: &TableMeta, key: &[Assign], column: &str) -> Result<CellData> {
    meta.column(column)?;
    if meta.row_key.is_empty() {
        return Err(Error::NotEditable("cannot locate the row without a key".into()));
    }
    let mut conds = Vec::new();
    let mut params = Vec::new();
    for k in key {
        meta.column(&k.column)?;
        let CellValue::Text(t) = &k.value else { return Err(Error::Invalid("key values must be text".into())) };
        conds.push(format!("{} = ?", q(&k.column)));
        params.push(Value::Bytes(t.as_bytes().to_vec()));
    }
    let sql = format!("SELECT {} FROM {} WHERE {} LIMIT 1", q(column), meta.qualified(), conds.join(" AND "));
    let mut qr = conn.exec_iter(sql, params).await?;
    let row = qr.next().await?.ok_or_else(|| Error::Invalid("row not found".into()))?;
    let col = row.columns()[0].clone();
    let value = row.unwrap().remove(0);
    drop(qr);
    let bytes = match value {
        Value::NULL => return Ok(CellData { is_null: true, len: 0, truncated: false, base64: None }),
        Value::Bytes(b) => b,
        other => cell_text(other, &col, None).0.unwrap_or_default().into_bytes(),
    };
    let len = bytes.len();
    let cut = &bytes[..len.min(FETCH_CELL_MAX)];
    Ok(CellData { is_null: false, len, truncated: len > FETCH_CELL_MAX, base64: Some(STANDARD.encode(cut)) })
}

#[derive(Debug, Serialize)]
pub struct LookupItem {
    pub value: String,
    /// A human-readable column from the referenced row, when the table has one.
    pub label: Option<String>,
}

/// Valid values of a foreign key's target column, optionally narrowed by `search`.
pub async fn lookup(conn: &mut Conn, ref_meta: &TableMeta, ref_column: &str, search: &str, limit: u32) -> Result<Vec<LookupItem>> {
    ref_meta.column(ref_column)?;
    let label = ref_meta
        .columns
        .iter()
        .find(|c| c.name != ref_column && matches!(c.data_type.to_lowercase().as_str(), "varchar" | "char" | "text" | "tinytext" | "mediumtext"))
        .map(|c| c.name.clone());
    let mut select = q(ref_column);
    if let Some(l) = &label {
        select = format!("{select}, {}", q(l));
    }
    let mut sql = format!("SELECT {select} FROM {}", ref_meta.qualified());
    let mut params = Vec::new();
    if !search.is_empty() {
        let pat = format!("%{}%", search.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        let mut conds = vec![format!("{} LIKE ?", q(ref_column))];
        params.push(Value::Bytes(pat.clone().into_bytes()));
        if let Some(l) = &label {
            conds.push(format!("{} LIKE ?", q(l)));
            params.push(Value::Bytes(pat.into_bytes()));
        }
        sql = format!("{sql} WHERE {}", conds.join(" OR "));
    }
    sql = format!("{sql} ORDER BY {} LIMIT {}", q(ref_column), limit.clamp(1, 500));
    let mut qr = conn.exec_iter(sql, params).await?;
    let mut out = Vec::new();
    while let Some(row) = qr.next().await? {
        let cols = row.columns();
        let mut vals = row.unwrap().into_iter().zip(cols.iter());
        let (v, c) = vals.next().expect("one column selected at least");
        let value = cell_text(v, c, Some(256)).0.unwrap_or_default();
        let label = vals.next().and_then(|(v, c)| cell_text(v, c, Some(256)).0);
        out.push(LookupItem { value, label });
    }
    Ok(out)
}
