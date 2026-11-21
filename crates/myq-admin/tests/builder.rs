//! The query builder's SQL on a real server: it must parse, run and return the rows the spec describes.
//! Uses its own `mylodb_itest_qb_*` database.

use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::{pool, ConnConfig, SslMode};
use myq_schema::builder::*;

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

fn t(db: &str, table: &str, alias: &str) -> TableRef {
    TableRef { db: db.into(), table: table.into(), alias: alias.into() }
}
fn sel(a: &str, c: &str) -> Select {
    Select { alias: a.into(), column: c.into(), name: None, agg: None }
}

#[tokio::test]
async fn built_queries_run_and_return_what_the_spec_says() {
    let p = pool(&cfg());
    let mut c = match p.get_conn().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP: no MySQL reachable: {e}");
            return;
        }
    };
    let db = format!("mylodb_itest_qb_{}", std::process::id());
    c.query_drop(format!("CREATE DATABASE `{db}`")).await.unwrap();
    let result = async {
        for sql in [
            format!("CREATE TABLE `{db}`.customers (id INT PRIMARY KEY, name VARCHAR(40), country VARCHAR(10))"),
            format!("INSERT INTO `{db}`.customers VALUES (1, 'Ann', 'DE'), (2, 'O''Neil', 'DE'), (3, '50% off_x', 'FR'), (4, 'Dee', NULL)"),
            format!("CREATE TABLE `{db}`.orders (id INT PRIMARY KEY, customer_id INT, total DECIMAL(10,2))"),
            format!("INSERT INTO `{db}`.orders VALUES (1, 1, 10.50), (2, 1, 20.00), (3, 2, 5.00), (4, 3, 99.99)"),
        ] {
            c.query_drop(sql).await.map_err(|e| format!("setup: {e}"))?;
        }
        let from = t(&db, "customers", "c");
        let join_orders = Join { kind: JoinKind::Left, table: t(&db, "orders", "o"), on: vec![(Side { alias: "c".into(), column: "id".into() }, "customer_id".into())] };
        let run = |sql: String| async move { sql };
        let _ = run;

        // a join with an aggregate, grouped and ordered
        let spec = QuerySpec { from: from.clone(), joins: vec![join_orders.clone()], select: vec![sel("c", "name"), Select { agg: Some(Agg::Sum), name: Some("spent".into()), ..sel("o", "total") }], distinct: false, filters: vec![], order: vec![Order { alias: "c".into(), column: "name".into(), desc: false }], limit: None };
        let sql = build(&spec)?;
        let rows: Vec<(String, Option<String>)> = c.query(sql.trim_end_matches(';')).await.map_err(|e| format!("{sql}\n{e}"))?;
        assert_eq!(rows, [("50% off_x".to_string(), Some("99.99".to_string())), ("Ann".into(), Some("30.50".into())), ("Dee".into(), None), ("O'Neil".into(), Some("5.00".into()))]);

        // text filters with quotes and wildcards taken literally
        for (needle, expect) in [("O'Neil", 1usize), ("50%", 1), ("%", 1), ("_x", 1), ("n_", 0), ("an", 1)] {
            let spec = QuerySpec { from: from.clone(), joins: vec![], select: vec![sel("c", "id")], distinct: false, filters: vec![Filter { alias: "c".into(), column: "name".into(), op: Op::Contains, values: vec![needle.into()], numeric: false }], order: vec![], limit: None };
            let sql = build(&spec)?;
            let n: Vec<i64> = c.query(sql.trim_end_matches(';')).await.map_err(|e| format!("{sql}\n{e}"))?;
            assert_eq!(n.len(), expect, "contains {needle:?}: {sql}");
        }

        // IN, BETWEEN, IS NULL, numeric comparison, DISTINCT and LIMIT
        let spec = QuerySpec { from: from.clone(), joins: vec![], select: vec![sel("c", "country")], distinct: true, filters: vec![Filter { alias: "c".into(), column: "id".into(), op: Op::Between, values: vec!["1".into(), "3".into()], numeric: true }, Filter { alias: "c".into(), column: "country".into(), op: Op::In, values: vec!["DE, FR".into()], numeric: false }], order: vec![Order { alias: "c".into(), column: "country".into(), desc: true }], limit: Some(1) };
        let sql = build(&spec)?;
        let v: Vec<String> = c.query(sql.trim_end_matches(';')).await.map_err(|e| format!("{sql}\n{e}"))?;
        assert_eq!(v, ["FR"]);
        let nulls = QuerySpec { filters: vec![Filter { alias: "c".into(), column: "country".into(), op: Op::IsNull, values: vec![], numeric: false }], select: vec![sel("c", "id")], distinct: false, order: vec![], limit: None, ..spec };
        let sql = build(&nulls)?;
        let ids: Vec<i64> = c.query(sql.trim_end_matches(';')).await.map_err(|e| format!("{sql}\n{e}"))?;
        assert_eq!(ids, [4]);
        Ok::<(), String>(())
    }
    .await;
    c.query_drop(format!("DROP DATABASE IF EXISTS `{db}`")).await.unwrap();
    result.unwrap();
}
