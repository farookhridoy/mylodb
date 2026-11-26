//! Scheduled jobs (backups, exports, imports): a list kept in `schedules.json` in the app's data folder, a check for jobs that are
//! due, and the log of what happened. The open app checks every 30 seconds; with "run when MyLoDB is closed" the system's scheduler
//! also starts `MyLoDB --run-due-jobs` (see `background.rs`). Because two processes can be at work, the file is the only copy of the
//! list: every change reads and writes it under a lock, and a job holds its own lock while it runs.

use chrono::{Datelike, Duration, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Rule {
    /// Every `hours` hours after the last run.
    Hours { hours: u32 },
    /// Every day at a time of day (local time).
    Daily { hour: u32, minute: u32 },
    /// Every week on a day (0 = Monday … 6 = Sunday) at a time of day.
    Weekly { weekday: u32, hour: u32, minute: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Local time, `YYYY-MM-DD HH:MM:SS`.
    pub time: String,
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub id: String,
    pub name: String,
    pub connection_id: i64,
    pub databases: Vec<String>,
    /// `backup` (a SQL dump of the databases), `export` (the result of one SELECT, run in the first database) or `import` (a file
    /// into the first database).
    #[serde(default = "backup_kind")]
    pub kind: String,
    #[serde(default)]
    pub sql: String,
    /// For an export: `csv`, `json`, `json_lines`, `sql_insert`, `xml`, `html` or `xlsx`.
    #[serde(default = "csv_format")]
    pub format: String,
    /// Folder the backups are written to (not used by an import).
    #[serde(default)]
    pub dir: String,
    /// Import: the file to read (`.csv`, `.tsv`, `.txt`, `.json`, `.jsonl`, `.ndjson`, or a `.sql` script or `.zip` of scripts).
    #[serde(default)]
    pub source: String,
    /// Import of a data file: the table it goes into.
    #[serde(default)]
    pub table: String,
    /// Import of a data file: `insert`, `ignore`, `update` or `replace` for rows whose key exists.
    #[serde(default = "insert_mode")]
    pub mode: String,
    /// Import of a data file: replace the table's rows with the file's.
    #[serde(default)]
    pub replace_all: bool,
    /// Import: do nothing when the file has not changed since the last successful import.
    #[serde(default = "yes")]
    pub only_if_changed: bool,
    /// Size and modification time of the file the last successful import read.
    #[serde(default)]
    pub last_source: Option<String>,
    pub rule: Rule,
    /// How many backups to keep (older ones of this job are deleted).
    pub keep: u32,
    #[serde(default)]
    pub compress: bool,
    #[serde(default = "yes")]
    pub data: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Local time of creation and of the last run, `YYYY-MM-DD HH:MM:SS`.
    pub created: String,
    #[serde(default)]
    pub last_run: Option<String>,
    #[serde(default)]
    pub log: Vec<LogEntry>,
}

fn yes() -> bool { true }
fn backup_kind() -> String { "backup".into() }
fn csv_format() -> String { "csv".into() }
fn insert_mode() -> String { "insert".into() }

/// Whether an import source is a SQL script (run statement by statement) and not a data file.
pub fn is_script(source: &str) -> bool {
    let s = source.trim().to_ascii_lowercase();
    s.ends_with(".sql") || s.ends_with(".zip")
}

/// What identifies the contents of a file between two runs: its size and modification time.
pub fn source_signature(path: &std::path::Path) -> Result<String, String> {
    let m = std::fs::metadata(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if !m.is_file() { return Err(format!("{} is not a file", path.display())); }
    let modified = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
    Ok(format!("{}:{modified}", m.len()))
}

pub const EXPORT_FORMATS: [(&str, &str); 7] = [("csv", "csv"), ("json", "json"), ("json_lines", "jsonl"), ("sql_insert", "sql"), ("xml", "xml"), ("html", "html"), ("xlsx", "xlsx")];

/// The file extension of an export format.
pub fn export_extension(format: &str) -> Option<&'static str> {
    EXPORT_FORMATS.iter().find(|(f, _)| *f == format).map(|(_, e)| *e)
}

pub const TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";
pub const LOG_LIMIT: usize = 40;

pub fn parse_time(s: &str) -> Option<NaiveDateTime> { NaiveDateTime::parse_from_str(s, TIME_FORMAT).ok() }
pub fn format_time(t: NaiveDateTime) -> String { t.format(TIME_FORMAT).to_string() }

/// The most recent moment this rule was supposed to run at or before `now`, for the time-of-day rules.
fn latest_slot(rule: &Rule, now: NaiveDateTime) -> Option<NaiveDateTime> {
    match rule {
        Rule::Hours { .. } => None,
        Rule::Daily { hour, minute } => {
            let slot = now.date().and_time(NaiveTime::from_hms_opt(*hour, *minute, 0)?);
            Some(if slot <= now { slot } else { slot - Duration::days(1) })
        }
        Rule::Weekly { weekday, hour, minute } => {
            let time = NaiveTime::from_hms_opt(*hour, *minute, 0)?;
            let back = (now.date().weekday().num_days_from_monday() + 7 - weekday) % 7;
            let slot = (now.date() - Duration::days(back as i64)).and_time(time);
            Some(if slot <= now { slot } else { slot - Duration::days(7) })
        }
    }
}

/// Is the job due at `now`? A time-of-day job runs once per slot, and not for a slot that passed before the job was made.
pub fn is_due(s: &Schedule, now: NaiveDateTime) -> bool {
    let target = if s.kind == "import" { &s.source } else { &s.dir };
    if !s.enabled || s.databases.is_empty() || target.trim().is_empty() {
        return false;
    }
    let created = parse_time(&s.created).unwrap_or(now);
    let last = s.last_run.as_deref().and_then(parse_time);
    match &s.rule {
        Rule::Hours { hours } => now >= last.unwrap_or(created) + Duration::hours((*hours).max(1) as i64),
        rule => match latest_slot(rule, now) {
            Some(slot) => slot > created && last.map_or(true, |l| l < slot),
            None => false,
        },
    }
}

/// The next time this job will run (for display), or `None` when it will not.
pub fn next_run(s: &Schedule, now: NaiveDateTime) -> Option<NaiveDateTime> {
    if !s.enabled { return None; }
    let created = parse_time(&s.created).unwrap_or(now);
    let last = s.last_run.as_deref().and_then(parse_time);
    match &s.rule {
        Rule::Hours { hours } => Some((last.unwrap_or(created) + Duration::hours((*hours).max(1) as i64)).max(now)),
        Rule::Daily { hour, minute } => {
            let slot = latest_slot(&s.rule, now)?;
            let next = slot + Duration::days(1);
            let _ = (hour, minute);
            Some(if is_due(s, now) { now } else { next })
        }
        Rule::Weekly { .. } => {
            let slot = latest_slot(&s.rule, now)?;
            Some(if is_due(s, now) { now } else { slot + Duration::days(7) })
        }
    }
}

pub fn valid(s: &Schedule) -> Result<(), String> {
    if s.name.trim().is_empty() { return Err("Give the job a name.".into()); }
    if s.databases.is_empty() { return Err("Choose at least one database.".into()); }
    if s.kind != "import" {
        if s.dir.trim().is_empty() { return Err("Choose the folder the backups go to.".into()); }
        if s.keep == 0 { return Err("Keep at least one copy.".into()); }
    }
    match s.kind.as_str() {
        "backup" => {}
        "export" => {
            if s.sql.trim().is_empty() { return Err("Write the SELECT to export.".into()); }
            if export_extension(&s.format).is_none() { return Err("Choose a file format.".into()); }
        }
        "import" => {
            if s.source.trim().is_empty() { return Err("Choose the file to import.".into()); }
            if !is_script(&s.source) {
                if s.table.trim().is_empty() { return Err("Choose the table the rows go into.".into()); }
                if !["insert", "ignore", "update", "replace"].contains(&s.mode.as_str()) { return Err("Choose what happens to rows that already exist.".into()); }
            }
        }
        _ => return Err("Choose what the job does: a backup, an export or an import.".into()),
    }
    match s.rule {
        Rule::Hours { hours } if !(1..=24 * 30).contains(&hours) => Err("Hours must be between 1 and 720.".into()),
        Rule::Daily { hour, minute } | Rule::Weekly { hour, minute, .. } if hour > 23 || minute > 59 => Err("That is not a time of day.".into()),
        Rule::Weekly { weekday, .. } if weekday > 6 => Err("That is not a day of the week.".into()),
        _ => Ok(()),
    }
}

pub fn load(path: &std::path::Path) -> Vec<Schedule> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save(path: &std::path::Path, list: &[Schedule]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(list)?)?;
    std::fs::rename(tmp, path)
}

fn lock_file(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    if let Some(dir) = path.parent() { std::fs::create_dir_all(dir)?; }
    std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(path)
}

/// Read the list, change it and write it back while no other thread or process can do the same.
pub fn update<R>(path: &std::path::Path, change: impl FnOnce(&mut Vec<Schedule>) -> Result<R, String>) -> Result<R, String> {
    let lock = lock_file(&path.with_extension("lock")).map_err(|e| e.to_string())?;
    lock.lock().map_err(|e| e.to_string())?;
    let mut list = match std::fs::read_to_string(path) {
        // never write over a file that cannot be understood: that would throw away every job
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{} is unreadable: {e}", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e.to_string()),
    };
    let out = change(&mut list)?;
    save(path, &list).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Claim a job for running. `None` while this or another process is running it. The claim ends when the file is dropped, also when the
/// process dies.
pub fn claim(path: &std::path::Path, id: &str) -> Option<std::fs::File> {
    let safe: String = id.chars().filter(char::is_ascii_alphanumeric).collect();
    let file = lock_file(&path.with_file_name("schedule-locks").join(format!("{safe}.lock"))).ok()?;
    file.try_lock().ok().map(|_| file)
}

pub fn push_log(s: &mut Schedule, now: NaiveDateTime, ok: bool, message: String) {
    s.log.insert(0, LogEntry { time: format_time(now), ok, message });
    s.log.truncate(LOG_LIMIT);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> NaiveDateTime { parse_time(s).unwrap() }
    fn job(rule: Rule, created: &str, last: Option<&str>) -> Schedule {
        Schedule { id: "j".into(), name: "n".into(), connection_id: 1, databases: vec!["shop".into()], dir: "/b".into(), rule, keep: 5, compress: false, data: true, enabled: true, created: created.into(), last_run: last.map(String::from), log: vec![], kind: "backup".into(), sql: String::new(), format: "csv".into(), source: String::new(), table: String::new(), mode: "insert".into(), replace_all: false, only_if_changed: true, last_source: None }
    }

    #[test]
    fn hourly_jobs_run_that_long_after_the_last_run_or_creation() {
        let j = job(Rule::Hours { hours: 6 }, "2026-11-25 08:00:00", None);
        assert!(!is_due(&j, at("2026-11-25 13:59:59")));
        assert!(is_due(&j, at("2026-11-25 14:00:00")));
        let j = job(Rule::Hours { hours: 6 }, "2026-11-25 08:00:00", Some("2026-11-25 14:05:00"));
        assert!(!is_due(&j, at("2026-11-25 20:04:00")) && is_due(&j, at("2026-11-25 20:05:00")));
    }

    #[test]
    fn daily_jobs_run_once_per_day_after_their_time() {
        let j = job(Rule::Daily { hour: 3, minute: 30 }, "2026-11-20 10:00:00", Some("2026-11-24 03:30:10"));
        assert!(!is_due(&j, at("2026-11-25 03:29:00")), "before the time");
        assert!(is_due(&j, at("2026-11-25 03:30:00")));
        assert!(is_due(&j, at("2026-11-25 18:00:00")), "a missed slot still runs when the app is opened later the same day");
        let done = job(Rule::Daily { hour: 3, minute: 30 }, "2026-11-20 10:00:00", Some("2026-11-25 03:30:10"));
        assert!(!is_due(&done, at("2026-11-25 18:00:00")), "once per slot");
        assert!(is_due(&done, at("2026-11-26 03:31:00")));
    }

    #[test]
    fn a_new_job_does_not_run_for_a_time_that_already_passed() {
        let j = job(Rule::Daily { hour: 3, minute: 0 }, "2026-11-25 09:00:00", None);
        assert!(!is_due(&j, at("2026-11-25 09:01:00")), "today's 03:00 is before the job was made");
        assert!(is_due(&j, at("2026-11-26 03:00:00")));
    }

    #[test]
    fn weekly_jobs_use_the_weekday_and_the_last_slot() {
        // 2026-11-25 is a Wednesday; weekday 0 is Monday
        let j = job(Rule::Weekly { weekday: 0, hour: 2, minute: 0 }, "2026-11-01 00:00:00", Some("2026-11-16 02:00:05"));
        assert!(is_due(&j, at("2026-11-25 12:00:00")), "Monday 23 Nov 02:00 was missed");
        let done = job(Rule::Weekly { weekday: 0, hour: 2, minute: 0 }, "2026-11-01 00:00:00", Some("2026-11-23 02:00:05"));
        assert!(!is_due(&done, at("2026-11-25 12:00:00")) && !is_due(&done, at("2026-11-30 01:59:00")));
        assert!(is_due(&done, at("2026-11-30 02:00:00")));
    }

    #[test]
    fn a_disabled_or_incomplete_job_never_runs_and_bad_input_is_refused() {
        let mut j = job(Rule::Hours { hours: 1 }, "2026-11-01 00:00:00", None);
        j.enabled = false;
        assert!(!is_due(&j, at("2026-12-01 00:00:00")));
        j.enabled = true; j.databases.clear();
        assert!(!is_due(&j, at("2026-12-01 00:00:00")) && valid(&j).is_err());
        let ok = job(Rule::Daily { hour: 24, minute: 0 }, "2026-11-01 00:00:00", None);
        assert!(valid(&ok).is_err());
        assert!(valid(&job(Rule::Weekly { weekday: 7, hour: 1, minute: 0 }, "2026-11-01 00:00:00", None)).is_err());
        assert!(valid(&job(Rule::Daily { hour: 3, minute: 0 }, "2026-11-01 00:00:00", None)).is_ok());
    }

    #[test]
    fn the_next_run_is_shown_and_the_log_is_capped() {
        let j = job(Rule::Daily { hour: 3, minute: 0 }, "2026-11-20 00:00:00", Some("2026-11-25 03:00:05"));
        assert_eq!(next_run(&j, at("2026-11-25 10:00:00")), Some(at("2026-11-26 03:00:00")));
        let mut j = j;
        for i in 0..60 { push_log(&mut j, at("2026-11-25 10:00:00"), i % 2 == 0, format!("run {i}")); }
        assert_eq!(j.log.len(), LOG_LIMIT);
        assert_eq!(j.log[0].message, "run 59", "newest first");
    }

    #[test]
    fn an_export_needs_its_select_and_a_known_format_and_old_files_still_load() {
        let mut j = job(Rule::Daily { hour: 3, minute: 0 }, "2026-11-01 00:00:00", None);
        j.kind = "export".into();
        assert!(valid(&j).unwrap_err().contains("SELECT"));
        j.sql = "SELECT 1".into();
        assert!(valid(&j).is_ok());
        j.format = "pdf".into();
        assert!(valid(&j).is_err());
        assert_eq!(export_extension("json_lines"), Some("jsonl"));
        j.kind = "other".into();
        assert!(valid(&j).is_err());
        // a job saved before exports existed has none of the new fields
        let old = r#"{"id":"a","name":"n","connection_id":1,"databases":["shop"],"dir":"/b","rule":{"kind":"hours","hours":6},"keep":3,"created":"2026-11-01 00:00:00"}"#;
        let s: Schedule = serde_json::from_str(old).unwrap();
        assert_eq!((s.kind.as_str(), s.format.as_str(), s.data, s.enabled), ("backup", "csv", true, true));
    }

    #[test]
    fn an_import_needs_a_file_and_a_table_unless_it_is_a_script() {
        let mut j = job(Rule::Hours { hours: 1 }, "2026-11-01 00:00:00", None);
        j.kind = "import".into(); j.dir.clear(); j.keep = 0;
        assert!(valid(&j).unwrap_err().contains("file"));
        j.source = "/in/orders.csv".into();
        assert!(valid(&j).unwrap_err().contains("table"));
        j.table = "orders".into();
        assert!(valid(&j).is_ok() && is_due(&j, at("2026-11-02 00:00:00")), "an import needs no folder");
        j.mode = "drop".into();
        assert!(valid(&j).is_err());
        j.source = "/in/Load.SQL".into(); j.table.clear();
        assert!(valid(&j).is_ok(), "a script names its own tables");
    }

    #[test]
    fn a_running_job_cannot_be_claimed_twice_and_changes_go_through_the_file() {
        let dir = std::env::temp_dir().join(format!("mylodb_claim_{}", std::process::id()));
        let p = dir.join("schedules.json");
        let first = claim(&p, "a1").expect("free");
        assert!(claim(&p, "a1").is_none() && claim(&p, "b2").is_some());
        drop(first);
        assert!(claim(&p, "a1").is_some(), "released when dropped");
        update(&p, |l| { l.push(job(Rule::Hours { hours: 1 }, "2026-11-01 00:00:00", None)); Ok(()) }).unwrap();
        assert_eq!(load(&p).len(), 1);
        assert!(update(&p, |l| { l.clear(); Err::<(), _>("no".into()) }).is_err());
        assert_eq!(load(&p).len(), 1, "a refused change writes nothing");
        std::fs::write(&p, "not json").unwrap();
        assert!(update(&p, |_| Ok(())).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "not json", "an unreadable file is left alone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_file_round_trips() {
        let p = std::env::temp_dir().join(format!("mylodb_sched_{}.json", std::process::id()));
        let j = job(Rule::Weekly { weekday: 2, hour: 4, minute: 15 }, "2026-11-01 00:00:00", None);
        save(&p, &[j]).unwrap();
        let back = load(&p);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].rule, Rule::Weekly { weekday: 2, hour: 4, minute: 15 });
        let _ = std::fs::remove_file(&p);
        assert!(load(&p).is_empty(), "a missing file is an empty list");
    }
}
