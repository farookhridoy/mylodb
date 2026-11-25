//! Server diagnostics: processes, variables, status, table maintenance and logs.

use crate::{Error, Result};
use myq_driver::mysql_async::{prelude::Queryable, Value};
use myq_driver::Conn;
use serde::{Deserialize, Serialize};

use myq_core::{lit, q};

/// Something the server cannot do or has switched off (log not enabled, table missing on this version).
fn unavailable(msg: impl Into<String>) -> Error {
    Error::Unavailable(msg.into())
}

// ---------- processes ----------

#[derive(Debug, Clone, Serialize)]
pub struct Process {
    pub id: u64,
    pub user: String,
    pub host: String,
    pub db: Option<String>,
    pub command: String,
    pub time: i64,
    pub state: Option<String>,
    pub info: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ProcessList {
    pub processes: Vec<Process>,
    /// The connection that asked: it always shows up as running `SHOW PROCESSLIST` and cannot be killed here.
    pub self_id: u64,
}

pub async fn processlist(conn: &mut Conn) -> Result<ProcessList> {
    // The first eight columns are the same everywhere; MariaDB adds `Progress` at the end, so rows are read by position, not as a fixed tuple.
    use myq_driver::mysql_async::{from_value_opt, Row, Value};
    let rows: Vec<Row> = conn.query("SHOW FULL PROCESSLIST").await?;
    let cell = |r: &Row, i: usize| -> Option<String> { match r.as_ref(i)? { Value::NULL => None, v => from_value_opt::<String>(v.clone()).ok() } };
    let processes = rows
        .iter()
        .filter_map(|r| {
            Some(Process {
                id: cell(r, 0)?.parse().ok()?,
                user: cell(r, 1).unwrap_or_default(),
                host: cell(r, 2).unwrap_or_default(),
                db: cell(r, 3),
                command: cell(r, 4).unwrap_or_default(),
                time: cell(r, 5).and_then(|t| t.parse().ok()).unwrap_or(0),
                state: cell(r, 6),
                info: cell(r, 7),
            })
        })
        .collect();
    Ok(ProcessList { self_id: conn.id() as u64, processes })
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KillKind {
    /// Stop the running statement and keep the connection (`KILL QUERY`).
    Query,
    /// Close the connection (`KILL CONNECTION`).
    Connection,
}

pub async fn kill(conn: &mut Conn, id: u64, kind: KillKind) -> Result<()> {
    if id == conn.id() as u64 {
        return Err(Error::Rejected("this is the connection used for monitoring; it cannot kill itself".into()));
    }
    let sql = match kind {
        KillKind::Query => format!("KILL QUERY {id}"),
        KillKind::Connection => format!("KILL CONNECTION {id}"),
    };
    Ok(conn.query_drop(sql).await?)
}

// ---------- variables and status ----------

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Global,
    Session,
}

#[derive(Debug, Clone, Serialize)]
pub struct Variable {
    pub name: String,
    pub value: String,
    /// Where the value came from (`COMPILED`, `GLOBAL`, `EXPLICIT`, `DYNAMIC` …), when the server says.
    pub source: Option<String>,
}

pub async fn variables(conn: &mut Conn, scope: Scope) -> Result<Vec<Variable>> {
    let kw = if scope == Scope::Global { "GLOBAL" } else { "SESSION" };
    let rows: Vec<(String, String)> = conn.query(format!("SHOW {kw} VARIABLES")).await?;
    // MySQL 8 can tell where each value was set; older servers and MariaDB cannot.
    let sources: std::collections::HashMap<String, String> = conn
        .query::<(String, String), _>("SELECT VARIABLE_NAME, VARIABLE_SOURCE FROM performance_schema.variables_info")
        .await
        .map(|v| v.into_iter().map(|(n, s)| (n.to_lowercase(), s)).collect())
        .unwrap_or_default();
    Ok(rows.into_iter().map(|(name, value)| Variable { source: sources.get(&name.to_lowercase()).cloned(), name, value }).collect())
}

pub async fn status(conn: &mut Conn, scope: Scope) -> Result<Vec<(String, String)>> {
    let kw = if scope == Scope::Global { "GLOBAL" } else { "SESSION" };
    Ok(conn.query(format!("SHOW {kw} STATUS")).await?)
}

/// `SET GLOBAL|SESSION name = value` as SQL plus bound parameters. The name cannot be bound, so it is
/// restricted to plain identifiers; the value is typed (number, keyword or string) because the server
/// rejects a string for a numeric variable.
pub(crate) fn set_variable_sql(scope: Scope, name: &str, value: &str) -> Result<(String, Vec<Value>)> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(Error::Rejected(format!("`{name}` is not a valid variable name")));
    }
    let kw = if scope == Scope::Global { "GLOBAL" } else { "SESSION" };
    let v = value.trim();
    let upper = v.to_uppercase();
    Ok(if matches!(upper.as_str(), "DEFAULT" | "ON" | "OFF" | "TRUE" | "FALSE") {
        (format!("SET {kw} {name} = {upper}"), vec![])
    } else if let Ok(i) = v.parse::<i64>() {
        (format!("SET {kw} {name} = ?"), vec![Value::Int(i)])
    } else if let Ok(f) = v.parse::<f64>().and_then(|f| if f.is_finite() { Ok(f) } else { "x".parse::<f64>() }) {
        (format!("SET {kw} {name} = ?"), vec![Value::Double(f)])
    } else {
        (format!("SET {kw} {name} = ?"), vec![Value::Bytes(value.as_bytes().to_vec())])
    })
}

/// Set a variable. Returns the server's warnings: MySQL silently adjusts a value it does not like
/// (a non-numeric string for a number becomes the minimum, an oversized one the maximum) and only
/// reports it as a warning, so the caller must show them.
pub async fn set_variable(conn: &mut Conn, scope: Scope, name: &str, value: &str) -> Result<Vec<String>> {
    let (sql, params) = set_variable_sql(scope, name, value)?;
    let result = conn.exec_iter(sql, params).await?;
    let count = result.warnings();
    result.drop_result().await?;
    // The OK packet carries the warning count. Without it, MariaDB's SHOW WARNINGS still shows the diagnostics of an earlier failed statement.
    if count == 0 {
        return Ok(vec![]);
    }
    let warnings: Vec<(String, u32, String)> = conn.query("SHOW WARNINGS").await?;
    Ok(warnings.into_iter().map(|(level, code, message)| format!("{level} {code}: {message}")).collect())
}

#[derive(Debug, Serialize)]
pub struct ServerInfo {
    pub version: String,
    pub version_comment: String,
    pub hostname: String,
    pub port: u32,
    pub datadir: String,
    pub charset: String,
    pub collation: String,
    pub sql_mode: String,
    pub time_zone: String,
    pub max_connections: u64,
    pub uptime_seconds: u64,
    pub ssl_cipher: Option<String>,
}

pub async fn server_info(conn: &mut Conn) -> Result<ServerInfo> {
    let row: Option<(String, String, String, u32, String, String, String, String, String, u64)> = conn
        .query_first("SELECT @@version, @@version_comment, @@hostname, @@port, @@datadir, @@character_set_server, @@collation_server, @@sql_mode, @@global.time_zone, @@max_connections")
        .await?;
    let (version, version_comment, hostname, port, datadir, charset, collation, sql_mode, time_zone, max_connections) = row.ok_or_else(|| unavailable("the server returned no information"))?;
    let st: std::collections::HashMap<String, String> = status(conn, Scope::Global).await?.into_iter().collect();
    let cipher: Option<(String, String)> = conn.query_first("SHOW SESSION STATUS LIKE 'Ssl_cipher'").await?;
    Ok(ServerInfo {
        version, version_comment, hostname, port, datadir, charset, collation, sql_mode, time_zone, max_connections,
        uptime_seconds: st.get("Uptime").and_then(|u| u.parse().ok()).unwrap_or(0),
        ssl_cipher: cipher.map(|c| c.1).filter(|c| !c.is_empty()),
    })
}

// ---------- table maintenance ----------

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckOption {
    Quick,
    Fast,
    Changed,
    Medium,
    Extended,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Maintenance {
    Check { option: Option<CheckOption> },
    Analyze,
    Optimize,
    Repair { quick: bool, extended: bool, use_frm: bool },
}

#[derive(Debug, Clone, Serialize)]
pub struct MaintenanceRow {
    pub table: String,
    pub op: String,
    pub msg_type: String,
    pub msg_text: String,
}

pub(crate) fn maintenance_sql(db: &str, table: &str, op: Maintenance) -> String {
    let t = format!("{}.{}", q(db), q(table));
    match op {
        Maintenance::Check { option } => {
            let o = match option {
                Some(CheckOption::Quick) => " QUICK",
                Some(CheckOption::Fast) => " FAST",
                Some(CheckOption::Changed) => " CHANGED",
                Some(CheckOption::Medium) => " MEDIUM",
                Some(CheckOption::Extended) => " EXTENDED",
                None => "",
            };
            format!("CHECK TABLE {t}{o}")
        }
        Maintenance::Analyze => format!("ANALYZE TABLE {t}"),
        Maintenance::Optimize => format!("OPTIMIZE TABLE {t}"),
        Maintenance::Repair { quick, extended, use_frm } => {
            format!("REPAIR TABLE {t}{}{}{}", if quick { " QUICK" } else { "" }, if extended { " EXTENDED" } else { "" }, if use_frm { " USE_FRM" } else { "" })
        }
    }
}

/// Run a maintenance operation on each table in turn (one statement per table so a failure on one does
/// not hide the others). Every message the server returns becomes a row.
pub async fn maintain(conn: &mut Conn, db: &str, tables: &[String], op: Maintenance) -> Result<Vec<MaintenanceRow>> {
    if tables.is_empty() {
        return Err(Error::Rejected("choose at least one table".into()));
    }
    let mut out = Vec::new();
    for t in tables {
        match conn.query::<(String, String, String, String), _>(maintenance_sql(db, t, op)).await {
            Ok(rows) => out.extend(rows.into_iter().map(|(table, op, msg_type, msg_text)| MaintenanceRow { table, op, msg_type, msg_text })),
            // a statement-level error (e.g. an unknown table in an old server) is reported as a row, not as a failure of the batch
            Err(myq_driver::mysql_async::Error::Server(e)) => out.push(MaintenanceRow { table: format!("{db}.{t}"), op: "error".into(), msg_type: "Error".into(), msg_text: e.message }),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(out)
}

// ---------- logs ----------

#[derive(Debug, Serialize)]
pub struct LogSettings {
    pub general_log: bool,
    pub slow_query_log: bool,
    pub log_output: String,
    pub long_query_time: String,
    pub log_queries_not_using_indexes: bool,
    pub log_error: Option<String>,
    pub slow_query_log_file: Option<String>,
    pub general_log_file: Option<String>,
    pub log_bin: bool,
    pub error_log_in_table: bool,
}

pub async fn log_settings(conn: &mut Conn) -> Result<LogSettings> {
    let vars: std::collections::HashMap<String, String> = variables(conn, Scope::Global).await?.into_iter().map(|v| (v.name.to_lowercase(), v.value)).collect();
    let on = |k: &str| vars.get(k).is_some_and(|v| v == "ON" || v == "1");
    let error_log_in_table = conn.query_first::<u8, _>("SELECT 1 FROM performance_schema.error_log LIMIT 1").await.is_ok();
    Ok(LogSettings {
        general_log: on("general_log"),
        slow_query_log: on("slow_query_log"),
        log_output: vars.get("log_output").cloned().unwrap_or_default(),
        long_query_time: vars.get("long_query_time").cloned().unwrap_or_default(),
        log_queries_not_using_indexes: on("log_queries_not_using_indexes"),
        log_error: vars.get("log_error").cloned(),
        slow_query_log_file: vars.get("slow_query_log_file").cloned(),
        general_log_file: vars.get("general_log_file").cloned(),
        log_bin: on("log_bin"),
        error_log_in_table,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    pub time: String,
    pub level: String,
    pub source: String,
    pub text: String,
}

fn like(s: &str) -> String {
    format!("%{}%", s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"))
}

/// The server error log (`performance_schema.error_log`, MySQL 8.0.22+). `levels` filters by priority.
pub async fn error_log(conn: &mut Conn, limit: u32, levels: &[String], search: &str) -> Result<Vec<LogLine>> {
    let mut sql = String::from("SELECT CAST(LOGGED AS CHAR), PRIO, COALESCE(SUBSYSTEM,''), DATA FROM performance_schema.error_log WHERE 1=1");
    let mut params: Vec<Value> = Vec::new();
    let allowed: Vec<&String> = levels.iter().filter(|l| matches!(l.as_str(), "System" | "Error" | "Warning" | "Note")).collect();
    if !allowed.is_empty() {
        sql.push_str(&format!(" AND PRIO IN ({})", vec!["?"; allowed.len()].join(",")));
        params.extend(allowed.iter().map(|l| Value::Bytes(l.as_bytes().to_vec())));
    }
    if !search.is_empty() {
        sql.push_str(" AND DATA LIKE ?");
        params.push(Value::Bytes(like(search).into_bytes()));
    }
    sql.push_str(&format!(" ORDER BY LOGGED DESC LIMIT {}", limit.clamp(1, 2000)));
    match conn.exec::<(String, String, String, String), _, _>(sql, params).await {
        Ok(rows) => Ok(rows.into_iter().map(|(time, level, source, text)| LogLine { time, level, source, text }).collect()),
        Err(myq_driver::mysql_async::Error::Server(e)) if e.code == 1146 || e.code == 1109 => {
            Err(unavailable("this server version has no error log table (needs MySQL 8.0.22 or later); read the log file named by `log_error` on the server instead"))
        }
        Err(e) => Err(e.into()),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SlowQuery {
    pub start_time: String,
    pub user_host: String,
    pub query_time: String,
    pub lock_time: String,
    pub rows_sent: i64,
    pub rows_examined: i64,
    pub db: String,
    pub sql_text: String,
}

/// The slow query log, when `log_output` includes TABLE.
pub async fn slow_log(conn: &mut Conn, limit: u32, search: &str) -> Result<Vec<SlowQuery>> {
    let out: Option<(String, String)> = conn.query_first("SHOW GLOBAL VARIABLES LIKE 'log_output'").await?;
    if !out.is_some_and(|o| o.1.to_uppercase().contains("TABLE")) {
        return Err(unavailable("slow queries are not written to a table (log_output does not include TABLE); they go to the file named by `slow_query_log_file`"));
    }
    let mut sql = String::from(
        "SELECT CAST(start_time AS CHAR), user_host, CAST(query_time AS CHAR), CAST(lock_time AS CHAR), rows_sent, rows_examined, COALESCE(db,''), CONVERT(sql_text USING utf8mb4) FROM mysql.slow_log",
    );
    let mut params: Vec<Value> = Vec::new();
    if !search.is_empty() {
        sql.push_str(" WHERE CONVERT(sql_text USING utf8mb4) LIKE ?");
        params.push(Value::Bytes(like(search).into_bytes()));
    }
    sql.push_str(&format!(" ORDER BY start_time DESC LIMIT {}", limit.clamp(1, 1000)));
    let rows: Vec<(String, String, String, String, i64, i64, String, String)> = conn.exec(sql, params).await?;
    Ok(rows.into_iter().map(|(start_time, user_host, query_time, lock_time, rows_sent, rows_examined, db, sql_text)| SlowQuery { start_time, user_host, query_time, lock_time, rows_sent, rows_examined, db, sql_text }).collect())
}

/// One normalised statement from the Performance Schema, with how often it ran and how long it took in total.
#[derive(Debug, Clone, Serialize)]
pub struct TopStatement {
    pub db: String,
    /// The statement with its literals replaced by `?`.
    pub digest_text: String,
    pub calls: u64,
    pub total_ms: f64,
    pub avg_ms: f64,
    pub max_ms: f64,
    pub rows_examined: u64,
    pub rows_sent: u64,
    /// How many runs used no index.
    pub no_index: u64,
    pub last_seen: String,
}

/// The statements that cost the most, from `performance_schema.events_statements_summary_by_digest`. `sort` is `total`, `avg`,
/// `calls` or `examined`. Fails with a plain message when the Performance Schema is off.
pub async fn top_statements(conn: &mut Conn, sort: &str, limit: u32, search: &str) -> Result<Vec<TopStatement>> {
    let order = match sort { "avg" => "AVG_TIMER_WAIT", "calls" => "COUNT_STAR", "examined" => "SUM_ROWS_EXAMINED", _ => "SUM_TIMER_WAIT" };
    let mut sql = format!(
        "SELECT COALESCE(SCHEMA_NAME,''), COALESCE(DIGEST_TEXT,''), CAST(COUNT_STAR AS CHAR), CAST(SUM_TIMER_WAIT/1000000000 AS CHAR), CAST(AVG_TIMER_WAIT/1000000000 AS CHAR), CAST(MAX_TIMER_WAIT/1000000000 AS CHAR), \
         CAST(SUM_ROWS_EXAMINED AS CHAR), CAST(SUM_ROWS_SENT AS CHAR), CAST(SUM_NO_INDEX_USED AS CHAR), COALESCE(CAST(LAST_SEEN AS CHAR),'') \
         FROM performance_schema.events_statements_summary_by_digest WHERE DIGEST_TEXT IS NOT NULL"
    );
    let mut params: Vec<Value> = Vec::new();
    if !search.is_empty() {
        sql.push_str(" AND DIGEST_TEXT LIKE ?");
        params.push(Value::Bytes(like(search).into_bytes()));
    }
    sql.push_str(&format!(" ORDER BY {order} DESC LIMIT {}", limit.clamp(1, 500)));
    let rows: Vec<(String, String, String, String, String, String, String, String, String, String)> = conn
        .exec(sql, params)
        .await
        .map_err(|e| unavailable(&format!("the Performance Schema statement summary is not available ({e}). Turn it on with performance_schema=ON in the server settings and restart the server")))?;
    let n = |s: &str| s.parse::<u64>().unwrap_or(0);
    let f = |s: &str| (s.parse::<f64>().unwrap_or(0.0) * 100.0).round() / 100.0;
    Ok(rows.into_iter().map(|(db, digest_text, calls, total, avg, max, ex, sent, noidx, last)| TopStatement { db, digest_text, calls: n(&calls), total_ms: f(&total), avg_ms: f(&avg), max_ms: f(&max), rows_examined: n(&ex), rows_sent: n(&sent), no_index: n(&noidx), last_seen: last }).collect())
}

#[derive(Debug, Clone, Serialize)]
pub struct BinaryLog {
    pub name: String,
    pub size: u64,
}

pub async fn binary_logs(conn: &mut Conn) -> Result<Vec<BinaryLog>> {
    match conn.query::<myq_driver::mysql_async::Row, _>("SHOW BINARY LOGS").await {
        Ok(rows) => Ok(rows
            .into_iter()
            .filter_map(|r| {
                let name: String = r.get(0)?;
                let size: u64 = r.get(1)?;
                Some(BinaryLog { name, size })
            })
            .collect()),
        Err(myq_driver::mysql_async::Error::Server(e)) if e.code == 1381 => Err(unavailable("binary logging is not enabled on this server")),
        Err(e) => Err(e.into()),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BinlogEvent {
    pub pos: u64,
    pub event_type: String,
    pub end_pos: u64,
    pub info: String,
}

/// Events of one binary log file. The file name must be one the server lists, so it cannot smuggle SQL.
pub async fn binlog_events(conn: &mut Conn, file: &str, from_pos: u64, limit: u32) -> Result<Vec<BinlogEvent>> {
    if !binary_logs(conn).await?.iter().any(|l| l.name == file) {
        return Err(Error::Rejected(format!("`{file}` is not a binary log of this server")));
    }
    let sql = format!("SHOW BINLOG EVENTS IN {} FROM {} LIMIT {}", lit(file), from_pos, limit.clamp(1, 1000));
    let rows: Vec<myq_driver::mysql_async::Row> = conn.query(sql).await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| Some(BinlogEvent { pos: r.get(1)?, event_type: r.get(2)?, end_pos: r.get(4)?, info: r.get::<Option<String>, _>(5)?.unwrap_or_default() }))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_variable_types_the_value_and_guards_the_name() {
        let t = |s: &str, n: &str, v: &str| set_variable_sql(s_of(s), n, v).unwrap();
        fn s_of(s: &str) -> Scope { if s == "g" { Scope::Global } else { Scope::Session } }
        assert_eq!(t("g", "max_connections", "250"), ("SET GLOBAL max_connections = ?".into(), vec![Value::Int(250)]));
        assert_eq!(t("s", "long_query_time", "0.5"), ("SET SESSION long_query_time = ?".into(), vec![Value::Double(0.5)]));
        assert_eq!(t("s", "autocommit", " off "), ("SET SESSION autocommit = OFF".into(), vec![]));
        assert_eq!(t("g", "sort_buffer_size", "default"), ("SET GLOBAL sort_buffer_size = DEFAULT".into(), vec![]));
        assert_eq!(t("s", "sql_mode", "STRICT_ALL_TABLES"), ("SET SESSION sql_mode = ?".into(), vec![Value::Bytes(b"STRICT_ALL_TABLES".to_vec())]));
        assert_eq!(t("s", "x", "o'brien; DROP TABLE t").1, vec![Value::Bytes(b"o'brien; DROP TABLE t".to_vec())], "a hostile value is only ever a bound string");
        for bad in ["", "x; DROP TABLE t", "a b", "x`y", "x.y"] {
            assert!(set_variable_sql(Scope::Global, bad, "1").is_err(), "{bad:?}");
        }
        assert!(matches!(set_variable_sql(Scope::Global, "x", "NaN").unwrap().1[0], Value::Bytes(_)), "non-finite numbers stay strings");
    }

    #[test]
    fn maintenance_statements() {
        let sql = |op| maintenance_sql("d`b", "t", op);
        assert_eq!(sql(Maintenance::Check { option: Some(CheckOption::Quick) }), "CHECK TABLE `d``b`.`t` QUICK");
        assert_eq!(sql(Maintenance::Check { option: None }), "CHECK TABLE `d``b`.`t`");
        assert_eq!(sql(Maintenance::Analyze), "ANALYZE TABLE `d``b`.`t`");
        assert_eq!(sql(Maintenance::Optimize), "OPTIMIZE TABLE `d``b`.`t`");
        assert_eq!(sql(Maintenance::Repair { quick: true, extended: false, use_frm: true }), "REPAIR TABLE `d``b`.`t` QUICK USE_FRM");
    }

    #[test]
    fn like_patterns_are_literal() {
        assert_eq!(like("50%_off\\"), "%50\\%\\_off\\\\%");
    }
}
