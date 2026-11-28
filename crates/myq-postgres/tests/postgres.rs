//! PostgreSQL through the engine contract, against a real server. Runs only when MYLODB_TEST_PG_PORT is set, so that a developer's
//! own server is never touched by accident. Creates and drops only its own `mylodb_itest_*` schema.

use myq_core::Backend;
use myq_data::{Assign, CellValue, Change, ColumnFilter, Dest, ExportFormat, ExportOptions, Op, PageSpec, Sort};
use myq_driver::{ConnConfig, SslMode};
use myq_postgres::{apply_changes, count_rows, estimate_count, export_query, export_table, fetch_cell, fetch_page, lookup, preview_changes, probe, table_meta, CancelToken, PgBackend};
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

fn text(column: &str, value: &str) -> Assign {
    Assign { column: column.into(), value: CellValue::Text(value.into()) }
}

fn filter(column: &str, op: Op, values: &[&str]) -> ColumnFilter {
    ColumnFilter { column: column.into(), op, values: values.iter().map(|v| v.to_string()).collect() }
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

const GRID: &str = "CREATE TYPE {s}.mood AS ENUM ('ok', 'sad');
CREATE TABLE {s}.\"Cust\" (id int GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, \"Name\" varchar(10) NOT NULL);
CREATE TABLE {s}.row (id serial PRIMARY KEY, cust int REFERENCES {s}.\"Cust\", note text, amount numeric(10,2), flag boolean DEFAULT true, at timestamptz, mood {s}.mood,
  tags text[], doc jsonb, raw bytea, twice numeric GENERATED ALWAYS AS (amount * 2) STORED);
INSERT INTO {s}.\"Cust\" (\"Name\") VALUES ('Ann'), ('Bob');
INSERT INTO {s}.row (cust, note, amount, at, mood, tags, doc, raw) SELECT 1 + g % 2, 'note ' || g, g, '2026-01-02 03:04:05.123456+00', 'ok', ARRAY['a,b', 'c'], '{\"k\": 1}', '\\x00ff' FROM generate_series(1, 25) g;
CREATE VIEW {s}.v AS SELECT id FROM {s}.row;
CREATE TABLE {s}.nokey (a int);
CREATE TABLE {s}.uniq (a int NOT NULL, b int NOT NULL, c int UNIQUE, UNIQUE (a, b))";

#[tokio::test]
async fn the_grid_pages_filters_and_counts() {
    with_schema("grid", |_, b, s| async move {
        let r = b.run(None, &GRID.replace("{s}", &s)).await.unwrap();
        assert!(r.iter().all(|r| r.error.is_none()), "{r:?}");
        let m = table_meta(&b, &s, "row").await.unwrap();
        assert_eq!((m.meta.primary_key.clone(), m.meta.row_key.clone(), m.meta.read_only_reason.clone()), (vec!["id".to_string()], vec!["id".to_string()], None));
        let col = |n: &str| m.meta.columns.iter().find(|c| c.name == n).unwrap();
        assert_eq!((col("id").data_type.as_str(), col("id").auto_increment, col("note").data_type.as_str(), col("raw").binary, col("twice").generated), ("int", true, "text", true, true));
        assert_eq!(col("flag").default.as_deref(), Some("true"));
        assert_eq!(m.meta.foreign_keys[0].ref_table, "Cust");
        assert_eq!(table_meta(&b, &s, "uniq").await.unwrap().meta.row_key, ["a", "b"].map(String::from).to_vec(), "a nullable unique column cannot identify a row");
        assert!(table_meta(&b, &s, "v").await.unwrap().meta.read_only_reason.unwrap().contains("views"));
        assert!(table_meta(&b, &s, "nokey").await.unwrap().meta.read_only_reason.is_some());
        assert!(table_meta(&b, &s, "missing").await.err().unwrap().contains("not found"));

        let page = fetch_page(&b, &m, &PageSpec { limit: 10, offset: 20, ..Default::default() }).await.unwrap();
        assert_eq!((page.rows.len(), page.has_more, page.has_prev, page.rows[0][0].as_deref()), (5, false, true, Some("21")));
        let first = &page.rows[0];
        let at = |n: &str| page.columns.iter().position(|c| c.name == n).unwrap();
        assert_eq!((first[at("raw")].as_deref(), first[at("flag")].as_deref(), first[at("tags")].as_deref()), (Some("00FF"), Some("true"), Some("{\"a,b\",c}")));

        // every filter kind, on text, numbers, dates, enums and json; the sort is by value, not by text
        let spec = |filters| PageSpec { filters, sort: vec![Sort { column: "id".into(), desc: true }], ..Default::default() };
        for (f, want) in [
            (filter("note", Op::Contains, &["NOTE 2"]), 7),
            (filter("note", Op::NotContains, &["note 2"]), 18),
            (filter("note", Op::StartsWith, &["note 1"]), 11),
            (filter("note", Op::EndsWith, &["5"]), 3),
            (filter("note", Op::Regexp, &["^note [0-9]$"]), 9),
            (filter("note", Op::IsEmpty, &[]), 0),
            (filter("amount", Op::Between, &["3", "5"]), 3),
            (filter("id", Op::In, &["1", "2", "99"]), 2),
            (filter("amount", Op::Gt, &["20"]), 5),
            (filter("mood", Op::Eq, &["ok"]), 25),
            (filter("at", Op::Le, &["2026-01-02 03:04:05.123456+00"]), 25),
            (filter("doc", Op::Contains, &["\"k\""]), 25),
            (filter("cust", Op::IsNull, &[]), 0),
        ] {
            let got = count_rows(&b, &m, std::slice::from_ref(&f), None, None).await.unwrap_or_else(|e| panic!("{f:?}: {e}"));
            assert_eq!(got, want, "{f:?}");
            assert_eq!(fetch_page(&b, &m, &spec(vec![f])).await.unwrap().rows.len() as u64, want);
        }
        assert_eq!(fetch_page(&b, &m, &spec(vec![])).await.unwrap().rows[0][0].as_deref(), Some("25"), "25 before 3: sorted as numbers");
        assert_eq!(count_rows(&b, &m, &[], Some("id < 4 -- comment"), None).await.unwrap(), 3);
        assert!(count_rows(&b, &m, &[filter("id", Op::Eq, &["x"])], None, None).await.is_err());
        assert!(count_rows(&b, &m, &[filter("nope", Op::Eq, &["1"])], None, None).await.is_err());
        // a typed condition cannot change anything, even by calling something that writes
        let seq = format!("nextval('{s}.row_id_seq') > 0");
        assert!(count_rows(&b, &m, &[], Some(&seq), None).await.unwrap_err().contains("25006"));
        b.run(None, &format!("ANALYZE {s}.row")).await.unwrap();
        assert_eq!(estimate_count(&b, &m, &[], None).await.unwrap(), Some(25));

        let cust = table_meta(&b, &s, "Cust").await.unwrap();
        let found = lookup(&b, &cust, "id", "bo", 10).await.unwrap();
        assert_eq!((found.len(), found[0].value.as_str(), found[0].label.as_deref()), (1, "2", Some("Bob")));
        let cell = fetch_cell(&b, &m, &[text("id", "1")], "raw").await.unwrap();
        assert_eq!((cell.len, cell.base64.as_deref()), (2, Some("AP8=")));
        b.run(None, &format!("UPDATE {s}.row SET note = repeat('é', 5000) WHERE id = 1")).await.unwrap();
        let page = fetch_page(&b, &m, &PageSpec { limit: 1, ..Default::default() }).await.unwrap();
        assert_eq!(page.long_cells.len(), 1);
        assert!(page.rows[0][page.long_cells[0].1].as_ref().unwrap().len() <= myq_data::CELL_PREVIEW);
        assert_eq!(fetch_cell(&b, &m, &[text("id", "1")], "note").await.unwrap().len, 10_000);
    })
    .await;
}

#[tokio::test]
async fn staged_edits_apply_together_or_not_at_all() {
    with_schema("edit", |cfg, b, s| async move {
        b.run(None, &GRID.replace("{s}", &s)).await.unwrap();
        let m = table_meta(&b, &s, "row").await.unwrap();
        let count = |sql: String| { let b = b.clone(); async move { b.run(None, &sql).await.unwrap()[0].sets[0].rows[0][0].clone().unwrap() } };
        let changes = vec![
            Change::Update { key: vec![text("id", "1")], set: vec![text("note", "O'Neil \\ x"), text("raw", "0xCAFE"), text("mood", "sad"), text("tags", "{x}"), Assign { column: "flag".into(), value: CellValue::Default }, Assign { column: "at".into(), value: CellValue::Null }] },
            Change::Insert { values: vec![text("cust", "2"), text("amount", "1.5"), Assign { column: "id".into(), value: CellValue::Default }] },
            Change::Delete { key: vec![text("id", "2")] },
        ];
        let shown = preview_changes(&m, &changes).unwrap();
        assert!(shown[0].contains("'O''Neil \\ x'") && shown[0].contains("'\\xCAFE'") && shown[0].contains("DEFAULT"), "{}", shown[0]);
        let (done, ran) = apply_changes(&b, &s, "row", &changes).await.unwrap();
        assert_eq!((done.applied, done.insert_ids.clone(), ran.len()), (3, vec![26], 3));
        assert_eq!(count(format!("SELECT concat_ws('|', note, encode(raw, 'hex'), mood, tags, flag, at IS NULL) FROM {s}.row WHERE id = 1")).await, "O'Neil \\ x|cafe|sad|{x}|t|t");
        // the preview is runnable SQL
        let r = b.run(None, &shown[0]).await.unwrap();
        assert!(r[0].error.is_none(), "{r:?}");

        // the second change fails, so the first is undone
        for bad in [
            Change::Delete { key: vec![text("id", "999")] },
            Change::Update { key: vec![text("id", "3")], set: vec![text("cust", "77")] },
            Change::Update { key: vec![text("id", "3")], set: vec![text("amount", "abc")] },
            Change::Update { key: vec![text("id", "3")], set: vec![text("twice", "1")] },
            Change::Update { key: vec![text("note", "x")], set: vec![text("amount", "1")] },
        ] {
            let e = apply_changes(&b, &s, "row", &[Change::Delete { key: vec![text("id", "4")] }, bad]).await.err().expect("refused");
            assert!(e.starts_with("change 2:"), "{e}");
        }
        assert_eq!(count(format!("SELECT count(*) FROM {s}.row WHERE id = 4")).await, "1");
        // a value too long for its column is refused, never cut
        let e = apply_changes(&b, &s, "Cust", &[Change::Update { key: vec![text("id", "1")], set: vec![text("Name", "much too long for ten")] }]).await.err().unwrap();
        assert!(e.contains("22001"), "{e}");
        let ro = PgBackend::new(ConnConfig { read_only: true, ..cfg });
        assert!(table_meta(&ro, &s, "row").await.unwrap().meta.read_only_reason.is_some());
        assert!(apply_changes(&ro, &s, "row", &[Change::Delete { key: vec![text("id", "4")] }]).await.err().unwrap().contains("read-only"));
    })
    .await;
}

#[tokio::test]
async fn exports_stream_to_every_kind_of_file() {
    with_schema("export", |_, b, s| async move {
        b.run(None, &GRID.replace("{s}", &s)).await.unwrap();
        let m = table_meta(&b, &s, "row").await.unwrap();
        let dir = std::env::temp_dir().join(format!("mylodb_pgx_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let options = |f: &str| -> ExportOptions { serde_json::from_value(serde_json::json!({ "format": f, "name": "row" })).unwrap() };
        let spec = PageSpec { filters: vec![filter("id", Op::Le, &["3"])], ..Default::default() };
        let csv = dir.join("t.csv");
        let out = export_table(&b, &m, &spec, &options("csv"), Dest::File(&csv), Arc::new(CancelToken::new()), &mut |_| true).await.unwrap();
        assert_eq!(out.rows, 3);
        assert!(std::fs::read_to_string(&csv).unwrap().contains("note 3"));
        // SQL INSERT output loads back into an empty copy of the table
        let sql = dir.join("t.sql");
        assert_eq!(options("sql_insert").format, ExportFormat::SqlInsert);
        export_query(&b, Some(&s), "SELECT id, name FROM (SELECT id, \"Name\" AS name FROM \"Cust\" ORDER BY id) q", &options("sql_insert"), Dest::File(&sql), Arc::new(CancelToken::new()), &mut |_| true).await.unwrap();
        let text = std::fs::read_to_string(&sql).unwrap();
        assert!(text.contains("INSERT INTO \"row\" (\"id\", \"name\")"), "{text}");
        let full = dir.join("full.sql");
        export_table(&b, &m, &spec, &options("sql_insert"), Dest::File(&full), Arc::new(CancelToken::new()), &mut |_| true).await.unwrap();
        // without the generated column, which cannot be inserted
        let insert = std::fs::read_to_string(&full).unwrap();
        let r = b.run(Some(&s), &format!("CREATE TABLE copy_of (LIKE row); {}", insert.replace("INSERT INTO \"row\"", "INSERT INTO copy_of"))).await.unwrap();
        assert!(r.iter().all(|r| r.error.is_none()), "{r:?}\n{insert}");
        let r = b.run(Some(&s), "SELECT count(*) FROM (SELECT id, note, amount, raw, tags, doc, at FROM row WHERE id <= 3 EXCEPT SELECT id, note, amount, raw, tags, doc, at FROM copy_of) d").await.unwrap();
        // jsonb and arrays compare by value
        assert_eq!(r[0].sets[0].rows[0][0].as_deref(), Some("0"));
        for f in ["json", "json_lines", "xml", "html", "xlsx"] {
            let p = dir.join(format!("q.{f}"));
            let out = export_query(&b, Some(&s), "TABLE \"Cust\"", &options(f), Dest::File(&p), Arc::new(CancelToken::new()), &mut |_| true).await.unwrap();
            assert!(out.rows == 2 && std::fs::metadata(&p).unwrap().len() > 0, "{f}");
        }
        for refused in ["DELETE FROM row", "SELECT 1; SELECT 2", "WITH d AS (DELETE FROM row RETURNING *) SELECT * FROM d"] {
            let e = export_query(&b, Some(&s), refused, &options("csv"), Dest::File(&dir.join("no.csv")), Arc::new(CancelToken::new()), &mut |_| true).await;
            assert!(e.is_err(), "{refused}");
        }
        // stopping part way
        let e = export_query(&b, None, "SELECT g FROM generate_series(1, 5000) g", &options("csv"), Dest::File(&dir.join("c.csv")), Arc::new(CancelToken::new()), &mut |_| false).await;
        assert_eq!(e.err().as_deref(), Some("cancelled"));
        assert!(b.run(None, "SELECT 1").await.unwrap()[0].error.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    })
    .await;
}
