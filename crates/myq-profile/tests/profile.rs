//! EXPLAIN and profiling against a local MySQL, in a uniquely named database dropped afterwards.

use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::{pool, ConnConfig, Pool, SslMode};
use myq_profile::*;

fn cfg() -> ConnConfig {
    ConnConfig {
        host: std::env::var("MYLODB_TEST_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port: std::env::var("MYLODB_TEST_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3306),
        user: std::env::var("MYLODB_TEST_USER").unwrap_or_else(|_| "root".into()),
        password: std::env::var("MYLODB_TEST_PASSWORD").ok(),
        database: None,
        ssl: SslMode::Disabled,
        ..Default::default()
    }
}

#[tokio::test]
async fn explain_and_profile_against_a_real_server() {
    let pool = pool(&cfg());
    let mut admin = match pool.get_conn().await {
        Ok(c) => c,
        Err(e) => return eprintln!("SKIP: no MySQL reachable: {e}"),
    };
    let db = format!("mylodb_itest_prof_{}", std::process::id());
    admin.query_drop(format!("CREATE DATABASE `{db}`")).await.unwrap();
    let outcome = tokio::spawn(body(pool.clone(), db.clone())).await;
    admin.query_drop(format!("DROP DATABASE `{db}`")).await.unwrap();
    outcome.unwrap();
}

async fn body(pool: Pool, db: String) {
    let db = db.as_str();
    let mut c = pool.get_conn().await.unwrap();
    c.query_drop(format!("USE `{db}`")).await.unwrap();
    c.query_drop("CREATE TABLE t (id INT PRIMARY KEY, grp INT, note VARCHAR(40), INDEX by_grp (grp))").await.unwrap();
    // 20 000 rows without needing a helper table (the recursion limit must allow them)
    // recursive CTE depth: MySQL calls the limit cte_max_recursion_depth, MariaDB max_recursive_iterations
    if c.query_drop("SET SESSION cte_max_recursion_depth = 30000").await.is_err() {
        c.query_drop("SET SESSION max_recursive_iterations = 30000").await.unwrap();
    }
    c.query_drop("INSERT INTO t SELECT n, n % 50, CONCAT('note-', n) FROM (WITH RECURSIVE s(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM s WHERE n < 20000) SELECT n FROM s) q").await.unwrap_or_else(|e| panic!("{e}"));
    c.query_drop("ANALYZE TABLE t").await.unwrap();

    // ---- EXPLAIN: a full scan is called out, an indexed lookup is not ----
    let scan = explain(&mut c, Some(db), "SELECT * FROM t WHERE note LIKE '%99%'", false).await.unwrap();
    assert_eq!(scan.plan.rows.len(), 1);
    assert!(scan.hints.iter().any(|h| h.severity == Severity::Warning && h.message.contains("Full table scan on `t`")), "{:?}", scan.hints);
    let doc: serde_json::Value = serde_json::from_str(scan.json.as_deref().expect("json plan")).unwrap();
    // MySQL 8 uses `query_block`; 9.x (JSON format v2) uses `query_plan`. The UI renders either generically.
    assert!(doc.get("query_block").is_some() || doc.get("query_plan").is_some(), "unexpected plan document: {doc}");
    assert!(scan.tree.is_none(), "no ANALYZE unless asked");

    let point = explain(&mut c, Some(db), "SELECT * FROM t WHERE id = 7", false).await.unwrap();
    assert!(point.hints.iter().all(|h| h.severity != Severity::Warning), "{:?}", point.hints);

    let sorted = explain(&mut c, Some(db), "SELECT * FROM t ORDER BY note", false).await.unwrap();
    assert!(sorted.hints.iter().any(|h| h.message.contains("filesort")), "{:?}", sorted.hints);
    let grouped = explain(&mut c, Some(db), "SELECT note, COUNT(*) FROM t GROUP BY note", false).await.unwrap();
    assert!(grouped.hints.iter().any(|h| h.message.contains("temporary") || h.message.contains("Full table scan")), "{:?}", grouped.hints);

    // plain EXPLAIN of a write is fine and does not run it
    let upd = explain(&mut c, Some(db), "DELETE FROM t WHERE id > 0", false).await.unwrap();
    assert_eq!(upd.plan.rows.len(), 1);
    let n: Option<i64> = c.query_first("SELECT COUNT(*) FROM t").await.unwrap();
    assert_eq!(n, Some(20000), "EXPLAIN must not execute the statement");

    // ANALYZE executes, so it is read-only; it returns the timed iterator tree
    let an = explain(&mut c, Some(db), "SELECT COUNT(*) FROM t WHERE grp = 3", true).await.unwrap();
    // MySQL prints "actual time" in the iterator tree; MariaDB's ANALYZE FORMAT=JSON has r_total_time_ms
    assert!(an.tree.as_deref().is_some_and(|t| t.contains("actual time") || t.contains("r_total_time_ms")), "{:?}", an.tree);
    assert!(matches!(explain(&mut c, Some(db), "DELETE FROM t", true).await, Err(Error::Rejected(_))));
    let n: Option<i64> = c.query_first("SELECT COUNT(*) FROM t").await.unwrap();
    assert_eq!(n, Some(20000), "the rejected ANALYZE did not run the delete");

    // rejections and errors
    assert!(matches!(explain(&mut c, Some(db), "SELECT 1; SELECT 2", false).await, Err(Error::Rejected(_))));
    assert!(matches!(explain(&mut c, Some(db), "DROP TABLE t", false).await, Err(Error::Rejected(_))));
    assert!(matches!(explain(&mut c, Some(db), "SELECT * FROM nope", false).await, Err(Error::Server(m)) if m.contains("nope")));

    // ---- profile: work done and stage timings ----
    let full = profile(&mut c, Some(db), "SELECT COUNT(*) FROM t WHERE note LIKE '%99%'").await.unwrap();
    let get = |p: &ProfileResult, n: &str| p.counters.iter().find(|c| c.name == n).map(|c| c.value).unwrap_or(0);
    assert_eq!(full.rows_returned, 1);
    // MariaDB has no per-session Innodb_rows_read; its handler counter shows the same scan
    assert!(get(&full, "Innodb_rows_read").max(get(&full, "Handler_read_rnd_next")) >= 15000, "a full scan reads every row: {:?}", full.counters);
    assert!(!full.stages.is_empty(), "stages: {:?} note: {:?}", full.stages, full.stages_note);
    assert!(full.stages.iter().all(|s| s.ms >= 0.0));

    let point = profile(&mut c, Some(db), "SELECT * FROM t WHERE id = 7").await.unwrap();
    assert!(get(&point, "Innodb_rows_read") < 50, "an index lookup reads a handful of rows: {:?}", point.counters);
    assert!(get(&point, "Handler_read_key") >= 1, "{:?}", point.counters);
    // the measurement's own overhead is subtracted: an idle SELECT 1 shows no row reads
    let idle = profile(&mut c, Some(db), "SELECT 1").await.unwrap();
    assert_eq!(get(&idle, "Innodb_rows_read"), 0, "{:?}", idle.counters);

    let sort = profile(&mut c, Some(db), "SELECT * FROM t ORDER BY note LIMIT 10").await.unwrap();
    assert!(get(&sort, "Sort_rows") > 0, "{:?}", sort.counters);

    assert!(matches!(profile(&mut c, Some(db), "DELETE FROM t").await, Err(Error::Rejected(_))));
    assert!(matches!(profile(&mut c, Some(db), "SELECT * FROM nope").await, Err(Error::Server(_))));
    // the connection is left sane after a failed profile
    let r = profile(&mut c, Some(db), "SELECT 2").await.unwrap();
    assert_eq!(r.rows_returned, 1);
}
