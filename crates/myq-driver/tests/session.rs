//! The session helpers against a real MySQL. They create nothing.

use myq_driver::{pool, ConnConfig, SslMode};

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
async fn session_helpers_read_the_server() {
    let p = pool(&cfg());
    let mut conn = match p.get_conn().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP: no MySQL reachable: {e}");
            return;
        }
    };
    let probe = myq_driver::probe(&cfg()).await.unwrap();
    assert!(!probe.version.is_empty());

    let facts = myq_driver::session_facts(&mut conn).await.unwrap();
    assert!(facts.account.contains('@') && !facts.connection_collation.is_empty());

    let opts = myq_driver::server_options(&mut conn).await.unwrap();
    assert!(opts.engines.iter().any(|e| e == "InnoDB"));
    assert!(opts.charsets.iter().any(|(cs, cols, def)| cs == "utf8mb4" && cols.contains(def)));

    let g = myq_driver::query_grid(&mut conn, "SELECT 1 AS one, NULL AS nothing").await.unwrap();
    assert_eq!((g.columns.as_slice(), g.rows.as_slice()), (["one".to_string(), "nothing".to_string()].as_slice(), [vec![Some("1".to_string()), None]].as_slice()));

    myq_driver::set_foreign_key_checks(&mut conn, false).await.unwrap();
    let off: Option<i64> = myq_driver::mysql_async::prelude::Queryable::query_first(&mut conn, "SELECT @@foreign_key_checks").await.unwrap();
    assert_eq!(off, Some(0));
    myq_driver::restore_foreign_key_checks(&mut conn).await;
    let on: Option<i64> = myq_driver::mysql_async::prelude::Queryable::query_first(&mut conn, "SELECT @@foreign_key_checks").await.unwrap();
    assert_eq!(on, Some(1));
}

#[tokio::test]
async fn capabilities_follow_the_real_server() {
    let p = pool(&cfg());
    let mut conn = match p.get_conn().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP: no MySQL reachable: {e}");
            return;
        }
    };
    let caps = myq_driver::capabilities(&mut conn).await.unwrap();
    let version: Option<String> = myq_driver::mysql_async::prelude::Queryable::query_first(&mut conn, "SELECT VERSION()").await.unwrap();
    let version = version.unwrap();
    assert_eq!(caps.flavor == myq_core::Flavor::MariaDb, version.to_lowercase().contains("mariadb"), "{version}");
    assert!(caps.users.available && !caps.schemas.available);
}
