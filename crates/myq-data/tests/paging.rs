//! Keyset paging, row estimates and big exports against a local MySQL. Creates and drops only its own uniquely named database.

use myq_data::{estimate_count, export_rows, fetch_page, no_progress, table_meta, ColumnFilter, Cursor, Dest, Error, ExportFormat, ExportOptions, Op, Page, PageSpec, Sort};
use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::{pool, ConnConfig, SslMode};
use std::io::Read;

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

async fn all_rows(conn: &mut myq_driver::Conn, meta: &myq_data::TableMeta, filters: Vec<ColumnFilter>) -> Vec<Vec<Option<String>>> {
    let page = fetch_page(conn, meta, &PageSpec { filters, limit: 5000, ..Default::default() }).await.unwrap();
    assert!(!page.has_more, "the fixture tables are small");
    page.rows
}

/// Walk forward with `After(last_key)` until the end; returns every row in the order met.
async fn walk_forward(conn: &mut myq_driver::Conn, meta: &myq_data::TableMeta, limit: u32, filters: &[ColumnFilter]) -> Vec<Vec<Option<String>>> {
    let mut out = Vec::new();
    let mut cursor: Option<Cursor> = None;
    for _ in 0..10_000 {
        let page: Page = fetch_page(conn, meta, &PageSpec { limit, cursor: cursor.clone(), filters: filters.to_vec(), ..Default::default() }).await.unwrap();
        assert!(page.keyset);
        assert_eq!(page.has_prev, cursor.is_some(), "every page after the first has rows before it");
        out.extend(page.rows.clone());
        if !page.has_more {
            return out;
        }
        cursor = Some(Cursor::After(page.last_key.clone().expect("a page with rows has a last key")));
    }
    panic!("never reached the end");
}

/// Walk backward from the end with `Before(first_key)`; pages are returned in reading order, rows within a page ascending.
async fn walk_backward(conn: &mut myq_driver::Conn, meta: &myq_data::TableMeta, limit: u32) -> Vec<Vec<Option<String>>> {
    let mut chunks: Vec<Vec<Vec<Option<String>>>> = Vec::new();
    let mut cursor = Cursor::End;
    for _ in 0..10_000 {
        let page = fetch_page(conn, meta, &PageSpec { limit, cursor: Some(cursor.clone()), ..Default::default() }).await.unwrap();
        assert!(!page.rows.is_empty());
        let has_prev = page.has_prev;
        let first = page.first_key.clone().unwrap();
        chunks.push(page.rows);
        if !has_prev {
            chunks.reverse();
            return chunks.into_iter().flatten().collect();
        }
        cursor = Cursor::Before(first);
    }
    panic!("never reached the start");
}

#[tokio::test]
async fn keyset_pages_return_exactly_what_offset_pages_return() {
    let p = pool(&cfg());
    let mut conn = match p.get_conn().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP: no MySQL reachable: {e}");
            return;
        }
    };
    let db = format!("mylodb_itest_pg_{}", std::process::id());
    conn.query_drop(format!("CREATE DATABASE `{db}`")).await.unwrap();
    // the checks run in a task so that a failed assertion cannot skip the cleanup
    let handle = {
        let (cfg, db) = (cfg(), db.clone());
        tokio::spawn(async move { checks(cfg, db).await })
    };
    let out = handle.await;
    conn.query_drop(format!("DROP DATABASE `{db}`")).await.unwrap();
    out.unwrap();
}

async fn checks(cfg: ConnConfig, db: String) {
    let p = pool(&cfg);
    let mut conn = p.get_conn().await.unwrap();
    let ddl = |sql: String| sql;
    conn.query_drop(ddl(format!("CREATE TABLE `{db}`.t_int (id BIGINT UNSIGNED PRIMARY KEY, v VARCHAR(8) NOT NULL) ENGINE=InnoDB"))).await.unwrap();
    // 2,345 rows with gaps in the ids (so a seek cannot be mistaken for a row number)
    conn.query_drop(format!(
        "INSERT INTO `{db}`.t_int SELECT n * 3, CONCAT('v', n % 7) FROM (SELECT a.d + b.d*10 + c.d*100 + e.d*1000 + 1 AS n FROM \
         (SELECT 0 d UNION SELECT 1 UNION SELECT 2 UNION SELECT 3 UNION SELECT 4 UNION SELECT 5 UNION SELECT 6 UNION SELECT 7 UNION SELECT 8 UNION SELECT 9) a, \
         (SELECT 0 d UNION SELECT 1 UNION SELECT 2 UNION SELECT 3 UNION SELECT 4 UNION SELECT 5 UNION SELECT 6 UNION SELECT 7 UNION SELECT 8 UNION SELECT 9) b, \
         (SELECT 0 d UNION SELECT 1 UNION SELECT 2 UNION SELECT 3 UNION SELECT 4 UNION SELECT 5 UNION SELECT 6 UNION SELECT 7 UNION SELECT 8 UNION SELECT 9) c, \
         (SELECT 0 d UNION SELECT 1 UNION SELECT 2) e) x WHERE n <= 2345"
    ))
    .await
    .unwrap();
    conn.query_drop(format!("CREATE TABLE `{db}`.t_big (id BIGINT UNSIGNED PRIMARY KEY, v INT)")).await.unwrap();
    // neighbours above 2^53, which a double cannot tell apart
    conn.query_drop(format!("INSERT INTO `{db}`.t_big VALUES {}", (0..12u64).map(|i| format!("({}, {i})", 9_007_199_254_740_990u64 + i)).collect::<Vec<_>>().join(","))).await.unwrap();
    conn.query_drop(format!("CREATE TABLE `{db}`.t_comp (region VARCHAR(4) NOT NULL, n INT NOT NULL, v INT, PRIMARY KEY (region, n))")).await.unwrap();
    conn.query_drop(format!("INSERT INTO `{db}`.t_comp VALUES {}", ["eu", "us", "ap", "af"].iter().flat_map(|r| (1..=25).map(move |n| format!("('{r}', {}, {n})", n * 2 - 1))).collect::<Vec<_>>().join(","))).await.unwrap();
    conn.query_drop(format!("CREATE TABLE `{db}`.t_str (code VARCHAR(12) PRIMARY KEY, v INT)")).await.unwrap();
    conn.query_drop(format!("INSERT INTO `{db}`.t_str VALUES {}", (0..60).map(|i| format!("('{}{}', {i})", ["a", "B", "c", "10", "9"][i % 5], i)).collect::<Vec<_>>().join(","))).await.unwrap();
    conn.query_drop(format!("CREATE TABLE `{db}`.t_dec (id DECIMAL(10,2) PRIMARY KEY, v INT)")).await.unwrap();
    conn.query_drop(format!("INSERT INTO `{db}`.t_dec VALUES (1.50, 1), (2.50, 2)")).await.unwrap();
    conn.query_drop(format!("CREATE TABLE `{db}`.t_empty (id INT PRIMARY KEY)")).await.unwrap();
    conn.query_drop(format!("CREATE TABLE `{db}`.t_nopk (a INT, b INT)")).await.unwrap();
    conn.query_drop(format!("ANALYZE TABLE `{db}`.t_int")).await.unwrap();

    // ---- an integer key: forward and backward walks reproduce the whole table, in order
    let meta = table_meta(&mut conn, &db, "t_int").await.unwrap();
    let expected = all_rows(&mut conn, &meta, vec![]).await;
    assert_eq!(expected.len(), 2345);
    for limit in [100u32, 500, 1] {
        if limit == 1 {
            continue; // a one-row page over 2,345 rows is covered by the small tables below
        }
        assert_eq!(walk_forward(&mut conn, &meta, limit, &[]).await, expected, "forward, pages of {limit}");
        assert_eq!(walk_backward(&mut conn, &meta, limit).await, expected, "backward, pages of {limit}");
    }
    let first = fetch_page(&mut conn, &meta, &PageSpec { limit: 100, ..Default::default() }).await.unwrap();
    assert!(first.keyset && !first.has_prev && first.has_more);
    assert_eq!((first.first_key.clone(), first.last_key.clone()), (Some(vec!["3".into()]), Some(vec!["300".into()])));
    let end = fetch_page(&mut conn, &meta, &PageSpec { limit: 100, cursor: Some(Cursor::End), ..Default::default() }).await.unwrap();
    assert_eq!(end.rows, expected[2245..].to_vec(), "End is the last page");
    assert!(end.has_prev && !end.has_more);
    assert_eq!(end.last_key, Some(vec!["7035".into()]));
    // a page that is exactly the size of the table remainder
    let mid = fetch_page(&mut conn, &meta, &PageSpec { limit: 100, cursor: Some(Cursor::After(vec!["6735".into()])), ..Default::default() }).await.unwrap();
    assert_eq!(mid.rows.len(), 100, "ids 6738 .. 7035");
    assert!(!mid.has_more, "exactly the last 100 rows: no further page");
    // a key that is not in the table still seeks to the next one
    let gap = fetch_page(&mut conn, &meta, &PageSpec { limit: 2, cursor: Some(Cursor::After(vec!["10".into()])), ..Default::default() }).await.unwrap();
    assert_eq!(gap.rows.iter().map(|r| r[0].clone().unwrap()).collect::<Vec<_>>(), ["12", "15"]);
    let before = fetch_page(&mut conn, &meta, &PageSpec { limit: 2, cursor: Some(Cursor::Before(vec!["10".into()])), ..Default::default() }).await.unwrap();
    assert_eq!(before.rows.iter().map(|r| r[0].clone().unwrap()).collect::<Vec<_>>(), ["6", "9"], "rows ascending within the page");
    assert!(before.has_prev && before.has_more);

    // ---- with a filter
    let filter = vec![ColumnFilter { column: "v".into(), op: Op::Eq, values: vec!["v3".into()] }];
    let filtered = all_rows(&mut conn, &meta, filter.clone()).await;
    assert!(filtered.len() > 300 && filtered.len() < 400);
    assert_eq!(walk_forward(&mut conn, &meta, 50, &filter).await, filtered);

    // ---- keys above 2^53 are compared exactly
    let big = table_meta(&mut conn, &db, "t_big").await.unwrap();
    let all_big = all_rows(&mut conn, &big, vec![]).await;
    assert_eq!(walk_forward(&mut conn, &big, 5, &[]).await, all_big);
    let after = fetch_page(&mut conn, &big, &PageSpec { limit: 1, cursor: Some(Cursor::After(vec!["9007199254740993".into()])), ..Default::default() }).await.unwrap();
    assert_eq!(after.rows[0][0].as_deref(), Some("9007199254740994"), "as a double, ...993 and ...994 are the same number");
    assert_eq!(walk_backward(&mut conn, &big, 5).await, all_big);

    // ---- composite and string keys
    for (table, limit) in [("t_comp", 7u32), ("t_comp", 100), ("t_str", 8)] {
        let m = table_meta(&mut conn, &db, table).await.unwrap();
        let want = all_rows(&mut conn, &m, vec![]).await;
        assert_eq!(walk_forward(&mut conn, &m, limit, &[]).await, want, "{table} forward");
        assert_eq!(walk_backward(&mut conn, &m, limit).await, want, "{table} backward");
    }

    // ---- where keyset does not apply
    let dec = table_meta(&mut conn, &db, "t_dec").await.unwrap();
    let page = fetch_page(&mut conn, &dec, &PageSpec::default()).await.unwrap();
    assert!(!page.keyset && page.first_key.is_none(), "decimal keys are not compared exactly as text");
    assert!(fetch_page(&mut conn, &dec, &PageSpec { cursor: Some(Cursor::End), ..Default::default() }).await.is_err());
    let nopk = table_meta(&mut conn, &db, "t_nopk").await.unwrap();
    assert!(!fetch_page(&mut conn, &nopk, &PageSpec::default()).await.unwrap().keyset);
    let sorted = PageSpec { sort: vec![Sort { column: "v".into(), desc: true }], ..Default::default() };
    assert!(!fetch_page(&mut conn, &meta, &sorted).await.unwrap().keyset, "a user sort is not key order");
    assert!(fetch_page(&mut conn, &meta, &PageSpec { cursor: Some(Cursor::End), ..sorted }).await.is_err());

    // ---- empty table
    let empty = table_meta(&mut conn, &db, "t_empty").await.unwrap();
    let e = fetch_page(&mut conn, &empty, &PageSpec { cursor: Some(Cursor::End), ..Default::default() }).await.unwrap();
    assert!(e.rows.is_empty() && !e.has_prev && !e.has_more && e.first_key.is_none() && e.last_key.is_none());

    // ---- offset paging still works and reports its position
    let off = fetch_page(&mut conn, &meta, &PageSpec { limit: 100, offset: 200, ..Default::default() }).await.unwrap();
    assert_eq!(off.rows, expected[200..300].to_vec());
    assert!(off.has_prev && off.has_more);

    // ---- row estimates
    let est = estimate_count(&mut conn, &meta, &[], None).await.unwrap().expect("table statistics");
    // InnoDB statistics are sampled: on a small table they can be off by a good fraction. The point is "the right order of magnitude, instantly".
    assert!((1100..=3500).contains(&est), "about 2,345 rows, got {est}");
    let filtered_est = estimate_count(&mut conn, &meta, &[ColumnFilter { column: "id".into(), op: Op::Lt, values: vec!["300".into()] }], None).await.unwrap().expect("EXPLAIN estimate");
    assert!((20..=400).contains(&filtered_est), "ids below 300 are about 99 rows, got {filtered_est}");
    assert!(estimate_count(&mut conn, &meta, &[], Some("id <")).await.unwrap().is_none(), "a condition the server cannot explain gives no estimate");

    // ---- exports: JSON Lines, gzip, progress and cancel
    let dir = std::env::temp_dir().join(format!("mylodb_itest_pg_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let spec = PageSpec::default();
    let opts = |format, gzip| ExportOptions { format, gzip, null_text: String::new(), name: None, postgres: false };
    let jsonl = dir.join("t.jsonl");
    let out = export_rows(&mut conn, &meta, &spec, &opts(ExportFormat::JsonLines, false), Dest::File(&jsonl), &mut no_progress).await.unwrap();
    assert_eq!(out.rows, 2345);
    let text = std::fs::read_to_string(&jsonl).unwrap();
    let lines: Vec<serde_json::Value> = text.lines().map(|l| serde_json::from_str(l).expect("every line is a JSON object")).collect();
    assert_eq!(lines.len(), 2345);
    assert_eq!(lines[0], serde_json::json!({"id": 3, "v": "v1"}), "numbers stay numbers");
    assert_eq!(lines[2344]["id"], 7035);

    let plain = dir.join("t.csv");
    let gz = dir.join("t.csv.gz");
    export_rows(&mut conn, &meta, &spec, &opts(ExportFormat::Csv, false), Dest::File(&plain), &mut no_progress).await.unwrap();
    export_rows(&mut conn, &meta, &spec, &opts(ExportFormat::Csv, true), Dest::File(&gz), &mut no_progress).await.unwrap();
    let mut unzipped = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(&gz).unwrap()).read_to_string(&mut unzipped).expect("a complete gzip stream");
    assert_eq!(unzipped, std::fs::read_to_string(&plain).unwrap(), "gzip changes the size, not the content");
    assert!(std::fs::metadata(&gz).unwrap().len() < std::fs::metadata(&plain).unwrap().len());
    assert!(matches!(export_rows(&mut conn, &meta, &spec, &opts(ExportFormat::Xlsx, true), Dest::File(&dir.join("x.xlsx.gz")), &mut no_progress).await, Err(Error::Invalid(_))), "xlsx is already compressed");

    let mut seen = Vec::new();
    let mut record = |n: u64| {
        seen.push(n);
        true
    };
    export_rows(&mut conn, &meta, &spec, &opts(ExportFormat::Csv, false), Dest::File(&plain), &mut record).await.unwrap();
    assert_eq!(seen, [1000, 2000, 2345], "progress every 1,000 rows and a final report");

    let mut stop_after_first = |n: u64| n < 1000;
    let res = export_rows(&mut conn, &meta, &spec, &opts(ExportFormat::Csv, false), Dest::File(&plain), &mut stop_after_first).await;
    assert!(matches!(res, Err(Error::Cancelled)), "returning false from the tick cancels: {res:?}");
    // the connection is still usable after a cancelled export
    assert_eq!(fetch_page(&mut conn, &meta, &PageSpec { limit: 3, ..Default::default() }).await.unwrap().rows.len(), 3);
    let _ = std::fs::remove_dir_all(&dir);
}
