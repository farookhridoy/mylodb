//! The SQL plans of `myq_schema::plan` run against a real MySQL. Creates and drops only its own uniquely named databases.

use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::{execute_script, pool, ConnConfig, SslMode};
use myq_schema::plan;

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

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

async fn tables(conn: &mut myq_driver::Conn, db: &str) -> Vec<String> {
    let mut t: Vec<String> = conn.query(format!("SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = '{db}' ORDER BY 1")).await.unwrap();
    t.sort();
    t
}

async fn run(conn: &mut myq_driver::Conn, statements: &[String]) -> Vec<Option<String>> {
    execute_script(conn, None, &statements.join("\n")).await.into_iter().map(|r| r.error).collect()
}

#[tokio::test]
async fn plans_do_what_the_preview_says() {
    let p = pool(&cfg());
    let mut conn = match p.get_conn().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP: no MySQL reachable: {e}");
            return;
        }
    };
    let id = std::process::id();
    let (db, copy, moved) = (format!("mylodb_itest_pl_{id}"), format!("mylodb_itest_pl_copy_{id}"), format!("mylodb_itest_pl_mv_{id}"));
    conn.query_drop(format!("CREATE DATABASE `{db}`")).await.unwrap();
    // the checks run in a task so that a failed assertion cannot skip the cleanup
    let handle = {
        let (cfg, db, copy, moved) = (cfg(), db.clone(), copy.clone(), moved.clone());
        tokio::spawn(async move { checks(cfg, db, copy, moved).await })
    };
    let out = handle.await;
    for d in [&db, &copy, &moved] {
        conn.query_drop(format!("DROP DATABASE IF EXISTS `{d}`")).await.unwrap();
    }
    out.unwrap();
}

async fn checks(cfg: ConnConfig, db: String, copy: String, moved: String) {
    let p = pool(&cfg);
    let mut conn = p.get_conn().await.unwrap();
    for sql in [
        format!("CREATE TABLE `{db}`.parent (id INT PRIMARY KEY, v VARCHAR(5))"),
        format!("CREATE TABLE `{db}`.child (id INT PRIMARY KEY, parent_id INT, CONSTRAINT fk_c FOREIGN KEY (parent_id) REFERENCES `{db}`.parent(id))"),
        format!("INSERT INTO `{db}`.parent VALUES (1,'a'),(2,'b')"),
        format!("INSERT INTO `{db}`.child VALUES (10,1)"),
        format!("CREATE TABLE `{db}`.tmp_x (id INT)"),
        format!("CREATE TABLE `{db}`.tmp_y (id INT)"),
    ] {
        conn.query_drop(sql).await.unwrap();
    }

    // ---- a bulk drop of tables that reference each other: refused with checks on, nothing changes
    let drop_both = plan::drop_tables(&db, &[("parent".into(), false), ("child".into(), false)]).unwrap();
    assert_eq!(drop_both, [format!("DROP TABLE `{db}`.`parent`, `{db}`.`child`;")]);
    let errors = run(&mut conn, &plan::with_fk_checks(&drop_both, true)).await;
    // one statement drops both in one go, so MySQL resolves the dependency itself; MariaDB refuses (error 1451) and needs the FK switch the dialog offers
    if errors[0].is_some() {
        assert!(errors[0].as_deref().unwrap().contains("1451"), "{errors:?}");
        // MariaDB may already have dropped one of them before refusing: finish with the switch off
        let rest = vec![format!("DROP TABLE IF EXISTS `{db}`.`parent`, `{db}`.`child`;")];
        let off = run(&mut conn, &plan::with_fk_checks(&rest, false)).await;
        assert!(off.iter().all(|e| e.is_none()), "with the foreign key switch off it works on every server: {off:?}");
    }
    assert_eq!(tables(&mut conn, &db).await, ["tmp_x", "tmp_y"]);

    // ---- dropping only the referenced table needs the checks switched off
    for sql in [
        format!("CREATE TABLE `{db}`.parent (id INT PRIMARY KEY)"),
        format!("CREATE TABLE `{db}`.child (id INT PRIMARY KEY, parent_id INT, CONSTRAINT fk_c FOREIGN KEY (parent_id) REFERENCES `{db}`.parent(id))"),
    ] {
        conn.query_drop(sql).await.unwrap();
    }
    let drop_parent = plan::drop_tables(&db, &[("parent".into(), false)]).unwrap();
    let on = run(&mut conn, &plan::with_fk_checks(&drop_parent, true)).await;
    assert!(on[0].as_deref().is_some_and(|e| e.contains("foreign key")), "checks on: {on:?}");
    assert!(tables(&mut conn, &db).await.contains(&"parent".to_string()), "a failed plan changes nothing");
    let off = run(&mut conn, &plan::with_fk_checks(&drop_parent, false)).await;
    assert!(off.iter().all(|e| e.is_none()), "checks off: {off:?}");
    assert!(!tables(&mut conn, &db).await.contains(&"parent".to_string()));
    let flag: Option<i64> = conn.query_first("SELECT @@session.foreign_key_checks").await.unwrap();
    assert_eq!(flag, Some(1), "the plan puts the switch back");
    conn.query_drop(format!("DROP TABLE `{db}`.child")).await.unwrap();

    // ---- empty, copy, prefixes
    conn.query_drop(format!("CREATE TABLE `{db}`.keep (id INT PRIMARY KEY, v INT)")).await.unwrap();
    conn.query_drop(format!("INSERT INTO `{db}`.keep VALUES (1,1),(2,2),(3,3)")).await.unwrap();
    let cp = plan::copy_tables(&db, &[("keep".into(), "keep_copy".into())], true).unwrap();
    assert!(run(&mut conn, &cp).await.iter().all(|e| e.is_none()));
    let n: Option<i64> = conn.query_first(format!("SELECT COUNT(*) FROM `{db}`.keep_copy")).await.unwrap();
    assert_eq!(n, Some(3), "structure and rows copied");
    let idx: Option<i64> = conn.query_first(format!("SELECT COUNT(*) FROM information_schema.STATISTICS WHERE TABLE_SCHEMA='{db}' AND TABLE_NAME='keep_copy' AND INDEX_NAME='PRIMARY'")).await.unwrap();
    assert_eq!(idx, Some(1), "the copy keeps the primary key");
    assert!(run(&mut conn, &plan::empty_tables(&db, &[("keep_copy".into(), false)]).unwrap()).await.iter().all(|e| e.is_none()));
    let n: Option<i64> = conn.query_first(format!("SELECT COUNT(*) FROM `{db}`.keep_copy")).await.unwrap();
    assert_eq!(n, Some(0));
    assert!(run(&mut conn, &plan::replace_prefix(&db, &s(&["tmp_x", "tmp_y", "keep"]), "tmp_", "prod_").unwrap()).await.iter().all(|e| e.is_none()));
    assert!(tables(&mut conn, &db).await.contains(&"prod_x".to_string()) && tables(&mut conn, &db).await.contains(&"keep".to_string()));
    assert!(run(&mut conn, &plan::add_prefix(&db, &s(&["prod_x", "prod_y"]), "old_").unwrap()).await.iter().all(|e| e.is_none()));
    assert_eq!(tables(&mut conn, &db).await, ["keep", "keep_copy", "old_prod_x", "old_prod_y"]);
    let checksum = execute_script(&mut conn, None, &plan::checksum_tables(&db, &s(&["keep"])).unwrap().join(" ")).await;
    assert!(checksum[0].error.is_none() && checksum[0].sets[0].rows.len() == 1, "{checksum:?}");

    // ---- copy a database (structure and rows), then rename it by moving the tables
    let all = tables(&mut conn, &db).await;
    let plan_copy = plan::copy_database(&db, &copy, &all, true, Some("utf8mb4_bin")).unwrap();
    assert!(run(&mut conn, &plan_copy).await.iter().all(|e| e.is_none()));
    assert_eq!(tables(&mut conn, &copy).await, all);
    let n: Option<i64> = conn.query_first(format!("SELECT COUNT(*) FROM `{copy}`.keep")).await.unwrap();
    assert_eq!(n, Some(3));
    let coll: Option<String> = conn.query_first(format!("SELECT DEFAULT_COLLATION_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME='{copy}'")).await.unwrap();
    assert_eq!(coll.as_deref(), Some("utf8mb4_bin"));
    let rename = plan::rename_database(&copy, &moved, &all, 0, None).unwrap();
    assert!(run(&mut conn, &rename).await.iter().all(|e| e.is_none()), "{rename:?}");
    assert_eq!(tables(&mut conn, &moved).await, all, "every table moved, data included");
    let gone: Option<String> = conn.query_first(format!("SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME='{copy}'")).await.unwrap();
    assert!(gone.is_none(), "the old database is gone");
    let n: Option<i64> = conn.query_first(format!("SELECT COUNT(*) FROM `{moved}`.keep")).await.unwrap();
    assert_eq!(n, Some(3));

    // ---- ALTER DATABASE, and dropping the databases through the plan
    assert!(run(&mut conn, &plan::alter_database(&moved, None, Some("utf8mb4_unicode_ci")).unwrap()).await.iter().all(|e| e.is_none()));
    let coll: Option<String> = conn.query_first(format!("SELECT DEFAULT_COLLATION_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME='{moved}'")).await.unwrap();
    assert_eq!(coll.as_deref(), Some("utf8mb4_unicode_ci"));
    assert!(run(&mut conn, &plan::drop_databases(&[moved.clone()]).unwrap()).await.iter().all(|e| e.is_none()));
    let gone: Option<String> = conn.query_first(format!("SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME='{moved}'")).await.unwrap();
    assert!(gone.is_none());
}
