//! Schema compare against a real server: build two databases that differ, compare, apply the script to the target, and compare again.
//! The second comparison must find nothing: that is the proof that the generated statements do what the diff says.
//! Uses its own `mylodb_itest_cmp_*` databases, dropped afterwards.

use myq_admin::{read_table_def, run_statements};
use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::{pool, Conn, ConnConfig, SslMode};
use myq_schema::compare::{compare, ChangeKind};
use myq_schema::ddl::TableDef;

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

async fn defs(c: &mut Conn, db: &str) -> Vec<TableDef> {
    let names: Vec<String> = c.query(format!("SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = '{db}' AND TABLE_TYPE = 'BASE TABLE' ORDER BY 1")).await.unwrap();
    let mut out = Vec::new();
    for n in names {
        out.push(read_table_def(c, db, &n).await.unwrap().0);
    }
    out
}

#[tokio::test]
async fn applying_a_schema_comparison_makes_the_target_match_the_source() {
    let p = pool(&cfg());
    let mut c = match p.get_conn().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP: no MySQL reachable: {e}");
            return;
        }
    };
    let id = std::process::id();
    let (src, tgt) = (format!("mylodb_itest_cmp_s_{id}"), format!("mylodb_itest_cmp_t_{id}"));
    for d in [&src, &tgt] {
        c.query_drop(format!("CREATE DATABASE `{d}`")).await.unwrap();
    }
    let result = async {
        // source: users (changed + new column + new index), posts (new, with a foreign key to users), same_t (identical)
        for sql in [
            format!("CREATE TABLE `{src}`.users (id INT NOT NULL AUTO_INCREMENT PRIMARY KEY, name VARCHAR(100) NOT NULL, email VARCHAR(255) NULL, KEY idx_name (name)) ENGINE=InnoDB"),
            format!("CREATE TABLE `{src}`.posts (id INT NOT NULL PRIMARY KEY, user_id INT NOT NULL, title VARCHAR(80) NULL, CONSTRAINT fk_posts_user FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE) ENGINE=InnoDB"),
            format!("CREATE TABLE `{src}`.same_t (id INT NOT NULL PRIMARY KEY, v VARCHAR(10) NULL) ENGINE=InnoDB"),
            // target: users (older shape, an extra column), same_t, old_log (not in the source)
            format!("CREATE TABLE `{tgt}`.users (id INT NOT NULL AUTO_INCREMENT PRIMARY KEY, name VARCHAR(40) NOT NULL, legacy INT NULL) ENGINE=InnoDB"),
            format!("CREATE TABLE `{tgt}`.same_t (id INT NOT NULL PRIMARY KEY, v VARCHAR(10) NULL) ENGINE=InnoDB"),
            format!("CREATE TABLE `{tgt}`.old_log (id INT NOT NULL PRIMARY KEY) ENGINE=InnoDB"),
        ] {
            c.query_drop(sql).await.map_err(|e| format!("setup: {e}"))?;
        }

        let diff = compare(&defs(&mut c, &src).await, &src, &defs(&mut c, &tgt).await, &tgt)?;
        let kinds: Vec<(&str, ChangeKind)> = diff.changes.iter().map(|x| (x.table.as_str(), x.kind)).collect();
        assert_eq!(kinds, [("old_log", ChangeKind::Removed), ("posts", ChangeKind::Added), ("users", ChangeKind::Changed)]);
        assert_eq!(diff.identical, 1);

        // without the drop, apply: creates, alters, then the new table's foreign key
        let script = diff.script(&["posts".into(), "users".into()]);
        run_statements(&mut c, &script).await.map_err(|e| format!("apply: {} -> {}", e.sql, e.message))?;

        let again = compare(&defs(&mut c, &src).await, &src, &defs(&mut c, &tgt).await, &tgt)?;
        let left: Vec<(&str, ChangeKind)> = again.changes.iter().map(|x| (x.table.as_str(), x.kind)).collect();
        assert_eq!(left, [("old_log", ChangeKind::Removed)], "only the table nobody chose to drop is left: {:#?}", again.changes);

        // the foreign key and the data type really arrived
        let fk: Option<String> = c.query_first(format!("SELECT CONSTRAINT_NAME FROM information_schema.TABLE_CONSTRAINTS WHERE TABLE_SCHEMA='{tgt}' AND TABLE_NAME='posts' AND CONSTRAINT_TYPE='FOREIGN KEY'")).await.map_err(|e| e.to_string())?;
        assert_eq!(fk.as_deref(), Some("fk_posts_user"));
        let ty: Option<String> = c.query_first(format!("SELECT COLUMN_TYPE FROM information_schema.COLUMNS WHERE TABLE_SCHEMA='{tgt}' AND TABLE_NAME='users' AND COLUMN_NAME='name'")).await.map_err(|e| e.to_string())?;
        assert_eq!(ty.as_deref(), Some("varchar(100)"));

        // choosing the drop finishes the job
        run_statements(&mut c, &again.script(&["old_log".into()])).await.map_err(|e| e.message)?;
        let last = compare(&defs(&mut c, &src).await, &src, &defs(&mut c, &tgt).await, &tgt)?;
        assert!(last.changes.is_empty() && last.identical == 3, "{last:?}");
        Ok::<(), String>(())
    }
    .await;
    for d in [&src, &tgt] {
        c.query_drop(format!("DROP DATABASE IF EXISTS `{d}`")).await.unwrap();
    }
    result.unwrap();
}
