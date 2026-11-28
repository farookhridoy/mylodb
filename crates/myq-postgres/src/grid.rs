//! The data grid's reads for a PostgreSQL table: metadata, a filtered and sorted page, counts, one full cell and foreign-key values.
//! It answers with the same `myq_data` types the MySQL path does, so the UI cannot tell the engines apart. Every read runs inside a
//! read-only transaction, so that a typed WHERE condition cannot change anything. Values travel as text.

use crate::backend::{explain, CancelToken, PgBackend};
use base64::{engine::general_purpose::STANDARD, Engine};
use myq_core::{ColumnInfo, Dialect, Postgres};
use myq_data::{Assign, CellData, CellValue, ColumnFilter, ColumnMeta, ForeignKey, LookupItem, Op, Page, PageSpec, TableMeta, CELL_PREVIEW, FETCH_CELL_MAX, MAX_PAGE};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio_postgres::types::ToSql;
use tokio_postgres::{Client, Row};

type Result<T> = std::result::Result<T, String>;

pub(crate) fn q(s: &str) -> String {
    Postgres.quote_ident(s)
}

fn err(e: tokio_postgres::Error) -> String {
    explain(e).0
}

/// A table's metadata, plus the type each column's values are cast to when they are compared or written.
pub struct PgMeta {
    pub meta: TableMeta,
    /// Column name to its type without a length, so that a long value is refused and never silently cut.
    pub(crate) casts: HashMap<String, String>,
}

impl PgMeta {
    pub(crate) fn qualified(&self) -> String {
        format!("{}.{}", q(&self.meta.database), q(&self.meta.table))
    }

    pub(crate) fn column(&self, name: &str) -> Result<&ColumnMeta> {
        self.meta.columns.iter().find(|c| c.name == name).ok_or_else(|| format!("column `{name}` does not exist"))
    }

    /// `CAST($n::text AS type)`: the parameter travels as text and the server turns it into the column's type.
    pub(crate) fn typed(&self, column: &str, n: usize) -> String {
        format!("CAST(${n}::text AS {})", self.casts[column])
    }
}

/// The names the grid knows, for the types it treats specially (numbers, long text, JSON, binary, dates).
fn data_type(name: &str) -> String {
    match name {
        "int2" => "smallint",
        "int4" => "int",
        "int8" => "bigint",
        "numeric" => "decimal",
        "float4" => "float",
        "float8" => "double",
        "bytea" => "blob",
        "bpchar" => "char",
        "timestamptz" => "timestamp",
        "timestamp" => "datetime",
        other => other,
    }
    .to_string()
}

pub async fn table_meta(b: &PgBackend, schema: &str, table: &str) -> Result<PgMeta> {
    let c = b.reading().await.map_err(|e| e.0)?;
    let out = read_meta(&c, schema, table, !b.read_only()).await;
    b.give_back(c).await;
    out
}

pub(crate) async fn read_meta(c: &Client, schema: &str, table: &str, writable: bool) -> Result<PgMeta> {
    let found = c
        .query_opt("SELECT c.oid, c.relkind::text FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r','p','f','v','m')", &[&schema, &table])
        .await
        .map_err(err)?
        .ok_or_else(|| format!("table `{schema}.{table}` not found"))?;
    let oid: u32 = found.get(0);
    let kind: String = found.get(1);
    let is_view = kind == "v" || kind == "m";
    let rows = c
        .query(
            "SELECT a.attname::text, t.typname::text, pg_catalog.format_type(a.atttypid, a.atttypmod), pg_catalog.format_type(a.atttypid, -1), a.attnotnull,
                pg_catalog.pg_get_expr(d.adbin, d.adrelid), a.attidentity::text, a.attgenerated::text, COALESCE(pg_catalog.col_description(a.attrelid, a.attnum), '')
             FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_type t ON t.oid = a.atttypid
             LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
             WHERE a.attrelid = $1 AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
            &[&oid],
        )
        .await
        .map_err(err)?;
    // unique indexes over plain columns that always apply: the primary key first, then the shortest
    let keys = c
        .query(
            "SELECT i.indisprimary, ARRAY(SELECT a.attname::text FROM generate_series(0, i.indnkeyatts::int - 1) k JOIN pg_catalog.pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = i.indkey[k] ORDER BY k),
                (SELECT bool_and(a.attnotnull) FROM generate_series(0, i.indnkeyatts::int - 1) k JOIN pg_catalog.pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = i.indkey[k]), i.indnkeyatts::int
             FROM pg_catalog.pg_index i WHERE i.indrelid = $1 AND i.indisunique AND i.indisvalid AND i.indimmediate AND i.indpred IS NULL AND i.indexprs IS NULL
             ORDER BY NOT i.indisprimary, i.indnkeyatts, i.indexrelid",
            &[&oid],
        )
        .await
        .map_err(err)?;
    let primary_key: Vec<String> = keys.iter().find(|r| r.get::<_, bool>(0)).map(|r| r.get(1)).unwrap_or_default();
    let row_key: Vec<String> = if primary_key.is_empty() {
        keys.iter().find(|r| r.get::<_, Option<bool>>(2) == Some(true)).map(|r| r.get(1)).unwrap_or_default()
    } else {
        primary_key.clone()
    };
    let mut casts = HashMap::new();
    let columns: Vec<ColumnMeta> = rows
        .iter()
        .map(|r| {
            let name: String = r.get(0);
            let ty: String = r.get(1);
            casts.insert(name.clone(), r.get::<_, String>(3));
            let default: Option<String> = r.get(5);
            let identity: String = r.get(6);
            let generated: String = r.get(7);
            let serial = default.as_deref().is_some_and(|d| d.starts_with("nextval("));
            let extra = if identity == "a" {
                "identity always"
            } else if identity == "d" {
                "identity"
            } else if !generated.is_empty() {
                "stored generated"
            } else if serial {
                "auto_increment"
            } else {
                ""
            };
            ColumnMeta {
                data_type: data_type(&ty),
                column_type: r.get(2),
                nullable: !r.get::<_, bool>(4),
                key: if primary_key.contains(&name) { "PRI".into() } else { String::new() },
                // a generated column's expression is not a default
                default: default.filter(|_| generated.is_empty()),
                extra: extra.into(),
                comment: r.get(8),
                binary: ty == "bytea",
                // PostgreSQL refuses values for both of these
                generated: !generated.is_empty() || identity == "a",
                auto_increment: serial || !identity.is_empty(),
                name,
            }
        })
        .collect();
    let fks = c
        .query(
            "SELECT k.conname::text, n.nspname::text, t.relname::text,
                ARRAY(SELECT a.attname::text FROM unnest(k.conkey) WITH ORDINALITY u(num, ord) JOIN pg_catalog.pg_attribute a ON a.attrelid = k.conrelid AND a.attnum = u.num ORDER BY u.ord),
                ARRAY(SELECT a.attname::text FROM unnest(k.confkey) WITH ORDINALITY u(num, ord) JOIN pg_catalog.pg_attribute a ON a.attrelid = k.confrelid AND a.attnum = u.num ORDER BY u.ord)
             FROM pg_catalog.pg_constraint k JOIN pg_catalog.pg_class t ON t.oid = k.confrelid JOIN pg_catalog.pg_namespace n ON n.oid = t.relnamespace
             WHERE k.contype = 'f' AND k.conrelid = $1 ORDER BY 1",
            &[&oid],
        )
        .await
        .map_err(err)?;
    let foreign_keys = fks.iter().map(|r| ForeignKey { name: r.get(0), ref_schema: r.get(1), ref_table: r.get(2), columns: r.get(3), ref_columns: r.get(4) }).collect();
    let read_only_reason = if !writable {
        Some("this connection is read-only".to_string())
    } else if is_view {
        Some("views are read-only".to_string())
    } else if row_key.is_empty() {
        Some("table has no primary key or non-null unique key, so rows cannot be identified safely".to_string())
    } else {
        None
    };
    Ok(PgMeta { meta: TableMeta { database: schema.into(), table: table.into(), is_view, columns, primary_key, row_key, foreign_keys, read_only_reason }, casts })
}

fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

pub(crate) fn params(values: &[String]) -> Vec<&(dyn ToSql + Sync)> {
    values.iter().map(|v| v as &(dyn ToSql + Sync)).collect()
}

/// `WHERE …` (or nothing) and its values. Column names are checked against the table, values are always bound.
pub(crate) fn where_clause(m: &PgMeta, filters: &[ColumnFilter], where_sql: Option<&str>) -> Result<(String, Vec<String>)> {
    let mut parts = Vec::new();
    let mut values: Vec<String> = Vec::new();
    for f in filters {
        m.column(&f.column)?;
        let c = q(&f.column);
        let v = &f.values;
        let need = |n: usize| if v.len() == n { Ok(()) } else { Err(format!("filter on `{}` needs {n} value(s)", f.column)) };
        let next = |values: &mut Vec<String>, value: String| {
            values.push(value);
            values.len()
        };
        let clause = match f.op {
            Op::IsNull => { need(0)?; format!("{c} IS NULL") }
            Op::NotNull => { need(0)?; format!("{c} IS NOT NULL") }
            Op::IsEmpty => { need(0)?; format!("{c}::text = ''") }
            Op::Between => {
                need(2)?;
                let a = next(&mut values, v[0].clone());
                let b = next(&mut values, v[1].clone());
                format!("{c} BETWEEN {} AND {}", m.typed(&f.column, a), m.typed(&f.column, b))
            }
            Op::In => {
                if v.is_empty() {
                    return Err(format!("filter on `{}` needs at least one value", f.column));
                }
                let list: Vec<String> = v.iter().map(|x| { let n = next(&mut values, x.clone()); m.typed(&f.column, n) }).collect();
                format!("{c} IN ({})", list.join(", "))
            }
            Op::Contains | Op::NotContains | Op::StartsWith | Op::EndsWith => {
                need(1)?;
                let x = like_escape(&v[0]);
                let pattern = match f.op { Op::StartsWith => format!("{x}%"), Op::EndsWith => format!("%{x}"), _ => format!("%{x}%") };
                let n = next(&mut values, pattern);
                // case does not matter, as with the usual MySQL collations
                format!("{c}::text {}ILIKE ${n}::text", if f.op == Op::NotContains { "NOT " } else { "" })
            }
            Op::Regexp => { need(1)?; let n = next(&mut values, v[0].clone()); format!("{c}::text ~* ${n}::text") }
            op => {
                need(1)?;
                let sign = match op { Op::Eq => "=", Op::Ne => "<>", Op::Lt => "<", Op::Le => "<=", Op::Gt => ">", _ => ">=" };
                let n = next(&mut values, v[0].clone());
                format!("{c} {sign} {}", m.typed(&f.column, n))
            }
        };
        parts.push(clause);
    }
    if let Some(w) = where_sql.map(str::trim).filter(|w| !w.is_empty()) {
        // the line break ends a trailing comment before the closing parenthesis
        parts.push(format!("({w}\n)"));
    }
    Ok((if parts.is_empty() { String::new() } else { format!(" WHERE {}", parts.join(" AND ")) }, values))
}

/// The user's sort; the key breaks ties, so that a row is on one page only (PostgreSQL has no order of its own).
pub(crate) fn order_clause(m: &PgMeta, spec: &PageSpec) -> Result<String> {
    let mut order = Vec::new();
    for s in &spec.sort {
        m.column(&s.column)?;
        // by the column's own value, not by the text the grid is sent
        order.push(format!("t.{}{}", q(&s.column), if s.desc { " DESC" } else { "" }));
    }
    for k in &m.meta.row_key {
        if !spec.sort.iter().any(|s| &s.column == k) {
            order.push(format!("t.{}", q(k)));
        }
    }
    Ok(if order.is_empty() { String::new() } else { format!(" ORDER BY {}", order.join(", ")) })
}

fn row_texts(row: &Row, n: usize) -> Vec<Option<String>> {
    (0..n).map(|i| row.get(i)).collect()
}

/// One page of rows. Paging is by `OFFSET` (no keyset cursor): `keyset` is reported as false so the grid does the same.
pub async fn fetch_page(b: &PgBackend, m: &PgMeta, spec: &PageSpec) -> Result<Page> {
    let started = Instant::now();
    let (cond, values) = where_clause(m, &spec.filters, spec.where_sql.as_deref())?;
    let order = order_clause(m, spec)?;
    let limit = if spec.limit == 0 { 500 } else { spec.limit.min(MAX_PAGE) };
    // long values are cut on the server, one character past the preview, so that the cut is noticed
    let list: Vec<String> = m
        .meta
        .columns
        .iter()
        .map(|c| {
            let name = q(&c.name);
            if c.binary {
                format!("upper(encode(substring(t.{name} from 1 for {}), 'hex')) AS {name}", CELL_PREVIEW / 2 + 1)
            } else {
                format!("left(t.{name}::text, {}) AS {name}", CELL_PREVIEW + 1)
            }
        })
        .collect();
    let sql = format!("SELECT {} FROM {} AS t{cond}{order} {}", list.join(", "), m.qualified(), Postgres.limit_offset(u64::from(limit) + 1, spec.offset));
    let c = b.reading().await.map_err(|e| e.0)?;
    let found = c.query(&sql, &params(&values)).await.map_err(err);
    b.give_back(c).await;
    let found = found?;
    let n = m.meta.columns.len();
    let has_more = found.len() > limit as usize;
    let mut long_cells = Vec::new();
    let rows = found
        .iter()
        .take(limit as usize)
        .enumerate()
        .map(|(r, row)| {
            let mut cells = row_texts(row, n);
            for (i, cell) in cells.iter_mut().enumerate() {
                if let Some(t) = cell {
                    if t.len() > CELL_PREVIEW {
                        let mut cut = CELL_PREVIEW;
                        while !t.is_char_boundary(cut) {
                            cut -= 1;
                        }
                        t.truncate(cut);
                        long_cells.push((r, i));
                    }
                }
            }
            cells
        })
        .collect();
    let columns = m.meta.columns.iter().map(|c| ColumnInfo { name: c.name.clone(), type_name: c.column_type.clone(), binary: c.binary }).collect();
    Ok(Page { columns, rows, long_cells, has_more, has_prev: spec.offset > 0, keyset: false, first_key: None, last_key: None, elapsed_ms: started.elapsed().as_millis() as u64, sql })
}

/// Exact count. `token` lets a long one be cancelled.
pub async fn count_rows(b: &PgBackend, m: &PgMeta, filters: &[ColumnFilter], where_sql: Option<&str>, token: Option<Arc<CancelToken>>) -> Result<u64> {
    let (cond, values) = where_clause(m, filters, where_sql)?;
    let c = b.reading().await.map_err(|e| e.0)?;
    if let Some(t) = &token {
        t.watch(&c, b.config());
    }
    let n = c.query_one(&format!("SELECT count(*) FROM {} AS t{cond}", m.qualified()), &params(&values)).await.map_err(err);
    if let Some(t) = &token {
        t.release();
    }
    b.give_back(c).await;
    Ok(n?.get::<_, i64>(0) as u64)
}

/// The planner's row estimate for a whole table; none when filtered, for a view, or for a table never analysed.
pub async fn estimate_count(b: &PgBackend, m: &PgMeta, filters: &[ColumnFilter], where_sql: Option<&str>) -> Result<Option<u64>> {
    if !filters.is_empty() || where_sql.is_some_and(|w| !w.trim().is_empty()) {
        return Ok(None);
    }
    let c = b.reading().await.map_err(|e| e.0)?;
    let row = c.query_one("SELECT reltuples::float8 FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r','m')", &[&m.meta.database, &m.meta.table]).await;
    b.give_back(c).await;
    Ok(row.ok().map(|r| r.get::<_, f64>(0)).filter(|n| *n >= 0.0).map(|n| n as u64))
}

/// Load one full cell value, identified by the row key.
pub async fn fetch_cell(b: &PgBackend, m: &PgMeta, key: &[Assign], column: &str) -> Result<CellData> {
    let col = m.column(column)?;
    if m.meta.row_key.is_empty() || key.is_empty() {
        return Err("cannot locate the row without a key".into());
    }
    let mut conds = Vec::new();
    let mut values = Vec::new();
    for k in key {
        m.column(&k.column)?;
        let CellValue::Text(t) = &k.value else { return Err("key values must be text".into()) };
        values.push(t.clone());
        conds.push(format!("{} = {}", q(&k.column), m.typed(&k.column, values.len())));
    }
    let name = q(column);
    // at most the transfer limit leaves the server; the full length says whether that was all of it
    let select = if col.binary {
        format!("octet_length({name})::int8, substring({name} from 1 for {FETCH_CELL_MAX})")
    } else {
        format!("octet_length({name}::text)::int8, convert_to(left({name}::text, {FETCH_CELL_MAX}), 'UTF8')")
    };
    let c = b.reading().await.map_err(|e| e.0)?;
    let row = c.query_opt(&format!("SELECT {select} FROM {} WHERE {} LIMIT 1", m.qualified(), conds.join(" AND ")), &params(&values)).await.map_err(err);
    b.give_back(c).await;
    let row = row?.ok_or("row not found")?;
    let Some(bytes) = row.get::<_, Option<Vec<u8>>>(1) else { return Ok(CellData { is_null: true, len: 0, truncated: false, base64: None }) };
    let len = row.get::<_, Option<i64>>(0).unwrap_or(0) as usize;
    let cut = &bytes[..bytes.len().min(FETCH_CELL_MAX)];
    Ok(CellData { is_null: false, len, truncated: len > cut.len(), base64: Some(STANDARD.encode(cut)) })
}

/// Valid values of a foreign key's target column, optionally narrowed by `search`.
pub async fn lookup(b: &PgBackend, m: &PgMeta, ref_column: &str, search: &str, limit: u32) -> Result<Vec<LookupItem>> {
    m.column(ref_column)?;
    let label = m.meta.columns.iter().find(|c| c.name != ref_column && matches!(c.data_type.as_str(), "varchar" | "char" | "text")).map(|c| c.name.clone());
    let key = q(ref_column);
    let mut select = format!("left(t.{key}::text, 256)");
    if let Some(l) = &label {
        select.push_str(&format!(", left(t.{}::text, 256)", q(l)));
    }
    let mut sql = format!("SELECT {select} FROM {} AS t", m.qualified());
    let mut values = Vec::new();
    if !search.is_empty() {
        values.push(format!("%{}%", like_escape(search)));
        let mut conds = vec![format!("t.{key}::text ILIKE $1::text")];
        if let Some(l) = &label {
            conds.push(format!("t.{}::text ILIKE $1::text", q(l)));
        }
        sql.push_str(&format!(" WHERE {}", conds.join(" OR ")));
    }
    sql.push_str(&format!(" ORDER BY t.{key} LIMIT {}", limit.clamp(1, 500)));
    let c = b.reading().await.map_err(|e| e.0)?;
    let rows = c.query(&sql, &params(&values)).await.map_err(err);
    b.give_back(c).await;
    Ok(rows?.iter().map(|r| LookupItem { value: r.get::<_, Option<String>>(0).unwrap_or_default(), label: label.as_ref().and_then(|_| r.get(1)) }).collect())
}
