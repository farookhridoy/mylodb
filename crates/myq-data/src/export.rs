use crate::{meta::TableMeta, query::{select_sql, PageSpec}, q, Result};
use myq_driver::mysql_async::{prelude::Queryable, Value};
use myq_driver::{cell_text, is_numeric, Conn};
use serde::Deserialize;
use std::io::Write;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Csv,
    Json,
    SqlInsert,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExportOptions {
    pub format: ExportFormat,
    /// Text written for SQL NULL in CSV (JSON and SQL always use null / NULL).
    #[serde(default)]
    pub null_text: String,
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

fn sql_literal(v: &Option<String>, numeric: bool, binary: bool) -> String {
    match v {
        None => "NULL".into(),
        Some(s) if numeric => s.clone(),
        Some(s) if binary && s.starts_with("0x") => format!("X'{}'", &s[2..]),
        Some(s) => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "''")),
    }
}

fn json_string(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

/// Stream every row matching the filters/sort of `spec` (paging is ignored) to `out`. Returns the row count.
pub async fn export_rows(conn: &mut Conn, meta: &TableMeta, spec: &PageSpec, opts: &ExportOptions, out: &mut impl Write) -> Result<u64> {
    let all = PageSpec { limit: u32::MAX, offset: 0, ..spec.clone() };
    let (sql, params, _) = select_sql(meta, &all)?;
    // `select_sql` appends LIMIT/OFFSET for paging; exports want everything, so drop that tail.
    let sql = sql.rsplit_once(" LIMIT ").map(|(head, _)| head.to_string()).unwrap_or(sql);
    let mut qr = conn.exec_iter(sql, params).await?;
    let columns: Vec<(String, bool, bool)> = qr
        .columns()
        .map(|c| c.iter().map(|c| (c.name_str().to_string(), is_numeric(c), myq_driver::is_binary(c))).collect())
        .unwrap_or_default();
    let names: Vec<String> = columns.iter().map(|c| c.0.clone()).collect();

    match opts.format {
        ExportFormat::Csv => writeln!(out, "{}", names.iter().map(|n| csv_field(n)).collect::<Vec<_>>().join(","))?,
        ExportFormat::Json => write!(out, "[")?,
        ExportFormat::SqlInsert => {}
    }
    let insert_head = format!("INSERT INTO {} ({}) VALUES\n", q(&meta.table), names.iter().map(|n| q(n)).collect::<Vec<_>>().join(", "));
    let mut rows = 0u64;
    while let Some(row) = qr.next().await? {
        let cols = row.columns();
        let cells: Vec<Option<String>> = row.unwrap().into_iter().zip(cols.iter()).map(|(v, c): (Value, _)| cell_text(v, c, None).0).collect();
        match opts.format {
            ExportFormat::Csv => {
                let line: Vec<String> = cells.iter().map(|c| c.as_deref().map_or_else(|| csv_field(&opts.null_text), csv_field)).collect();
                writeln!(out, "{}", line.join(","))?;
            }
            ExportFormat::Json => {
                write!(out, "{}\n  {{", if rows == 0 { "" } else { "," })?;
                let fields: Vec<String> = cells
                    .iter()
                    .zip(&columns)
                    .map(|(c, (name, numeric, _))| {
                        let v = match c {
                            None => "null".to_string(),
                            Some(s) if *numeric && s.parse::<f64>().is_ok() => s.clone(),
                            Some(s) => json_string(s),
                        };
                        format!("{}: {v}", json_string(name))
                    })
                    .collect();
                write!(out, "{}}}", fields.join(", "))?;
            }
            ExportFormat::SqlInsert => {
                // Batches of 100 rows per statement.
                if rows % 100 == 0 {
                    if rows > 0 {
                        writeln!(out, ";")?;
                    }
                    write!(out, "{insert_head}")?;
                } else {
                    writeln!(out, ",")?;
                }
                let vals: Vec<String> = cells.iter().zip(&columns).map(|(c, (_, n, b))| sql_literal(c, *n, *b)).collect();
                write!(out, "({})", vals.join(", "))?;
            }
        }
        rows += 1;
    }
    match opts.format {
        ExportFormat::Json => writeln!(out, "{}]", if rows == 0 { "" } else { "\n" })?,
        ExportFormat::SqlInsert if rows > 0 => writeln!(out, ";")?,
        _ => {}
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_quotes_only_when_needed() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\"\nnow"), "\"say \"\"hi\"\"\nnow\"");
    }

    #[test]
    fn sql_literals_escape_and_keep_numbers_and_binary() {
        assert_eq!(sql_literal(&None, false, false), "NULL");
        assert_eq!(sql_literal(&Some("12.50".into()), true, false), "12.50");
        assert_eq!(sql_literal(&Some("it's \\".into()), false, false), "'it''s \\\\'");
        assert_eq!(sql_literal(&Some("0xCAFE".into()), false, true), "X'CAFE'");
        assert_eq!(sql_literal(&Some("0xCAFE".into()), false, false), "'0xCAFE'", "only binary columns are hex literals");
    }
}
