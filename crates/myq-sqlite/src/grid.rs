//! The data grid's reads for a SQLite table: metadata, a filtered and sorted page, and an exact count.
//! It answers with the same `myq_data` types the MySQL path does, so the UI cannot tell the engines apart.

use crate::backend::{cell, open, table_columns, table_pk};
use crate::MAIN;
use myq_core::{ColumnInfo, Dialect, Sqlite};
use myq_data::{ColumnFilter, ColumnMeta, ForeignKey, Op, Page, PageSpec, TableMeta, CELL_PREVIEW, MAX_PAGE};
use rusqlite::{types::Value, Connection};
use std::path::Path;
use std::time::Instant;

type Result<T> = std::result::Result<T, String>;

fn q(s: &str) -> String {
    Sqlite.quote_ident(s)
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Metadata of one table or view. `read_only_reason` is set for a view, a table without a primary key, and every table of a file
/// opened read-only (`writable` false).
pub fn table_meta(path: &Path, table: &str, writable: bool) -> Result<TableMeta> {
    let c = open(path, false).map_err(err)?;
    let kind: Option<String> = c.query_row("SELECT type FROM sqlite_master WHERE name = ?1 AND type IN ('table','view')", [table], |r| r.get(0)).ok();
    let Some(kind) = kind else { return Err(format!("table `{table}` not found")) };
    let details = table_columns(&c, table).map_err(err)?;
    let mut st = c.prepare(&format!("PRAGMA table_xinfo({})", Sqlite.quote_string(table))).map_err(err)?;
    let extra: Vec<(Option<String>, i64)> = st.query_map([], |r| Ok((r.get::<_, Option<String>>(4)?, r.get::<_, i64>(6)?))).map_err(err)?.collect::<rusqlite::Result<_>>().map_err(err)?;
    let columns: Vec<ColumnMeta> = details
        .into_iter()
        .zip(extra)
        .map(|(d, (default, hidden))| ColumnMeta {
            data_type: d.column_type.to_lowercase(),
            binary: d.column_type.to_lowercase().contains("blob"),
            name: d.name,
            column_type: d.column_type,
            nullable: d.nullable,
            key: d.key,
            default,
            extra: d.extra,
            comment: String::new(),
            generated: hidden >= 2,
            auto_increment: false,
        })
        .collect();
    let mut pk = table_pk(&c, table).map_err(err)?;
    pk.sort();
    let primary_key: Vec<String> = pk.into_iter().map(|p| p.1).collect();
    let pk_empty = primary_key.is_empty();
    Ok(TableMeta {
        database: MAIN.into(),
        table: table.into(),
        is_view: kind == "view",
        columns,
        row_key: primary_key.clone(),
        primary_key,
        foreign_keys: foreign_keys(&c, table).map_err(err)?,
        read_only_reason: if !writable {
            Some("this SQLite file is open read-only".into())
        } else if kind == "view" {
            Some("views are read-only".into())
        } else if pk_empty {
            Some("the table has no primary key".into())
        } else {
            None
        },
    })
}

fn foreign_keys(c: &Connection, table: &str) -> rusqlite::Result<Vec<ForeignKey>> {
    let mut st = c.prepare(&format!("PRAGMA foreign_key_list({})", Sqlite.quote_string(table)))?;
    let rows: Vec<(i64, String, String, Option<String>)> = st.query_map([], |r| Ok((r.get(0)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.collect::<rusqlite::Result<_>>()?;
    let mut out: Vec<ForeignKey> = Vec::new();
    for (id, ref_table, from, to) in rows {
        let to = to.unwrap_or_default();
        match out.iter_mut().find(|f| f.name == format!("fk{id}")) {
            Some(f) => {
                f.columns.push(from);
                f.ref_columns.push(to);
            }
            None => out.push(ForeignKey { name: format!("fk{id}"), columns: vec![from], ref_schema: MAIN.into(), ref_table, ref_columns: vec![to] }),
        }
    }
    Ok(out)
}

fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// `WHERE` content (without the keyword) and its bound values. Column names are checked against the table, values are always bound.
fn build_where(meta: &TableMeta, filters: &[ColumnFilter], where_sql: Option<&str>) -> Result<(String, Vec<Value>)> {
    let mut parts = Vec::new();
    let mut params: Vec<Value> = Vec::new();
    let text = |s: &str| Value::Text(s.to_string());
    for f in filters {
        if !meta.columns.iter().any(|c| c.name == f.column) {
            return Err(format!("unknown column `{}`", f.column));
        }
        let c = q(&f.column);
        let v = &f.values;
        let need = |n: usize| if v.len() == n { Ok(()) } else { Err(format!("filter on `{}` needs {n} value(s)", f.column)) };
        let clause = match f.op {
            Op::IsNull => { need(0)?; format!("{c} IS NULL") }
            Op::NotNull => { need(0)?; format!("{c} IS NOT NULL") }
            Op::IsEmpty => { need(0)?; format!("{c} = ''") }
            Op::Between => { need(2)?; params.extend([text(&v[0]), text(&v[1])]); format!("{c} BETWEEN ? AND ?") }
            Op::In => {
                if v.is_empty() {
                    return Err(format!("filter on `{}` needs at least one value", f.column));
                }
                params.extend(v.iter().map(|x| text(x)));
                format!("{c} IN ({})", vec!["?"; v.len()].join(","))
            }
            Op::Regexp => return Err("SQLite has no REGEXP unless an extension adds it; use contains or a pattern with LIKE".into()),
            op => {
                need(1)?;
                let (sql, val, like) = match op {
                    Op::Eq => ("=", v[0].clone(), false),
                    Op::Ne => ("<>", v[0].clone(), false),
                    Op::Lt => ("<", v[0].clone(), false),
                    Op::Le => ("<=", v[0].clone(), false),
                    Op::Gt => (">", v[0].clone(), false),
                    Op::Ge => (">=", v[0].clone(), false),
                    Op::Contains => ("LIKE", format!("%{}%", like_escape(&v[0])), true),
                    Op::NotContains => ("NOT LIKE", format!("%{}%", like_escape(&v[0])), true),
                    Op::StartsWith => ("LIKE", format!("{}%", like_escape(&v[0])), true),
                    Op::EndsWith => ("LIKE", format!("%{}", like_escape(&v[0])), true),
                    _ => unreachable!("handled above"),
                };
                params.push(text(&val));
                if like { format!("{c} {sql} ? ESCAPE '\\'") } else { format!("{c} {sql} ?") }
            }
        };
        parts.push(clause);
    }
    if let Some(w) = where_sql.map(str::trim).filter(|w| !w.is_empty()) {
        parts.push(format!("({w})"));
    }
    Ok((parts.join(" AND "), params))
}

/// One page of rows. Paging is by `OFFSET` (no keyset cursor): `keyset` is reported as false so the grid does the same.
pub fn fetch_page(path: &Path, meta: &TableMeta, spec: &PageSpec) -> Result<Page> {
    let started = Instant::now();
    let c = open(path, false).map_err(err)?;
    let (cond, params) = build_where(meta, &spec.filters, spec.where_sql.as_deref())?;
    let where_clause = if cond.is_empty() { String::new() } else { format!(" WHERE {cond}") };
    let mut order = Vec::new();
    for s in &spec.sort {
        if !meta.columns.iter().any(|c| c.name == s.column) {
            return Err(format!("unknown column `{}`", s.column));
        }
        order.push(format!("{}{}", q(&s.column), if s.desc { " DESC" } else { "" }));
    }
    let order = if order.is_empty() { String::new() } else { format!(" ORDER BY {}", order.join(", ")) };
    let limit = if spec.limit == 0 { 500 } else { spec.limit.min(MAX_PAGE) };
    let sql = format!("SELECT * FROM {}{where_clause}{order} {}", q(&meta.table), Sqlite.limit_offset(u64::from(limit) + 1, spec.offset));
    let mut st = c.prepare(&sql).map_err(err)?;
    let columns: Vec<ColumnInfo> = st
        .columns()
        .iter()
        .map(|col| {
            let ty = col.decl_type().unwrap_or("").to_string();
            ColumnInfo { name: col.name().to_string(), binary: ty.to_lowercase().contains("blob"), type_name: ty }
        })
        .collect();
    let n = columns.len();
    let mut q_rows = st.query(rusqlite::params_from_iter(params.iter())).map_err(err)?;
    let (mut rows, mut long_cells, mut extra) = (Vec::new(), Vec::new(), false);
    while let Some(row) = q_rows.next().map_err(err)? {
        if rows.len() == limit as usize {
            extra = true;
            break;
        }
        let mut cells = Vec::with_capacity(n);
        for i in 0..n {
            let mut text = cell(row.get_ref(i).map_err(err)?);
            if let Some(t) = &mut text {
                if t.len() > CELL_PREVIEW {
                    let mut cut = CELL_PREVIEW;
                    while !t.is_char_boundary(cut) {
                        cut -= 1;
                    }
                    t.truncate(cut);
                    long_cells.push((rows.len(), i));
                }
            }
            cells.push(text);
        }
        rows.push(cells);
    }
    Ok(Page { columns, rows, long_cells, has_more: extra, has_prev: spec.offset > 0, keyset: false, first_key: None, last_key: None, elapsed_ms: started.elapsed().as_millis() as u64, sql })
}

/// Exact count (SQLite keeps no statistics; counting a file is cheap enough).
pub fn count_rows(path: &Path, meta: &TableMeta, filters: &[ColumnFilter], where_sql: Option<&str>) -> Result<u64> {
    let c = open(path, false).map_err(err)?;
    let (cond, params) = build_where(meta, filters, where_sql)?;
    let where_clause = if cond.is_empty() { String::new() } else { format!(" WHERE {cond}") };
    let n: i64 = c.query_row(&format!("SELECT COUNT(*) FROM {}{where_clause}", q(&meta.table)), rusqlite::params_from_iter(params.iter()), |r| r.get(0)).map_err(err)?;
    Ok(n as u64)
}
