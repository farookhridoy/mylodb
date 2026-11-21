//! A JSON file imported through the CSV pipeline into a real table. Uses its own `mylodb_itest_json_*` database.

use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::{pool, ConnConfig, SslMode};
use myq_transfer::{import_csv, json_to_csv, CsvFormat, ImportMode, ImportPlan, Progress, NULL_TEXT};
use std::sync::atomic::AtomicBool;

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
async fn a_json_file_loads_into_a_table_with_nulls_quotes_and_nested_values() {
    let p = pool(&cfg());
    let mut c = match p.get_conn().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP: no MySQL reachable: {e}");
            return;
        }
    };
    let db = format!("mylodb_itest_json_{}", std::process::id());
    c.query_drop(format!("CREATE DATABASE `{db}`")).await.unwrap();
    let dir = std::env::temp_dir().join(format!("mylodb_itest_json_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let outcome = async {
        c.query_drop(format!("CREATE TABLE `{db}`.people (id INT PRIMARY KEY, name VARCHAR(50), score DECIMAL(5,2) NULL, tags TEXT NULL)")).await.map_err(|e| e.to_string())?;
        let src = dir.join("people.json");
        std::fs::write(&src, r#"[{"id":1,"name":"Ann","score":9.5,"tags":["a","b"]},{"id":2,"name":"O'Neil, \"Bo\"","score":null},{"id":3,"name":"Cy"}]"#).unwrap();
        let csv = dir.join("people.csv");
        let conv = json_to_csv(&src, &csv).map_err(|e| e.to_string())?;
        assert_eq!((conv.rows, conv.columns.clone()), (3, ["id", "name", "score", "tags"].iter().map(|s| s.to_string()).collect::<Vec<_>>()));

        let fmt = CsvFormat { null_text: Some(NULL_TEXT.into()), ..Default::default() };
        let plan = ImportPlan { database: db.clone(), table: "people".into(), mapping: conv.columns.iter().map(|c| Some(c.clone())).collect(), mode: ImportMode::Insert, batch_rows: 500, stop_on_error: true, truncate_first: false, empty_as_null: true, disable_fk_checks: false };
        let mut log = |_: Progress| {};
        let cancel = AtomicBool::new(false);
        let r = import_csv(&mut c, &csv, &fmt, &plan, &mut log, &cancel).await.map_err(|e| e.to_string())?;
        assert_eq!((r.rows_read, r.affected_rows, r.aborted), (3, 3, false), "{r:?}");

        let rows: Vec<(i64, String, Option<String>, Option<String>)> = c.query(format!("SELECT id, name, CAST(score AS CHAR), tags FROM `{db}`.people ORDER BY id")).await.map_err(|e| e.to_string())?;
        assert_eq!(rows[0], (1, "Ann".into(), Some("9.50".into()), Some("[\"a\",\"b\"]".into())), "a nested array is kept as JSON text");
        assert_eq!(rows[1], (2, "O'Neil, \"Bo\"".into(), None, None), "JSON null and a missing key are both NULL; quotes and commas survive");
        assert_eq!(rows[2], (3, "Cy".into(), None, None));
        Ok::<(), String>(())
    }
    .await;
    c.query_drop(format!("DROP DATABASE IF EXISTS `{db}`")).await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    outcome.unwrap();
}
