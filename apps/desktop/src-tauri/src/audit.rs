//! The audit log: one JSON line per statement that changed something, in `audit.log` in the app's data folder. It is off until the user
//! turns it on (Settings, Audit log). It is a plain file, so it can be read, kept and shipped with any tool; it is never sent anywhere.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    /// Local time, `YYYY-MM-DD HH:MM:SS`.
    pub time: String,
    pub connection: String,
    /// `user@host` for a server, the file for SQLite.
    pub who: String,
    pub database: Option<String>,
    pub ok: bool,
    pub ms: u64,
    pub sql: String,
}

const MAX_BYTES: u64 = 10 * 1024 * 1024;
const MAX_SQL: usize = 8 * 1024;
const KEEP: usize = 3;

pub struct Audit {
    file: PathBuf,
    marker: PathBuf,
    enabled: AtomicBool,
    lock: Mutex<()>,
}

/// Does a statement (or script) belong in the log? Reads do not; neither does a backup, which only reads.
pub fn should_log(sql: &str) -> bool {
    let t = sql.trim();
    if t.is_empty() || t.starts_with("BACKUP ") {
        return false;
    }
    myq_sql::split_statements(t).iter().any(|s| is_change(&s.text))
}

fn is_change(statement: &str) -> bool {
    match myq_sql::statement_kind(statement) {
        myq_sql::StatementKind::Read => false,
        myq_sql::StatementKind::Write => true,
        myq_sql::StatementKind::Other => {
            let mut words = statement.split(|c: char| !c.is_ascii_alphanumeric() && c != '_').filter(|w| !w.is_empty()).map(|w| w.to_ascii_uppercase());
            match (words.next().as_deref(), words.next().as_deref()) {
                (Some("SHOW" | "DESCRIBE" | "DESC" | "EXPLAIN" | "USE" | "HELP"), _) => false,
                // a session setting changes nothing that stays; a global one does
                (Some("SET"), Some("GLOBAL" | "PERSIST" | "PERSIST_ONLY")) => true,
                (Some("SET"), _) => false,
                _ => true,
            }
        }
    }
}

impl Audit {
    /// The log lives in `dir`; `audit-enabled` next to it remembers the switch.
    pub fn open(dir: &Path) -> Audit {
        let marker = dir.join("audit-enabled");
        Audit { file: dir.join("audit.log"), enabled: AtomicBool::new(marker.exists()), marker, lock: Mutex::new(()) }
    }

    pub fn path(&self) -> &Path {
        &self.file
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn set_enabled(&self, on: bool) -> std::io::Result<()> {
        if on {
            std::fs::write(&self.marker, b"1")?;
        } else if self.marker.exists() {
            std::fs::remove_file(&self.marker)?;
        }
        self.enabled.store(on, Ordering::Relaxed);
        Ok(())
    }

    pub fn size(&self) -> u64 {
        std::fs::metadata(&self.file).map(|m| m.len()).unwrap_or(0)
    }

    /// Append one entry (nothing happens when the log is off). A failure to write is not allowed to break the action it records.
    pub fn record(&self, mut entry: Entry) {
        if !self.enabled() {
            return;
        }
        if entry.sql.len() > MAX_SQL {
            let mut cut = MAX_SQL;
            while !entry.sql.is_char_boundary(cut) {
                cut -= 1;
            }
            entry.sql.truncate(cut);
            entry.sql.push_str(" …[cut]");
        }
        let Ok(line) = serde_json::to_string(&entry) else { return };
        let _guard = self.lock.lock().unwrap();
        self.rotate();
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&self.file) {
            let _ = writeln!(f, "{line}");
        }
    }

    /// `audit.log` becomes `audit.log.1` when it is large, and older ones shift up; only the newest few are kept.
    fn rotate(&self) {
        if self.size() < MAX_BYTES {
            return;
        }
        let name = |n: usize| self.file.with_file_name(format!("audit.log.{n}"));
        let _ = std::fs::remove_file(name(KEEP));
        for n in (1..KEEP).rev() {
            let _ = std::fs::rename(name(n), name(n + 1));
        }
        let _ = std::fs::rename(&self.file, name(1));
    }

    /// The newest `limit` entries, newest first. A damaged line is skipped.
    pub fn tail(&self, limit: usize) -> Vec<Entry> {
        let Ok(text) = std::fs::read_to_string(&self.file) else { return vec![] };
        text.lines().rev().filter_map(|l| serde_json::from_str::<Entry>(l).ok()).take(limit).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(sql: &str) -> Entry {
        Entry { time: "2026-11-26 10:00:00".into(), connection: "Local".into(), who: "root@127.0.0.1".into(), database: Some("shop".into()), ok: true, ms: 3, sql: sql.into() }
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mylodb_audit_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn only_changes_are_logged() {
        assert!(should_log("UPDATE orders SET total = 1 WHERE id = 2"));
        assert!(should_log("DELETE FROM orders"));
        assert!(should_log("CREATE TABLE t (id int)"));
        assert!(should_log("SELECT 1; DROP TABLE t"), "a script with one change is logged");
        assert!(should_log("RESTORE /tmp/x.sql"));
        assert!(!should_log("SELECT * FROM orders"));
        assert!(!should_log("SHOW TABLES") && !should_log("USE shop") && !should_log("SET NAMES utf8mb4") && !should_log("EXPLAIN SELECT 1"));
        assert!(should_log("SET GLOBAL max_connections = 300"));
        assert!(!should_log("BACKUP shop"));
        assert!(!should_log("   "));
    }

    #[test]
    fn nothing_is_written_until_it_is_switched_on_and_the_switch_is_remembered() {
        let d = dir("switch");
        let a = Audit::open(&d);
        a.record(entry("DELETE FROM t"));
        assert!(!a.path().exists() && a.tail(10).is_empty(), "off by default");
        a.set_enabled(true).unwrap();
        a.record(entry("DELETE FROM t"));
        assert_eq!(a.tail(10).len(), 1);
        assert!(Audit::open(&d).enabled(), "a new run remembers it is on");
        a.set_enabled(false).unwrap();
        a.record(entry("DROP TABLE t"));
        assert_eq!(a.tail(10).len(), 1, "off again: nothing new");
        assert!(!Audit::open(&d).enabled());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn entries_come_back_newest_first_and_a_damaged_line_is_skipped() {
        let d = dir("tail");
        let a = Audit::open(&d);
        a.set_enabled(true).unwrap();
        for i in 0..5 {
            a.record(entry(&format!("UPDATE t SET a = {i}")));
        }
        std::fs::OpenOptions::new().append(true).open(a.path()).unwrap().write_all(b"not json\n").unwrap();
        let t = a.tail(3);
        assert_eq!(t.iter().map(|e| e.sql.as_str()).collect::<Vec<_>>(), vec!["UPDATE t SET a = 4", "UPDATE t SET a = 3", "UPDATE t SET a = 2"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn long_statements_are_cut_and_a_big_log_rotates() {
        let d = dir("rotate");
        let a = Audit::open(&d);
        a.set_enabled(true).unwrap();
        a.record(entry(&format!("INSERT INTO t VALUES ('{}')", "x".repeat(20_000))));
        let e = &a.tail(1)[0];
        assert!(e.sql.len() < 9_000 && e.sql.ends_with("…[cut]"));
        // fill past the limit: the next write starts a fresh file and keeps the old one as audit.log.1
        std::fs::write(a.path(), vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        a.record(entry("DELETE FROM t"));
        assert!(d.join("audit.log.1").exists());
        assert_eq!(a.tail(10).len(), 1);
        assert!(a.size() < 1000);
        let _ = std::fs::remove_dir_all(&d);
    }
}
