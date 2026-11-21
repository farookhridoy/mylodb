//! The `Backend` contract exercised through the generic interface only, against a real MySQL / MariaDB.
//! Creates and drops only its own `mylodb_itest_be_*` database.

use myq_core::Backend;
use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::{pool, ConnConfig, MySqlBackend, SslMode};

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

/// Everything below knows nothing about MySQL: it is the code a second engine has to satisfy.
async fn exercise<B: Backend>(b: &B, db: &str) {
    let d = b.dialect();
    assert_eq!(d.quote_ident("we`ird"), "`we``ird`");

    let caps = b.capabilities().await.unwrap();
    assert!(!caps.schemas.available && caps.users.available);

    let dbs = b.databases().await.unwrap();
    assert!(dbs.iter().any(|x| x.name == db && !x.system), "the new database is listed: {dbs:?}");

    let tables = b.tables(db).await.unwrap();
    assert_eq!(tables.iter().map(|t| (t.name.as_str(), t.is_view)).collect::<Vec<_>>(), [("item", false), ("v_item", true)]);

    let cols = b.columns(db, "item").await.unwrap();
    assert_eq!(cols.iter().map(|c| (c.name.as_str(), c.key.as_str())).collect::<Vec<_>>(), [("id", "PRI"), ("name", "")]);
    assert!(!cols[0].nullable && cols[1].nullable);

    let idx = b.indexes(db, "item").await.unwrap();
    assert!(idx.iter().any(|i| i.name == "PRIMARY" && i.unique && i.columns == ["id"]), "{idx:?}");

    let sql = format!("INSERT INTO {t} VALUES (1, 'a'), (2, NULL); SELECT id, name FROM {t} ORDER BY id; SELECT nope FROM {t};", t = format_args!("{}.{}", d.quote_ident(db), d.quote_ident("item")));
    let results = b.run(Some(db), &sql).await.unwrap();
    assert_eq!(results.len(), 3);
    assert_eq!((results[0].affected_rows, results[0].error.as_deref()), (2, None));
    let rows = &results[1].sets[0].rows;
    assert_eq!(rows, &vec![vec![Some("1".to_string()), Some("a".to_string())], vec![Some("2".to_string()), None]], "NULL stays distinct from text");
    assert!(results[2].error.is_some(), "a failing statement is reported in its result, not as an Err");
}

#[tokio::test]
async fn the_mysql_backend_satisfies_the_engine_contract() {
    let p = pool(&cfg());
    let mut admin = match p.get_conn().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP: no MySQL reachable: {e}");
            return;
        }
    };
    let db = format!("mylodb_itest_be_{}", std::process::id());
    admin.query_drop(format!("CREATE DATABASE `{db}`")).await.unwrap();
    admin.query_drop(format!("CREATE TABLE `{db}`.item (id INT NOT NULL PRIMARY KEY, name VARCHAR(20) NULL)")).await.unwrap();
    admin.query_drop(format!("CREATE VIEW `{db}`.v_item AS SELECT id FROM `{db}`.item")).await.unwrap();
    let backend = MySqlBackend::new(&cfg());
    let name = db.clone();
    let outcome = tokio::spawn(async move { exercise(&backend, &name).await }).await;
    admin.query_drop(format!("DROP DATABASE IF EXISTS `{db}`")).await.unwrap();
    outcome.unwrap();
}
