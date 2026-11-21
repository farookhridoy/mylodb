//! Data search against a real server. Creates and drops only its own `mylodb_itest_search_*` database.

use myq_data::{search_database, SearchRequest};
use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::{pool, ConnConfig, SslMode};
use std::sync::atomic::{AtomicUsize, Ordering};

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

fn req(needle: &str) -> SearchRequest {
    SearchRequest { needle: needle.into(), exact: false, all_columns: false, tables: None }
}

#[tokio::test]
async fn data_search_finds_values_per_table_and_column() {
    let p = pool(&cfg());
    let mut c = match p.get_conn().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP: no MySQL reachable: {e}");
            return;
        }
    };
    let db = format!("mylodb_itest_search_{}", std::process::id());
    c.query_drop(format!("CREATE DATABASE `{db}`")).await.unwrap();
    let outcome = async {
        for sql in [
            format!("CREATE TABLE `{db}`.customers (id INT PRIMARY KEY, name VARCHAR(50), note TEXT, code CHAR(6), n INT)"),
            format!("INSERT INTO `{db}`.customers VALUES (1, 'Ann Needle', 'likes needles', 'AB_12', 42), (2, 'Bob', NULL, 'ABC12', 7), (3, '100% sure', 'x', 'Z', 5)"),
            format!("CREATE TABLE `{db}`.orders (id INT PRIMARY KEY, ref VARCHAR(20), qty INT)"),
            format!("INSERT INTO `{db}`.orders VALUES (1, 'needle-1', 42), (2, 'other', 42)"),
            format!("CREATE TABLE `{db}`.blobs (id INT PRIMARY KEY, data BLOB)"),
            format!("INSERT INTO `{db}`.blobs VALUES (1, 'needle')"),
            format!("CREATE TABLE `{db}`.numbers_only (id INT PRIMARY KEY, v INT)"),
            format!("INSERT INTO `{db}`.numbers_only VALUES (1, 42)"),
            format!("CREATE VIEW `{db}`.v AS SELECT name FROM `{db}`.customers"),
        ] {
            c.query_drop(sql).await.map_err(|e| format!("setup: {e}"))?;
        }
        let never = || false;
        let mut seen = Vec::new();
        let mut progress = |d: usize, t: usize| seen.push((d, t));

        // contains, case-insensitive under the default collation; per-column counts; blobs are never searched; views are not searched
        let r = search_database(&mut c, &db, &req("needle"), &never, &mut progress).await.map_err(|e| e.to_string())?;
        let hit = |t: &str| r.hits.iter().find(|h| h.table == t).cloned();
        let cust = hit("customers").ok_or("customers should match")?;
        assert_eq!((cust.rows, cust.columns.clone()), (1, vec![("name".to_string(), 1), ("note".to_string(), 1)]));
        assert_eq!(hit("orders").unwrap().columns, vec![("ref".to_string(), 1)]);
        assert!(hit("blobs").is_none() && hit("numbers_only").is_none());
        assert!(r.skipped.iter().any(|s| s.starts_with("blobs:")) && r.skipped.iter().any(|s| s.starts_with("numbers_only:")), "{:?}", r.skipped);
        assert_eq!((r.tables_total, r.tables_scanned, r.cancelled), (4, 4, false));
        assert_eq!(seen.last(), Some(&(4, 4)));

        // LIKE wildcards in the typed text are literal
        let under = search_database(&mut c, &db, &req("B_1"), &never, &mut |_, _| {}).await.map_err(|e| e.to_string())?;
        assert_eq!(under.hits.iter().map(|h| (h.table.as_str(), h.rows)).collect::<Vec<_>>(), [("customers", 1)], "`_` is not a wildcard: ABC12 must not match B_1");
        let pct = search_database(&mut c, &db, &req("100%"), &never, &mut |_, _| {}).await.map_err(|e| e.to_string())?;
        assert_eq!(pct.hits.len(), 1);
        let only_pct = search_database(&mut c, &db, &req("%"), &never, &mut |_, _| {}).await.map_err(|e| e.to_string())?;
        assert_eq!(only_pct.hits.iter().map(|h| h.rows).sum::<u64>(), 1, "`%` is not a wildcard");

        // exact
        let exact = search_database(&mut c, &db, &SearchRequest { exact: true, ..req("Bob") }, &never, &mut |_, _| {}).await.map_err(|e| e.to_string())?;
        assert_eq!(exact.hits.len(), 1);
        let partial = search_database(&mut c, &db, &SearchRequest { exact: true, ..req("Bo") }, &never, &mut |_, _| {}).await.map_err(|e| e.to_string())?;
        assert!(partial.hits.is_empty());

        // numbers only with all_columns
        let nums = search_database(&mut c, &db, &req("42"), &never, &mut |_, _| {}).await.map_err(|e| e.to_string())?;
        assert!(nums.hits.is_empty(), "text columns only by default: {:?}", nums.hits);
        let all = search_database(&mut c, &db, &SearchRequest { all_columns: true, exact: true, ..req("42") }, &never, &mut |_, _| {}).await.map_err(|e| e.to_string())?;
        let tables: Vec<&str> = all.hits.iter().map(|h| h.table.as_str()).collect();
        assert_eq!(tables, ["customers", "numbers_only", "orders"]);
        assert_eq!(all.hits.iter().find(|h| h.table == "orders").unwrap().rows, 2);

        // only the chosen tables
        let some = search_database(&mut c, &db, &SearchRequest { tables: Some(vec!["orders".into()]), ..req("needle") }, &never, &mut |_, _| {}).await.map_err(|e| e.to_string())?;
        assert_eq!((some.tables_total, some.hits.len()), (1, 1));

        // cancelling between tables keeps what was found so far
        let calls = AtomicUsize::new(0);
        let cancel_after_two = || calls.fetch_add(1, Ordering::Relaxed) >= 2; // tables in name order: blobs, customers, ...
        let part = search_database(&mut c, &db, &req("needle"), &cancel_after_two, &mut |_, _| {}).await.map_err(|e| e.to_string())?;
        assert!(part.cancelled && part.tables_scanned == 2 && part.hits.len() == 1 && part.hits[0].table == "customers", "{part:?}");

        assert!(search_database(&mut c, &db, &req(""), &never, &mut |_, _| {}).await.is_err());
        Ok::<(), String>(())
    }
    .await;
    c.query_drop(format!("DROP DATABASE IF EXISTS `{db}`")).await.unwrap();
    outcome.unwrap();
}
