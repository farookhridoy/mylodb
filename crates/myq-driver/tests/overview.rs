//! Table overview against a local MySQL. Creates and drops only its own uniquely named database.

use myq_driver::{pool, table_overview, ConnConfig, SslMode};
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
async fn overview_reports_keys_in_both_directions_triggers_partitions_and_ddl() {
    let pool = pool(&cfg());
    let mut conn = match pool.get_conn().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP: no MySQL reachable: {e}");
            return;
        }
    };
    let db = format!("mylodb_itest_ov_{}", std::process::id());
    conn.query_drop(format!("CREATE DATABASE `{db}`")).await.unwrap();
    let result = async {
        conn.query_drop(format!("CREATE TABLE `{db}`.users (id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY, email VARCHAR(80) NOT NULL, UNIQUE KEY u_email (email)) COMMENT='people'")).await?;
        conn.query_drop(format!(
            "CREATE TABLE `{db}`.cat (id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY, parent_id BIGINT UNSIGNED NULL, created_by BIGINT UNSIGNED NOT NULL, note TEXT, \
             CONSTRAINT fk_cat_parent FOREIGN KEY (parent_id) REFERENCES cat(id) ON DELETE SET NULL, \
             CONSTRAINT fk_cat_user FOREIGN KEY (created_by) REFERENCES users(id) ON DELETE CASCADE ON UPDATE RESTRICT)"
        ))
        .await?;
        conn.query_drop(format!("CREATE TABLE `{db}`.item (id INT PRIMARY KEY, cat_id BIGINT UNSIGNED, CONSTRAINT fk_item_cat FOREIGN KEY (cat_id) REFERENCES cat(id))")).await?;
        conn.query_drop(format!("CREATE TRIGGER `{db}`.cat_bi BEFORE INSERT ON `{db}`.cat FOR EACH ROW SET NEW.note = 'x'")).await?;
        conn.query_drop(format!("CREATE TABLE `{db}`.logs (id INT NOT NULL, d INT NOT NULL, PRIMARY KEY (id, d)) PARTITION BY HASH(id) PARTITIONS 3")).await?;
        conn.query_drop(format!("CREATE VIEW `{db}`.v AS SELECT id FROM `{db}`.users")).await?;

        // ---- the table in the middle: references users and itself, is referenced by item and itself
        let o = table_overview(&pool, &db, "cat").await?;
        assert!(!o.info.is_view);
        assert_eq!(o.info.engine.as_deref(), Some("InnoDB"));
        assert!(o.info.charset.as_deref().is_some_and(|c| c.starts_with("utf8")), "{:?}", o.info.charset);
        assert!(o.info.collation.is_some());
        assert_eq!(o.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["id", "parent_id", "created_by", "note"]);
        assert_eq!(o.columns[0].key, "PRI");
        assert_eq!(o.columns[0].extra, "auto_increment");
        assert!(o.columns[1].nullable && !o.columns[2].nullable);
        assert_eq!(o.columns[1].default, None);

        let mut out: Vec<_> = o.outgoing.iter().map(|f| (f.name.as_str(), f.ref_table.as_str(), f.on_delete.as_str(), f.on_update.as_str())).collect();
        out.sort();
        assert_eq!(out, [("fk_cat_parent", "cat", "SET NULL", "NO ACTION"), ("fk_cat_user", "users", "CASCADE", "RESTRICT")]);
        let user = o.outgoing.iter().find(|f| f.name == "fk_cat_user").unwrap();
        assert_eq!((user.columns.as_slice(), user.ref_columns.as_slice(), user.table.as_str()), (&["created_by".to_string()][..], &["id".to_string()][..], "cat"));

        let mut inc: Vec<_> = o.incoming.iter().map(|f| (f.table.as_str(), f.name.as_str())).collect();
        inc.sort();
        assert_eq!(inc, [("cat", "fk_cat_parent"), ("item", "fk_item_cat")], "referenced by item, and by itself");

        let kinds: Vec<_> = o.constraints.iter().map(|c| (c.kind.as_str(), c.name.as_str())).collect();
        assert_eq!(kinds[0], ("PRIMARY KEY", "PRIMARY"), "primary key first");
        assert!(kinds.contains(&("FOREIGN KEY", "fk_cat_user")));
        assert!(o.indexes.iter().any(|i| i.name == "PRIMARY"));
        assert_eq!(o.triggers.len(), 1);
        assert_eq!((o.triggers[0].name.as_str(), o.triggers[0].timing.as_str(), o.triggers[0].event.as_str()), ("cat_bi", "BEFORE", "INSERT"));
        assert!(o.partitions.is_empty() && !o.info.partitioned);
        let ddl = o.ddl.clone().unwrap_or_default();
        assert!(ddl.contains("CREATE TABLE `cat`") && ddl.contains("fk_cat_user"), "{ddl}");

        // ---- settings, comment and statistics
        let u = table_overview(&pool, &db, "users").await?;
        assert_eq!(u.info.comment, "people");
        assert_eq!(u.info.auto_increment, Some(1));
        assert!(u.info.data_length.is_some());
        assert_eq!(u.incoming.len(), 1, "cat.created_by");
        assert!(u.constraints.iter().any(|c| c.kind == "UNIQUE" && c.columns == ["email"]));

        // ---- partitions
        let l = table_overview(&pool, &db, "logs").await?;
        assert!(l.info.partitioned);
        assert_eq!(l.partitions.len(), 3);
        assert_eq!(l.partitions[0].method.as_deref(), Some("HASH"));

        // ---- a view and a table that does not exist
        let v = table_overview(&pool, &db, "v").await?;
        assert!(v.info.is_view);
        assert!(v.ddl.unwrap_or_default().to_uppercase().contains("VIEW"));
        let none = table_overview(&pool, &db, "missing").await?;
        assert!(none.columns.is_empty() && none.ddl.is_none());
        Ok::<_, myq_driver::Error>(())
    }
    .await;
    conn.query_drop(format!("DROP DATABASE `{db}`")).await.unwrap();
    result.unwrap();
}
