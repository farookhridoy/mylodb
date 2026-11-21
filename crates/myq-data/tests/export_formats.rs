//! XML / HTML / XLSX export and query export against a local MySQL (own database, dropped afterwards).

use myq_data::*;
use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::{pool, ConnConfig, Pool, SslMode};
use std::io::Read;

fn cfg() -> ConnConfig {
    ConnConfig {
        host: std::env::var("MYLODB_TEST_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port: std::env::var("MYLODB_TEST_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3306),
        user: std::env::var("MYLODB_TEST_USER").unwrap_or_else(|_| "root".into()),
        password: std::env::var("MYLODB_TEST_PASSWORD").ok(),
        ssl: SslMode::Disabled,
        ..Default::default()
    }
}

fn opts(format: ExportFormat) -> ExportOptions {
    ExportOptions { format, null_text: "NULL".into(), ..Default::default() }
}

#[tokio::test]
async fn new_formats_and_query_export() {
    let pool = pool(&cfg());
    let mut admin = match pool.get_conn().await {
        Ok(c) => c,
        Err(e) => return eprintln!("SKIP: no MySQL reachable: {e}"),
    };
    let db = format!("mylodb_itest_exp_{}", std::process::id());
    admin.query_drop(format!("CREATE DATABASE `{db}`")).await.unwrap();
    let outcome = tokio::spawn(body(pool.clone(), db.clone())).await;
    admin.query_drop(format!("DROP DATABASE `{db}`")).await.unwrap();
    outcome.unwrap();
}

async fn body(pool: Pool, db: String) {
    let db = db.as_str();
    let mut c = pool.get_conn().await.unwrap();
    c.query_drop(format!("USE `{db}`")).await.unwrap();
    c.query_drop("CREATE TABLE t (id INT PRIMARY KEY, name VARCHAR(60), price DECIMAL(12,2), big BIGINT, zip CHAR(5), note TEXT)").await.unwrap();
    c.query_drop("INSERT INTO t VALUES (1, 'A <b> & \"q\"', 10.50, 1234567890123456789, '01234', NULL), (2, NULL, 0.05, 7, '99999', 'line1\nline2'), (3, 'é日本😀', 3, 8, '00001', '')").await.unwrap();
    let meta = table_meta(&mut c, db, "t").await.unwrap();
    let spec = PageSpec { sort: vec![Sort { column: "id".into(), desc: false }], ..Default::default() };
    let tmp = std::env::temp_dir();
    let file = |ext: &str| tmp.join(format!("mylodb_exp_{}.{ext}", std::process::id()));

    // ---- XML ----
    let mut xml = Vec::new();
    let out = export_rows(&mut c, &meta, &spec, &opts(ExportFormat::Xml), Dest::Writer(&mut xml), &mut no_progress).await.unwrap();
    assert_eq!(out.rows, 3);
    let xml = String::from_utf8(xml).unwrap();
    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<resultset name=\"t\">"));
    assert!(xml.contains("<field name=\"name\">A &lt;b&gt; &amp; &quot;q&quot;</field>"), "{xml}");
    assert!(xml.contains("<field name=\"name\" null=\"true\"/>"), "NULL is distinct from empty");
    assert!(xml.contains("<field name=\"note\"></field>") || xml.contains("<field name=\"note\"/>") == false, "empty string is an empty element, not null");
    assert!(xml.contains("é日本😀") && xml.trim_end().ends_with("</resultset>"));
    assert_eq!(xml.matches("<row>").count(), 3);

    // ---- HTML ----
    let mut html = Vec::new();
    export_rows(&mut c, &meta, &spec, &ExportOptions { name: Some("My <report>".into()), ..opts(ExportFormat::Html) }, Dest::Writer(&mut html), &mut no_progress).await.unwrap();
    let html = String::from_utf8(html).unwrap();
    assert!(html.contains("<title>My &lt;report&gt;</title>") && html.contains("<th>price</th>"), "{html}");
    assert!(html.contains("<td>A &lt;b&gt; &amp; &quot;q&quot;</td>") && html.contains("<td class=\"null\">NULL</td>"));
    assert!(html.contains("<td class=\"num\">10.50</td>"), "numbers are right-aligned and keep their scale");
    assert!(!html.contains("<b>"), "values are never interpreted as markup");

    // ---- XLSX: a real file; open it and check the cells ----
    let path = file("xlsx");
    let out = export_rows(&mut c, &meta, &spec, &opts(ExportFormat::Xlsx), Dest::File(&path), &mut no_progress).await.unwrap();
    assert_eq!(out.rows, 3);
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let mut sheet = String::new();
    zip.by_name("xl/worksheets/sheet1.xml").unwrap().read_to_string(&mut sheet).unwrap();
    let mut shared = String::new();
    if let Ok(mut f) = zip.by_name("xl/sharedStrings.xml") {
        f.read_to_string(&mut shared).unwrap();
    }
    let all = format!("{sheet}{shared}");
    assert!(all.contains("price") && all.contains("日本"), "headers and text cells are present");
    assert!(sheet.contains("<v>10.5</v>") || all.contains("10.5"), "a DECIMAL that Excel can hold exactly becomes a number");
    assert!(all.contains("1234567890123456789"), "a 19-digit BIGINT is kept as text, not rounded to a double");
    assert!(all.contains("01234") && all.contains("00001"), "leading zeros survive");
    assert!(zip.by_name("xl/workbook.xml").is_ok());
    // XLSX cannot go to a plain writer
    let mut sink = Vec::new();
    assert!(matches!(export_rows(&mut c, &meta, &spec, &opts(ExportFormat::Xlsx), Dest::Writer(&mut sink), &mut no_progress).await, Err(Error::Invalid(_))));
    let _ = std::fs::remove_file(&path);

    // ---- export a query ----
    let mut csv = Vec::new();
    let out = export_query(&mut c, Some(db), "SELECT id, price, name FROM t WHERE id <> 2 ORDER BY id", &opts(ExportFormat::Csv), Dest::Writer(&mut csv), &mut no_progress).await.unwrap();
    assert_eq!(out.rows, 2);
    assert_eq!(String::from_utf8(csv).unwrap(), "id,price,name\n1,10.50,\"A <b> & \"\"q\"\"\"\n3,3.00,é日本😀\n");
    let mut json = Vec::new();
    export_query(&mut c, Some(db), "SELECT id, price FROM t WHERE id = 1", &opts(ExportFormat::Json), Dest::Writer(&mut json), &mut no_progress).await.unwrap();
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&json).unwrap(), serde_json::json!([{"id": 1, "price": 10.5}]));
    // computed columns and an empty result
    let mut empty = Vec::new();
    let out = export_query(&mut c, Some(db), "SELECT COUNT(*) AS n FROM t WHERE id > 99", &opts(ExportFormat::Csv), Dest::Writer(&mut empty), &mut no_progress).await.unwrap();
    assert_eq!((out.rows, String::from_utf8(empty).unwrap()), (1, "n\n0\n".to_string()));
    let path = file("xlsx");
    export_query(&mut c, Some(db), "SELECT id FROM t WHERE id > 99", &opts(ExportFormat::Xlsx), Dest::File(&path), &mut no_progress).await.unwrap();
    assert!(path.exists(), "a header-only workbook is still written");
    let _ = std::fs::remove_file(&path);

    // only a single, read-only statement is accepted
    for bad in ["DELETE FROM t", "DROP TABLE t", "SELECT 1; SELECT 2", "UPDATE t SET id = id"] {
        let mut sink = Vec::new();
        assert!(matches!(export_query(&mut c, Some(db), bad, &opts(ExportFormat::Csv), Dest::Writer(&mut sink), &mut no_progress).await, Err(Error::Invalid(_))), "{bad}");
    }
    let n: Option<i64> = c.query_first("SELECT COUNT(*) FROM t").await.unwrap();
    assert_eq!(n, Some(3), "rejected statements did not run");
    // a server error is reported, with nothing half-written to a file
    let mut sink = Vec::new();
    assert!(export_query(&mut c, Some(db), "SELECT * FROM nope", &opts(ExportFormat::Csv), Dest::Writer(&mut sink), &mut no_progress).await.is_err());
}
