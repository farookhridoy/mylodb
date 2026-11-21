use crate::{meta::TableMeta, q, Error, Result};
use myq_driver::mysql_async::{prelude::Queryable, Row, Value};
use myq_driver::{row_cells, ColumnInfo, Conn};
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Largest page the server will return in one call.
pub const MAX_PAGE: u32 = 5_000;
/// Bytes of a cell sent to the grid; longer cells are flagged and fetched on demand with `fetch_cell`.
pub const CELL_PREVIEW: usize = 2_048;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Contains,
    NotContains,
    Eq,
    Ne,
    StartsWith,
    EndsWith,
    Lt,
    Le,
    Gt,
    Ge,
    Between,
    In,
    Regexp,
    IsNull,
    NotNull,
    IsEmpty,
}

/// One column-wise filter. `values` holds 0 (NULL ops), 1, 2 (between) or many (in) entries.
#[derive(Debug, Clone, Deserialize)]
pub struct ColumnFilter {
    pub column: String,
    pub op: Op,
    #[serde(default)]
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Sort {
    pub column: String,
    #[serde(default)]
    pub desc: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PageSpec {
    #[serde(default)]
    pub filters: Vec<ColumnFilter>,
    /// Free-form SQL condition typed by the user (like the WHERE box in an IDE grid).
    #[serde(default)]
    pub where_sql: Option<String>,
    #[serde(default)]
    pub sort: Vec<Sort>,
    #[serde(default)]
    pub limit: u32,
    #[serde(default)]
    pub offset: u64,
    /// Keyset paging: continue from a primary-key position instead of counting past `offset` rows. See [`Cursor`].
    #[serde(default)]
    pub cursor: Option<Cursor>,
}

/// A position in primary-key order. Cheaper than `OFFSET` on big tables, because the server seeks to the key instead of reading and
/// discarding every row before the page. Only used when the result is in primary-key order (no user sort) and the key columns are
/// integers, strings or dates; see [`Page::keyset`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cursor {
    /// The page after the row with these key values (one value per primary-key column, in key order).
    After(Vec<String>),
    /// The page before the row with these key values.
    Before(Vec<String>),
    /// The last page.
    End,
}

#[derive(Debug, Serialize)]
pub struct Page {
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<Vec<Option<String>>>,
    /// (row, column) positions whose text was cut at `CELL_PREVIEW` bytes.
    pub long_cells: Vec<(usize, usize)>,
    pub has_more: bool,
    /// Whether rows exist before this page.
    pub has_prev: bool,
    /// Whether keyset navigation works for this table and request. When false, page with `offset`.
    pub keyset: bool,
    /// Primary-key values of the first and last row of this page (the cursors for "previous" and "next"), when `keyset` is true.
    pub first_key: Option<Vec<String>>,
    pub last_key: Option<Vec<String>>,
    pub elapsed_ms: u64,
    /// The statement that produced this page, for "View queries".
    pub sql: String,
}

fn text(s: &str) -> Value {
    Value::Bytes(s.as_bytes().to_vec())
}

fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// Build `WHERE` content (without the keyword) and its parameters. Column names are checked against
/// the table's metadata, values are always bound, never interpolated.
pub(crate) fn build_where(meta: &TableMeta, filters: &[ColumnFilter], where_sql: Option<&str>) -> Result<(String, Vec<Value>)> {
    let mut parts = Vec::new();
    let mut params = Vec::new();
    for f in filters {
        meta.column(&f.column)?;
        let c = q(&f.column);
        let need = |n: usize| -> Result<()> {
            if f.values.len() == n { Ok(()) } else { Err(Error::Invalid(format!("filter on `{}` needs {n} value(s)", f.column))) }
        };
        let v = &f.values;
        let clause = match f.op {
            Op::IsNull => { need(0)?; format!("{c} IS NULL") }
            Op::NotNull => { need(0)?; format!("{c} IS NOT NULL") }
            Op::IsEmpty => { need(0)?; format!("{c} = ''") }
            Op::Between => {
                need(2)?;
                params.push(text(&v[0]));
                params.push(text(&v[1]));
                format!("{c} BETWEEN ? AND ?")
            }
            Op::In => {
                if v.is_empty() {
                    return Err(Error::Invalid(format!("filter on `{}` needs at least one value", f.column)));
                }
                params.extend(v.iter().map(|x| text(x)));
                format!("{c} IN ({})", vec!["?"; v.len()].join(","))
            }
            op => {
                need(1)?;
                let (sql, val) = match op {
                    Op::Eq => ("=", v[0].clone()),
                    Op::Ne => ("<>", v[0].clone()),
                    Op::Lt => ("<", v[0].clone()),
                    Op::Le => ("<=", v[0].clone()),
                    Op::Gt => (">", v[0].clone()),
                    Op::Ge => (">=", v[0].clone()),
                    Op::Contains => ("LIKE", format!("%{}%", like_escape(&v[0]))),
                    Op::NotContains => ("NOT LIKE", format!("%{}%", like_escape(&v[0]))),
                    Op::StartsWith => ("LIKE", format!("{}%", like_escape(&v[0]))),
                    Op::EndsWith => ("LIKE", format!("%{}", like_escape(&v[0]))),
                    Op::Regexp => ("REGEXP", v[0].clone()),
                    _ => unreachable!("handled above"),
                };
                params.push(text(&val));
                format!("{c} {sql} ?")
            }
        };
        parts.push(clause);
    }
    if let Some(w) = where_sql.map(str::trim).filter(|w| !w.is_empty()) {
        parts.push(format!("({w})"));
    }
    Ok((parts.join(" AND "), params))
}

fn order_by(meta: &TableMeta, sort: &[Sort]) -> Result<String> {
    if sort.is_empty() {
        return Ok(String::new());
    }
    let items: Result<Vec<String>> = sort
        .iter()
        .map(|s| {
            meta.column(&s.column)?;
            Ok(format!("{}{}", q(&s.column), if s.desc { " DESC" } else { "" }))
        })
        .collect();
    Ok(format!(" ORDER BY {}", items?.join(", ")))
}

/// Types whose values keyset paging can compare exactly: integers (bound as integers, never as text, which MySQL would compare as
/// floating point and so lose precision above 2^53), strings and temporal types.
fn keyset_kind(col: &crate::meta::ColumnMeta) -> Option<bool> {
    if col.binary {
        return None;
    }
    match col.data_type.to_ascii_lowercase().as_str() {
        "tinyint" | "smallint" | "mediumint" | "int" | "integer" | "bigint" => Some(true),
        "char" | "varchar" | "date" | "datetime" | "timestamp" => Some(false),
        _ => None,
    }
}

/// The primary-key columns when keyset paging is possible for the table (every key column has a usable type).
fn keyset_columns(meta: &TableMeta) -> Option<Vec<&crate::meta::ColumnMeta>> {
    if meta.is_view || meta.primary_key.is_empty() {
        return None;
    }
    meta.primary_key.iter().map(|n| meta.columns.iter().find(|c| &c.name == n).filter(|c| keyset_kind(c).is_some())).collect()
}

fn key_value(col: &crate::meta::ColumnMeta, v: &str) -> Result<Value> {
    if keyset_kind(col) == Some(true) {
        if col.column_type.to_ascii_lowercase().contains("unsigned") {
            v.parse::<u64>().map(Value::UInt).map_err(|_| Error::Invalid(format!("`{}` is not a valid value for key column `{}`", v, col.name)))
        } else {
            v.parse::<i64>().map(Value::Int).map_err(|_| Error::Invalid(format!("`{}` is not a valid value for key column `{}`", v, col.name)))
        }
    } else {
        Ok(text(v))
    }
}

pub(crate) struct Select {
    pub sql: String,
    pub params: Vec<Value>,
    pub limit: u32,
    /// The query reads the page backwards (`ORDER BY … DESC`); the rows must be reversed.
    pub reverse: bool,
}

pub(crate) fn select_sql(meta: &TableMeta, spec: &PageSpec) -> Result<Select> {
    let limit = if spec.limit == 0 { 500 } else { spec.limit.min(MAX_PAGE) };
    let (w, mut params) = build_where(meta, &spec.filters, spec.where_sql.as_deref())?;
    let mut conditions = if w.is_empty() { vec![] } else { vec![w] };
    let mut order = order_by(meta, &spec.sort)?;
    let mut offset = spec.offset;
    let mut reverse = false;
    if let Some(cursor) = &spec.cursor {
        let cols = keyset_columns(meta).filter(|_| spec.sort.is_empty()).ok_or_else(|| Error::Invalid("keyset paging is not available for this table or sort order".into()))?;
        let names: Vec<String> = cols.iter().map(|c| q(&c.name)).collect();
        let descending = !matches!(cursor, Cursor::After(_));
        order = format!(" ORDER BY {}", names.iter().map(|n| if descending { format!("{n} DESC") } else { n.clone() }).collect::<Vec<_>>().join(", "));
        reverse = descending;
        offset = 0;
        let (Cursor::After(key) | Cursor::Before(key)) = cursor else { return finish_select(meta, conditions, params, order, limit, offset, reverse) };
        if key.len() != cols.len() {
            return Err(Error::Invalid(format!("the cursor has {} key value(s) but the primary key has {} column(s)", key.len(), cols.len())));
        }
        for (c, v) in cols.iter().zip(key) {
            params.push(key_value(c, v)?);
        }
        let op = if matches!(cursor, Cursor::After(_)) { ">" } else { "<" };
        // a row constructor compares the key columns in order, which is exactly primary-key order
        conditions.push(format!("({}) {op} ({})", names.join(", "), vec!["?"; names.len()].join(", ")));
    }
    finish_select(meta, conditions, params, order, limit, offset, reverse)
}

fn finish_select(meta: &TableMeta, conditions: Vec<String>, params: Vec<Value>, order: String, limit: u32, offset: u64, reverse: bool) -> Result<Select> {
    let where_clause = if conditions.is_empty() { String::new() } else { format!(" WHERE {}", conditions.join(" AND ")) };
    // One extra row tells us whether a next page exists without a COUNT(*).
    let sql = format!("SELECT * FROM {}{}{} {}", meta.qualified(), where_clause, order, myq_core::Dialect::limit_offset(&myq_core::MySql, u64::from(limit) + 1, u64::from(offset)));
    Ok(Select { sql, params, limit, reverse })
}

pub async fn fetch_page(conn: &mut Conn, meta: &TableMeta, spec: &PageSpec) -> Result<Page> {
    let started = Instant::now();
    let Select { sql, params, limit, reverse } = select_sql(meta, spec)?;
    let mut qr = conn.exec_iter(sql.as_str(), params).await?;
    let columns: Vec<ColumnInfo> = qr
        .columns()
        .map(|c| c.iter().map(|c| ColumnInfo { name: c.name_str().to_string(), type_name: format!("{:?}", c.column_type()), binary: myq_driver::is_binary(c) }).collect())
        .unwrap_or_default();
    let mut raw: Vec<(Vec<Option<String>>, Vec<usize>)> = Vec::new();
    let mut extra = false;
    while let Some(row) = qr.next().await? {
        if raw.len() == limit as usize {
            extra = true;
            break;
        }
        raw.push(row_cells(row, Some(CELL_PREVIEW)));
    }
    drop(qr); // discard the unread remainder so the connection is reusable
    if reverse {
        raw.reverse();
    }
    let mut rows = Vec::with_capacity(raw.len());
    let mut long_cells = Vec::new();
    for (cells, cut) in raw {
        long_cells.extend(cut.into_iter().map(|c| (rows.len(), c)));
        rows.push(cells);
    }

    // keyset bookkeeping: the key of the first and last row, when the table supports it and no key cell was cut short
    let key_cols = keyset_columns(meta).filter(|_| spec.sort.is_empty());
    let key_of = |r: usize| -> Option<Vec<String>> {
        let cols = key_cols.as_ref()?;
        cols.iter()
            .map(|c| {
                let i = columns.iter().position(|x| x.name == c.name)?;
                if long_cells.contains(&(r, i)) { None } else { rows.get(r)?.get(i)?.clone() }
            })
            .collect()
    };
    let (first_key, last_key) = if rows.is_empty() { (None, None) } else { (key_of(0), key_of(rows.len() - 1)) };
    let (has_more, has_prev) = match &spec.cursor {
        None => (extra, spec.offset > 0),
        Some(Cursor::After(_)) => (extra, true),
        Some(Cursor::Before(_)) => (true, extra),
        Some(Cursor::End) => (false, extra),
    };
    Ok(Page { columns, rows, long_cells, has_more, has_prev, keyset: key_cols.is_some(), first_key, last_key, elapsed_ms: started.elapsed().as_millis() as u64, sql })
}

/// A row count without scanning the table: the table's statistics when nothing filters it, else the optimizer's estimate from `EXPLAIN`.
/// Instant on any size, and roughly right (MySQL's statistics can be off by several percent). None when the server has no estimate.
pub async fn estimate_count(conn: &mut Conn, meta: &TableMeta, filters: &[ColumnFilter], where_sql: Option<&str>) -> Result<Option<u64>> {
    let (w, params) = build_where(meta, filters, where_sql)?;
    if w.is_empty() {
        let n: Option<Option<u64>> = conn
            .exec_first("SELECT TABLE_ROWS FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?", (meta.database.clone(), meta.table.clone()))
            .await?;
        return Ok(n.flatten());
    }
    let select = format!("SELECT * FROM {} WHERE {w}", meta.qualified());
    // MySQL 8.0.32+ may default to a text tree without a `rows` column; ask for the table form, and fall back for servers that do not know the keyword
    for prefix in ["EXPLAIN FORMAT=TRADITIONAL ", "EXPLAIN "] {
        if let Ok(rows) = conn.exec::<Row, _, _>(format!("{prefix}{select}"), params.clone()).await {
            return Ok(rows.first().and_then(|r| r.get_opt::<Option<u64>, _>("rows")).and_then(|v| v.ok()).flatten());
        }
    }
    Ok(None)
}

/// Exact row count for the same filters. Separate from `fetch_page` because it can be slow on large tables.
pub async fn count_rows(conn: &mut Conn, meta: &TableMeta, filters: &[ColumnFilter], where_sql: Option<&str>) -> Result<u64> {
    let (w, params) = build_where(meta, filters, where_sql)?;
    let sql = format!("SELECT COUNT(*) FROM {}{}", meta.qualified(), if w.is_empty() { String::new() } else { format!(" WHERE {w}") });
    let n: Option<u64> = conn.exec_first(sql, params).await?;
    Ok(n.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta::ColumnMeta;

    fn meta() -> TableMeta {
        let col = |n: &str| ColumnMeta {
            name: n.into(), data_type: "varchar".into(), column_type: "varchar(10)".into(), nullable: true, key: String::new(),
            default: None, extra: String::new(), comment: String::new(), binary: false, generated: false, auto_increment: false,
        };
        TableMeta {
            database: "d".into(), table: "t".into(), is_view: false, columns: vec![col("a"), col("we`ird")],
            primary_key: vec![], row_key: vec![], foreign_keys: vec![], read_only_reason: None,
        }
    }

    fn f(column: &str, op: Op, values: &[&str]) -> ColumnFilter {
        ColumnFilter { column: column.into(), op, values: values.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    fn every_operator_binds_values_and_quotes_identifiers() {
        let m = meta();
        let (sql, p) = build_where(&m, &[f("a", Op::Contains, &["50%_off"]), f("we`ird", Op::In, &["1", "2"]), f("a", Op::IsNull, &[]), f("a", Op::Between, &["1", "9"])], Some(" b > 1 ")).unwrap();
        assert_eq!(sql, "`a` LIKE ? AND `we``ird` IN (?,?) AND `a` IS NULL AND `a` BETWEEN ? AND ? AND (b > 1)");
        assert_eq!(p.len(), 5);
        assert_eq!(p[0], Value::Bytes(b"%50\\%\\_off%".to_vec()), "LIKE wildcards in user input are escaped");
    }

    #[test]
    fn starts_ends_regexp_and_negations() {
        let m = meta();
        let (sql, p) = build_where(&m, &[f("a", Op::StartsWith, &["x"]), f("a", Op::EndsWith, &["y"]), f("a", Op::NotContains, &["z"]), f("a", Op::Regexp, &["^r"]), f("a", Op::Ne, &["n"])], None).unwrap();
        assert_eq!(sql, "`a` LIKE ? AND `a` LIKE ? AND `a` NOT LIKE ? AND `a` REGEXP ? AND `a` <> ?");
        assert_eq!(p[0], Value::Bytes(b"x%".to_vec()));
        assert_eq!(p[1], Value::Bytes(b"%y".to_vec()));
    }

    #[test]
    fn rejects_unknown_columns_and_wrong_arity() {
        let m = meta();
        assert!(matches!(build_where(&m, &[f("nope", Op::Eq, &["1"])], None), Err(Error::UnknownColumn(_))));
        assert!(matches!(build_where(&m, &[f("a", Op::Eq, &[])], None), Err(Error::Invalid(_))));
        assert!(matches!(build_where(&m, &[f("a", Op::Between, &["1"])], None), Err(Error::Invalid(_))));
        assert!(matches!(build_where(&m, &[f("a", Op::In, &[])], None), Err(Error::Invalid(_))));
        assert!(matches!(build_where(&m, &[f("a", Op::IsNull, &["x"])], None), Err(Error::Invalid(_))));
    }

    #[test]
    fn select_sql_sorts_pages_and_caps_limit() {
        let m = meta();
        let spec = PageSpec { sort: vec![Sort { column: "a".into(), desc: true }, Sort { column: "we`ird".into(), desc: false }], limit: 99_999, offset: 40, ..Default::default() };
        let Select { sql, limit, .. } = select_sql(&m, &spec).unwrap();
        assert_eq!(limit, MAX_PAGE);
        assert_eq!(sql, format!("SELECT * FROM `d`.`t` ORDER BY `a` DESC, `we``ird` LIMIT {} OFFSET 40", MAX_PAGE + 1));
        assert!(select_sql(&m, &PageSpec { sort: vec![Sort { column: "x".into(), desc: false }], ..Default::default() }).is_err());
    }

    fn keyed(pk: &[(&str, &str, &str)]) -> TableMeta {
        let col = |n: &str, t: &str, ct: &str| ColumnMeta {
            name: n.into(), data_type: t.into(), column_type: ct.into(), nullable: false, key: "PRI".into(),
            default: None, extra: String::new(), comment: String::new(), binary: false, generated: false, auto_increment: false,
        };
        TableMeta {
            database: "d".into(), table: "t".into(), is_view: false,
            columns: pk.iter().map(|(n, t, ct)| col(n, t, ct)).chain(std::iter::once(col("v", "varchar", "varchar(10)"))).collect(),
            primary_key: pk.iter().map(|(n, _, _)| n.to_string()).collect(), row_key: pk.iter().map(|(n, _, _)| n.to_string()).collect(), foreign_keys: vec![], read_only_reason: None,
        }
    }

    #[test]
    fn keyset_pages_seek_by_primary_key_and_bind_integers_exactly() {
        let m = keyed(&[("id", "bigint", "bigint unsigned")]);
        let after = PageSpec { limit: 500, cursor: Some(Cursor::After(vec!["9007199254740993".into()])), offset: 77, ..Default::default() };
        let s = select_sql(&m, &after).unwrap();
        assert_eq!(s.sql, "SELECT * FROM `d`.`t` WHERE (`id`) > (?) ORDER BY `id` LIMIT 501 OFFSET 0", "the offset is ignored under a cursor");
        assert_eq!(s.params, vec![Value::UInt(9_007_199_254_740_993)], "an integer key is bound as an integer: as text MySQL would compare it as a double");
        assert!(!s.reverse);
        let before = select_sql(&m, &PageSpec { cursor: Some(Cursor::Before(vec!["10".into()])), ..Default::default() }).unwrap();
        assert_eq!(before.sql, "SELECT * FROM `d`.`t` WHERE (`id`) < (?) ORDER BY `id` DESC LIMIT 501 OFFSET 0");
        assert!(before.reverse);
        let end = select_sql(&m, &PageSpec { cursor: Some(Cursor::End), ..Default::default() }).unwrap();
        assert_eq!(end.sql, "SELECT * FROM `d`.`t` ORDER BY `id` DESC LIMIT 501 OFFSET 0");
        assert!(end.reverse && end.params.is_empty());
        // filters stay in front of the key condition
        let filtered = select_sql(&keyed(&[("id", "int", "int")]), &PageSpec { filters: vec![f("v", Op::Eq, &["x"])], cursor: Some(Cursor::After(vec!["-5".into()])), ..Default::default() }).unwrap();
        assert_eq!(filtered.sql, "SELECT * FROM `d`.`t` WHERE `v` = ? AND (`id`) > (?) ORDER BY `id` LIMIT 501 OFFSET 0");
        assert_eq!(filtered.params[1], Value::Int(-5));
    }

    #[test]
    fn composite_keys_compare_as_a_row() {
        let m = keyed(&[("region", "varchar", "varchar(8)"), ("id", "int", "int")]);
        let s = select_sql(&m, &PageSpec { cursor: Some(Cursor::After(vec!["eu".into(), "7".into()])), ..Default::default() }).unwrap();
        assert_eq!(s.sql, "SELECT * FROM `d`.`t` WHERE (`region`, `id`) > (?, ?) ORDER BY `region`, `id` LIMIT 501 OFFSET 0");
        assert_eq!(s.params, vec![Value::Bytes(b"eu".to_vec()), Value::Int(7)]);
        let back = select_sql(&m, &PageSpec { cursor: Some(Cursor::Before(vec!["eu".into(), "7".into()])), ..Default::default() }).unwrap();
        assert!(back.sql.ends_with("ORDER BY `region` DESC, `id` DESC LIMIT 501 OFFSET 0"));
    }

    #[test]
    fn a_cursor_is_refused_when_it_cannot_be_right() {
        let m = keyed(&[("id", "int", "int")]);
        let sorted = PageSpec { sort: vec![Sort { column: "v".into(), desc: false }], cursor: Some(Cursor::End), ..Default::default() };
        assert!(select_sql(&m, &sorted).is_err(), "a user sort is not key order");
        assert!(select_sql(&m, &PageSpec { cursor: Some(Cursor::After(vec![])), ..Default::default() }).is_err(), "wrong number of key values");
        assert!(select_sql(&m, &PageSpec { cursor: Some(Cursor::After(vec!["abc".into()])), ..Default::default() }).is_err(), "not an integer");
        let decimal = keyed(&[("id", "decimal", "decimal(10,2)")]);
        assert!(select_sql(&decimal, &PageSpec { cursor: Some(Cursor::End), ..Default::default() }).is_err(), "decimal keys are not compared exactly as text");
        let mut no_pk = keyed(&[("id", "int", "int")]);
        no_pk.primary_key.clear();
        assert!(select_sql(&no_pk, &PageSpec { cursor: Some(Cursor::End), ..Default::default() }).is_err());
        let mut view = keyed(&[("id", "int", "int")]);
        view.is_view = true;
        assert!(select_sql(&view, &PageSpec { cursor: Some(Cursor::End), ..Default::default() }).is_err());
        // without a cursor nothing changes for any of them
        assert!(select_sql(&decimal, &PageSpec::default()).is_ok());
    }
}
