//! Database backup to SQL: structure and data of tables, plus views, routines, triggers and events.
//!
//! The dump is written natively (no `mysqldump` needed): `SHOW CREATE …` for definitions and a streamed
//! `SELECT` per table for rows. Data is read inside one consistent snapshot, so the dump is a single
//! point in time even while the database is in use (InnoDB). Output is one `.sql` file, one file per
//! table, or either of those inside a `.zip`.

use crate::{check_cancel, q, Error, Progress, ProgressFn, Result};
use myq_driver::mysql_async::{prelude::Queryable, Column, Value};
use myq_driver::{is_binary, is_numeric, Conn};
use myq_driver::mysql_async::consts::ColumnType;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

#[derive(Debug, Clone, Deserialize)]
pub struct DbSelection {
    pub name: String,
    /// Only these tables and views (everything when `None`).
    #[serde(default)]
    pub tables: Option<Vec<String>>,
}

fn yes() -> bool {
    true
}
fn rows_default() -> u32 {
    100
}

#[derive(Debug, Clone, Deserialize)]
pub struct BackupOptions {
    pub databases: Vec<DbSelection>,
    #[serde(default = "yes")]
    pub structure: bool,
    #[serde(default = "yes")]
    pub data: bool,
    #[serde(default = "yes")]
    pub drop_if_exists: bool,
    /// Emit `CREATE DATABASE IF NOT EXISTS` and `USE` (so the dump restores into the same database name).
    #[serde(default = "yes")]
    pub create_database: bool,
    #[serde(default = "yes")]
    pub views: bool,
    #[serde(default = "yes")]
    pub routines: bool,
    #[serde(default = "yes")]
    pub triggers: bool,
    #[serde(default = "yes")]
    pub events: bool,
    /// Rows per INSERT statement.
    #[serde(default = "rows_default")]
    pub rows_per_insert: u32,
    /// Read all data in one consistent snapshot.
    #[serde(default = "yes")]
    pub snapshot: bool,
    /// Add `FOREIGN_KEY_CHECKS=0` around the dump so tables can be restored in any order.
    #[serde(default = "yes")]
    pub disable_fk_checks: bool,
    /// Remove `DEFINER=…` so objects restore under the restoring user (the definer may not exist elsewhere).
    #[serde(default = "yes")]
    pub strip_definer: bool,
    #[serde(default)]
    pub file_per_table: bool,
    /// Put the output in a `.zip`.
    #[serde(default)]
    pub compress: bool,
}

#[derive(Debug, Clone)]
pub struct BackupTarget {
    pub dir: PathBuf,
    /// File (or folder) name before the timestamp.
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct BackupResult {
    /// The file or folder that was created.
    pub path: PathBuf,
    pub files: u64,
    pub databases: u64,
    pub tables: u64,
    pub rows: u64,
    /// Bytes of SQL written (before compression).
    pub bytes: u64,
    pub warnings: Vec<String>,
    pub elapsed_ms: u64,
}

// ---------- output ----------

trait Out: Send {
    /// Start a logical file (`rel` is its path inside a zip or folder).
    fn begin(&mut self, rel: &str) -> std::io::Result<()>;
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()>;
    fn end(&mut self) -> std::io::Result<()>;
    /// Finish and return the path to report.
    fn finish(self: Box<Self>) -> std::io::Result<PathBuf>;
    /// Remove everything that was written.
    fn discard(self: Box<Self>);
}

struct SingleFile {
    path: PathBuf,
    w: Option<BufWriter<File>>,
}

impl Out for SingleFile {
    fn begin(&mut self, _: &str) -> std::io::Result<()> {
        if self.w.is_none() {
            self.w = Some(BufWriter::new(File::create(&self.path)?));
        }
        Ok(())
    }
    fn write(&mut self, b: &[u8]) -> std::io::Result<()> {
        self.w.as_mut().expect("begin first").write_all(b)
    }
    fn end(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn finish(mut self: Box<Self>) -> std::io::Result<PathBuf> {
        if let Some(w) = self.w.as_mut() {
            w.flush()?;
        }
        Ok(self.path.clone())
    }
    fn discard(self: Box<Self>) {
        drop(self.w);
        let _ = std::fs::remove_file(&self.path);
    }
}

struct FolderOut {
    root: PathBuf,
    cur: Option<BufWriter<File>>,
}

impl Out for FolderOut {
    fn begin(&mut self, rel: &str) -> std::io::Result<()> {
        let path = self.root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        self.cur = Some(BufWriter::new(File::create(path)?));
        Ok(())
    }
    fn write(&mut self, b: &[u8]) -> std::io::Result<()> {
        self.cur.as_mut().expect("begin first").write_all(b)
    }
    fn end(&mut self) -> std::io::Result<()> {
        if let Some(mut w) = self.cur.take() {
            w.flush()?;
        }
        Ok(())
    }
    fn finish(mut self: Box<Self>) -> std::io::Result<PathBuf> {
        self.end()?;
        Ok(self.root.clone())
    }
    fn discard(self: Box<Self>) {
        drop(self.cur);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct ZipOut {
    path: PathBuf,
    zip: zip::ZipWriter<File>,
    /// Single-file mode: every logical file lands in this one entry.
    single_entry: Option<String>,
    started: bool,
}

impl Out for ZipOut {
    fn begin(&mut self, rel: &str) -> std::io::Result<()> {
        let name = self.single_entry.clone().unwrap_or_else(|| rel.to_string());
        if self.single_entry.is_some() && self.started {
            return Ok(());
        }
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true);
        self.zip.start_file(name, opts).map_err(|e| std::io::Error::other(e.to_string()))?;
        self.started = true;
        Ok(())
    }
    fn write(&mut self, b: &[u8]) -> std::io::Result<()> {
        self.zip.write_all(b)
    }
    fn end(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn finish(self: Box<Self>) -> std::io::Result<PathBuf> {
        self.zip.finish().map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(self.path.clone())
    }
    fn discard(self: Box<Self>) {
        let path = self.path.clone();
        drop(self);
        let _ = std::fs::remove_file(path);
    }
}

struct Sink<'a> {
    out: Box<dyn Out>,
    bytes: u64,
    files: u64,
    progress: ProgressFn<'a>,
    cancel: &'a AtomicBool,
    per_file: bool,
    began: bool,
}

impl Sink<'_> {
    fn put(&mut self, s: &str) -> Result<()> {
        self.bytes += s.len() as u64;
        self.out.write(s.as_bytes())?;
        Ok(())
    }
}

// ---------- text helpers ----------

pub(crate) use myq_core::lit as quote_string;

/// Length of an account such as `` `user`@`host` ``, `'user'@'%'` or `user@localhost` at the start of `s`.
fn account_len(s: &str) -> usize {
    fn part(s: &str) -> usize {
        match s.chars().next() {
            Some(quote @ ('`' | '\'' | '"')) => {
                let mut i = 1;
                let b = s.as_bytes();
                while i < b.len() {
                    if b[i] == quote as u8 {
                        if b.get(i + 1) == Some(&(quote as u8)) {
                            i += 2;
                            continue;
                        }
                        return i + 1;
                    }
                    i += 1;
                }
                s.len()
            }
            _ => s.find(|c: char| c == '@' || c.is_whitespace()).unwrap_or(s.len()),
        }
    }
    let user = part(s);
    if s[user..].starts_with('@') {
        user + 1 + part(&s[user + 1..])
    } else {
        user
    }
}

/// Remove every `DEFINER=account ` from a CREATE statement.
pub fn strip_definer(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut rest = sql;
    while let Some(i) = rest.find("DEFINER=") {
        out.push_str(&rest[..i]);
        let after = &rest[i + "DEFINER=".len()..];
        let n = account_len(after);
        rest = after[n..].trim_start_matches(' ');
    }
    out.push_str(rest);
    out
}

/// Names that are safe as a file name on every platform.
/// Delete the oldest backups of `name` in `dir`, keeping the newest `keep`. Only entries this module could have written are touched:
/// `<name>-YYYYMMDD-HHMMSS.sql`, `.zip`, or a folder of that name. Returns what was removed.
pub fn prune_backups(dir: &Path, name: &str, keep: usize) -> std::io::Result<Vec<PathBuf>> {
    prune_files(dir, name, keep, &["sql", "zip"], true)
}

/// The same for any job that writes `<name>-YYYYMMDD-HHMMSS.<ext>` with one of `exts` (a scheduled export); `folders` also counts a
/// folder of that name (a backup with one file per table).
pub fn prune_files(dir: &Path, name: &str, keep: usize, exts: &[&str], folders: bool) -> std::io::Result<Vec<PathBuf>> {
    let prefix = format!("{}-", file_safe(name));
    let stamp = |rest: &str| -> bool {
        let base = exts.iter().find_map(|x| rest.strip_suffix(&format!(".{x}"))).unwrap_or(rest);
        let b = base.as_bytes();
        b.len() == 15 && b[8] == b'-' && b.iter().enumerate().all(|(i, c)| i == 8 || c.is_ascii_digit())
    };
    let mut found: Vec<(String, PathBuf)> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().into_owned();
            let rest = file.strip_prefix(&prefix)?;
            let ok = stamp(rest) && ((folders && e.path().is_dir()) || exts.iter().any(|x| rest.ends_with(&format!(".{x}"))));
            ok.then(|| (rest.to_string(), e.path()))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0)); // the stamp sorts oldest to newest, so reversed: newest first
    let mut removed = Vec::new();
    for (_, path) in found.into_iter().skip(keep) {
        if path.is_dir() { std::fs::remove_dir_all(&path)?; } else { std::fs::remove_file(&path)?; }
        removed.push(path);
    }
    Ok(removed)
}

fn file_safe(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ' ') { c } else { '_' }).collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() { "_".into() } else { s }
}

/// Views in an order where each comes after the views it reads from (`deps`: view -> views it uses).
fn order_views(views: &[String], deps: &BTreeMap<String, BTreeSet<String>>) -> Vec<String> {
    let mut done: Vec<String> = Vec::new();
    let mut remaining: Vec<String> = views.to_vec();
    remaining.sort();
    while !remaining.is_empty() {
        let ready = remaining.iter().position(|v| deps.get(v).is_none_or(|d| d.iter().all(|x| done.contains(x) || !views.contains(x))));
        // a cycle cannot exist in MySQL; if the data says otherwise, fall back to name order
        let i = ready.unwrap_or(0);
        done.push(remaining.remove(i));
    }
    done
}

/// How one cell appears in an INSERT. Works on the raw bytes the server sent, so nothing is reformatted.
fn literal(v: &Value, col: &Column) -> String {
    match v {
        Value::NULL => "NULL".into(),
        Value::Bytes(b) => {
            if col.column_type() == ColumnType::MYSQL_TYPE_BIT {
                return b.iter().fold(0u64, |acc, x| (acc << 8) | *x as u64).to_string();
            }
            if is_binary(col) {
                return format!("X'{}'", myq_driver::hex(b));
            }
            match std::str::from_utf8(b) {
                Ok(s) if is_numeric(col) && s.bytes().all(|c| c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.' | b'e' | b'E')) && !s.is_empty() => s.to_string(),
                Ok(s) => quote_string(s),
                // not valid UTF-8 (a text column in a legacy character set): keep the exact bytes
                Err(_) => format!("X'{}'", myq_driver::hex(b)),
            }
        }
        other => other.as_sql(true),
    }
}

// ---------- the dump ----------

struct Ctx<'a, 'b> {
    conn: &'a mut Conn,
    sink: &'a mut Sink<'b>,
    opts: &'a BackupOptions,
    header: String,
    footer: String,
    res: BackupResult,
    total_objects: u64,
    done_objects: u64,
}

fn session_header(version: &str, now: &str, dbs: &[String], fk: bool) -> (String, String) {
    let mut h = format!(
        "-- MyLoDB backup\n-- Server version: {version}\n-- Created: {now}\n-- Databases: {}\n\nSET NAMES utf8mb4;\nSET TIME_ZONE = '+00:00';\nSET SQL_MODE = 'NO_AUTO_VALUE_ON_ZERO';\n",
        dbs.join(", ")
    );
    let mut f = String::from("\n");
    if fk {
        h.push_str("SET FOREIGN_KEY_CHECKS = 0;\nSET UNIQUE_CHECKS = 0;\n");
        f.push_str("SET FOREIGN_KEY_CHECKS = 1;\nSET UNIQUE_CHECKS = 1;\n");
    }
    h.push('\n');
    (h, f)
}

impl Ctx<'_, '_> {
    fn tick(&mut self, message: String) -> Result<()> {
        check_cancel(self.sink.cancel)?;
        (self.sink.progress)(Progress { phase: "Backing up".into(), done: self.done_objects, total: Some(self.total_objects), message });
        Ok(())
    }

    /// Open a logical file. In file-per-table mode every file is self-contained (header, database, footer).
    fn begin(&mut self, rel: &str, db: &str, create_db: Option<&str>) -> Result<()> {
        let first_single = !self.sink.per_file && !self.sink.began;
        if self.sink.per_file || first_single {
            self.sink.out.begin(rel)?;
            self.sink.files += 1;
            self.sink.began = true;
            let h = self.header.clone();
            self.sink.put(&h)?;
        }
        if self.sink.per_file {
            self.use_db(db, create_db)?;
        }
        Ok(())
    }

    fn use_db(&mut self, db: &str, create_db: Option<&str>) -> Result<()> {
        if self.opts.create_database {
            if let Some(c) = create_db {
                self.sink.put(&format!("{c};\n"))?;
            }
            self.sink.put(&format!("USE {};\n\n", q(db)))?;
        }
        Ok(())
    }

    fn end(&mut self) -> Result<()> {
        if self.sink.per_file {
            let f = self.footer.clone();
            self.sink.put(&f)?;
            self.sink.out.end()?;
        }
        Ok(())
    }

    fn maybe_strip(&self, sql: String) -> String {
        if self.opts.strip_definer { strip_definer(&sql) } else { sql }
    }

    async fn dump_table(&mut self, db: &str, table: &str, engine: &str) -> Result<()> {
        self.tick(format!("{db}.{table}"))?;
        if self.opts.structure {
            let row: Option<(String, String)> = self.conn.query_first(format!("SHOW CREATE TABLE {}.{}", q(db), q(table))).await?;
            let (_, create) = row.ok_or_else(|| Error::Invalid(format!("table {db}.{table} disappeared")))?;
            self.sink.put(&format!("--\n-- Table structure for {}\n--\n\n", q(table)))?;
            if self.opts.drop_if_exists {
                self.sink.put(&format!("DROP TABLE IF EXISTS {};\n", q(table)))?;
            }
            self.sink.put(&format!("{create};\n\n"))?;
        }
        if self.opts.data {
            if self.opts.snapshot && !engine.eq_ignore_ascii_case("InnoDB") {
                self.res.warnings.push(format!("{db}.{table} uses the {engine} engine, so its rows are not part of the consistent snapshot"));
            }
            self.dump_rows(db, table).await?;
        }
        self.res.tables += 1;
        self.done_objects += 1;
        Ok(())
    }

    async fn dump_rows(&mut self, db: &str, table: &str) -> Result<()> {
        // generated columns cannot be inserted; invisible ones must be named explicitly
        let cols: Vec<(String, String, String)> = self
            .conn
            .exec("SELECT COLUMN_NAME, EXTRA, DATA_TYPE FROM information_schema.COLUMNS WHERE TABLE_SCHEMA=? AND TABLE_NAME=? ORDER BY ORDINAL_POSITION", (db, table))
            .await?;
        let cols: Vec<(String, bool)> = cols
            .into_iter()
            .filter(|(_, extra, _)| {
                let e = extra.to_uppercase();
                !(e.contains("GENERATED") && !e.contains("DEFAULT_GENERATED"))
            })
            .map(|(n, _, ty)| (n, ty.eq_ignore_ascii_case("float")))
            .collect();
        if cols.is_empty() {
            return Ok(());
        }
        let list = cols.iter().map(|(n, _)| q(n)).collect::<Vec<_>>().join(", ");
        // MySQL prints a FLOAT with ~6 significant digits, which would change the stored value. `x + 0e0` is the
        // exact DOUBLE value of the float, and inserting it into a FLOAT column rounds back to the same float.
        let select_list = cols.iter().map(|(n, is_float)| if *is_float { format!("({} + 0e0)", q(n)) } else { q(n) }).collect::<Vec<_>>().join(", ");
        let head = format!("INSERT INTO {} ({list}) VALUES\n", q(table));
        let per_stmt = self.opts.rows_per_insert.max(1) as usize;
        const MAX_STATEMENT_BYTES: usize = 1 << 20; // stays well under the default max_allowed_packet

        let mut qr = self.conn.query_iter(format!("SELECT {select_list} FROM {}.{}", q(db), q(table))).await?;
        let columns: Vec<Column> = qr.columns().map(|c| c.to_vec()).unwrap_or_default();
        let mut stmt = String::new();
        let mut n = 0usize;
        let mut table_rows = 0u64;
        let mut wrote_header = false;
        while let Some(row) = qr.next().await? {
            if !wrote_header {
                self.sink.put(&format!("--\n-- Data for {}\n--\n\n", q(table)))?;
                wrote_header = true;
            }
            let tuple: Vec<String> = row.unwrap().iter().zip(&columns).map(|(v, c)| literal(v, c)).collect();
            stmt.push_str(if n == 0 { &head } else { ",\n" });
            stmt.push('(');
            stmt.push_str(&tuple.join(","));
            stmt.push(')');
            n += 1;
            table_rows += 1;
            if n >= per_stmt || stmt.len() >= MAX_STATEMENT_BYTES {
                stmt.push_str(";\n");
                self.sink.put(&stmt)?;
                stmt.clear();
                n = 0;
                check_cancel(self.sink.cancel)?;
            }
        }
        if n > 0 {
            stmt.push_str(";\n");
            self.sink.put(&stmt)?;
        }
        if wrote_header {
            self.sink.put("\n")?;
        }
        self.res.rows += table_rows;
        Ok(())
    }

    async fn dump_views(&mut self, db: &str, views: &[String]) -> Result<()> {
        if views.is_empty() {
            return Ok(());
        }
        // order by dependency: a view must be created after the views it selects from
        let usage: std::result::Result<Vec<(String, String)>, _> = self
            .conn
            .exec("SELECT VIEW_NAME, TABLE_NAME FROM information_schema.VIEW_TABLE_USAGE WHERE VIEW_SCHEMA=? AND TABLE_SCHEMA=?", (db, db))
            .await;
        let mut deps: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        match usage {
            Ok(rows) => {
                for (v, t) in rows {
                    deps.entry(v).or_default().insert(t);
                }
            }
            Err(_) => {
                // MariaDB has no VIEW_TABLE_USAGE: read the other views' names out of each definition (MariaDB writes them back-quoted).
                let defs: Vec<(String, String)> = self
                    .conn
                    .exec("SELECT TABLE_NAME, VIEW_DEFINITION FROM information_schema.VIEWS WHERE TABLE_SCHEMA=?", (db,))
                    .await
                    .unwrap_or_default();
                for (view, definition) in defs {
                    for other in views.iter().filter(|o| **o != view) {
                        if definition.contains(&format!("`{}`", other.replace('`', "``"))) {
                            deps.entry(view.clone()).or_default().insert(other.clone());
                        }
                    }
                }
            }
        }
        self.sink.put("--\n-- Views\n--\n\n")?;
        for v in order_views(views, &deps) {
            self.tick(format!("view {db}.{v}"))?;
            // SHOW CREATE VIEW has four columns (view, definition, client charset, collation): read by position
            let row: Option<myq_driver::mysql_async::Row> = self.conn.query_first(format!("SHOW CREATE VIEW {}.{}", q(db), q(&v))).await?;
            let Some(create) = row.and_then(|r| r.get::<Option<String>, _>(1).flatten()) else {
                self.res.warnings.push(format!("the definition of view {db}.{v} could not be read and was skipped"));
                continue;
            };
            if self.opts.drop_if_exists {
                self.sink.put(&format!("DROP VIEW IF EXISTS {};\n", q(&v)))?;
            }
            let create = self.maybe_strip(create);
            self.sink.put(&format!("{create};\n\n"))?;
            self.done_objects += 1;
        }
        Ok(())
    }

    /// A CREATE statement that contains `;` needs its own delimiter, and was written under the sql_mode
    /// it was created with, so that mode is set around it.
    fn put_routine_like(&mut self, drop_sql: Option<String>, sql_mode: &str, create: &str) -> Result<()> {
        if let Some(d) = drop_sql {
            self.sink.put(&format!("{d};\n"))?;
        }
        self.sink.put(&format!("SET @mylodb_saved_mode = @@SESSION.sql_mode;\nSET SESSION sql_mode = {};\nDELIMITER ;;\n{create} ;;\nDELIMITER ;\nSET SESSION sql_mode = @mylodb_saved_mode;\n\n", quote_string(sql_mode)))
    }

    async fn dump_routines(&mut self, db: &str) -> Result<()> {
        let routines: Vec<(String, String)> = self.conn.exec("SELECT ROUTINE_NAME, ROUTINE_TYPE FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA=? ORDER BY ROUTINE_TYPE, ROUTINE_NAME", (db,)).await?;
        if routines.is_empty() {
            return Ok(());
        }
        self.sink.put("--\n-- Stored routines\n--\n\n")?;
        for (name, kind) in routines {
            self.tick(format!("{} {db}.{name}", kind.to_lowercase()))?;
            let row: Option<myq_driver::mysql_async::Row> = self.conn.query_first(format!("SHOW CREATE {kind} {}.{}", q(db), q(&name))).await?;
            let (mode, create) = match row.as_ref().map(|r| (r.get::<Option<String>, _>(1).flatten().unwrap_or_default(), r.get::<Option<String>, _>(2).flatten())) {
                Some((m, Some(c))) => (m, c),
                _ => {
                    self.res.warnings.push(format!("the definition of {kind} {db}.{name} could not be read (missing privilege?) and was skipped"));
                    continue;
                }
            };
            let create = self.maybe_strip(create);
            let drop = self.opts.drop_if_exists.then(|| format!("DROP {kind} IF EXISTS {}", q(&name)));
            self.put_routine_like(drop, &mode, &create)?;
            self.done_objects += 1;
        }
        Ok(())
    }

    async fn dump_triggers(&mut self, db: &str, tables: Option<&[String]>) -> Result<()> {
        let triggers: Vec<(String, String)> = self.conn.exec("SELECT TRIGGER_NAME, EVENT_OBJECT_TABLE FROM information_schema.TRIGGERS WHERE TRIGGER_SCHEMA=? ORDER BY TRIGGER_NAME", (db,)).await?;
        let triggers: Vec<String> = triggers.into_iter().filter(|(_, t)| tables.is_none_or(|ts| ts.contains(t))).map(|(n, _)| n).collect();
        if triggers.is_empty() {
            return Ok(());
        }
        // after the data, so loading rows does not fire them
        self.sink.put("--\n-- Triggers\n--\n\n")?;
        for name in triggers {
            self.tick(format!("trigger {db}.{name}"))?;
            let row: Option<myq_driver::mysql_async::Row> = self.conn.query_first(format!("SHOW CREATE TRIGGER {}.{}", q(db), q(&name))).await?;
            let Some((mode, Some(create))) = row.as_ref().map(|r| (r.get::<Option<String>, _>(1).flatten().unwrap_or_default(), r.get::<Option<String>, _>(2).flatten())) else {
                self.res.warnings.push(format!("the definition of trigger {db}.{name} could not be read and was skipped"));
                continue;
            };
            let create = self.maybe_strip(create);
            let drop = self.opts.drop_if_exists.then(|| format!("DROP TRIGGER IF EXISTS {}", q(&name)));
            self.put_routine_like(drop, &mode, &create)?;
            self.done_objects += 1;
        }
        Ok(())
    }

    async fn dump_events(&mut self, db: &str) -> Result<()> {
        let events: Vec<String> = self.conn.exec("SELECT EVENT_NAME FROM information_schema.EVENTS WHERE EVENT_SCHEMA=? ORDER BY EVENT_NAME", (db,)).await?;
        if events.is_empty() {
            return Ok(());
        }
        self.sink.put("--\n-- Events\n--\n\n")?;
        for name in events {
            self.tick(format!("event {db}.{name}"))?;
            let row: Option<myq_driver::mysql_async::Row> = self.conn.query_first(format!("SHOW CREATE EVENT {}.{}", q(db), q(&name))).await?;
            let Some((mode, Some(create))) = row.as_ref().map(|r| (r.get::<Option<String>, _>(1).flatten().unwrap_or_default(), r.get::<Option<String>, _>(3).flatten())) else {
                self.res.warnings.push(format!("the definition of event {db}.{name} could not be read and was skipped"));
                continue;
            };
            let create = self.maybe_strip(create);
            let drop = self.opts.drop_if_exists.then(|| format!("DROP EVENT IF EXISTS {}", q(&name)));
            self.put_routine_like(drop, &mode, &create)?;
            self.done_objects += 1;
        }
        Ok(())
    }
}

/// Create a backup. Returns what was written; on cancellation or error everything written is removed.
pub async fn backup(conn: &mut Conn, opts: &BackupOptions, target: &BackupTarget, progress: ProgressFn<'_>, cancel: &AtomicBool) -> Result<BackupResult> {
    let started = Instant::now();
    if opts.databases.is_empty() {
        return Err(Error::Invalid("choose at least one database".into()));
    }
    if !opts.structure && !opts.data {
        return Err(Error::Invalid("choose structure, data, or both".into()));
    }
    std::fs::create_dir_all(&target.dir)?;

    // a dump session that reads the same way every time
    conn.query_drop("SET SESSION time_zone = '+00:00'").await?;
    conn.query_drop("SET SESSION sql_mode = ''").await?;
    conn.query_drop("SET NAMES utf8mb4").await?;
    if opts.snapshot && opts.data {
        conn.query_drop("SET SESSION TRANSACTION ISOLATION LEVEL REPEATABLE READ").await?;
        conn.query_drop("START TRANSACTION WITH CONSISTENT SNAPSHOT").await?;
    }

    // per database: its tables (with engine) and views, in the selected scope
    struct Plan {
        name: String,
        tables: Vec<(String, String)>,
        views: Vec<String>,
        selected: Option<Vec<String>>,
        create_db: Option<String>,
    }
    let mut plans = Vec::new();
    for sel in &opts.databases {
        let exists: Option<String> = conn.exec_first("SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME=?", (&sel.name,)).await?;
        if exists.is_none() {
            return Err(Error::Invalid(format!("database `{}` does not exist", sel.name)));
        }
        let objs: Vec<(String, String, Option<String>)> =
            conn.exec("SELECT TABLE_NAME, TABLE_TYPE, ENGINE FROM information_schema.TABLES WHERE TABLE_SCHEMA=? ORDER BY TABLE_NAME", (&sel.name,)).await?;
        let wanted = |n: &str| sel.tables.as_ref().is_none_or(|t| t.iter().any(|x| x == n));
        let tables: Vec<(String, String)> = objs.iter().filter(|(n, t, _)| t == "BASE TABLE" && wanted(n)).map(|(n, _, e)| (n.clone(), e.clone().unwrap_or_default())).collect();
        let views: Vec<String> = if opts.views { objs.iter().filter(|(n, t, _)| t.contains("VIEW") && wanted(n)).map(|(n, _, _)| n.clone()).collect() } else { vec![] };
        let create_db = if opts.create_database {
            let row: Option<(String, String)> = conn.query_first(format!("SHOW CREATE DATABASE {}", q(&sel.name))).await?;
            row.map(|(_, c)| c.replacen("CREATE DATABASE", "CREATE DATABASE IF NOT EXISTS", 1))
        } else {
            None
        };
        plans.push(Plan { name: sel.name.clone(), tables, views, selected: sel.tables.clone(), create_db });
    }

    let version: Option<String> = conn.query_first("SELECT VERSION()").await?;
    let (stamp, now): (String, String) = conn.query_first("SELECT DATE_FORMAT(NOW(), '%Y%m%d-%H%M%S'), DATE_FORMAT(NOW(), '%Y-%m-%d %H:%i:%s')").await?.unwrap_or_default();
    let stem = format!("{}-{stamp}", file_safe(&target.name));
    let multi_db = plans.len() > 1;

    let out: Box<dyn Out> = match (opts.file_per_table, opts.compress) {
        (false, false) => Box::new(SingleFile { path: target.dir.join(format!("{stem}.sql")), w: None }),
        (false, true) => {
            let path = target.dir.join(format!("{stem}.zip"));
            Box::new(ZipOut { zip: zip::ZipWriter::new(File::create(&path)?), path, single_entry: Some(format!("{stem}.sql")), started: false })
        }
        (true, false) => Box::new(FolderOut { root: target.dir.join(&stem), cur: None }),
        (true, true) => {
            let path = target.dir.join(format!("{stem}.zip"));
            Box::new(ZipOut { zip: zip::ZipWriter::new(File::create(&path)?), path, single_entry: None, started: false })
        }
    };
    let (header, footer) = session_header(version.as_deref().unwrap_or("?"), &now, &plans.iter().map(|p| p.name.clone()).collect::<Vec<_>>(), opts.disable_fk_checks);
    let mut sink = Sink { out, bytes: 0, files: 0, progress, cancel, per_file: opts.file_per_table, began: false };
    let total_objects = plans.iter().map(|p| (p.tables.len() + p.views.len()) as u64).sum::<u64>().max(1);
    let mut ctx = Ctx {
        conn,
        sink: &mut sink,
        opts,
        header,
        footer,
        res: BackupResult { path: PathBuf::new(), files: 0, databases: plans.len() as u64, tables: 0, rows: 0, bytes: 0, warnings: vec![], elapsed_ms: 0 },
        total_objects,
        done_objects: 0,
    };

    let run: Result<()> = async {
        for plan in &plans {
            let db = plan.name.as_str();
            let cdb = plan.create_db.as_deref();
            let dir = if multi_db || opts.file_per_table { format!("{}/", file_safe(db)) } else { String::new() };
            if !opts.file_per_table {
                ctx.begin(&format!("{stem}.sql"), db, cdb)?;
                ctx.use_db(db, cdb)?;
            }
            let mut used: BTreeSet<String> = BTreeSet::new();
            for (table, engine) in &plan.tables {
                if opts.file_per_table {
                    let mut fname = file_safe(table);
                    while !used.insert(fname.to_lowercase()) {
                        fname.push('_'); // two table names that differ only in case or punctuation
                    }
                    ctx.begin(&format!("{dir}10_{fname}.sql"), db, cdb)?;
                }
                ctx.dump_table(db, table, engine).await?;
                ctx.end()?;
            }
            if opts.structure {
                if opts.views && !plan.views.is_empty() {
                    ctx.begin(&format!("{dir}80_views.sql"), db, cdb)?;
                    ctx.dump_views(db, &plan.views).await?;
                    ctx.end()?;
                }
                if opts.routines {
                    ctx.begin(&format!("{dir}85_routines.sql"), db, cdb)?;
                    ctx.dump_routines(db).await?;
                    ctx.end()?;
                }
                if opts.triggers {
                    ctx.begin(&format!("{dir}90_triggers.sql"), db, cdb)?;
                    ctx.dump_triggers(db, plan.selected.as_deref()).await?;
                    ctx.end()?;
                }
                if opts.events {
                    ctx.begin(&format!("{dir}95_events.sql"), db, cdb)?;
                    ctx.dump_events(db).await?;
                    ctx.end()?;
                }
            }
        }
        if !opts.file_per_table {
            let f = ctx.footer.clone();
            ctx.sink.put(&f)?;
        }
        Ok(())
    }
    .await;

    let mut res = std::mem::replace(&mut ctx.res, BackupResult { path: PathBuf::new(), files: 0, databases: 0, tables: 0, rows: 0, bytes: 0, warnings: vec![], elapsed_ms: 0 });
    let _ = ctx.conn.query_drop("COMMIT").await;
    drop(ctx);
    match run {
        Ok(()) => {
            res.bytes = sink.bytes;
            res.files = sink.files;
            res.path = sink.out.finish()?;
            res.elapsed_ms = started.elapsed().as_millis() as u64;
            Ok(res)
        }
        Err(e) => {
            sink.out.discard();
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pruning_keeps_the_newest_and_touches_only_this_jobs_backups() {
        let dir = std::env::temp_dir().join(format!("mylodb_prune_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("shop-20260103-030000")).unwrap(); // a folder backup
        for f in ["shop-20260101-030000.sql", "shop-20260102-030000.zip", "shop-20260104-030000.sql", "shop-20260105-030000.sql", "shop-notes.sql", "shop-20260101-030000.txt", "other-20260101-030000.sql", "shop.sql", "readme.md"] {
            std::fs::write(dir.join(f), "x").unwrap();
        }
        let removed = prune_backups(&dir, "shop", 2).unwrap();
        let mut gone: Vec<String> = removed.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        gone.sort();
        assert_eq!(gone, vec!["shop-20260101-030000.sql", "shop-20260102-030000.zip", "shop-20260103-030000"], "the three oldest go");
        // the two newest stay, and everything that is not one of this job's backups is left alone
        for keep in ["shop-20260104-030000.sql", "shop-20260105-030000.sql", "shop-notes.sql", "shop-20260101-030000.txt", "other-20260101-030000.sql", "shop.sql", "readme.md"] {
            assert!(dir.join(keep).exists(), "{keep} must stay");
        }
        assert!(prune_backups(&dir, "shop", 5).unwrap().is_empty(), "nothing to remove when there are fewer than the limit");
        // exports: only the chosen extensions, and never a folder
        for f in ["sales-20260101-010000.csv", "sales-20260102-010000.csv", "sales-20260103-010000.csv", "sales-20260101-010000.sql"] {
            std::fs::write(dir.join(f), "x").unwrap();
        }
        std::fs::create_dir_all(dir.join("sales-20260102-010000")).unwrap();
        let gone = prune_files(&dir, "sales", 1, &["csv"], false).unwrap();
        assert_eq!(gone.len(), 2, "the two older csv files go");
        assert!(dir.join("sales-20260103-010000.csv").exists() && dir.join("sales-20260101-010000.sql").exists() && dir.join("sales-20260102-010000").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn strings_are_quoted_so_they_survive_a_restore() {
        assert_eq!(quote_string("plain"), "'plain'");
        assert_eq!(quote_string("it's \\ \0 \u{1a} \n"), "'it''s \\\\ \\0 \\Z \n'");
        assert_eq!(quote_string("日本😀"), "'日本😀'");
    }

    #[test]
    fn definers_are_removed_in_every_form() {
        assert_eq!(strip_definer("CREATE DEFINER=`root`@`localhost` PROCEDURE p() BEGIN END"), "CREATE PROCEDURE p() BEGIN END");
        assert_eq!(strip_definer("CREATE ALGORITHM=UNDEFINED DEFINER=`a b`@`%` SQL SECURITY DEFINER VIEW v AS SELECT 1"), "CREATE ALGORITHM=UNDEFINED SQL SECURITY DEFINER VIEW v AS SELECT 1");
        assert_eq!(strip_definer("CREATE DEFINER='x'@'h' TRIGGER t"), "CREATE TRIGGER t");
        assert_eq!(strip_definer("CREATE DEFINER=bob@localhost EVENT e"), "CREATE EVENT e");
        assert_eq!(strip_definer("CREATE DEFINER=`we``ird`@`h@x` FUNCTION f"), "CREATE FUNCTION f");
        assert_eq!(strip_definer("SELECT 'no definer here'"), "SELECT 'no definer here'");
        assert_eq!(strip_definer("CREATE DEFINER=CURRENT_USER FUNCTION f"), "CREATE FUNCTION f");
    }

    #[test]
    fn views_come_after_the_views_they_read() {
        let views: Vec<String> = ["a_top", "m_mid", "z_base"].iter().map(|s| s.to_string()).collect();
        let mut deps = BTreeMap::new();
        deps.insert("a_top".to_string(), BTreeSet::from(["m_mid".to_string(), "some_table".to_string()]));
        deps.insert("m_mid".to_string(), BTreeSet::from(["z_base".to_string()]));
        assert_eq!(order_views(&views, &deps), vec!["z_base", "m_mid", "a_top"]);
        assert_eq!(order_views(&views, &BTreeMap::new()), vec!["a_top", "m_mid", "z_base"], "no information: name order");
        // a cycle (impossible in MySQL) still terminates
        let mut cyc = BTreeMap::new();
        cyc.insert("a_top".to_string(), BTreeSet::from(["m_mid".to_string()]));
        cyc.insert("m_mid".to_string(), BTreeSet::from(["a_top".to_string()]));
        assert_eq!(order_views(&views, &cyc).len(), 3);
    }

    #[test]
    fn file_names_are_safe_everywhere() {
        assert_eq!(file_safe("orders"), "orders");
        assert_eq!(file_safe("a/b\\c:d*e"), "a_b_c_d_e");
        assert_eq!(file_safe("../../etc"), "_.._etc", "no path traversal: separators become underscores and leading dots are dropped");
        assert_eq!(file_safe(".."), "_");
        assert_eq!(file_safe(""), "_");
        assert!(!file_safe("../../etc/passwd").contains('/'));
    }

    #[test]
    fn the_session_header_and_footer_pair_up() {
        let (h, f) = session_header("9.7.2", "2026-01-01 00:00:00", &["a".into(), "b".into()], true);
        assert!(h.contains("SET FOREIGN_KEY_CHECKS = 0;") && f.contains("SET FOREIGN_KEY_CHECKS = 1;"));
        assert!(h.contains("SET SQL_MODE = 'NO_AUTO_VALUE_ON_ZERO'") && h.contains("SET TIME_ZONE = '+00:00'") && h.contains("-- Databases: a, b"));
        let (h2, f2) = session_header("x", "y", &[], false);
        assert!(!h2.contains("FOREIGN_KEY_CHECKS") && !f2.contains("FOREIGN_KEY_CHECKS"));
    }
}
