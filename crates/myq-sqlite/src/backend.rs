use crate::MAIN;
use myq_core::{
    Backend, BackendError, BackendResult, Capabilities, ColumnDetail, ColumnInfo, DatabaseRow, Dialect, IndexDetail, Kind, Node, ResultSet, Sqlite, StatementResult,
    TableRow,
};
use rusqlite::{types::ValueRef, Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Rows kept per result set; the rest are counted but dropped (same limit as the MySQL path).
const MAX_ROWS: usize = 10_000;

fn failed(e: impl std::fmt::Display) -> BackendError {
    BackendError(e.to_string())
}

/// A SQLite file. Every call opens its own connection (cheap for a file), so there is no pool and no shared state to lose.
/// A read-only backend opens the file by the read-only flag and `PRAGMA query_only`; a writable one opens it read-write
/// (an existing file only: opening never creates one).
#[derive(Clone, Debug)]
pub struct SqliteBackend {
    path: PathBuf,
    writable: bool,
}

/// One connection to the file. `writable` opens it read-write with foreign keys enforced and a 5 s wait on a locked file.
pub(crate) fn open(path: &Path, writable: bool) -> rusqlite::Result<Connection> {
    if writable {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "foreign_keys", true)?;
        Ok(conn)
    } else {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        conn.pragma_update(None, "query_only", true)?;
        Ok(conn)
    }
}

impl SqliteBackend {
    /// Opens the file once, read-only, to check that it is a SQLite database.
    pub fn open(path: impl Into<PathBuf>) -> Result<SqliteBackend, String> {
        SqliteBackend::open_mode(path, false)
    }

    /// Like `open`, but later calls may change the file. The file must already exist and be writable by this user.
    pub fn open_writable(path: impl Into<PathBuf>) -> Result<SqliteBackend, String> {
        SqliteBackend::open_mode(path, true)
    }

    fn open_mode(path: impl Into<PathBuf>, writable: bool) -> Result<SqliteBackend, String> {
        let path = path.into();
        let conn = open(&path, writable).map_err(|e| format!("cannot open {}{}: {e}", path.display(), if writable { " for writing (is the file read-only? use the Read-only option)" } else { "" }))?;
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0)).map_err(|e| format!("{} is not a SQLite database: {e}", path.display()))?;
        Ok(SqliteBackend { path, writable })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether this backend may change the file.
    pub fn writable(&self) -> bool {
        self.writable
    }

    async fn blocking<T: Send + 'static>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T> + Send + 'static) -> BackendResult<T> {
        let (path, writable) = (self.path.clone(), self.writable);
        tokio::task::spawn_blocking(move || {
            let conn = open(&path, writable)?;
            f(&conn)
        })
        .await
        .map_err(failed)?
        .map_err(failed)
    }
}

fn is_internal(name: &str) -> bool {
    name.starts_with("sqlite_")
}

/// Text for a cell; BLOBs as hex. `None` is SQL NULL.
pub(crate) fn cell(v: ValueRef<'_>) -> Option<String> {
    match v {
        ValueRef::Null => None,
        ValueRef::Integer(i) => Some(i.to_string()),
        ValueRef::Real(f) => Some(f.to_string()),
        ValueRef::Text(t) => Some(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Some(b.iter().map(|x| format!("{x:02X}")).collect()),
    }
}

pub(crate) fn table_columns(conn: &Connection, table: &str) -> rusqlite::Result<Vec<ColumnDetail>> {
    // table_xinfo also reports hidden and generated columns
    let mut st = conn.prepare(&format!("PRAGMA table_xinfo({})", Sqlite.quote_string(table)))?;
    let rows = st.query_map([], |r| {
        let (name, ty, notnull, pk): (String, String, i64, i64) = (r.get(1)?, r.get(2)?, r.get(3)?, r.get(5)?);
        let hidden: i64 = r.get(6)?;
        Ok(ColumnDetail {
            name,
            column_type: ty,
            nullable: notnull == 0 && pk == 0,
            key: if pk > 0 { "PRI".into() } else { String::new() },
            extra: match hidden {
                2 => "virtual generated".into(),
                3 => "stored generated".into(),
                1 => "hidden".into(),
                _ => String::new(),
            },
        })
    })?;
    rows.collect()
}

impl Backend for SqliteBackend {
    fn dialect(&self) -> &dyn Dialect {
        &Sqlite
    }

    async fn capabilities(&self) -> BackendResult<Capabilities> {
        Ok(Capabilities::sqlite(self.writable))
    }

    async fn index(&self, include_system: bool) -> BackendResult<Vec<Node>> {
        self.blocking(move |c| {
            let mut out = vec![Node { database: MAIN.into(), kind: Kind::Database, name: MAIN.into(), parent: None }];
            let mut st = c.prepare("SELECT type, name, tbl_name FROM sqlite_master WHERE type IN ('table','view','trigger') ORDER BY name")?;
            let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?;
            for row in rows {
                let (ty, name, tbl) = row?;
                if !include_system && is_internal(&name) {
                    continue;
                }
                let (kind, parent) = match ty.as_str() {
                    "view" => (Kind::View, None),
                    "trigger" => (Kind::Trigger, Some(tbl)),
                    _ => (Kind::Table, None),
                };
                out.push(Node { database: MAIN.into(), kind, name, parent });
            }
            Ok(out)
        })
        .await
    }

    async fn column_nodes(&self, database: &str) -> BackendResult<Vec<Node>> {
        if database != MAIN {
            return Ok(vec![]);
        }
        self.blocking(|c| {
            let mut tables = c.prepare("SELECT name FROM sqlite_master WHERE type IN ('table','view') ORDER BY name")?;
            let names: Vec<String> = tables.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            let mut out = Vec::new();
            for t in names.into_iter().filter(|t| !is_internal(t)) {
                for col in table_columns(c, &t)? {
                    out.push(Node { database: MAIN.into(), kind: Kind::Column, name: col.name, parent: Some(t.clone()) });
                }
            }
            Ok(out)
        })
        .await
    }

    async fn databases(&self) -> BackendResult<Vec<DatabaseRow>> {
        Ok(vec![DatabaseRow { name: MAIN.into(), collation: "BINARY".into(), charset: "UTF-8".into(), system: false }])
    }

    async fn tables(&self, database: &str) -> BackendResult<Vec<TableRow>> {
        if database != MAIN {
            return Ok(vec![]);
        }
        self.blocking(|c| {
            let mut st = c.prepare("SELECT name, type FROM sqlite_master WHERE type IN ('table','view') ORDER BY name")?;
            let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            let mut out = Vec::new();
            for row in rows {
                let (name, ty) = row?;
                if is_internal(&name) {
                    continue;
                }
                out.push(TableRow { name, is_view: ty == "view", engine: None, rows: None, collation: None, bytes: None, overhead: None, comment: String::new() });
            }
            Ok(out)
        })
        .await
    }

    async fn columns(&self, database: &str, table: &str) -> BackendResult<Vec<ColumnDetail>> {
        if database != MAIN {
            return Ok(vec![]);
        }
        let table = table.to_string();
        self.blocking(move |c| table_columns(c, &table)).await
    }

    async fn indexes(&self, database: &str, table: &str) -> BackendResult<Vec<IndexDetail>> {
        if database != MAIN {
            return Ok(vec![]);
        }
        let table = table.to_string();
        self.blocking(move |c| {
            let mut out = Vec::new();
            // the primary key is reported as the index named PRIMARY (the MySQL convention the UI knows)
            let mut pk: Vec<(i64, String)> = table_pk(c, &table)?;
            pk.sort();
            if !pk.is_empty() {
                out.push(IndexDetail { name: "PRIMARY".into(), unique: true, kind: "BTREE".into(), columns: pk.into_iter().map(|p| p.1).collect() });
            }
            let mut list = c.prepare(&format!("PRAGMA index_list({})", Sqlite.quote_string(&table)))?;
            let idx: Vec<(String, bool, String)> =
                list.query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, i64>(2)? != 0, r.get::<_, String>(3)?)))?.collect::<rusqlite::Result<_>>()?;
            for (name, unique, origin) in idx {
                if origin == "pk" {
                    continue; // already listed as PRIMARY
                }
                let mut info = c.prepare(&format!("PRAGMA index_info({})", Sqlite.quote_string(&name)))?;
                let cols: Vec<String> = info.query_map([], |r| r.get::<_, Option<String>>(2))?.collect::<rusqlite::Result<Vec<_>>>()?.into_iter().map(|c| c.unwrap_or_else(|| "(expression)".into())).collect();
                out.push(IndexDetail { name, unique, kind: "BTREE".into(), columns: cols });
            }
            Ok(out)
        })
        .await
    }

    async fn run(&self, _database: Option<&str>, sql: &str) -> BackendResult<Vec<StatementResult>> {
        let sql = sql.to_string();
        self.blocking(move |c| Ok(run_script(c, &sql))).await
    }
}

pub(crate) fn table_pk(conn: &Connection, table: &str) -> rusqlite::Result<Vec<(i64, String)>> {
    let mut st = conn.prepare(&format!("PRAGMA table_info({})", Sqlite.quote_string(table)))?;
    let rows = st.query_map([], |r| Ok((r.get::<_, i64>(5)?, r.get::<_, String>(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?.into_iter().filter(|p| p.0 > 0).collect())
}

fn run_script(conn: &Connection, sql: &str) -> Vec<StatementResult> {
    let mut results = Vec::new();
    for stmt in myq_sql::split_statements(sql) {
        let text = stmt.text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        let started = Instant::now();
        let mut res = StatementResult { sql: text.clone(), ..Default::default() };
        if let Err(e) = run_one(conn, &text, &mut res) {
            res.error = Some(e.to_string());
        }
        res.elapsed_ms = started.elapsed().as_millis() as u64;
        results.push(res);
    }
    results
}

fn run_one(conn: &Connection, sql: &str, res: &mut StatementResult) -> rusqlite::Result<()> {
    let mut st = conn.prepare(sql)?;
    if st.column_count() == 0 {
        res.affected_rows = st.execute([])? as u64;
        return Ok(());
    }
    let columns: Vec<ColumnInfo> = st
        .columns()
        .iter()
        .map(|c| {
            let ty = c.decl_type().unwrap_or("").to_string();
            ColumnInfo { name: c.name().to_string(), binary: ty.to_lowercase().contains("blob"), type_name: ty }
        })
        .collect();
    let n = columns.len();
    let mut rows = st.query([])?;
    let (mut kept, mut total) = (Vec::new(), 0u64);
    while let Some(row) = rows.next()? {
        total += 1;
        if kept.len() < MAX_ROWS {
            kept.push((0..n).map(|i| row.get_ref(i).map(cell)).collect::<rusqlite::Result<Vec<_>>>()?);
        }
    }
    res.sets.push(ResultSet { columns, rows: kept, total_rows: total, truncated: total > MAX_ROWS as u64 });
    Ok(())
}
