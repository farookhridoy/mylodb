//! Integration test against a local MySQL. Creates and drops only its own uniquely named database.
//! Configure with MYLODB_TEST_URL-style env vars; skipped when no server is reachable.

use myq_driver::{introspect, pool, ConnConfig, IntrospectOptions, SslMode};
use myq_schema::filter::{Filter, Kind};
use mysql_async::prelude::Queryable;

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
async fn introspects_and_filters_all_object_kinds() {
    let pool = pool(&cfg());
    let mut conn = match pool.get_conn().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP: no MySQL reachable: {e}");
            return;
        }
    };
    let db = format!("mylodb_itest_{}", std::process::id());
    conn.query_drop(format!("CREATE DATABASE `{db}`")).await.unwrap();

    let result = async {
        conn.query_drop(format!("CREATE TABLE `{db}`.orders (id INT PRIMARY KEY, email VARCHAR(50))")).await?;
        conn.query_drop(format!("CREATE VIEW `{db}`.v_orders AS SELECT id FROM `{db}`.orders")).await?;
        conn.query_drop(format!("CREATE PROCEDURE `{db}`.p_noop() BEGIN END")).await?;
        conn.query_drop(format!("CREATE FUNCTION `{db}`.f_one() RETURNS INT DETERMINISTIC RETURN 1")).await?;
        conn.query_drop(format!(
            "CREATE TRIGGER `{db}`.t_ins BEFORE INSERT ON `{db}`.orders FOR EACH ROW SET NEW.email = LOWER(NEW.email)"
        ))
        .await?;
        conn.query_drop(format!("CREATE EVENT `{db}`.e_never ON SCHEDULE EVERY 1 YEAR DISABLE DO SELECT 1")).await?;

        let index = introspect(&pool, &IntrospectOptions { include_columns: true, ..Default::default() }).await?;
        let mine: Vec<_> = index.iter().filter(|n| n.database == db).collect();
        for kind in [Kind::Database, Kind::Table, Kind::View, Kind::Procedure, Kind::Function, Kind::Trigger, Kind::Event, Kind::Column] {
            assert!(mine.iter().any(|n| n.kind == kind), "missing {kind:?}");
        }
        assert!(!index.iter().any(|n| n.database == "mysql"), "system dbs excluded by default");

        // columns are opt-in: the default index has names only, and one database's columns load separately
        let names_only = introspect(&pool, &IntrospectOptions::default()).await?;
        assert!(!names_only.iter().any(|n| n.kind == Kind::Column), "connecting must not load every column on the server");
        let cols = myq_driver::introspect_columns(&pool, &db).await?;
        assert!(cols.iter().all(|c| c.database == db && c.kind == Kind::Column && c.parent.is_some()));
        let orders: Vec<&str> = cols.iter().filter(|c| c.parent.as_deref() == Some("orders")).map(|c| c.name.as_str()).collect();
        assert_eq!(orders, ["id", "email"], "columns come back in table order");
        assert!(myq_driver::introspect_columns(&pool, "no_such_database_xyz").await?.is_empty());

        let hits = Filter::parse(&format!("db:{db} type:view ord")).unwrap().apply(&index);
        assert_eq!(hits.len(), 1);
        assert_eq!(index[hits[0]].name, "v_orders");
        Ok::<_, myq_driver::Error>(())
    }
    .await;

    conn.query_drop(format!("DROP DATABASE `{db}`")).await.unwrap();
    result.unwrap();
}

#[tokio::test]
async fn reports_column_types_and_table_sizes() {
    let pool = pool(&cfg());
    let mut conn = match pool.get_conn().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP: no MySQL reachable: {e}");
            return;
        }
    };
    let db = format!("mylodb_itest_stats_{}", std::process::id());
    conn.query_drop(format!("CREATE DATABASE `{db}`")).await.unwrap();
    let result = async {
        conn.query_drop(format!(
            "CREATE TABLE `{db}`.t (id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY, code CHAR(255) NOT NULL, note VARCHAR(40) NULL, KEY (code(20)))"
        ))
        .await?;
        conn.query_drop(format!("CREATE VIEW `{db}`.v AS SELECT id FROM `{db}`.t")).await?;
        conn.query_drop(format!("INSERT INTO `{db}`.t (code) VALUES ('a'),('b')")).await?;

        let cols = myq_driver::column_details(&pool, &db, "t").await?;
        assert_eq!(cols.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["id", "code", "note"], "declared order");
        assert!(cols[0].column_type.starts_with("bigint") && cols[0].column_type.ends_with("unsigned"), "{}", cols[0].column_type);
        assert_eq!((cols[0].key.as_str(), cols[0].extra.as_str(), cols[0].nullable), ("PRI", "auto_increment", false));
        assert_eq!(cols[1].column_type, "char(255)");
        assert!(!cols[1].nullable && cols[2].nullable);
        assert!(myq_driver::column_details(&pool, &db, "missing").await?.is_empty());

        let stats = myq_driver::table_stats(&pool, &db).await?;
        let t = stats.iter().find(|s| s.name == "t").expect("table listed");
        assert!(t.bytes.unwrap_or(0) > 0, "an InnoDB table with an index has a size");
        let v = stats.iter().find(|s| s.name == "v").expect("view listed");
        assert_eq!(v.bytes, None, "views have no size");
        assert!(myq_driver::table_stats(&pool, "mylodb_no_such_db").await?.is_empty());
        Ok::<_, myq_driver::Error>(())
    }
    .await;
    conn.query_drop(format!("DROP DATABASE `{db}`")).await.unwrap();
    result.unwrap();
}
