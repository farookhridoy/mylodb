use crate::{meta::TableMeta, q, Error, Result};
use myq_driver::mysql_async::{prelude::Queryable, Value};
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
}

#[derive(Debug, Serialize)]
pub struct Page {
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<Vec<Option<String>>>,
    /// (row, column) positions whose text was cut at `CELL_PREVIEW` bytes.
    pub long_cells: Vec<(usize, usize)>,
    pub has_more: bool,
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

pub(crate) fn select_sql(meta: &TableMeta, spec: &PageSpec) -> Result<(String, Vec<Value>, u32)> {
    let limit = if spec.limit == 0 { 500 } else { spec.limit.min(MAX_PAGE) };
    let (w, params) = build_where(meta, &spec.filters, spec.where_sql.as_deref())?;
    let where_clause = if w.is_empty() { String::new() } else { format!(" WHERE {w}") };
    // One extra row tells us whether a next page exists without a COUNT(*).
    let sql = format!(
        "SELECT * FROM {}{}{} LIMIT {} OFFSET {}",
        meta.qualified(),
        where_clause,
        order_by(meta, &spec.sort)?,
        limit + 1,
        spec.offset
    );
    Ok((sql, params, limit))
}

pub async fn fetch_page(conn: &mut Conn, meta: &TableMeta, spec: &PageSpec) -> Result<Page> {
    let started = Instant::now();
    let (sql, params, limit) = select_sql(meta, spec)?;
    let mut qr = conn.exec_iter(sql.as_str(), params).await?;
    let columns: Vec<ColumnInfo> = qr
        .columns()
        .map(|c| c.iter().map(|c| ColumnInfo { name: c.name_str().to_string(), type_name: format!("{:?}", c.column_type()), binary: myq_driver::is_binary(c) }).collect())
        .unwrap_or_default();
    let mut rows = Vec::new();
    let mut long_cells = Vec::new();
    let mut has_more = false;
    while let Some(row) = qr.next().await? {
        if rows.len() == limit as usize {
            has_more = true;
            break;
        }
        let (cells, cut) = row_cells(row, Some(CELL_PREVIEW));
        long_cells.extend(cut.into_iter().map(|c| (rows.len(), c)));
        rows.push(cells);
    }
    drop(qr); // discard the unread remainder so the connection is reusable
    Ok(Page { columns, rows, long_cells, has_more, elapsed_ms: started.elapsed().as_millis() as u64, sql })
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
        let (sql, _, limit) = select_sql(&m, &spec).unwrap();
        assert_eq!(limit, MAX_PAGE);
        assert_eq!(sql, format!("SELECT * FROM `d`.`t` ORDER BY `a` DESC, `we``ird` LIMIT {} OFFSET 40", MAX_PAGE + 1));
        assert!(select_sql(&m, &PageSpec { sort: vec![Sort { column: "x".into(), desc: false }], ..Default::default() }).is_err());
    }
}
