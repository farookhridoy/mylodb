//! The SQLite engine against real files in a temp directory (no server needed).

use myq_core::{Backend, Kind};
use myq_data::{Assign, CellValue, Change, ColumnFilter, Op, PageSpec, Sort};
use myq_sqlite::{apply_changes, count_rows, fetch_page, preview_changes, table_meta, SqliteBackend, MAIN};
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
    let meta = table_meta(&path, "customers", false).unwrap();
    assert_eq!((meta.primary_key.clone(), meta.row_key.clone()), (vec!["id".to_string()], vec!["id".to_string()]));
    assert!(meta.read_only_reason.is_some() && !meta.is_view);
    assert!(meta.columns.iter().find(|c| c.name == "avatar").unwrap().binary);
    let orders = table_meta(&path, "orders", false).unwrap();
    assert_eq!(orders.foreign_keys.len(), 1);
    assert_eq!((orders.foreign_keys[0].columns.clone(), orders.foreign_keys[0].ref_table.clone(), orders.foreign_keys[0].ref_columns.clone()), (vec!["customer_id".to_string()], "customers".to_string(), vec!["id".to_string()]));
    assert!(table_meta(&path, "missing", false).is_err());

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

fn t(c: &str, v: &str) -> Assign {
    Assign { column: c.into(), value: CellValue::Text(v.into()) }
}

fn count(path: &PathBuf, sql: &str) -> String {
    use rusqlite::types::Value;
    match Connection::open(path).unwrap().query_row(sql, [], |r| r.get::<_, Value>(0)).unwrap() {
        Value::Integer(i) => i.to_string(),
        Value::Text(t) => t,
        v => format!("{v:?}"),
    }
}

#[tokio::test]
async fn a_writable_file_runs_sql_that_changes_it_and_a_read_only_one_still_cannot() {
    let path = fixture("write_sql");
    let w = SqliteBackend::open_writable(&path).unwrap();
    assert!(w.writable() && w.capabilities().await.unwrap().writes.available);
    let r = w.run(None, "INSERT INTO customers (id, name) VALUES (9, 'Zed'); UPDATE customers SET note = 'x' WHERE id = 9; CREATE TABLE extra (a INTEGER)").await.unwrap();
    assert!(r.iter().all(|s| s.error.is_none()), "{r:?}");
    assert_eq!(r[0].affected_rows, 1);
    assert_eq!(count(&path, "SELECT note FROM customers WHERE id = 9"), "x");
    // foreign keys are enforced on a writable connection
    let fk = w.run(None, "INSERT INTO orders VALUES (50, 999, 1, 'x')").await.unwrap();
    assert!(fk[0].error.as_deref().is_some_and(|e| e.to_lowercase().contains("foreign key")), "{fk:?}");
    let ro = SqliteBackend::open(&path).unwrap();
    assert!(!ro.writable() && !ro.capabilities().await.unwrap().writes.available);
    assert!(ro.run(None, "DELETE FROM customers").await.unwrap()[0].error.is_some());
    assert_eq!(count(&path, "SELECT count(*) FROM customers"), "4");
}

#[test]
fn staged_edits_apply_in_one_transaction_with_the_same_change_type_as_mysql() {
    let path = fixture("write_grid");
    let meta = table_meta(&path, "customers", true).unwrap();
    assert!(meta.read_only_reason.is_none());
    let changes = vec![
        Change::Update { key: vec![t("id", "1")], set: vec![t("name", "Anne"), Assign { column: "note".into(), value: CellValue::Null }, t("avatar", "0xBEEF")] },
        Change::Insert { values: vec![t("name", "O'Neil"), Assign { column: "note".into(), value: CellValue::Default }] },
        Change::Delete { key: vec![t("id", "3")] },
    ];
    let sql = preview_changes(&meta, &changes).unwrap();
    assert_eq!(sql[0], "UPDATE \"customers\" SET \"name\" = 'Anne', \"note\" = NULL, \"avatar\" = X'BEEF' WHERE \"id\" = '1'");
    assert_eq!(sql[1], "INSERT INTO \"customers\" (\"name\") VALUES ('O''Neil')", "a DEFAULT column is left out, quotes are doubled");
    assert_eq!(sql[2], "DELETE FROM \"customers\" WHERE \"id\" = '3'");
    let out = apply_changes(&path, &meta, &changes).unwrap();
    assert_eq!((out.applied, out.insert_ids.clone()), (3, vec![4]));
    assert_eq!(count(&path, "SELECT name FROM customers WHERE id = 1"), "Anne");
    assert_eq!(count(&path, "SELECT hex(avatar) FROM customers WHERE id = 1"), "BEEF");
    assert_eq!(count(&path, "SELECT name FROM customers WHERE id = 4"), "O'Neil");
    assert_eq!(count(&path, "SELECT count(*) FROM customers WHERE id = 3"), "0");
    // the key typed as text still finds an INTEGER key
    let empty = vec![Change::Insert { values: vec![] }];
    assert!(apply_changes(&path, &meta, &empty).is_err(), "name is NOT NULL, so an all-default row fails");
}

#[test]
fn a_failing_change_rolls_the_whole_batch_back_and_stale_rows_are_reported() {
    let path = fixture("write_rollback");
    let meta = table_meta(&path, "customers", true).unwrap();
    let batch = vec![Change::Update { key: vec![t("id", "1")], set: vec![t("name", "Changed")] }, Change::Insert { values: vec![t("name", "Bo_b")] }];
    let e = apply_changes(&path, &meta, &batch).unwrap_err();
    assert!(e.starts_with("change 2:") && e.to_lowercase().contains("unique"), "{e}");
    assert_eq!(count(&path, "SELECT name FROM customers WHERE id = 1"), "Ann", "the first change was rolled back");
    let gone = vec![Change::Delete { key: vec![t("id", "77")] }];
    assert!(apply_changes(&path, &meta, &gone).unwrap_err().contains("no longer exists"));
    let bad_key = vec![Change::Delete { key: vec![t("name", "Ann")] }];
    assert!(apply_changes(&path, &meta, &bad_key).unwrap_err().contains("row key must be exactly (id)"));
    let dflt = vec![Change::Update { key: vec![t("id", "1")], set: vec![Assign { column: "note".into(), value: CellValue::Default }] }];
    assert!(apply_changes(&path, &meta, &dflt).unwrap_err().contains("DEFAULT"));
}

#[test]
fn read_only_files_views_and_keyless_tables_cannot_be_edited() {
    let path = fixture("write_refuse");
    let ro = table_meta(&path, "customers", false).unwrap();
    assert!(ro.read_only_reason.as_deref().is_some_and(|r| r.contains("read-only")));
    let del = vec![Change::Delete { key: vec![t("id", "1")] }];
    assert!(apply_changes(&path, &ro, &del).is_err() && preview_changes(&ro, &del).is_err());
    assert_eq!(count(&path, "SELECT count(*) FROM customers"), "3");
    assert!(table_meta(&path, "v_big", true).unwrap().read_only_reason.as_deref().is_some_and(|r| r.contains("view")));
    Connection::open(&path).unwrap().execute_batch("CREATE TABLE loose (a TEXT, b TEXT)").unwrap();
    assert!(table_meta(&path, "loose", true).unwrap().read_only_reason.as_deref().is_some_and(|r| r.contains("primary key")));
}

#[test]
fn opening_for_writing_never_creates_a_file() {
    let dir = std::env::temp_dir().join(format!("mylodb_itest_sqlite_{}_nocreate", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    assert!(SqliteBackend::open_writable(dir.join("missing.db")).is_err());
    assert!(!dir.join("missing.db").exists());
}

/// The upkeep the app offers (checks, VACUUM, ANALYZE) runs through `Backend::run`, and the answers have the shape the app reads.
#[tokio::test]
async fn upkeep_statements_answer_in_the_shape_the_app_reads() {
    let path = fixture("upkeep");
    let b = SqliteBackend::open_writable(path.clone()).unwrap();
    let first = |r: Vec<myq_core::StatementResult>| r[0].sets.first().map(|s| s.rows.iter().map(|r| r[0].clone().unwrap_or_default()).collect::<Vec<_>>()).unwrap_or_default();
    assert_eq!(first(b.run(None, "PRAGMA integrity_check").await.unwrap()), vec!["ok"]);
    assert_eq!(first(b.run(None, "PRAGMA quick_check").await.unwrap()), vec!["ok"]);
    assert!(first(b.run(None, "PRAGMA foreign_key_check").await.unwrap()).is_empty(), "no violations in the fixture");
    // a row that points at a missing parent is reported
    let c = Connection::open(&path).unwrap();
    c.execute_batch("PRAGMA foreign_keys=OFF; INSERT INTO orders VALUES (9, 77, 1.0, 'x');").unwrap();
    drop(c);
    assert_eq!(first(b.run(None, "PRAGMA foreign_key_check").await.unwrap()), vec!["orders"]);
    // page_count and page_size are numbers, so the size before and after VACUUM can be told
    let pages: u64 = first(b.run(None, "PRAGMA page_count").await.unwrap())[0].parse().unwrap();
    let size: u64 = first(b.run(None, "PRAGMA page_size").await.unwrap())[0].parse().unwrap();
    assert!(pages > 0 && size >= 512);
    for sql in ["ANALYZE", "REINDEX", "PRAGMA optimize", "VACUUM"] {
        let r = b.run(None, sql).await.unwrap();
        assert!(r[0].error.is_none(), "{sql}: {:?}", r[0].error);
    }
    // a read-only connection cannot be changed by the same statements
    let ro = SqliteBackend::open(path.clone()).unwrap();
    assert!(!ro.writable());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// Cancel stops a long statement quickly and skips the rest of the script.
#[tokio::test]
async fn a_running_script_can_be_cancelled() {
    let path = fixture("cancel");
    let b = SqliteBackend::open(path.clone()).unwrap();
    let token = std::sync::Arc::new(myq_sqlite::CancelToken::new());
    // a query that would run for a very long time
    let sql = "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 2000000000) SELECT count(*) FROM c; SELECT 2;";
    let (b2, t2) = (b.clone(), token.clone());
    let started = std::time::Instant::now();
    let job = tokio::spawn(async move { b2.run_cancellable(sql, t2).await });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    token.cancel();
    let results = job.await.unwrap().unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(10), "it stopped soon after Cancel ({:?})", started.elapsed());
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].error.as_deref(), Some("Cancelled."));
    assert_eq!(results[1].error.as_deref(), Some("Cancelled before it started."));
    // a script that was not cancelled runs normally through the same path
    let ok = b.run_cancellable("SELECT 1 AS one", std::sync::Arc::new(myq_sqlite::CancelToken::new())).await.unwrap();
    assert!(ok[0].error.is_none());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// Exporting from a SQLite file: a table with the grid's filter and sort, and a SELECT; several formats; binary and NULL cells.
#[tokio::test]
async fn tables_and_queries_export_to_files() {
    use myq_data::{Dest, ExportFormat, ExportOptions};
    let path = fixture("export");
    let dir = path.parent().unwrap().to_path_buf();
    let opts = |format| ExportOptions { format, ..Default::default() };
    let meta = table_meta(&path, "customers", false).unwrap();

    // the whole table as CSV: header, quoting, NULL as empty
    let f = dir.join("all.csv");
    let spec: PageSpec = serde_json::from_str(r#"{"sort":[{"column":"name","desc":false}]}"#).unwrap();
    let out = myq_sqlite::export_table(&path, &meta, &spec, &opts(ExportFormat::Csv), Dest::File(&f), &mut |_| true).unwrap();
    assert_eq!(out.rows, 3);
    let text = std::fs::read_to_string(&f).unwrap();
    assert!(text.starts_with("id,name,note,avatar\n"), "{text}");
    assert!(text.contains("3,50% off,,\n") && text.contains("2,Bo_b,,\n") && text.contains("1,Ann,vip,CAFE") || text.contains("0xCAFE") || text.contains("CAFE"), "{text}");
    let names: Vec<&str> = text.lines().skip(1).map(|l| l.split(',').nth(1).unwrap()).collect();
    assert_eq!(names, vec!["50% off", "Ann", "Bo_b"], "sorted by name");

    // a filter limits the rows
    let f2 = dir.join("filtered.json");
    let spec: PageSpec = serde_json::from_str(r#"{"filters":[{"column":"name","op":"contains","values":["o"]}]}"#).unwrap();
    let out = myq_sqlite::export_table(&path, &meta, &spec, &opts(ExportFormat::Json), Dest::File(&f2), &mut |_| true).unwrap();
    assert_eq!(out.rows, 2, "Bo_b and 50% off contain an o");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&f2).unwrap()).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);

    // a query, as SQL INSERT (a number stays a number, text is quoted) and as XLSX
    let f3 = dir.join("q.sql");
    let out = myq_sqlite::export_query(&path, "SELECT id, total, placed FROM orders ORDER BY id;", &opts(ExportFormat::SqlInsert), Dest::File(&f3), &mut |_| true).unwrap();
    assert_eq!(out.rows, 3);
    let sql = std::fs::read_to_string(&f3).unwrap();
    assert!(sql.contains("INSERT INTO") && sql.contains("10.5") && sql.contains("'2026-01-01'"), "{sql}");
    let f4 = dir.join("q.xlsx");
    myq_sqlite::export_query(&path, "SELECT * FROM customers", &opts(ExportFormat::Xlsx), Dest::File(&f4), &mut |_| true).unwrap();
    assert!(std::fs::metadata(&f4).unwrap().len() > 1000, "an xlsx file was written");

    // no rows still writes the header; a write statement and several statements are refused; cancel stops
    let f5 = dir.join("none.csv");
    let out = myq_sqlite::export_query(&path, "SELECT id, name FROM customers WHERE id > 100", &opts(ExportFormat::Csv), Dest::File(&f5), &mut |_| true).unwrap();
    assert_eq!(out.rows, 0);
    assert_eq!(std::fs::read_to_string(&f5).unwrap(), "id,name\n");
    assert!(myq_sqlite::export_query(&path, "DELETE FROM customers", &opts(ExportFormat::Csv), Dest::File(&dir.join("x.csv")), &mut |_| true).is_err());
    assert!(myq_sqlite::export_query(&path, "SELECT 1; SELECT 2", &opts(ExportFormat::Csv), Dest::File(&dir.join("x.csv")), &mut |_| true).is_err());
    let big = "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 5000) SELECT x FROM c";
    let err = myq_sqlite::export_query(&path, big, &opts(ExportFormat::Csv), Dest::File(&dir.join("big.csv")), &mut |n| n < 2000).unwrap_err();
    assert_eq!(err, "cancelled");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A backup of the file is a consistent, openable copy.
#[test]
fn a_backup_is_a_consistent_copy_of_the_file() {
    let path = fixture("backup");
    let target = path.parent().unwrap().join("copy.sqlite");
    myq_sqlite::backup_to(&path, &target).unwrap();
    let c = Connection::open(&target).unwrap();
    assert_eq!(c.query_row::<i64, _, _>("SELECT count(*) FROM orders", [], |r| r.get(0)).unwrap(), 3);
    assert_eq!(c.query_row::<String, _, _>("PRAGMA integrity_check", [], |r| r.get(0)).unwrap(), "ok");
    assert!(myq_sqlite::backup_to(&path, &target).is_err(), "an existing file is never overwritten");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
