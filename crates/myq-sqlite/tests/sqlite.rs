//! The SQLite engine against real files in a temp directory (no server needed).

use myq_core::{Backend, Kind};
use myq_data::{ColumnFilter, Op, PageSpec, Sort};
use myq_sqlite::{count_rows, fetch_page, table_meta, SqliteBackend, MAIN};
use rusqlite::Connection;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mylodb_itest_sqlite_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("shop.db");
    let c = Connection::open(&path).unwrap();
    c.execute_batch(
        "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, note TEXT, avatar BLOB);
         CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL REFERENCES customers(id), total REAL, placed TEXT);
         CREATE INDEX idx_orders_customer ON orders (customer_id, placed);
         CREATE UNIQUE INDEX idx_customers_name ON customers (name);
         CREATE VIEW v_big AS SELECT id FROM orders WHERE total > 100;
         CREATE TRIGGER trg_orders AFTER INSERT ON orders BEGIN SELECT 1; END;
         INSERT INTO customers VALUES (1, 'Ann', 'vip', x'CAFE'), (2, 'Bo_b', NULL, NULL), (3, '50% off', '', NULL);
         INSERT INTO orders VALUES (1, 1, 10.5, '2026-01-01'), (2, 1, 250, '2026-01-02'), (3, 2, 99.99, '2026-01-03');",
    )
    .unwrap();
    path
}

#[tokio::test]
async fn the_sqlite_backend_browses_a_file() {
    let path = fixture("browse");
    let b = SqliteBackend::open(&path).unwrap();

    let caps = b.capabilities().await.unwrap();
    assert!(!caps.users.available && !caps.writes.available);
    assert_eq!(b.dialect().quote_ident("a\"b"), "\"a\"\"b\"");

    let index = b.index(false).await.unwrap();
    let names = |k: Kind| index.iter().filter(|n| n.kind == k).map(|n| n.name.as_str()).collect::<Vec<_>>();
    assert_eq!(names(Kind::Database), [MAIN]);
    assert_eq!(names(Kind::Table), ["customers", "orders"]);
    assert_eq!(names(Kind::View), ["v_big"]);
    assert_eq!(index.iter().find(|n| n.kind == Kind::Trigger).and_then(|n| n.parent.clone()), Some("orders".to_string()));

    let dbs = b.databases().await.unwrap();
    assert_eq!(dbs.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), [MAIN]);

    let tables = b.tables(MAIN).await.unwrap();
    assert_eq!(tables.iter().map(|t| (t.name.as_str(), t.is_view)).collect::<Vec<_>>(), [("customers", false), ("orders", false), ("v_big", true)]);

    let cols = b.columns(MAIN, "customers").await.unwrap();
    assert_eq!(cols.iter().map(|c| (c.name.as_str(), c.column_type.as_str(), c.key.as_str(), c.nullable)).collect::<Vec<_>>(), [("id", "INTEGER", "PRI", false), ("name", "TEXT", "", false), ("note", "TEXT", "", true), ("avatar", "BLOB", "", true)]);

    let idx = b.indexes(MAIN, "orders").await.unwrap();
    assert!(idx.iter().any(|i| i.name == "PRIMARY" && i.unique && i.columns == ["id"]), "{idx:?}");
    assert!(idx.iter().any(|i| i.name == "idx_orders_customer" && !i.unique && i.columns == ["customer_id", "placed"]), "{idx:?}");
    assert!(b.indexes(MAIN, "customers").await.unwrap().iter().any(|i| i.name == "idx_customers_name" && i.unique));

    let col_nodes = b.column_nodes(MAIN).await.unwrap();
    assert_eq!(col_nodes.iter().filter(|n| n.parent.as_deref() == Some("orders")).count(), 4);
}

#[tokio::test]
async fn running_sql_reports_results_nulls_blobs_and_errors_and_cannot_write() {
    let path = fixture("run");
    let b = SqliteBackend::open(&path).unwrap();
    let r = b.run(None, "SELECT id, note, avatar FROM customers ORDER BY id; SELECT nope FROM customers; SELECT 1;").await.unwrap();
    assert_eq!(r.len(), 3);
    let rows = &r[0].sets[0].rows;
    assert_eq!(rows[0], vec![Some("1".into()), Some("vip".into()), Some("CAFE".into())], "a BLOB is shown as hex");
    assert_eq!(rows[1], vec![Some("2".into()), None, None], "NULL is not the empty string");
    assert_eq!(rows[2][1], Some(String::new()));
    assert!(r[0].sets[0].columns[2].binary);
    assert!(r[1].error.as_deref().is_some_and(|e| e.contains("nope")), "{:?}", r[1].error);
    assert_eq!(r[2].sets[0].rows[0][0].as_deref(), Some("1"), "a later statement still runs after an error");

    let w = b.run(None, "DELETE FROM customers; DROP TABLE orders; INSERT INTO customers VALUES (9, 'x', NULL, NULL)").await.unwrap();
    assert!(w.iter().all(|s| s.error.as_deref().is_some_and(|e| e.to_lowercase().contains("readonly") || e.to_lowercase().contains("read-only") || e.to_lowercase().contains("read only"))), "{w:?}");
    let still: Vec<_> = b.run(None, "SELECT count(*) FROM customers").await.unwrap();
    assert_eq!(still[0].sets[0].rows[0][0].as_deref(), Some("3"), "the file is unchanged");
}

#[test]
fn the_grid_pages_filters_sorts_and_counts() {
    let path = fixture("grid");
    let meta = table_meta(&path, "customers").unwrap();
    assert_eq!((meta.primary_key.clone(), meta.row_key.clone()), (vec!["id".to_string()], vec!["id".to_string()]));
    assert!(meta.read_only_reason.is_some() && !meta.is_view);
    assert!(meta.columns.iter().find(|c| c.name == "avatar").unwrap().binary);
    let orders = table_meta(&path, "orders").unwrap();
    assert_eq!(orders.foreign_keys.len(), 1);
    assert_eq!((orders.foreign_keys[0].columns.clone(), orders.foreign_keys[0].ref_table.clone(), orders.foreign_keys[0].ref_columns.clone()), (vec!["customer_id".to_string()], "customers".to_string(), vec!["id".to_string()]));
    assert!(table_meta(&path, "missing").is_err());

    let page = fetch_page(&path, &meta, &PageSpec { limit: 2, ..Default::default() }).unwrap();
    assert_eq!((page.rows.len(), page.has_more, page.has_prev, page.keyset), (2, true, false, false));
    let next = fetch_page(&path, &meta, &PageSpec { limit: 2, offset: 2, ..Default::default() }).unwrap();
    assert_eq!((next.rows.len(), next.has_more, next.has_prev), (1, false, true));

    let sorted = fetch_page(&path, &meta, &PageSpec { sort: vec![Sort { column: "name".into(), desc: true }], ..Default::default() }).unwrap();
    assert_eq!(sorted.rows.iter().map(|r| r[1].clone().unwrap()).collect::<Vec<_>>(), ["Bo_b", "Ann", "50% off"]);

    // LIKE wildcards in the typed text are literal: "_" must not match any character, "%" must not match anything
    let f = |op: Op, v: &str| vec![ColumnFilter { column: "name".into(), op, values: vec![v.to_string()] }];
    assert_eq!(count_rows(&path, &meta, &f(Op::Contains, "o_b"), None).unwrap(), 1);
    assert_eq!(count_rows(&path, &meta, &f(Op::Contains, "n_n"), None).unwrap(), 0, "`_` is not a wildcard");
    assert_eq!(count_rows(&path, &meta, &f(Op::StartsWith, "50%"), None).unwrap(), 1);
    assert_eq!(count_rows(&path, &meta, &f(Op::Contains, "%"), None).unwrap(), 1, "`%` is not a wildcard");
    assert_eq!(count_rows(&path, &meta, &[], Some("id > 1")).unwrap(), 2);
    assert_eq!(count_rows(&path, &meta, &[], None).unwrap(), 3);
    assert!(count_rows(&path, &meta, &f(Op::Regexp, "x"), None).is_err());
    assert!(count_rows(&path, &meta, &[ColumnFilter { column: "nope".into(), op: Op::Eq, values: vec!["1".into()] }], None).is_err());

    let blob = fetch_page(&path, &meta, &PageSpec { filters: vec![ColumnFilter { column: "id".into(), op: Op::Eq, values: vec!["1".into()] }], ..Default::default() }).unwrap();
    assert_eq!(blob.rows[0][3].as_deref(), Some("CAFE"));
}

#[test]
fn a_file_that_is_not_a_database_is_refused_clearly() {
    let dir = std::env::temp_dir().join(format!("mylodb_itest_sqlite_{}_bad", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let junk = dir.join("junk.db");
    std::fs::write(&junk, "this is not sqlite, just text that is long enough to look like a header? no.").unwrap();
    let e = SqliteBackend::open(&junk).unwrap_err();
    assert!(e.contains("not a SQLite database") || e.contains("cannot open"), "{e}");
    assert!(SqliteBackend::open(dir.join("missing.db")).is_err());
    assert!(!dir.join("missing.db").exists(), "opening never creates a file");
}
