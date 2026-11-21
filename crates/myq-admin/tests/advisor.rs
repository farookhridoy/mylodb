//! The index advisor on a real server: read real definitions, apply the suggested statements, and ask again.
//! Uses its own `mylodb_itest_adv_*` database.

use myq_admin::{read_table_def, run_statements};
use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::{pool, ConnConfig, SslMode};
use myq_schema::advisor::{analyze, FindingKind};

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
async fn suggested_statements_run_and_clear_the_findings() {
    let p = pool(&cfg());
    let mut c = match p.get_conn().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP: no MySQL reachable: {e}");
            return;
        }
    };
    let db = format!("mylodb_itest_adv_{}", std::process::id());
    c.query_drop(format!("CREATE DATABASE `{db}`")).await.unwrap();
    let result = async {
        c.query_drop(format!(
            "CREATE TABLE `{db}`.orders (id INT NOT NULL PRIMARY KEY, customer_id INT, placed DATE, status VARCHAR(10),
               KEY idx_c (customer_id), KEY idx_c_p (customer_id, placed), KEY idx_s1 (status), KEY idx_s2 (status), KEY idx_id (id))"
        ))
        .await
        .map_err(|e| format!("setup: {e}"))?;
        let defs = vec![read_table_def(&mut c, &db, "orders").await.map_err(|e| e.to_string())?.0];
        let findings = analyze(&db, &defs);
        let mut got: Vec<(String, FindingKind)> = findings.iter().map(|f| (f.index.clone(), f.kind)).collect();
        got.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(got, [("idx_c".into(), FindingKind::Redundant), ("idx_id".into(), FindingKind::Duplicate), ("idx_s2".into(), FindingKind::Duplicate)], "{findings:#?}");

        let script: Vec<String> = findings.iter().map(|f| f.sql.clone()).collect();
        run_statements(&mut c, &script).await.map_err(|e| format!("{} -> {}", e.sql, e.message))?;

        let defs = vec![read_table_def(&mut c, &db, "orders").await.map_err(|e| e.to_string())?.0];
        assert!(analyze(&db, &defs).is_empty(), "{:#?}", analyze(&db, &defs));
        let names: Vec<String> = defs[0].indexes.iter().map(|i| i.name.clone()).collect();
        assert!(names.contains(&"idx_c_p".to_string()) && names.contains(&"idx_s1".to_string()) && names.contains(&"PRIMARY".to_string()), "the covering indexes stay: {names:?}");
        Ok::<(), String>(())
    }
    .await;
    c.query_drop(format!("DROP DATABASE IF EXISTS `{db}`")).await.unwrap();
    result.unwrap();
}
