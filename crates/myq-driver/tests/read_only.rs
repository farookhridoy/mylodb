//! A read-only connection is read-only on the server. Creates and drops only its own uniquely named database.

use myq_driver::{pool, ConnConfig, SslMode};
use mysql_async::prelude::Queryable;

fn cfg(read_only: bool) -> ConnConfig {
    ConnConfig {
        host: std::env::var("MYLODB_TEST_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port: std::env::var("MYLODB_TEST_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3306),
        user: std::env::var("MYLODB_TEST_USER").unwrap_or_else(|_| "root".into()),
        password: std::env::var("MYLODB_TEST_PASSWORD").ok(),
        database: None,
        ssl: SslMode::Disabled,
        read_only,
        ..Default::default()
    }
}

fn server_code(e: &mysql_async::Error) -> Option<u16> {
    match e {
        mysql_async::Error::Server(s) => Some(s.code),
        _ => None,
    }
}

#[tokio::test]
async fn a_read_only_connection_refuses_writes_and_schema_changes() {
    let rw = pool(&cfg(false));
    let mut admin = match rw.get_conn().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP: no MySQL reachable: {e}");
            return;
        }
    };
    let db = format!("mylodb_itest_ro_{}", std::process::id());
    admin.query_drop(format!("CREATE DATABASE `{db}`")).await.unwrap();
    // run the checks in a task so that a failed assertion cannot skip the cleanup below
    let (cfg_rw, cfg_ro, dbn) = (cfg(false), cfg(true), db.clone());
    let handle = tokio::spawn(async move { check(cfg_rw, cfg_ro, dbn).await });
    let outcome = handle.await;
    admin.query_drop(format!("DROP DATABASE `{db}`")).await.unwrap();
    outcome.unwrap().unwrap();
}

async fn check(cfg_rw: ConnConfig, cfg_ro: ConnConfig, db: String) -> Result<(), mysql_async::Error> {
    let rw = pool(&cfg_rw);
    let mut admin = rw.get_conn().await?;
    {
        admin.query_drop(format!("CREATE TABLE `{db}`.t (id INT PRIMARY KEY, v VARCHAR(10))")).await?;
        admin.query_drop(format!("INSERT INTO `{db}`.t VALUES (1, 'a')")).await?;

        let ro = pool(&cfg_ro);
        // reads work, on every connection the pool opens
        for _ in 0..3 {
            let mut c = ro.get_conn().await?;
            let n: Option<i64> = c.query_first(format!("SELECT COUNT(*) FROM `{db}`.t")).await?;
            assert_eq!(n, Some(1));
            let ro_flag: Option<i64> = c.query_first("SELECT @@session.transaction_read_only").await?;
            assert_eq!(ro_flag, Some(1), "the session is read-only");
        }
        let mut c = ro.get_conn().await?;
        for (what, sql) in [
            ("INSERT", format!("INSERT INTO `{db}`.t VALUES (2, 'b')")),
            ("UPDATE", format!("UPDATE `{db}`.t SET v = 'z'")),
            ("DELETE", format!("DELETE FROM `{db}`.t")),
            ("CREATE TABLE", format!("CREATE TABLE `{db}`.u (a INT)")),
            ("ALTER", format!("ALTER TABLE `{db}`.t ADD COLUMN x INT")),
            ("TRUNCATE", format!("TRUNCATE TABLE `{db}`.t")),
            ("DROP TABLE", format!("DROP TABLE `{db}`.t")),
        ] {
            let err = c.query_drop(sql).await.expect_err(&format!("{what} must be refused"));
            assert_eq!(server_code(&err), Some(1792), "{what}: {err}"); // ER_CANT_EXECUTE_IN_READ_ONLY_TRANSACTION
        }
        // nothing changed
        let n: Option<i64> = admin.query_first(format!("SELECT COUNT(*) FROM `{db}`.t")).await?;
        assert_eq!(n, Some(1));
        // the read-write connection is unaffected
        admin.query_drop(format!("INSERT INTO `{db}`.t VALUES (2, 'b')")).await?;
        // an explicit read-only transaction is still fine
        c.query_drop("START TRANSACTION READ ONLY").await?;
        c.query_drop("COMMIT").await?;
    }
    Ok(())
}

#[tokio::test]
async fn the_estimate_for_a_delete_comes_from_explain_without_running_it() {
    let rw = pool(&cfg(false));
    let mut c = match rw.get_conn().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP: no MySQL reachable: {e}");
            return;
        }
    };
    let db = format!("mylodb_itest_est_{}", std::process::id());
    c.query_drop(format!("CREATE DATABASE `{db}`")).await.unwrap();
    let handle = {
        let (cfg_rw, dbn) = (cfg(false), db.clone());
        tokio::spawn(async move {
            let rw = pool(&cfg_rw);
            let mut c = rw.get_conn().await?;
            c.query_drop(format!("CREATE TABLE `{dbn}`.t (id INT PRIMARY KEY, v INT)")).await?;
            c.query_drop(format!("INSERT INTO `{dbn}`.t (id, v) SELECT seq, seq FROM (SELECT 1 AS seq UNION SELECT 2 UNION SELECT 3 UNION SELECT 4 UNION SELECT 5) s")).await?;
            let all = myq_driver::estimate_rows(&mut c, Some(&dbn), "DELETE FROM t").await?;
            let one = myq_driver::estimate_rows(&mut c, Some(&dbn), "DELETE FROM t WHERE id = 3;").await?;
            let bad = myq_driver::estimate_rows(&mut c, Some(&dbn), "DELETE FROM missing_table").await?;
            let n: Option<i64> = c.query_first(format!("SELECT COUNT(*) FROM `{dbn}`.t")).await?;
            Ok::<_, myq_driver::Error>((all, one, bad, n))
        })
    };
    let out = handle.await;
    c.query_drop(format!("DROP DATABASE `{db}`")).await.unwrap();
    let (all, one, bad, n) = out.unwrap().unwrap();
    assert_eq!(n, Some(5), "EXPLAIN did not delete anything");
    assert_eq!(all, Some(5), "a full delete touches every row");
    assert_eq!(one, Some(1), "a primary-key lookup touches one row");
    assert_eq!(bad, None, "an unknown table cannot be explained");
}

#[tokio::test]
async fn a_run_that_exceeds_the_timeout_is_stopped_and_marked() {
    let p = pool(&cfg(false));
    let mut conn = match p.get_conn().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP: no MySQL reachable: {e}");
            return;
        }
    };
    // too slow: stopped after about a second, long before the 20 s sleep ends
    let started = std::time::Instant::now();
    let (results, timed_out) = myq_driver::execute_script_with_timeout(&p, &mut conn, None, "SELECT SLEEP(20)", Some(1)).await;
    assert!(timed_out);
    assert!(started.elapsed() < std::time::Duration::from_secs(10), "stopped early: {:?}", started.elapsed());
    let msg = results.last().and_then(|r| r.error.clone()).unwrap_or_default();
    assert!(msg.contains("1 s query timeout"), "{msg}");

    // the connection is still usable afterwards
    let (ok, timed_out) = myq_driver::execute_script_with_timeout(&p, &mut conn, None, "SELECT 1 AS one", Some(5)).await;
    assert!(!timed_out);
    assert!(ok[0].error.is_none() && ok[0].sets[0].rows[0][0].as_deref() == Some("1"), "{ok:?}");

    // no limit: never marked
    let (ok, timed_out) = myq_driver::execute_script_with_timeout(&p, &mut conn, None, "SELECT SLEEP(0.1)", None).await;
    assert!(!timed_out && ok[0].error.is_none());

    // an error in the script is kept as it is when the timeout did not fire
    let (bad, timed_out) = myq_driver::execute_script_with_timeout(&p, &mut conn, None, "SELEC 1", Some(5)).await;
    assert!(!timed_out && bad[0].error.is_some());
}
