//! Scheduled backups: a list of jobs kept in `schedules.json` in the app's data folder, a check every 30 seconds for jobs that are
//! due, and the log of what happened. Jobs only run while MyLoDB is open (there is no background service).

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
    /// Folder the backups are written to.
    pub dir: String,
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
    if !s.enabled || s.databases.is_empty() || s.dir.trim().is_empty() {
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
    if s.dir.trim().is_empty() { return Err("Choose the folder the backups go to.".into()); }
    if s.keep == 0 { return Err("Keep at least one backup.".into()); }
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
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(list)?)?;
    std::fs::rename(tmp, path)
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
        Schedule { id: "j".into(), name: "n".into(), connection_id: 1, databases: vec!["shop".into()], dir: "/b".into(), rule, keep: 5, compress: false, data: true, enabled: true, created: created.into(), last_run: last.map(String::from), log: vec![] }
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
