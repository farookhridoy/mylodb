//! Data compare and sync against a real server: two tables in the test's own `mylodb_itest_ds_*` databases are compared, the sync
//! statements are applied to the target in a transaction, and a second comparison finds nothing left.

use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::{pool, ConnConfig, SslMode};
use myq_transfer::{apply_sync, diff_tables, primary_key, read_table, sync_statements, SyncOptions};

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
async fn the_target_is_brought_in_line_with_the_source() {
    let p = pool(&cfg());
    let mut c = match p.get_conn().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP: no MySQL reachable: {e}");
            return;
        }
    };
    let (a, b) = (format!("mylodb_itest_ds_{}_a", std::process::id()), format!("mylodb_itest_ds_{}_b", std::process::id()));
    for d in [&a, &b] {
        c.query_drop(format!("CREATE DATABASE `{d}`")).await.unwrap();
    }
    let outcome = async {
        for d in [&a, &b] {
            c.query_drop(format!("CREATE TABLE `{d}`.items (id INT PRIMARY KEY, name VARCHAR(40), price DECIMAL(8,2) NULL, note TEXT NULL, raw BLOB NULL)")).await.map_err(|e| e.to_string())?;
        }
        c.query_drop(format!("INSERT INTO `{a}`.items VALUES (1,'Ann',10.50,NULL,NULL),(2,'Bo',5.00,'it''s',x'CAFE'),(3,'Cy',NULL,'new',NULL),(4,'Di',7.25,'a\\\\b',NULL)")).await.map_err(|e| e.to_string())?;
        c.query_drop(format!("INSERT INTO `{b}`.items VALUES (1,'Ann',10.5,NULL,NULL),(2,'Bob',5.00,'old',NULL),(5,'Extra',1.00,NULL,NULL)")).await.map_err(|e| e.to_string())?;

        assert_eq!(primary_key(&mut c, &a, "items").await.map_err(|e| e.to_string())?, vec!["id".to_string()]);
        let source = read_table(&mut c, &a, "items", None).await.map_err(|e| e.to_string())?;
        let target = read_table(&mut c, &b, "items", None).await.map_err(|e| e.to_string())?;
        let d = diff_tables(&source, &target, &["id".to_string()]).map_err(|e| e.to_string())?;
        assert_eq!((d.same, d.changed.len(), d.only_source.len(), d.only_target.len()), (1, 1, 2, 1), "1 same (10.50 = 10.5), 2 changed, 3 and 4 missing, 5 extra");

        // without delete: the extra row stays
        let sql = sync_statements(&d, &format!("`{b}`.`items`"), SyncOptions::default());
        let n = apply_sync(&mut c, &sql, false).await.map_err(|e| e.to_string())?;
        assert_eq!(n as usize, sql.len());
        let again = diff_tables(&read_table(&mut c, &a, "items", None).await.map_err(|e| e.to_string())?, &read_table(&mut c, &b, "items", None).await.map_err(|e| e.to_string())?, &["id".to_string()]).map_err(|e| e.to_string())?;
        assert_eq!((again.changed.len(), again.only_source.len(), again.only_target.len()), (0, 0, 1), "only the extra row is left");
        // the awkward values arrived intact
        let (note, raw): (Option<String>, Option<Vec<u8>>) = c.query_first(format!("SELECT note, raw FROM `{b}`.items WHERE id = 2")).await.map_err(|e| e.to_string())?.unwrap();
        assert_eq!((note.as_deref(), raw), (Some("it's"), Some(vec![0xCA, 0xFE])));
        let slash: Option<String> = c.query_first(format!("SELECT note FROM `{b}`.items WHERE id = 4")).await.map_err(|e| e.to_string())?.flatten();
        assert_eq!(slash.as_deref(), Some("a\\b"));

        // with delete: the target ends up identical
        let sql = sync_statements(&again, &format!("`{b}`.`items`"), SyncOptions { delete_extra: true, ..Default::default() });
        apply_sync(&mut c, &sql, false).await.map_err(|e| e.to_string())?;
        let last = diff_tables(&read_table(&mut c, &a, "items", None).await.map_err(|e| e.to_string())?, &read_table(&mut c, &b, "items", None).await.map_err(|e| e.to_string())?, &["id".to_string()]).map_err(|e| e.to_string())?;
        assert_eq!((last.same, last.changed.len(), last.only_source.len(), last.only_target.len()), (4, 0, 0, 0));

        // a failing statement leaves the target as it was
        let bad = vec!["INSERT INTO `".to_string() + &b + "`.items (id, name) VALUES (100, 'ok')", "INSERT INTO `".to_string() + &b + "`.items (id, name) VALUES (1, 'duplicate key')"];
        assert!(apply_sync(&mut c, &bad, false).await.is_err());
        let n: Option<i64> = c.query_first(format!("SELECT COUNT(*) FROM `{b}`.items WHERE id = 100")).await.map_err(|e| e.to_string())?;
        assert_eq!(n, Some(0), "the first statement was rolled back");
        Ok::<(), String>(())
    }
    .await;
    for d in [&a, &b] {
        let _ = c.query_drop(format!("DROP DATABASE IF EXISTS `{d}`")).await;
    }
    outcome.unwrap();
}
