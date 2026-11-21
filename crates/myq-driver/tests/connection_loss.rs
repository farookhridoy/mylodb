//! The server drops a connection in the middle of a statement (what a network failure or a restart looks like from here):
//! the error reaches the caller, the pool hands out a working connection afterwards, and read-only is still in force on it.

use myq_driver::{execute_script_with_timeout, pool, ConnConfig, SslMode};
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

#[tokio::test]
async fn a_connection_killed_mid_statement_reports_an_error_and_the_pool_recovers() {
    let p = pool(&cfg(true));
    let mut victim = match p.get_conn().await {
        Ok(c) => c,
        Err(e) => return eprintln!("SKIP: no MySQL reachable: {e}"),
    };
    let victim_id = victim.id();
    let killer_pool = pool(&cfg(false));
    let killer = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        let mut k = killer_pool.get_conn().await.unwrap();
        k.query_drop(format!("KILL CONNECTION {victim_id}")).await.unwrap();
    });

    let started = std::time::Instant::now();
    let (results, timed_out) = execute_script_with_timeout(&p, &mut victim, None, "SELECT SLEEP(20); SELECT 2", Some(60)).await;
    killer.await.unwrap();
    assert!(started.elapsed().as_secs() < 10, "the loss is noticed at once, not after the sleep: {:?}", started.elapsed());
    assert!(!timed_out, "this was not the query timeout");
    assert!(results.iter().any(|r| r.error.is_some()), "the caller gets an error, not an empty success: {results:?}");
    assert!(results.iter().all(|r| r.sets.iter().all(|s| s.rows.iter().all(|row| row[0].as_deref() != Some("2"))) ), "the statement after the loss did not run");
    drop(victim);

    // the next connection from the same pool works, and is still read-only
    let mut again = p.get_conn().await.expect("the pool hands out a fresh connection");
    let one: Option<i64> = again.query_first("SELECT 1").await.unwrap();
    assert_eq!(one, Some(1));
    let ro: Option<i64> = again.query_first("SELECT @@transaction_read_only").await.unwrap_or(None);
    assert_eq!(ro, Some(1), "read-only is re-applied to the replacement connection");
}
