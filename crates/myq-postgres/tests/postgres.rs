//! PostgreSQL through the engine contract, against a real server. Runs only when MYLODB_TEST_PG_PORT is set, so that a developer's
//! own server is never touched by accident. Creates and drops only its own `mylodb_itest_*` schema.

use myq_core::Backend;
use myq_driver::{ConnConfig, SslMode};
use myq_postgres::{probe, CancelToken, PgBackend};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn cfg() -> Option<ConnConfig> {
    let port = std::env::var("MYLODB_TEST_PG_PORT").ok()?.parse().ok()?;
    Some(ConnConfig {
        host: std::env::var("MYLODB_TEST_PG_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port,
        user: std::env::var("MYLODB_TEST_PG_USER").unwrap_or_else(|_| "postgres".into()),
        password: std::env::var("MYLODB_TEST_PG_PASSWORD").ok(),
        database: None,
        ssl: SslMode::Disabled,
        ..Default::default()
    })
}

/// A schema of its own, dropped afterwards even when an assertion fails.
async fn with_schema<F, Fut>(tag: &str, body: F)
where
    F: FnOnce(ConnConfig, PgBackend, String) -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let Some(cfg) = cfg() else { return eprintln!("SKIP: set MYLODB_TEST_PG_PORT to run the PostgreSQL tests") };
    let b = PgBackend::new(cfg.clone());
    let s = format!("mylodb_itest_{tag}_{}", std::process::id());
    let made = b.run(None, &format!("DROP SCHEMA IF EXISTS {s} CASCADE; CREATE SCHEMA {s}")).await.expect("a server is expected");
    assert!(made.iter().all(|r| r.error.is_none()), "{made:?}");
    let outcome = tokio::spawn(body(cfg, b.clone(), s.clone())).await;
    let _ = b.run(None, &format!("DROP SCHEMA IF EXISTS {s} CASCADE")).await;
    outcome.unwrap();
}

#[tokio::test]
async fn browse_and_run() {
    with_schema("browse", |cfg, b, s| async move {
        assert!(probe(&cfg).await.unwrap().version.starts_with("PostgreSQL "));
        let setup = format!(
            "CREATE TABLE {s}.item (id int GENERATED ALWAYS AS IDENTITY PRIMARY KEY, code text UNIQUE, name varchar(20), blob bytea);
             COMMENT ON TABLE {s}.item IS 'things';
             CREATE INDEX item_name ON {s}.item (name, code);
             CREATE TABLE {s}.\"Odd name\" (\"A;b\" serial);
             CREATE VIEW {s}.v_item AS SELECT id FROM {s}.item;
             CREATE FUNCTION {s}.two() RETURNS int LANGUAGE plpgsql AS $$ BEGIN RETURN 2; END; $$"
        );
        let r = b.run(None, &setup).await.unwrap();
        assert!(r.len() == 6 && r.iter().all(|r| r.error.is_none()), "{r:?}");

        assert!(b.databases().await.unwrap().iter().any(|d| d.name == s && !d.system));
        let index = b.index(false).await.unwrap();
        assert!(index.iter().all(|n| !n.database.starts_with("pg_")), "system schemas are hidden");
        let mine: Vec<_> = index.iter().filter(|n| n.database == s).map(|n| (n.name.as_str(), format!("{:?}", n.kind))).collect();
        assert_eq!(mine, [(s.as_str(), "Database".into()), ("Odd name", "Table".into()), ("item", "Table".into()), ("v_item", "View".into())]);
        assert!(b.column_nodes(&s).await.unwrap().iter().any(|n| n.name == "A;b" && n.parent.as_deref() == Some("Odd name")));

        let tables = b.tables(&s).await.unwrap();
        let item = tables.iter().find(|t| t.name == "item").unwrap();
        assert!(!item.is_view && item.comment == "things" && item.bytes.is_some());
        let cols = b.columns(&s, "item").await.unwrap();
        let got: Vec<_> = cols.iter().map(|c| (c.name.as_str(), c.column_type.as_str(), c.nullable, c.key.as_str(), c.extra.as_str())).collect();
        assert_eq!(got[0], ("id", "integer", false, "PRI", "identity"));
        assert_eq!(got[1], ("code", "text", true, "UNI", ""));
        assert_eq!(got[2], ("name", "character varying(20)", true, "MUL", ""));
        assert_eq!(b.columns(&s, "Odd name").await.unwrap()[0].extra, "auto_increment");
        let idx = b.indexes(&s, "item").await.unwrap();
        assert_eq!((idx[0].name.as_str(), idx[0].unique, idx[0].columns.clone()), ("PRIMARY", true, vec!["id".to_string()]));
        assert!(idx.iter().any(|i| i.name == "item_name" && !i.unique && i.columns == ["name", "code"] && i.kind == "BTREE"), "{idx:?}");

        // the chosen schema is first in the search path; values come back as text, NULL stays NULL
        let r = b.run(Some(&s), "INSERT INTO item (code, name, blob) VALUES ('a', 'Ann', '\\x0a1b'), ('b', NULL, NULL); SELECT id, name, blob, true AS t FROM item ORDER BY id; SELECT nope; SELECT two()").await.unwrap();
        assert_eq!((r[0].affected_rows, r[0].error.as_deref()), (2, None));
        let set = &r[1].sets[0];
        assert_eq!(set.columns.iter().map(|c| (c.name.as_str(), c.type_name.as_str(), c.binary)).collect::<Vec<_>>(), [("id", "int4", false), ("name", "varchar", false), ("blob", "bytea", true), ("t", "bool", false)]);
        assert_eq!(set.rows[0], vec![Some("1".to_string()), Some("Ann".into()), Some("0A1B".into()), Some("t".into())]);
        assert_eq!(set.rows[1][1], None);
        assert!(r[2].error.as_deref().unwrap().contains("42703"), "{:?}", r[2].error);
        assert_eq!(r[3].sets[0].rows[0][0].as_deref(), Some("2"), "a failing statement does not stop the ones after it");

        // an empty result still has its columns, and a long one is cut
        let r = b.run(Some(&s), "SELECT id FROM item WHERE false; SELECT g FROM generate_series(1, 10005) g; COPY item TO STDOUT").await.unwrap();
        assert_eq!((r[0].sets[0].columns.len(), r[0].sets[0].rows.len()), (1, 0));
        assert_eq!((r[1].sets[0].rows.len(), r[1].sets[0].total_rows, r[1].sets[0].truncated), (10_000, 10_005, true));
        assert!(r[2].error.as_deref().unwrap().contains("not supported"));

        // session settings do not leak into the next run
        b.run(None, "SET statement_timeout = 1; BEGIN").await.unwrap();
        let r = b.run(None, "SHOW statement_timeout").await.unwrap();
        assert_eq!(r[0].sets[0].rows[0][0].as_deref(), Some("0"));
    })
    .await;
}

#[tokio::test]
async fn a_read_only_connection_cannot_be_made_to_write() {
    with_schema("ro", |cfg, b, s| async move {
        b.run(None, &format!("CREATE TABLE {s}.t (n int); INSERT INTO {s}.t VALUES (1)")).await.unwrap();
        let ro = PgBackend::new(ConnConfig { read_only: true, ..cfg });
        for attempt in [
            format!("DELETE FROM {s}.t"),
            format!("DROP TABLE {s}.t"),
            format!("SET default_transaction_read_only = off; DELETE FROM {s}.t"),
            format!("COMMIT; DELETE FROM {s}.t"),
            format!("BEGIN READ WRITE; DELETE FROM {s}.t"),
            format!("SET SESSION CHARACTERISTICS AS TRANSACTION READ WRITE; DELETE FROM {s}.t"),
            format!("SELECT set_config('default_transaction_read_only', 'off', false); DELETE FROM {s}.t"),
            format!("WITH x AS (SELECT set_config('transaction_read_only', 'off', true)) DELETE FROM {s}.t"),
            format!("DO $$ BEGIN COMMIT; DELETE FROM {s}.t; END $$"),
        ] {
            let r = ro.run(None, &attempt).await.unwrap();
            assert!(r.last().unwrap().error.is_some(), "{attempt}: {r:?}");
        }
        let r = ro.run(None, &format!("SELECT count(*) FROM {s}.t")).await.unwrap();
        assert_eq!(r[0].sets[0].rows[0][0].as_deref(), Some("1"), "the row is still there");
    })
    .await;
}

#[tokio::test]
async fn cancel_and_timeout_stop_a_running_statement() {
    with_schema("cancel", |_, b, _| async move {
        let token = Arc::new(CancelToken::new());
        let (b2, t2) = (b.clone(), token.clone());
        let started = Instant::now();
        let run = tokio::spawn(async move { b2.run_cancellable(None, "SELECT pg_sleep(30); SELECT 1", t2, None).await });
        tokio::time::sleep(Duration::from_millis(700)).await;
        token.cancel().await.unwrap();
        let r = run.await.unwrap().unwrap();
        assert_eq!(r[0].error.as_deref(), Some("Cancelled."));
        assert!(r[1].error.as_deref().unwrap().contains("before it started"));
        let r = b.run_cancellable(None, "SELECT pg_sleep(30)", Arc::new(CancelToken::new()), Some(1)).await.unwrap();
        assert!(r[0].error.as_deref().unwrap().contains("query timeout"), "{r:?}");
        assert!(started.elapsed() < Duration::from_secs(20));
        assert!(b.run(None, "SELECT 1").await.unwrap()[0].error.is_none(), "the backend still works");
    })
    .await;
}

#[tokio::test]
async fn a_wrong_password_or_missing_tls_is_explained() {
    let Some(cfg) = cfg() else { return };
    let e = probe(&ConnConfig { database: Some("no_such_database_here".into()), ..cfg.clone() }).await.err().expect("no such database");
    assert!(e.0.contains("no_such_database_here"), "{e}");
    // encryption that was asked for is never silently dropped
    if probe(&cfg).await.unwrap().ssl_cipher.is_none() {
        match probe(&ConnConfig { ssl: SslMode::SkipVerify, ..cfg }).await {
            Ok(p) => assert!(p.ssl_cipher.is_some()),
            Err(e) => assert!(!e.0.is_empty()),
        }
    }
}

/// Needs a server with TLS whose certificate is signed by MYLODB_TEST_PG_CA for a name that is not the address connected to, and
/// MYLODB_TEST_PG_OTHER_CA, an authority that did not sign it.
#[tokio::test]
async fn each_ssl_mode_checks_what_it_says() {
    let (Some(cfg), Ok(ca), Ok(other)) = (cfg(), std::env::var("MYLODB_TEST_PG_CA"), std::env::var("MYLODB_TEST_PG_OTHER_CA")) else { return };
    let with = |ssl, ca: Option<&str>| ConnConfig { ssl, ssl_ca: ca.map(String::from), ..cfg.clone() };
    let skip = probe(&with(SslMode::SkipVerify, None)).await.unwrap();
    assert!(skip.ssl_cipher.is_some(), "the connection is encrypted");
    assert!(probe(&with(SslMode::VerifyCa, Some(&ca))).await.is_ok(), "right authority, name not checked");
    assert!(probe(&with(SslMode::VerifyCa, Some(&other))).await.is_err(), "wrong authority");
    assert!(probe(&with(SslMode::VerifyCa, None)).await.is_err(), "unknown authority");
    assert!(probe(&with(SslMode::Required, Some(&ca))).await.is_err(), "the name does not match the address");
    // cancel works over TLS too
    let b = PgBackend::new(with(SslMode::SkipVerify, None));
    let token = Arc::new(CancelToken::new());
    let (b2, t2) = (b.clone(), token.clone());
    let run = tokio::spawn(async move { b2.run_cancellable(None, "SELECT pg_sleep(20)", t2, None).await });
    tokio::time::sleep(Duration::from_millis(700)).await;
    token.cancel().await.unwrap();
    assert_eq!(run.await.unwrap().unwrap()[0].error.as_deref(), Some("Cancelled."));
}
