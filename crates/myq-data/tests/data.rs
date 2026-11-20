//! Data-access integration test against a local MySQL. Uses its own uniquely named database,
//! dropped afterwards even if an assertion fails.

use myq_data::*;
use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::{pool, ConnConfig, Conn, Pool, SslMode};

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

fn filter(column: &str, op: Op, values: &[&str]) -> ColumnFilter {
    ColumnFilter { column: column.into(), op, values: values.iter().map(|s| s.to_string()).collect() }
}
fn text(c: &str, v: &str) -> Assign {
    Assign { column: c.into(), value: CellValue::Text(v.into()) }
}
fn spec() -> PageSpec {
    PageSpec { limit: 100, ..Default::default() }
}

#[tokio::test]
async fn data_grid_backend_end_to_end() {
    let pool = pool(&cfg());
    let mut admin = match pool.get_conn().await {
        Ok(c) => c,
        Err(e) => return eprintln!("SKIP: no MySQL reachable: {e}"),
    };
    let db = format!("mylodb_itest_data_{}", std::process::id());
    admin.query_drop(format!("CREATE DATABASE `{db}`")).await.unwrap();
    let outcome = tokio::spawn(body(pool.clone(), db.clone())).await;
    admin.query_drop(format!("DROP DATABASE `{db}`")).await.unwrap();
    outcome.unwrap();
}

async fn body(pool: Pool, db: String) {
    let db = db.as_str();
    let mut c = pool.get_conn().await.unwrap();
    c.query_drop(format!("USE `{db}`")).await.unwrap();
    for stmt in [
        "CREATE TABLE customers (id INT PRIMARY KEY AUTO_INCREMENT, name VARCHAR(40) NOT NULL, email VARCHAR(80) NULL)",
        "CREATE TABLE orders (id INT PRIMARY KEY AUTO_INCREMENT, customer_id INT NOT NULL, total DECIMAL(10,2) NOT NULL,
            placed DATETIME NOT NULL, day DATE NULL, note TEXT NULL, img BLOB NULL, meta JSON NULL, flag BIT(3) NULL,
            gen INT AS (customer_id * 2) STORED, FOREIGN KEY (customer_id) REFERENCES customers(id))",
        "CREATE TABLE nokey (a INT, b VARCHAR(10))",
        "CREATE TABLE uniq (code VARCHAR(5) NOT NULL, v INT, UNIQUE KEY (code))",
        "CREATE VIEW v_orders AS SELECT id, total FROM orders",
        "INSERT INTO customers (name, email) VALUES ('Ann', 'ann@x.io'), ('Bob', NULL), ('50%_Off Co', ''), ('Dee', 'dee@y.io')",
        "INSERT INTO orders (customer_id, total, placed, day, note, img, meta, flag) VALUES
            (1, 10.50, '2024-01-02 03:04:05', '2024-01-02', 'first', X'CAFEBABE', '{\"a\": 1}', b'101'),
            (1, 99.99, '2024-02-03 00:00:00', NULL, NULL, NULL, NULL, NULL),
            (2, 5.00, '2024-03-04 10:00:00', '2024-03-04', 'third', X'00FF', NULL, NULL)",
        "INSERT INTO nokey VALUES (1, 'x'), (1, 'x')",
        "INSERT INTO uniq VALUES ('a', 1)",
    ] {
        c.query_drop(stmt).await.unwrap();
    }

    // ---- metadata ----
    let orders = table_meta(&mut c, db, "orders").await.unwrap();
    assert_eq!((orders.primary_key.clone(), orders.row_key.clone()), (vec!["id".to_string()], vec!["id".to_string()]));
    assert_eq!(orders.foreign_keys.len(), 1);
    assert_eq!((orders.foreign_keys[0].ref_table.as_str(), orders.foreign_keys[0].columns[0].as_str()), ("customers", "customer_id"));
    assert!(orders.column("img").unwrap().binary && orders.column("gen").unwrap().generated);
    assert!(orders.column("id").unwrap().auto_increment && orders.read_only_reason.is_none());
    let nokey = table_meta(&mut c, db, "nokey").await.unwrap();
    assert!(nokey.read_only_reason.is_some() && nokey.row_key.is_empty());
    assert_eq!(table_meta(&mut c, db, "uniq").await.unwrap().row_key, vec!["code".to_string()], "non-null unique key identifies rows");
    let view = table_meta(&mut c, db, "v_orders").await.unwrap();
    assert!(view.is_view && view.read_only_reason.is_some());
    assert!(matches!(table_meta(&mut c, db, "missing").await, Err(Error::TableNotFound(_))));

    // ---- typed cell formatting ----
    let page = fetch_page(&mut c, &orders, &PageSpec { sort: vec![Sort { column: "id".into(), desc: false }], ..spec() }).await.unwrap();
    let col = |n: &str| page.columns.iter().position(|c| c.name == n).unwrap();
    assert_eq!(page.rows.len(), 3);
    assert_eq!(page.rows[0][col("total")].as_deref(), Some("10.50"), "DECIMAL keeps its scale");
    assert_eq!(page.rows[0][col("placed")].as_deref(), Some("2024-01-02 03:04:05"));
    assert_eq!(page.rows[1][col("placed")].as_deref(), Some("2024-02-03 00:00:00"), "midnight DATETIME keeps its time");
    assert_eq!(page.rows[0][col("day")].as_deref(), Some("2024-01-02"));
    assert_eq!(page.rows[1][col("day")], None);
    assert_eq!(page.rows[0][col("img")].as_deref(), Some("0xCAFEBABE"));
    assert_eq!(page.rows[0][col("flag")].as_deref(), Some("5"), "BIT(3) b'101'");
    assert_eq!(page.rows[0][col("gen")].as_deref(), Some("2"));
    assert!(page.rows[0][col("meta")].as_deref().unwrap().contains("\"a\""));
    assert!(page.columns[col("img")].binary);

    // ---- filters (each operator against real rows) ----
    let names = |rows: &[Vec<Option<String>>], i: usize| rows.iter().map(|r| r[i].clone().unwrap_or_default()).collect::<Vec<_>>();
    let customers = table_meta(&mut c, db, "customers").await.unwrap();
    let run = |f: Vec<ColumnFilter>| PageSpec { filters: f, sort: vec![Sort { column: "id".into(), desc: false }], ..spec() };
    let cases: Vec<(ColumnFilter, Vec<&str>)> = vec![
        (filter("name", Op::Contains, &["o"]), vec!["Bob", "50%_Off Co"]),
        (filter("name", Op::Contains, &["%_"]), vec!["50%_Off Co"]),          // wildcards are literal
        (filter("name", Op::NotContains, &["o"]), vec!["Ann", "Dee"]),
        (filter("name", Op::StartsWith, &["A"]), vec!["Ann"]),
        (filter("name", Op::EndsWith, &["e"]), vec!["Dee"]),
        (filter("name", Op::Eq, &["Bob"]), vec!["Bob"]),
        (filter("name", Op::Ne, &["Bob"]), vec!["Ann", "50%_Off Co", "Dee"]),
        (filter("id", Op::Gt, &["2"]), vec!["50%_Off Co", "Dee"]),
        (filter("id", Op::Le, &["2"]), vec!["Ann", "Bob"]),
        (filter("id", Op::Between, &["2", "3"]), vec!["Bob", "50%_Off Co"]),
        (filter("name", Op::In, &["Ann", "Dee"]), vec!["Ann", "Dee"]),
        (filter("name", Op::Regexp, &["^[AB]"]), vec!["Ann", "Bob"]),
        (filter("email", Op::IsNull, &[]), vec!["Bob"]),
        (filter("email", Op::NotNull, &[]), vec!["Ann", "50%_Off Co", "Dee"]),
        (filter("email", Op::IsEmpty, &[]), vec!["50%_Off Co"]),
    ];
    for (f, want) in cases {
        let desc = format!("{:?}", f.op);
        let p = fetch_page(&mut c, &customers, &run(vec![f.clone()])).await.unwrap();
        assert_eq!(names(&p.rows, 1), want, "filter {desc}");
        assert_eq!(count_rows(&mut c, &customers, &[f], None).await.unwrap(), want.len() as u64, "count {desc}");
    }
    // combined column filters + raw WHERE + injection attempt is just a bound value
    let combo = run(vec![filter("name", Op::Contains, &["e"]), filter("email", Op::NotNull, &[])]);
    assert_eq!(names(&fetch_page(&mut c, &customers, &combo).await.unwrap().rows, 1), vec!["Dee"]);
    let raw = PageSpec { where_sql: Some("id < 3".into()), ..run(vec![filter("name", Op::Contains, &["n"])]) };
    assert_eq!(names(&fetch_page(&mut c, &customers, &raw).await.unwrap().rows, 1), vec!["Ann"]);
    let sneaky = fetch_page(&mut c, &customers, &run(vec![filter("name", Op::Eq, &["x' OR '1'='1"])])).await.unwrap();
    assert!(sneaky.rows.is_empty());
    assert!(fetch_page(&mut c, &customers, &PageSpec { where_sql: Some("1=1; DROP TABLE customers".into()), ..spec() }).await.is_err(), "multi-statement WHERE is rejected");
    assert_eq!(count_rows(&mut c, &customers, &[], None).await.unwrap(), 4, "customers table survived");

    // ---- sorting and paging ----
    let desc = PageSpec { sort: vec![Sort { column: "name".into(), desc: true }], limit: 2, ..Default::default() };
    let p1 = fetch_page(&mut c, &customers, &desc).await.unwrap();
    assert_eq!((names(&p1.rows, 1), p1.has_more), (vec!["Dee".to_string(), "Bob".to_string()], true));
    let p2 = fetch_page(&mut c, &customers, &PageSpec { offset: 2, ..desc.clone() }).await.unwrap();
    assert_eq!((names(&p2.rows, 1), p2.has_more), (vec!["Ann".to_string(), "50%_Off Co".to_string()], false));

    // ---- long cells are cut and flagged; full value loads on demand ----
    c.query_drop(format!("INSERT INTO `{db}`.orders (customer_id,total,placed,note,img) VALUES (1,1,NOW(),REPEAT('é',3000),REPEAT(X'AB',5000))")).await.unwrap();
    let big_id = c.last_insert_id().unwrap().to_string();
    let p = fetch_page(&mut c, &orders, &PageSpec { filters: vec![filter("id", Op::Eq, &[&big_id])], ..spec() }).await.unwrap();
    let (ni, ii) = (col("note"), col("img"));
    assert!(p.long_cells.contains(&(0, ni)) && p.long_cells.contains(&(0, ii)));
    let preview = p.rows[0][ni].as_deref().unwrap();
    assert!(preview.len() <= CELL_PREVIEW && preview.chars().all(|ch| ch == 'é'), "cut on a character boundary");
    let full = fetch_cell(&mut c, &orders, &[text("id", &big_id)], "note").await.unwrap();
    assert_eq!(full.len, 6000);
    let img = fetch_cell(&mut c, &orders, &[text("id", &big_id)], "img").await.unwrap();
    assert_eq!(img.len, 5000);
    use base64::{engine::general_purpose::STANDARD, Engine};
    assert!(STANDARD.decode(img.base64.unwrap()).unwrap().iter().all(|b| *b == 0xAB));
    assert!(fetch_cell(&mut c, &orders, &[text("id", "1")], "note").await.unwrap().base64.is_some());
    assert!(fetch_cell(&mut c, &orders, &[text("id", "2")], "note").await.unwrap().is_null);
    assert!(fetch_cell(&mut c, &nokey, &[text("a", "1")], "b").await.is_err(), "no key, no cell lookup");
    c.query_drop(format!("DELETE FROM `{db}`.orders WHERE id={big_id}")).await.unwrap();

    // ---- edits: update / insert / delete in one transaction ----
    let changes = vec![
        Change::Update { key: vec![text("id", "1")], set: vec![text("note", "edited"), Assign { column: "img".into(), value: CellValue::Text("0x0102".into()) }] },
        Change::Update { key: vec![text("id", "2")], set: vec![Assign { column: "note".into(), value: CellValue::Text("new".into()) }, Assign { column: "day".into(), value: CellValue::Default }] },
        Change::Insert { values: vec![text("customer_id", "2"), text("total", "7.25"), text("placed", "2025-05-05 05:05:05")] },
        Change::Delete { key: vec![text("id", "3")] },
    ];
    let preview = preview_changes(&orders, &changes).unwrap();
    assert!(preview[0].contains("X'0102'") && preview[3].starts_with("DELETE FROM"));
    let out = apply_changes(&mut c, &orders, &changes).await.unwrap();
    assert_eq!(out.applied, 4);
    assert_eq!(out.insert_ids.len(), 1);
    let rows = fetch_page(&mut c, &orders, &PageSpec { sort: vec![Sort { column: "id".into(), desc: false }], ..spec() }).await.unwrap().rows;
    assert_eq!(rows[0][col("note")].as_deref(), Some("edited"));
    assert_eq!(rows[0][col("img")].as_deref(), Some("0x0102"));
    assert_eq!(rows.len(), 3, "one deleted, one inserted");
    assert_eq!(rows[2][col("total")].as_deref(), Some("7.25"));

    // updating to the same value is not an error (MySQL reports 0 affected rows)
    apply_changes(&mut c, &orders, &[Change::Update { key: vec![text("id", "1")], set: vec![text("note", "edited")] }]).await.unwrap();

    // NULL vs empty string survive the round trip
    apply_changes(&mut c, &customers, &[
        Change::Update { key: vec![text("id", "1")], set: vec![text("email", "")] },
        Change::Update { key: vec![text("id", "3")], set: vec![Assign { column: "email".into(), value: CellValue::Null }] },
    ]).await.unwrap();
    let cr = fetch_page(&mut c, &customers, &PageSpec { sort: vec![Sort { column: "id".into(), desc: false }], ..spec() }).await.unwrap().rows;
    assert_eq!((cr[0][2].as_deref(), cr[2][2].clone()), (Some(""), None));

    // ---- failures roll the whole batch back and name the change ----
    let bad = vec![
        Change::Update { key: vec![text("id", "1")], set: vec![text("name", "SHOULD-NOT-PERSIST")] },
        Change::Insert { values: vec![text("name", "x".repeat(100).as_str())] }, // too long for VARCHAR(40) in strict mode
    ];
    let err = apply_changes(&mut c, &customers, &bad).await.unwrap_err();
    assert!(matches!(err, Error::Change { index: 1, .. }), "{err}");
    let ann = fetch_page(&mut c, &customers, &run(vec![filter("id", Op::Eq, &["1"])])).await.unwrap();
    assert_eq!(ann.rows[0][1].as_deref(), Some("Ann"), "first change was rolled back");
    // row deleted by someone else => conflict error, not silent success
    let gone = apply_changes(&mut c, &orders, &[Change::Update { key: vec![text("id", "3")], set: vec![text("note", "x")] }]).await.unwrap_err();
    assert!(gone.to_string().contains("no longer exists"), "{gone}");
    // FK violation is surfaced
    assert!(apply_changes(&mut c, &orders, &[Change::Insert { values: vec![text("customer_id", "999"), text("total", "1"), text("placed", "2025-01-01 00:00:00")] }]).await.is_err());
    // read-only structures refuse edits before touching the server
    assert!(matches!(apply_changes(&mut c, &nokey, &[Change::Delete { key: vec![text("a", "1")] }]).await, Err(Error::Change { .. })));
    assert!(matches!(apply_changes(&mut c, &view, &[Change::Insert { values: vec![] }]).await, Err(Error::Change { .. })));
    assert_eq!(count_rows(&mut c, &nokey, &[], None).await.unwrap(), 2);

    // ---- file upload into a BLOB ----
    let tmp = std::env::temp_dir().join(format!("mylodb_blob_{}.bin", std::process::id()));
    std::fs::write(&tmp, [1u8, 2, 3, 250]).unwrap();
    apply_changes(&mut c, &orders, &[Change::Update { key: vec![text("id", "1")], set: vec![Assign { column: "img".into(), value: CellValue::File(tmp.to_string_lossy().into()) }] }]).await.unwrap();
    std::fs::remove_file(&tmp).unwrap();
    let img = fetch_cell(&mut c, &orders, &[text("id", "1")], "img").await.unwrap();
    assert_eq!(STANDARD.decode(img.base64.unwrap()).unwrap(), vec![1, 2, 3, 250]);

    // ---- foreign-key lookup ----
    let all = lookup(&mut c, &customers, "id", "", 10).await.unwrap();
    assert_eq!(all.len(), 4);
    assert_eq!((all[0].value.as_str(), all[0].label.as_deref()), ("1", Some("Ann")));
    let found = lookup(&mut c, &customers, "id", "ob", 10).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].label.as_deref(), Some("Bob"));
    assert_eq!(lookup(&mut c, &customers, "id", "%", 10).await.unwrap().len(), 1, "search wildcards are literal");

    // ---- export ----
    let sorted = PageSpec { sort: vec![Sort { column: "id".into(), desc: false }], ..Default::default() };
    let mut csv = Vec::new();
    let n = export_rows(&mut c, &customers, &sorted, &ExportOptions { format: ExportFormat::Csv, null_text: "NULL".into() }, &mut csv).await.unwrap();
    assert_eq!(n, 4);
    assert_eq!(String::from_utf8(csv).unwrap(), "id,name,email\n1,Ann,\n2,Bob,ann@x.io\n3,50%_Off Co,NULL\n4,Dee,dee@y.io\n".replace("2,Bob,ann@x.io", "2,Bob,NULL"));
    let mut json = Vec::new();
    export_rows(&mut c, &customers, &PageSpec { filters: vec![filter("id", Op::Le, &["2"])], ..sorted.clone() }, &ExportOptions { format: ExportFormat::Json, null_text: String::new() }, &mut json).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&json).unwrap();
    assert_eq!(parsed, serde_json::json!([{"id": 1, "name": "Ann", "email": ""}, {"id": 2, "name": "Bob", "email": null}]));
    let mut sql = Vec::new();
    export_rows(&mut c, &customers, &PageSpec { filters: vec![filter("id", Op::Le, &["2"])], ..sorted }, &ExportOptions { format: ExportFormat::SqlInsert, null_text: String::new() }, &mut sql).await.unwrap();
    assert_eq!(String::from_utf8(sql).unwrap(), "INSERT INTO `customers` (`id`, `name`, `email`) VALUES\n(1, 'Ann', ''),\n(2, 'Bob', NULL);\n");
    let mut empty = Vec::new();
    export_rows(&mut c, &customers, &PageSpec { filters: vec![filter("id", Op::Gt, &["99"])], ..Default::default() }, &ExportOptions { format: ExportFormat::Json, null_text: String::new() }, &mut empty).await.unwrap();
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&empty).unwrap(), serde_json::json!([]));
}

#[allow(dead_code)]
fn _types(_: &Conn) {}
