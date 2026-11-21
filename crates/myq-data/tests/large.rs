//! Large-table behaviour and benchmarks against a local MySQL. The benchmark is `#[ignore]` (it builds a big table):
//!
//!   MYLODB_BENCH_ROWS=1000000 cargo test -p myq-data --test large -- --ignored --nocapture
//!
//! It uses its own database `mylodb_bench_data` (kept between runs so 10M rows are generated once; drop it when done).

use myq_data::{count_rows, estimate_count, export_rows, fetch_page, no_progress, table_meta, Cursor, Dest, ExportFormat, ExportOptions, PageSpec};
use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::{pool, ConnConfig, SslMode};
use std::time::Instant;

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

fn rss_mb() -> f64 {
    let out = std::process::Command::new("ps").args(["-o", "rss=", "-p", &std::process::id().to_string()]).output().ok();
    out.and_then(|o| String::from_utf8(o.stdout).ok()).and_then(|s| s.trim().parse::<f64>().ok()).unwrap_or(0.0) / 1024.0
}

/// Make sure `bench_N` exists with N rows (generated in 1M-row batches inside the server).
async fn ensure_table(conn: &mut myq_driver::Conn, rows: u64) -> String {
    let table = format!("bench_{rows}");
    conn.query_drop("CREATE DATABASE IF NOT EXISTS mylodb_bench_data").await.unwrap();
    let have: Option<u64> = conn
        .exec_first("SELECT TABLE_ROWS FROM information_schema.TABLES WHERE TABLE_SCHEMA='mylodb_bench_data' AND TABLE_NAME=?", (table.clone(),))
        .await
        .unwrap();
    if have.is_some() {
        let n: Option<u64> = conn.query_first(format!("SELECT COUNT(*) FROM mylodb_bench_data.{table}")).await.unwrap();
        if n == Some(rows) {
            return table;
        }
        conn.query_drop(format!("DROP TABLE mylodb_bench_data.{table}")).await.unwrap();
    }
    conn.query_drop(format!(
        "CREATE TABLE mylodb_bench_data.{table} (id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY, customer_id INT NOT NULL, status VARCHAR(12) NOT NULL, \
         amount DECIMAL(12,2) NOT NULL, note VARCHAR(100), created_at DATETIME NOT NULL, KEY idx_created (created_at)) ENGINE=InnoDB"
    ))
    .await
    .unwrap();
    conn.query_drop("CREATE TABLE IF NOT EXISTS mylodb_bench_data.nums (n INT PRIMARY KEY)").await.unwrap();
    let have_nums: Option<u64> = conn.query_first("SELECT COUNT(*) FROM mylodb_bench_data.nums").await.unwrap();
    if have_nums != Some(1000) {
        conn.query_drop("TRUNCATE mylodb_bench_data.nums").await.unwrap();
        conn.query_drop("INSERT INTO mylodb_bench_data.nums SELECT a.d + b.d*10 + c.d*100 FROM (SELECT 0 d UNION SELECT 1 UNION SELECT 2 UNION SELECT 3 UNION SELECT 4 UNION SELECT 5 UNION SELECT 6 UNION SELECT 7 UNION SELECT 8 UNION SELECT 9) a, (SELECT 0 d UNION SELECT 1 UNION SELECT 2 UNION SELECT 3 UNION SELECT 4 UNION SELECT 5 UNION SELECT 6 UNION SELECT 7 UNION SELECT 8 UNION SELECT 9) b, (SELECT 0 d UNION SELECT 1 UNION SELECT 2 UNION SELECT 3 UNION SELECT 4 UNION SELECT 5 UNION SELECT 6 UNION SELECT 7 UNION SELECT 8 UNION SELECT 9) c").await.unwrap();
    }
    let batches = rows.div_ceil(1_000_000);
    for k in 0..batches {
        let take = (rows - k * 1_000_000).min(1_000_000);
        conn.query_drop(format!(
            "INSERT INTO mylodb_bench_data.{table} (customer_id, status, amount, note, created_at) \
             SELECT (x.i * 7919) % 100000, ELT(1 + x.i % 4, 'new','paid','shipped','closed'), ROUND((x.i % 100000) / 7, 2), CONCAT('order note ', x.i), \
                    TIMESTAMP('2020-01-01') + INTERVAL x.i MINUTE \
             FROM (SELECT a.n * 1000 + b.n + {k} * 1000000 AS i FROM mylodb_bench_data.nums a, mylodb_bench_data.nums b) x WHERE x.i < {} ORDER BY x.i",
            k * 1_000_000 + take
        ))
        .await
        .unwrap();
    }
    conn.query_drop(format!("ANALYZE TABLE mylodb_bench_data.{table}")).await.unwrap();
    table
}

#[tokio::test]
#[ignore]
async fn benchmark_large_table() {
    let rows: u64 = std::env::var("MYLODB_BENCH_ROWS").ok().and_then(|v| v.parse().ok()).unwrap_or(1_000_000);
    let p = pool(&cfg());
    let mut conn = p.get_conn().await.expect("a local MySQL");
    let t0 = Instant::now();
    let table = ensure_table(&mut conn, rows).await;
    println!("\n== {rows} rows in mylodb_bench_data.{table} (ready in {:.1}s)", t0.elapsed().as_secs_f64());
    let meta = table_meta(&mut conn, "mylodb_bench_data", &table).await.unwrap();
    let mut report: Vec<(String, f64)> = Vec::new();
    let mut time = |label: &str, s: f64| {
        println!("{label:<52} {:>9.1} ms", s * 1000.0);
        report.push((label.to_string(), s));
    };

    let spec = |offset: u64| PageSpec { limit: 500, offset, ..Default::default() };
    let t = Instant::now();
    let first = fetch_page(&mut conn, &meta, &spec(0)).await.unwrap();
    time("first page (500 rows)", t.elapsed().as_secs_f64());
    assert_eq!(first.rows.len(), 500);

    let t = Instant::now();
    let mid = fetch_page(&mut conn, &meta, &spec(rows / 2)).await.unwrap();
    time("page at 50% (OFFSET)", t.elapsed().as_secs_f64());
    assert_eq!(mid.rows.len(), 500);

    let t = Instant::now();
    let kmid = fetch_page(&mut conn, &meta, &PageSpec { limit: 500, cursor: Some(Cursor::After(vec![(rows / 2).to_string()])), ..Default::default() }).await.unwrap();
    time("page at 50% (keyset)", t.elapsed().as_secs_f64());
    assert_eq!(kmid.rows[0][0].as_deref(), Some((rows / 2 + 1).to_string().as_str()));

    let t = Instant::now();
    let deep = fetch_page(&mut conn, &meta, &spec(rows - 500)).await.unwrap();
    time("last page (OFFSET)", t.elapsed().as_secs_f64());
    assert_eq!(deep.rows.len(), 500);

    let t = Instant::now();
    let end = fetch_page(&mut conn, &meta, &PageSpec { limit: 500, cursor: Some(Cursor::End), ..Default::default() }).await.unwrap();
    time("last page (keyset End)", t.elapsed().as_secs_f64());
    assert_eq!(end.rows, deep.rows, "the same rows either way");

    let t = Instant::now();
    let n = count_rows(&mut conn, &meta, &[], None).await.unwrap();
    time("exact COUNT(*)", t.elapsed().as_secs_f64());
    assert_eq!(n, rows);

    let t = Instant::now();
    let est = estimate_count(&mut conn, &meta, &[], None).await.unwrap();
    time("estimated count (statistics)", t.elapsed().as_secs_f64());
    println!("   estimate {:?} vs exact {n}", est);

    let t = Instant::now();
    let f_est = estimate_count(&mut conn, &meta, &[myq_data::ColumnFilter { column: "status".into(), op: myq_data::Op::Eq, values: vec!["paid".into()] }], None).await.unwrap();
    time("estimated count with a filter (EXPLAIN)", t.elapsed().as_secs_f64());
    println!("   filtered estimate {:?}", f_est);

    // how fast can rows be read at all? (no formatting, no file): the ceiling for an export
    let t = Instant::now();
    let mut qr = conn.exec_iter(format!("SELECT * FROM mylodb_bench_data.{table}"), ()).await.unwrap();
    let mut raw = 0u64;
    while let Some(_row) = qr.next().await.unwrap() {
        raw += 1;
    }
    drop(qr);
    println!("{:<52} {:>9.1} ms  ({:.0} rows/s)", "read every row, no formatting (ceiling)", t.elapsed().as_secs_f64() * 1000.0, raw as f64 / t.elapsed().as_secs_f64());

    // read and convert every row to text (what an export does before it writes anything)
    let t = Instant::now();
    let mut qr = conn.exec_iter(format!("SELECT * FROM mylodb_bench_data.{table}"), ()).await.unwrap();
    let mut conv = 0u64;
    while let Some(row) = qr.next().await.unwrap() {
        let (cells, _) = myq_driver::row_cells(row, None);
        conv += cells.len() as u64;
    }
    drop(qr);
    println!("{:<52} {:>9.1} ms  ({:.0} rows/s)", "read + convert cells to text (no writing)", t.elapsed().as_secs_f64() * 1000.0, rows as f64 / t.elapsed().as_secs_f64());
    assert!(conv > 0);
    if std::env::var("MYLODB_BENCH_SKIP_EXPORT").is_ok() {
        return;
    }

    // export to a temp file: time, throughput, memory
    let dir = std::env::temp_dir().join(format!("mylodb_bench_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, format, gzip) in [("CSV", ExportFormat::Csv, false), ("CSV (again)", ExportFormat::Csv, false), ("JSON Lines", ExportFormat::JsonLines, false), ("CSV + gzip", ExportFormat::Csv, true), ("JSON", ExportFormat::Json, false)] {
        let path = dir.join(format!("out.{}{}", format.extension(), if gzip { ".gz" } else { "" }));
        let before = rss_mb();
        let t = Instant::now();
        let out = export_rows(&mut conn, &meta, &PageSpec::default(), &ExportOptions { format, gzip, ..Default::default() }, Dest::File(&path), &mut no_progress).await.unwrap();
        let s = t.elapsed().as_secs_f64();
        let mb = std::fs::metadata(&path).unwrap().len() as f64 / 1_048_576.0;
        println!("{:<52} {:>9.1} ms  ({:.0} MB, {:.0} MB/s, {:.0} rows/s, RSS {:.0} -> {:.0} MB)", format!("export {name} (all rows)"), s * 1000.0, mb, mb / s, out.rows as f64 / s, before, rss_mb());
        assert_eq!(out.rows, rows);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
