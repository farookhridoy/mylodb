//! Export from a SQLite file: the rows of a table (with the grid's filters and sort) or the result of one SELECT, streamed into the same
//! writers the MySQL export uses (CSV, JSON, JSON Lines, SQL INSERT, XML, HTML, XLSX).

use crate::backend::{cell, open};
use crate::grid::build_where_for_export;
use myq_data::{Dest, ExportColumn, ExportOptions, ExportOutcome, Exporter, PageSpec};
use rusqlite::types::ValueRef;
use std::path::Path;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

const TICK_EVERY: u64 = 1_000;

fn numeric_type(decl: &str) -> bool {
    let d = decl.to_lowercase();
    ["int", "real", "floa", "doub", "num", "dec", "bool"].iter().any(|k| d.contains(k))
}

/// Run `sql` and write every row. `tick` is called every 1,000 rows with the count; returning false cancels (and is reported as an error).
fn stream(path: &Path, sql: &str, params: Vec<rusqlite::types::Value>, opts: &ExportOptions, dest: Dest<'_>, default_name: &str, tick: &mut dyn FnMut(u64) -> bool) -> Result<ExportOutcome, String> {
    let conn = open(path, false).map_err(err)?;
    let mut st = conn.prepare(sql).map_err(err)?;
    let decls: Vec<String> = st.columns().iter().map(|c| c.decl_type().unwrap_or("").to_string()).collect();
    let names: Vec<String> = st.columns().iter().map(|c| c.name().to_string()).collect();
    let n = names.len();
    let mut rows = st.query(rusqlite::params_from_iter(params.iter())).map_err(err)?;
    let mut exporter: Option<Exporter<'_>> = None;
    let mut dest = Some(dest);
    let mut count = 0u64;
    while let Some(row) = rows.next().map_err(err)? {
        let mut cells: Vec<Option<String>> = Vec::with_capacity(n);
        for i in 0..n {
            let v = row.get_ref(i).map_err(err)?;
            // a blob is written as 0x… so the SQL INSERT format can turn it into a binary literal
            cells.push(if matches!(v, ValueRef::Blob(_)) { cell(v).map(|h| format!("0x{h}")) } else { cell(v) });
        }
        if exporter.is_none() {
            // an expression has no declared type: the first row's values decide whether a column is a number
            let cols: Vec<ExportColumn> = (0..n)
                .map(|i| ExportColumn {
                    name: names[i].clone(),
                    numeric: if decls[i].is_empty() { matches!(row.get_ref(i), Ok(ValueRef::Integer(_) | ValueRef::Real(_))) } else { numeric_type(&decls[i]) },
                    binary: decls[i].to_lowercase().contains("blob") || matches!(row.get_ref(i), Ok(ValueRef::Blob(_))),
                })
                .collect();
            exporter = Some(Exporter::new(opts, dest.take().expect("destination used once"), default_name, &cols).map_err(err)?);
        }
        exporter.as_mut().expect("started above").row(&cells).map_err(err)?;
        count += 1;
        if count % TICK_EVERY == 0 && !tick(count) {
            return Err("cancelled".into());
        }
    }
    tick(count);
    // no rows: the file still gets its header
    let exporter = match exporter {
        Some(e) => e,
        None => {
            let cols: Vec<ExportColumn> = names.iter().zip(&decls).map(|(nm, d)| ExportColumn { name: nm.clone(), numeric: numeric_type(d), binary: d.to_lowercase().contains("blob") }).collect();
            Exporter::new(opts, dest.take().expect("destination unused"), default_name, &cols).map_err(err)?
        }
    };
    exporter.finish().map_err(err)
}

/// A consistent copy of the whole file, made by `VACUUM INTO` (safe while the file is in use). The target must not exist.
pub fn backup_to(path: &Path, target: &Path) -> Result<(), String> {
    if target.exists() {
        return Err(format!("{} already exists", target.display()));
    }
    // read-only on the source (VACUUM INTO only reads it), but without `query_only`, which would also block writing the copy
    let conn = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(err)?;
    conn.execute("VACUUM INTO ?1", [target.to_string_lossy().as_ref()]).map_err(err)?;
    Ok(())
}

/// Every row of a table that matches the grid's filters and sort.
pub fn export_table(path: &Path, meta: &myq_data::TableMeta, spec: &PageSpec, opts: &ExportOptions, dest: Dest<'_>, tick: &mut dyn FnMut(u64) -> bool) -> Result<ExportOutcome, String> {
    let (cond, params, order) = build_where_for_export(meta, spec)?;
    let sql = format!("SELECT * FROM \"{}\"{}{}", meta.table.replace('"', "\"\""), if cond.is_empty() { String::new() } else { format!(" WHERE {cond}") }, order);
    stream(path, &sql, params, opts, dest, &meta.table, tick)
}

/// The result of one read-only statement.
pub fn export_query(path: &Path, sql: &str, opts: &ExportOptions, dest: Dest<'_>, tick: &mut dyn FnMut(u64) -> bool) -> Result<ExportOutcome, String> {
    let stmts = myq_sql::split_statements(sql);
    let [stmt] = stmts.as_slice() else { return Err("export one statement at a time".into()) };
    if myq_sql::statement_kind(&stmt.text) != myq_sql::StatementKind::Read {
        return Err("only SELECT statements can be exported".into());
    }
    stream(path, stmt.text.trim().trim_end_matches(';'), vec![], opts, dest, "query", tick)
}
