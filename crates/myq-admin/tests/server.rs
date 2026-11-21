//! Diagnostics against a local MySQL. Only touches connections and objects the test creates itself;
//! GLOBAL variables are never changed (SESSION scope is used instead).

use myq_admin::*;
use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::{pool, ConnConfig, Pool, SslMode};
use std::time::{Duration, Instant};

fn cfg() -> ConnConfig {
    ConnConfig {
        host: std::env::var("MYLODB_TEST_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port: std::env::var("MYLODB_TEST_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3306),
        user: std::env::var("MYLODB_TEST_USER").unwrap_or_else(|_| "root".into()),
        password: std::env::var("MYLODB_TEST_PASSWORD").ok(),
        ssl: SslMode::Disabled,
        ..Default::default()
    }
}

#[tokio::test]
async fn processes_variables_maintenance_and_logs() {
    let pool = pool(&cfg());
    let mut admin = match pool.get_conn().await {
        Ok(c) => c,
        Err(e) => return eprintln!("SKIP: no MySQL reachable: {e}"),
    };
    let db = format!("mylodb_itest_srv_{}", std::process::id());
    admin.query_drop(format!("CREATE DATABASE `{db}`")).await.unwrap();
    let outcome = tokio::spawn(body(pool.clone(), db.clone())).await;
    admin.query_drop(format!("DROP DATABASE `{db}`")).await.unwrap();
    outcome.unwrap();
}

async fn body(pool: Pool, db: String) {
    let db = db.as_str();
    let mut c = pool.get_conn().await.unwrap();

    // ---------- server info ----------
    let info = server_info(&mut c).await.unwrap();
    assert!(!info.version.is_empty() && info.max_connections > 0 && info.uptime_seconds > 0 && !info.charset.is_empty());
    assert!(info.ssl_cipher.is_none(), "this connection is plain");

    // ---------- processes: find a sleeping query, stop it, then close its connection ----------
    let marker = format!("/* mylodb_itest_{} */", std::process::id());
    let mut victim = pool.get_conn().await.unwrap();
    let victim_id = victim.id() as u64;
    let sleeper = tokio::spawn(async move {
        let started = Instant::now();
        let r: std::result::Result<Option<i64>, myq_driver::mysql_async::Error> = victim.query_first(format!("SELECT SLEEP(20) {marker}")).await;
        (victim, r, started.elapsed())
    });
    let mut seen = None;
    for _ in 0..40 {
        let list = processlist(&mut c).await.unwrap();
        assert_eq!(list.self_id, c.id() as u64);
        assert!(list.processes.iter().any(|p| p.id == list.self_id), "the monitoring connection is listed");
        if let Some(p) = list.processes.into_iter().find(|p| p.id == victim_id && p.info.as_deref().is_some_and(|i| i.contains("mylodb_itest_"))) {
            seen = Some(p);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let p = seen.expect("the sleeping statement shows up in the process list");
    assert_eq!(p.command, "Query");
    assert!(p.info.unwrap().contains("SLEEP(20)"));

    let me = c.id() as u64;
    assert!(matches!(kill(&mut c, me, KillKind::Query).await, Err(Error::Rejected(_))), "cannot kill the monitoring connection");
    kill(&mut c, victim_id, KillKind::Query).await.unwrap();
    let (mut victim, result, elapsed) = sleeper.await.unwrap();
    assert!(elapsed < Duration::from_secs(10), "KILL QUERY must interrupt the statement (took {elapsed:?})");
    let _ = result; // MySQL reports an interrupted SLEEP as 1 or as an error depending on version
    let alive: Option<i64> = victim.query_first("SELECT 1").await.unwrap();
    assert_eq!(alive, Some(1), "KILL QUERY leaves the connection usable");

    kill(&mut c, victim_id, KillKind::Connection).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(processlist(&mut c).await.unwrap().processes.iter().all(|p| p.id != victim_id), "the connection is gone");
    assert!(victim.query_first::<i64, _>("SELECT 1").await.is_err());
    assert!(kill(&mut c, 4_000_000_000, KillKind::Query).await.is_err(), "unknown thread id is an error");

    // ---------- variables (SESSION scope only) ----------
    let g = variables(&mut c, VarScope::Global).await.unwrap();
    assert!(g.iter().any(|v| v.name == "max_connections") && g.iter().any(|v| v.name == "version"));
    assert!(g.len() > 200);
    set_variable(&mut c, VarScope::Session, "sort_buffer_size", "262144").await.unwrap();
    let v: Option<u64> = c.query_first("SELECT @@session.sort_buffer_size").await.unwrap();
    assert_eq!(v, Some(262144));
    set_variable(&mut c, VarScope::Session, "autocommit", "OFF").await.unwrap();
    let v: Option<i64> = c.query_first("SELECT @@session.autocommit").await.unwrap();
    assert_eq!(v, Some(0));
    set_variable(&mut c, VarScope::Session, "autocommit", "default").await.unwrap();
    set_variable(&mut c, VarScope::Session, "sql_mode", "STRICT_ALL_TABLES").await.unwrap();
    let v: Option<String> = c.query_first("SELECT @@session.sql_mode").await.unwrap();
    assert_eq!(v.as_deref(), Some("STRICT_ALL_TABLES"));
    set_variable(&mut c, VarScope::Session, "sql_mode", "default").await.unwrap();
    let session = variables(&mut c, VarScope::Session).await.unwrap();
    assert_eq!(session.iter().find(|v| v.name == "sort_buffer_size").unwrap().value, "262144");
    assert!(matches!(set_variable(&mut c, VarScope::Session, "x; DROP TABLE t", "1").await, Err(Error::Rejected(_))));
    assert!(set_variable(&mut c, VarScope::Session, "no_such_variable_xyz", "1").await.is_err());
    assert!(set_variable(&mut c, VarScope::Session, "version", "1").await.is_err(), "read-only variables are refused by the server");
    // the server adjusts a nonsense value and only warns: those warnings must reach the caller
    // MySQL adjusts it and warns; MariaDB refuses it with error 1232. Either way the caller hears about it.
    match set_variable(&mut c, VarScope::Session, "sort_buffer_size", "not-a-number").await {
        Ok(warned) => assert!(warned.iter().any(|w| w.to_lowercase().contains("incorrect") || w.to_lowercase().contains("truncated")), "{warned:?}"),
        Err(e) => assert!(e.to_string().to_lowercase().contains("incorrect argument type"), "{e}"),
    }
    let good = set_variable(&mut c, VarScope::Session, "sort_buffer_size", "262144").await.unwrap();
    assert!(good.is_empty(), "a good value has no warnings: {good:?}");
    let after_error: Option<i64> = c.query_first("SELECT 1").await.unwrap();
    assert_eq!(after_error, Some(1), "failed SETs leave the connection usable");

    // ---------- status ----------
    let st = status(&mut c, VarScope::Global).await.unwrap();
    assert!(st.iter().any(|(k, v)| k == "Uptime" && v.parse::<u64>().is_ok()));
    assert!(st.iter().any(|(k, _)| k == "Threads_connected") && st.iter().any(|(k, _)| k == "Questions"));
    let s1 = status(&mut c, VarScope::Session).await.unwrap();
    assert!(s1.iter().any(|(k, _)| k == "Bytes_sent"));

    // ---------- maintenance ----------
    c.query_drop(format!("CREATE TABLE `{db}`.m (id INT PRIMARY KEY, v VARCHAR(20))")).await.unwrap();
    c.query_drop(format!("INSERT INTO `{db}`.m VALUES (1,'a'),(2,'b')")).await.unwrap();
    c.query_drop(format!("CREATE TABLE `{db}`.m2 (id INT PRIMARY KEY)")).await.unwrap();
    let tables = vec!["m".to_string(), "m2".to_string()];
    for (op, expect_op) in [
        (Maintenance::Check { option: Some(CheckOption::Quick) }, "check"),
        (Maintenance::Check { option: None }, "check"),
        (Maintenance::Analyze, "analyze"),
        (Maintenance::Optimize, "optimize"),
    ] {
        let rows = maintain(&mut c, db, &tables, op).await.unwrap();
        assert!(rows.iter().any(|r| r.table == format!("{db}.m")) && rows.iter().any(|r| r.table == format!("{db}.m2")), "a row per table: {rows:?}");
        assert!(rows.iter().all(|r| r.op == expect_op), "{rows:?}");
        assert!(rows.iter().any(|r| r.msg_type == "status" && r.msg_text == "OK"), "{rows:?}");
    }
    // MySQL cannot repair InnoDB: the server says so, and it is reported, not hidden. MariaDB rebuilds the table and answers OK.
    let rows = maintain(&mut c, db, &["m".into()], Maintenance::Repair { quick: false, extended: false, use_frm: false }).await.unwrap();
    assert!(
        rows.iter().any(|r| r.msg_text.to_lowercase().contains("doesn't support repair") || r.msg_text.to_lowercase().contains("does not support") || (r.msg_type == "status" && r.msg_text == "OK")),
        "{rows:?}"
    );
    // a missing table is one failed row, and the other tables are still processed
    let rows = maintain(&mut c, db, &["nope".into(), "m".into()], Maintenance::Analyze).await.unwrap();
    assert!(rows.iter().any(|r| r.table.ends_with(".nope") && (r.msg_type == "Error" || r.msg_text.contains("doesn't exist"))), "{rows:?}");
    assert!(rows.iter().any(|r| r.table.ends_with(".m") && r.msg_text == "OK"), "{rows:?}");
    assert!(matches!(maintain(&mut c, db, &[], Maintenance::Analyze).await, Err(Error::Rejected(_))));
    // identifiers are quoted: a hostile table name cannot add a statement
    let rows = maintain(&mut c, db, &["m`; DROP TABLE m2; --".into()], Maintenance::Analyze).await.unwrap();
    assert!(rows.iter().all(|r| r.msg_type != "status" || r.msg_text != "OK"));
    let n: Option<i64> = c.query_first(format!("SELECT COUNT(*) FROM `{db}`.m2")).await.unwrap();
    assert_eq!(n, Some(0), "m2 still exists");

    // ---------- logs (read-only) ----------
    let ls = log_settings(&mut c).await.unwrap();
    assert!(!ls.log_output.is_empty());
    match error_log(&mut c, 20, &[], "").await {
        Ok(lines) => {
            assert!(lines.len() <= 20);
            assert!(lines.iter().all(|l| !l.time.is_empty() && !l.level.is_empty()));
            let errors = error_log(&mut c, 50, &["Error".into()], "").await.unwrap();
            assert!(errors.iter().all(|l| l.level == "Error"));
            let none = error_log(&mut c, 50, &[], "zz_no_such_text_%_zz").await.unwrap();
            assert!(none.is_empty());
            // `%` in a search is literal, so it does not match everything
            if !lines.is_empty() {
                assert!(error_log(&mut c, 50, &[], "%").await.unwrap().len() < lines.len().max(50) || lines.iter().all(|l| l.text.contains('%')));
            }
            assert!(ls.error_log_in_table);
        }
        Err(Error::Unavailable(_)) => assert!(!ls.error_log_in_table),
        Err(e) => panic!("{e}"),
    }
    match slow_log(&mut c, 10, "").await {
        Ok(_) => assert!(ls.log_output.to_uppercase().contains("TABLE")),
        Err(Error::Unavailable(m)) => assert!(m.contains("log_output"), "{m}"),
        Err(e) => panic!("{e}"),
    }
    match binary_logs(&mut c).await {
        Ok(logs) => {
            assert!(!logs.is_empty() && logs.iter().all(|l| !l.name.is_empty()));
            let first = &logs[0].name;
            let events = binlog_events(&mut c, first, 4, 5).await.unwrap();
            assert!(events.len() <= 5 && events.iter().all(|e| e.end_pos >= e.pos));
            assert!(matches!(binlog_events(&mut c, "x'; DROP TABLE t; --", 4, 5).await, Err(Error::Rejected(_))), "only listed files are accepted");
            assert!(ls.log_bin);
        }
        Err(Error::Unavailable(m)) => assert!(m.contains("binary logging"), "{m}"),
        Err(e) => panic!("{e}"),
    }
}
