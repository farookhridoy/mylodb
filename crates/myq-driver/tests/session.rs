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

#[tokio::test]
async fn the_er_model_reads_tables_columns_and_foreign_keys_in_one_go() {
    let p = pool(&cfg());
    let mut conn = match p.get_conn().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP: no MySQL reachable: {e}");
            return;
        }
    };
    let db = format!("mylodb_itest_er_{}", std::process::id());
    myq_driver::mysql_async::prelude::Queryable::query_drop(&mut conn, format!("CREATE DATABASE `{db}`")).await.unwrap();
    let outcome = async {
        for sql in [
            format!("CREATE TABLE `{db}`.users (id INT NOT NULL PRIMARY KEY, name VARCHAR(40) NOT NULL, bio TEXT NULL)"),
            format!("CREATE TABLE `{db}`.posts (id INT NOT NULL PRIMARY KEY, author_id INT NOT NULL, editor_id INT NULL, CONSTRAINT fk_author FOREIGN KEY (author_id) REFERENCES users(id) ON DELETE CASCADE, CONSTRAINT fk_editor FOREIGN KEY (editor_id) REFERENCES users(id) ON UPDATE CASCADE)"),
            format!("CREATE TABLE `{db}`.pairs (a INT NOT NULL, b INT NOT NULL, PRIMARY KEY (a, b), CONSTRAINT fk_pair FOREIGN KEY (a, b) REFERENCES `{db}`.pairs2 (a, b))"),
            format!("CREATE VIEW `{db}`.v AS SELECT id FROM `{db}`.users"),
        ] {
            // pairs references pairs2, which does not exist yet: create it first
            if sql.contains("pairs (") {
                myq_driver::mysql_async::prelude::Queryable::query_drop(&mut conn, format!("CREATE TABLE `{db}`.pairs2 (a INT NOT NULL, b INT NOT NULL, PRIMARY KEY (a, b))")).await.map_err(|e| e.to_string())?;
            }
            myq_driver::mysql_async::prelude::Queryable::query_drop(&mut conn, sql).await.map_err(|e| format!("setup: {e}"))?;
        }
        let m = myq_driver::er_model(&p, &db).await.map_err(|e| e.to_string())?;
        assert_eq!(m.tables.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["pairs", "pairs2", "posts", "users"], "base tables only, no view");
        let users = m.tables.iter().find(|t| t.name == "users").unwrap();
        assert_eq!(users.columns.iter().map(|c| (c.name.as_str(), c.primary, c.nullable)).collect::<Vec<_>>(), [("id", true, false), ("name", false, false), ("bio", false, true)]);
        assert!(users.columns[1].column_type.starts_with("varchar(40)"));
        let rel = |n: &str| m.relations.iter().find(|r| r.name == n).unwrap().clone();
        assert_eq!((rel("fk_author").table.as_str(), rel("fk_author").ref_table.as_str(), rel("fk_author").on_delete.as_str()), ("posts", "users", "CASCADE"));
        assert_eq!(rel("fk_editor").on_update, "CASCADE");
        assert_eq!((rel("fk_pair").columns, rel("fk_pair").ref_columns), (vec!["a".to_string(), "b".to_string()], vec!["a".to_string(), "b".to_string()]), "a composite key is one relation with ordered columns");
        assert_eq!(m.relations.len(), 3);
        Ok::<(), String>(())
    }
    .await;
    myq_driver::mysql_async::prelude::Queryable::query_drop(&mut conn, format!("DROP DATABASE IF EXISTS `{db}`")).await.unwrap();
    outcome.unwrap();
}
