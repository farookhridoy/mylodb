//! Running a SQL script from a file, a zip archive or a folder of files, statement by statement.

use crate::{check_cancel, q, Error, Progress, ProgressFn, Result};
use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::Conn;
use myq_sql::ScriptReader;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

#[derive(Debug, Clone, Deserialize)]
pub struct RunOptions {
    /// Stop at the first failing statement (otherwise continue and list every failure).
    #[serde(default)]
    pub stop_on_error: bool,
    /// Run with `FOREIGN_KEY_CHECKS=0`, as a restore needs (tables arrive in any order).
    #[serde(default = "yes")]
    pub disable_fk_checks: bool,
    /// Select this database first (for dumps that do not name one).
    #[serde(default)]
    pub database: Option<String>,
}

fn yes() -> bool {
    true
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions { stop_on_error: false, disable_fk_checks: true, database: None }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ScriptError {
    pub file: String,
    pub line: usize,
    /// Start of the statement that failed.
    pub statement: String,
    pub message: String,
}

#[derive(Debug, Default, Serialize)]
pub struct RunResult {
    pub files: u64,
    pub statements: u64,
    pub failed: u64,
    /// Up to 100 failures, in order.
    pub errors: Vec<ScriptError>,
    /// The run stopped at the first error because `stop_on_error` was set.
    pub aborted: bool,
    pub elapsed_ms: u64,
}

const MAX_ERRORS: usize = 100;

/// One `.sql` file to run, with the reader that yields its bytes.
struct Entry {
    name: String,
    size: u64,
    open: Box<dyn FnOnce() -> Result<Box<dyn Read + Send>> + Send>,
}

/// The ordered list of scripts behind a source: a `.sql` file, a `.zip` of them, or a folder.
fn entries(source: &Path) -> Result<Vec<Entry>> {
    let lower = source.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if source.is_dir() {
        let mut files = Vec::new();
        collect_sql(source, &mut files)?;
        files.sort();
        return Ok(files
            .into_iter()
            .map(|p| Entry {
                name: p.strip_prefix(source).unwrap_or(&p).to_string_lossy().into_owned(),
                size: std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0),
                open: Box::new(move || Ok(Box::new(File::open(&p)?) as Box<dyn Read + Send>)),
            })
            .collect());
    }
    if lower == "zip" {
        let mut archive = zip::ZipArchive::new(File::open(source)?)?;
        let mut names: Vec<(String, u64)> = Vec::new();
        for i in 0..archive.len() {
            let f = archive.by_index(i)?;
            // an entry whose name cannot be decoded cannot be a .sql file we know how to order
            let Ok(name) = f.name().map(|n| n.to_string()) else { continue };
            if !f.is_dir() && name.to_lowercase().ends_with(".sql") {
                names.push((name, f.size()));
            }
        }
        names.sort();
        if names.is_empty() {
            return Err(Error::Invalid("the archive contains no .sql files".into()));
        }
        // Each entry is read through its own handle on the archive, so entries can be streamed one by one.
        return Ok(names
            .into_iter()
            .map(|(name, size)| {
                let path: PathBuf = source.to_path_buf();
                let entry_name = name.clone();
                Entry {
                    name,
                    size,
                    // each entry gets its own handle on the archive, so entries stream one at a time
                    open: Box::new(move || Ok(Box::new(ZipEntryReader::new(zip::ZipArchive::new(File::open(&path)?)?, &entry_name)?) as Box<dyn Read + Send>)),
                }
            })
            .collect());
    }
    Ok(vec![Entry {
        name: source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        size: std::fs::metadata(source)?.len(),
        open: {
            let p = source.to_path_buf();
            Box::new(move || Ok(Box::new(File::open(&p)?) as Box<dyn Read + Send>))
        },
    }])
}

fn collect_sql(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_dir() {
            collect_sql(&p, out)?;
        } else if p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("sql")) {
            out.push(p);
        }
    }
    Ok(())
}

/// Streams one zip entry. `ZipFile` borrows its archive, so a thread decompresses into a bounded channel
/// and this reader drains it; memory stays constant for entries of any size.
struct ZipEntryReader {
    rx: std::sync::mpsc::Receiver<std::io::Result<Vec<u8>>>,
    buf: Vec<u8>,
    pos: usize,
}

impl ZipEntryReader {
    fn new(archive: zip::ZipArchive<File>, name: &str) -> Result<Self> {
        let index = archive.index_for_name(name).ok_or_else(|| Error::Invalid(format!("{name} is not in the archive")))?;
        let (tx, rx) = std::sync::mpsc::sync_channel(8);
        std::thread::spawn(move || {
            let mut archive = archive;
            let mut file = match archive.by_index(index) {
                Ok(f) => f,
                Err(e) => {
                    let _ = tx.send(Err(std::io::Error::other(e.to_string())));
                    return;
                }
            };
            loop {
                let mut chunk = vec![0u8; 64 * 1024];
                match file.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        chunk.truncate(n);
                        if tx.send(Ok(chunk)).is_err() {
                            break; // the reader was dropped (cancelled or stopped on an error)
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        break;
                    }
                }
            }
        });
        Ok(ZipEntryReader { rx, buf: Vec::new(), pos: 0 })
    }
}

impl Read for ZipEntryReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.buf.len() {
            match self.rx.recv() {
                Ok(Ok(chunk)) => {
                    self.buf = chunk;
                    self.pos = 0;
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

enum Item {
    Statement { file: String, line: usize, text: String, bytes_in_file: u64 },
    FileDone,
    ReadError(String),
}

/// What a script contains, found without running anything: how many statements of each kind, the first few, and how many can destroy data.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SourcePreview {
    pub files: u64,
    pub statements: u64,
    pub bytes: u64,
    /// `INSERT`, `CREATE TABLE`, `DROP TABLE`… with their counts, most frequent first.
    pub kinds: Vec<(String, u64)>,
    /// Statements that delete data or objects (DROP, TRUNCATE, DELETE, ALTER ... DROP).
    pub destructive: u64,
    pub first: Vec<PreviewStatement>,
    /// Read errors found while scanning (the run would hit them too).
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreviewStatement {
    pub file: String,
    pub line: u64,
    /// Cut at 400 characters.
    pub text: String,
}

/// `INSERT`, `DROP TABLE`, `CREATE TABLE`… the first one or two words of a statement, upper-cased.
fn statement_kind(text: &str) -> String {
    let mut words = text.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_').to_uppercase()).filter(|w| !w.is_empty());
    let first = words.next().unwrap_or_default();
    match first.as_str() {
        "CREATE" | "DROP" | "ALTER" | "TRUNCATE" => {
            let mut second = words.next().unwrap_or_default();
            // CREATE OR REPLACE VIEW, CREATE TEMPORARY TABLE, CREATE UNIQUE INDEX: look past the modifier
            while matches!(second.as_str(), "OR" | "REPLACE" | "TEMPORARY" | "UNIQUE" | "DEFINER" | "ALGORITHM") {
                second = words.next().unwrap_or_default();
            }
            format!("{first} {second}").trim().to_string()
        }
        _ => first,
    }
}

/// Scan a `.sql` file, `.zip` or folder without a database: the statements are counted by kind and the first `sample` are kept.
pub fn preview_source(source: &Path, sample: usize) -> Result<SourcePreview> {
    let list = entries(source)?;
    let mut out = SourcePreview { files: list.len() as u64, bytes: list.iter().map(|e| e.size).sum(), ..Default::default() };
    let mut kinds: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for entry in list {
        let name = entry.name.clone();
        let stream = match (entry.open)() {
            Ok(s) => s,
            Err(e) => {
                out.errors.push(format!("{name}: {e}"));
                continue;
            }
        };
        for st in ScriptReader::new(BufReader::with_capacity(256 * 1024, stream)) {
            match st {
                Ok(st) => {
                    out.statements += 1;
                    let kind = statement_kind(&st.text);
                    if kind.starts_with("DROP") || kind.starts_with("TRUNCATE") || kind == "DELETE" {
                        out.destructive += 1;
                    }
                    *kinds.entry(kind).or_default() += 1;
                    if out.first.len() < sample {
                        let text: String = st.text.chars().take(400).collect();
                        out.first.push(PreviewStatement { file: name.clone(), line: st.line as u64, text });
                    }
                }
                Err(e) => {
                    out.errors.push(format!("{name}: {e}"));
                    break;
                }
            }
        }
    }
    let mut kinds: Vec<(String, u64)> = kinds.into_iter().collect();
    kinds.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out.kinds = kinds;
    Ok(out)
}

/// Run every statement of `source` on `conn`.
pub async fn run_source(conn: &mut Conn, source: &Path, opts: &RunOptions, progress: ProgressFn<'_>, cancel: &AtomicBool) -> Result<RunResult> {
    let started = Instant::now();
    let list = entries(source)?;
    let sizes: Vec<u64> = list.iter().map(|e| e.size).collect();
    let total: u64 = sizes.iter().sum();
    let mut result = RunResult { files: list.len() as u64, ..Default::default() };

    if opts.disable_fk_checks {
        conn.query_drop("SET FOREIGN_KEY_CHECKS = 0").await?;
    }
    if let Some(db) = &opts.database {
        conn.query_drop(format!("USE {}", q(db))).await?;
    }

    // Reading and parsing happen on a blocking thread, feeding a bounded channel: the database never
    // waits on the disk, and a stop (error, cancel) closes the channel and ends the reader.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Item>(64);
    let reader = tokio::task::spawn_blocking(move || {
        for entry in list {
            let name = entry.name.clone();
            let stream = match (entry.open)() {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.blocking_send(Item::ReadError(format!("{name}: {e}")));
                    return;
                }
            };
            for st in ScriptReader::new(BufReader::with_capacity(256 * 1024, stream)) {
                match st {
                    Ok(st) => {
                        if tx.blocking_send(Item::Statement { file: name.clone(), line: st.line, text: st.text, bytes_in_file: st.bytes_read }).is_err() {
                            return;
                        }
                    }
                    Err(e) => {
                        let _ = tx.blocking_send(Item::ReadError(format!("{name}: {e}")));
                        return;
                    }
                }
            }
            if tx.blocking_send(Item::FileDone).is_err() {
                return;
            }
        }
    });

    let mut done_files_bytes = 0u64;
    let mut sizes = sizes.into_iter();
    let mut current_size = sizes.next().unwrap_or(0);
    let mut outcome: Result<()> = Ok(());

    while let Some(item) = rx.recv().await {
        if let Err(e) = check_cancel(cancel) {
            outcome = Err(e);
            break;
        }
        match item {
            Item::FileDone => {
                done_files_bytes += current_size;
                current_size = sizes.next().unwrap_or(0);
            }
            Item::ReadError(m) => {
                outcome = Err(Error::Io(std::io::Error::other(m)));
                break;
            }
            Item::Statement { file, line, text, bytes_in_file } => {
                result.statements += 1;
                if let Err(e) = conn.query_drop(&text).await {
                    match e {
                        myq_driver::mysql_async::Error::Server(s) => {
                            result.failed += 1;
                            if result.errors.len() < MAX_ERRORS {
                                result.errors.push(ScriptError { file, line, statement: text.chars().take(200).collect(), message: format!("{} ({})", s.message, s.code) });
                            }
                            if opts.stop_on_error {
                                result.aborted = true;
                                break;
                            }
                        }
                        // the connection itself failed: nothing more can run
                        other => {
                            outcome = Err(other.into());
                            break;
                        }
                    }
                }
                // every 100 statements; whoever displays progress decides how often to redraw
                if result.statements % 100 == 0 {
                    progress(Progress { phase: "Running script".into(), done: done_files_bytes + bytes_in_file, total: Some(total), message: format!("{} statements", result.statements) });
                }
            }
        }
    }
    drop(rx);
    let _ = reader.await;

    if opts.disable_fk_checks {
        let _ = conn.query_drop("SET FOREIGN_KEY_CHECKS = 1").await;
    }
    outcome?;
    progress(Progress { phase: "Running script".into(), done: total, total: Some(total), message: format!("{} statements", result.statements) });
    result.elapsed_ms = started.elapsed().as_millis() as u64;
    Ok(result)
}

#[cfg(test)]
mod preview_tests {
    use super::*;

    #[test]
    fn kinds_look_past_modifiers() {
        assert_eq!(statement_kind("insert into t values (1)"), "INSERT");
        assert_eq!(statement_kind("CREATE OR REPLACE VIEW v AS SELECT 1"), "CREATE VIEW");
        assert_eq!(statement_kind("create temporary table x (a int)"), "CREATE TABLE");
        assert_eq!(statement_kind("DROP TABLE IF EXISTS a"), "DROP TABLE");
        assert_eq!(statement_kind("/*!40101 SET x */"), "40101");
    }

    #[test]
    fn a_script_is_summarised_without_a_database() {
        let p = std::env::temp_dir().join(format!("mylodb_preview_{}.sql", std::process::id()));
        std::fs::write(&p, "DROP TABLE IF EXISTS a;\nCREATE TABLE a (id INT);\nINSERT INTO a VALUES (1);\nINSERT INTO a VALUES (2);\nDELETE FROM a;\n").unwrap();
        let pv = preview_source(&p, 3).unwrap();
        std::fs::remove_file(&p).unwrap();
        assert_eq!((pv.files, pv.statements, pv.destructive), (1, 5, 2));
        assert_eq!(pv.kinds[0], ("INSERT".to_string(), 2));
        assert_eq!(pv.first.len(), 3);
        assert_eq!(pv.first[0].line, 1);
        assert!(pv.errors.is_empty());
    }
}
