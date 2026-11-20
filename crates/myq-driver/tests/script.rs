//! Script execution against a local MySQL; uses its own uniquely named database.

use myq_driver::{execute_script, kill_query, pool, ConnConfig, SslMode};
use mysql_async::prelude::Queryable;
use std::time::{Duration, Instant};

fn cfg() -> ConnConfig {
    ConnConfig {
        host: std::env::var("MYLODB_TEST_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port: std::env::var("MYLODB_TEST_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3306),
        user: std::env::var("MYLODB_TEST_USER").unwrap_or_else(|_| "root".into()),
        password: std::env::var("MYLODB_TEST_PASSWORD").ok(),
        database: None,
        ssl: SslMode::Disabled,
    }
}

#[tokio::test]
async fn runs_scripts_reports_errors_and_cancels() {
    let pool = pool(&cfg());
    let mut admin = match pool.get_conn().await {
        Ok(c) => c,
        Err(e) => return eprintln!("SKIP: no MySQL reachable: {e}"),
    };
    let db = format!("mylodb_itest_run_{}", std::process::id());
    admin.query_drop(format!("CREATE DATABASE `{db}`")).await.unwrap();

    // Run the body in a task so the database is dropped even when an assertion fails.
    let body = tokio::spawn(body(pool.clone(), db.clone()));
    let outcome = body.await;
    admin.query_drop(format!("DROP DATABASE `{db}`")).await.unwrap();
    outcome.unwrap();
}

async fn body(pool: myq_driver::Pool, db: String) {
    let db = db.as_str();
    let mut conn = pool.get_conn().await.unwrap();
    // Multi-statement script with DML, a result set, NULL vs empty string, and a multi-result CALL.
    let r = execute_script(
        &mut conn,
        Some(db),
        "CREATE TABLE t (id INT PRIMARY KEY AUTO_INCREMENT, v VARCHAR(10) NULL);
         INSERT INTO t (v) VALUES ('a'), (NULL), ('');
         SELECT id, v FROM t ORDER BY id;
         DELIMITER $$
         CREATE PROCEDURE two() BEGIN SELECT 1 AS a; SELECT 2 AS b; END$$
         DELIMITER ;
         CALL two();",
    )
    .await;
    assert!(r.iter().all(|s| s.error.is_none()), "{r:?}");
    assert_eq!(r[1].affected_rows, 3);
    assert_eq!(r[1].last_insert_id, Some(1));
    let rows = &r[2].sets[0].rows;
    assert_eq!(rows[0], [Some("1".into()), Some("a".into())]);
    assert_eq!(rows[1][1], None, "NULL must stay distinct from empty string");
    assert_eq!(rows[2][1], Some(String::new()));
    assert_eq!(r[4].sets.len(), 2, "CALL returns two result sets");

    // Error stops the script; earlier results are kept.
    let r = execute_script(&mut conn, Some(db), "SELECT 1; SELECT * FROM missing; SELECT 3;").await;
    assert_eq!(r.len(), 2);
    assert!(r[1].error.as_deref().unwrap().contains("missing"));

    // Cancellation: KILL QUERY interrupts a long statement and the connection stays usable.
    let id = conn.id();
    let killer = pool.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        kill_query(&killer, id).await.unwrap();
    });
    let t = Instant::now();
    let r = execute_script(&mut conn, None, "SELECT SLEEP(20)").await;
    assert!(t.elapsed() < Duration::from_secs(10), "was not interrupted");
    // MySQL reports SLEEP interrupted as a normal row (1) or error 1317 depending on version.
    assert!(r[0].error.is_some() || r[0].sets[0].rows[0][0].as_deref() == Some("1"));
    let r = execute_script(&mut conn, None, "SELECT 42").await;
    assert_eq!(r[0].sets[0].rows[0][0].as_deref(), Some("42"));

}
