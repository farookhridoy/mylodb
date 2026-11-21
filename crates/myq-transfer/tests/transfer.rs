//! Backup, restore and CSV import against a local MySQL. Every database is uniquely named and dropped
//! afterwards, even when an assertion fails.

use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::{pool, Conn, ConnConfig, Pool, SslMode};
use myq_transfer::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

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

fn noop() -> impl FnMut(Progress) + Send {
    |_| {}
}

/// Run `body` against a connection, then drop the given databases and delete the scratch directory.
async fn with_env<F, Fut>(tag: &str, dbs: &[&str], body: F)
where
    F: FnOnce(Pool, Vec<String>, PathBuf) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let pool = pool(&cfg());
    let mut admin = match pool.get_conn().await {
        Ok(c) => c,
        Err(e) => return eprintln!("SKIP: no MySQL reachable: {e}"),
    };
    let names: Vec<String> = dbs.iter().map(|d| format!("mylodb_itest_{tag}_{d}_{}", std::process::id())).collect();
    for n in &names {
        admin.query_drop(format!("DROP DATABASE IF EXISTS `{n}`")).await.unwrap();
    }
    let dir = std::env::temp_dir().join(format!("mylodb_xfer_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let outcome = tokio::spawn(body(pool.clone(), names.clone(), dir.clone())).await;
    for n in &names {
        let _ = admin.query_drop(format!("DROP DATABASE IF EXISTS `{n}`")).await;
    }
    let _ = std::fs::remove_dir_all(&dir);
    outcome.unwrap();
}

/// Which columns of which rows differ between two copies of a table (for a readable failure message).
async fn table_diff(c: &mut Conn, a: &str, b: &str, table: &str) -> String {
    let cols: Vec<String> = c.exec("SELECT COLUMN_NAME FROM information_schema.COLUMNS WHERE TABLE_SCHEMA=? AND TABLE_NAME=? ORDER BY ORDINAL_POSITION", (a, table)).await.unwrap();
    let mut out = String::new();
    let create = |db: &str| format!("SHOW CREATE TABLE `{db}`.`{table}`");
    let (ca, cb): (Option<(String, String)>, Option<(String, String)>) = (c.query_first(create(a)).await.unwrap(), c.query_first(create(b)).await.unwrap());
    if ca.as_ref().map(|x| &x.1) != cb.as_ref().map(|x| &x.1) {
        out.push_str(&format!("definitions differ:\n{}\n---\n{}\n", ca.map(|x| x.1).unwrap_or_default(), cb.map(|x| x.1).unwrap_or_default()));
    }
    for col in cols {
        let q = |db: &str| format!("SELECT id, HEX(CAST(`{col}` AS BINARY)) FROM `{db}`.`{table}` ORDER BY id");
        let (x, y): (Vec<(i64, Option<String>)>, Vec<(i64, Option<String>)>) = (c.query(q(a)).await.unwrap_or_default(), c.query(q(b)).await.unwrap_or_default());
        if x != y {
            let bytes = |h: &Option<String>| h.as_deref().map(|h| String::from_utf8_lossy(&(0..h.len()).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(63)).collect::<Vec<u8>>()).chars().take(60).collect::<String>());
            for (l, r) in x.iter().zip(&y) {
                if l != r {
                    out.push_str(&format!("column {col} id {}: {:?} != {:?}\n", l.0, bytes(&l.1), bytes(&r.1)));
                }
            }
        }
    }
    out
}

async fn checksum(c: &mut Conn, db: &str, table: &str) -> u64 {
    let row: Option<(String, Option<u64>)> = c.query_first(format!("CHECKSUM TABLE `{db}`.`{table}`")).await.unwrap();
    row.unwrap().1.expect("checksum")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// A database with every awkward thing a dump has to carry.
async fn build_source(c: &mut Conn, db: &str) {
    for stmt in [
        format!("CREATE DATABASE `{db}` CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_ai_ci"),
        format!("USE `{db}`"),
        // edge TIMESTAMP values are only valid in UTC; the dump itself always runs in UTC
        "SET SESSION time_zone = '+00:00'".to_string(),
        "CREATE TABLE kitchen (
            id INT UNSIGNED AUTO_INCREMENT PRIMARY KEY, i8 TINYINT, i64 BIGINT UNSIGNED, dec_ DECIMAL(30,10), f FLOAT, d DOUBLE, bits BIT(12), yr YEAR,
            dt DATETIME(6), ts TIMESTAMP(3) NULL, dd DATE, tm TIME(3), c CHAR(5), v VARCHAR(100), t TEXT, j JSON, e ENUM('a','b','it''s'), st SET('x','y','z'),
            bl BLOB, vb VARBINARY(300), pt POINT, gen INT AS (i8 * 2) STORED, KEY by_v (v)
         ) AUTO_INCREMENT = 100"
            .to_string(),
        "CREATE TABLE parent (id INT PRIMARY KEY, name VARCHAR(20))".to_string(),
        "CREATE TABLE child (id INT AUTO_INCREMENT PRIMARY KEY, parent_id INT NOT NULL, note VARCHAR(20), CONSTRAINT fk_p FOREIGN KEY (parent_id) REFERENCES parent(id) ON DELETE CASCADE) AUTO_INCREMENT = 50".to_string(),
        "CREATE TABLE `odd name; DROP`(`weird``col` INT, `select` VARCHAR(10))".to_string(),
        "CREATE TABLE big (id INT PRIMARY KEY, grp INT, txt MEDIUMTEXT)".to_string(),
    ] {
        c.query_drop(stmt).await.unwrap_or_else(|e| panic!("setup: {e}"));
    }
    let all_bytes: Vec<u8> = (0..=255u8).collect();
    // a zero date is only accepted without strict mode; the dump has to carry it through anyway
    c.query_drop("SET SESSION sql_mode = ''").await.unwrap();
    let nul_text = hex("a\0b\u{1a}c\r\n\t\\ 'quote' \"dq\" é日本😀".as_bytes());
    c.query_drop(format!(
        "INSERT INTO kitchen (id,i8,i64,dec_,f,d,bits,yr,dt,ts,dd,tm,c,v,t,j,e,st,bl,vb,pt) VALUES
         (1,-128,18446744073709551615,-99999999999999999999.9999999999,3.4028234e38,1.7976931348623157e308,b'101010101010',1901,'1000-01-01 00:00:00.000001','1970-01-01 00:00:01.123','9999-12-31','-838:59:59.000','abcde',
          'O''Reilly \"q\" \\\\ back', CONVERT(X'{nul_text}' USING utf8mb4), '{{\"a\": [1, 2, {{\"b\": null}}], \"s\": \"x''y\"}}', 'it''s', 'x,z', X'{}', X'{}', ST_GeomFromText('POINT(1 2)')),
         (2,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL),
         (3,0,0,0,0,0,b'0',2000,'2000-02-29 12:00:00.5','2038-01-19 03:14:07.999','0000-00-00','00:00:00','','','','[]','a','',X'',X'',ST_GeomFromText('POINT(-1.5 0)'))",
        hex(&all_bytes),
        hex(&all_bytes[..255]),
    ))
    .await
    .unwrap_or_else(|e| panic!("kitchen insert: {e}"));
    c.query_drop("SET SESSION sql_mode = DEFAULT").await.unwrap();
    c.query_drop("INSERT INTO parent VALUES (1,'one'),(2,'two')").await.unwrap();
    c.query_drop("INSERT INTO child (parent_id, note) VALUES (1,'a'),(1,'b'),(2,'c')").await.unwrap();
    c.query_drop("INSERT INTO `odd name; DROP` VALUES (1,'x'),(2,'y')").await.unwrap();
    c.query_drop("SET SESSION cte_max_recursion_depth = 30000").await.unwrap();
    c.query_drop("INSERT INTO big SELECT n, n % 7, IF(n % 1000 = 0, REPEAT('é', 120000), CONCAT('row ', n, ' it''s')) FROM (WITH RECURSIVE s(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM s WHERE n < 20000) SELECT n FROM s) q").await.unwrap_or_else(|e| panic!("big insert: {e}"));
    for stmt in [
        "CREATE VIEW z_base AS SELECT id, v FROM kitchen",
        "CREATE VIEW m_mid AS SELECT id FROM z_base WHERE id < 3",
        "CREATE VIEW a_top AS SELECT COUNT(*) AS n FROM m_mid",
        "CREATE FUNCTION add_one(x INT) RETURNS INT DETERMINISTIC BEGIN DECLARE y INT; SET y = x + 1; RETURN y; END",
        "CREATE PROCEDURE two_sets() BEGIN SELECT 1 AS a; SELECT 2 AS b; END",
        // created after the rows exist, so a restore that fired it while loading data would change them
        "CREATE TRIGGER parent_bi BEFORE INSERT ON parent FOR EACH ROW BEGIN SET NEW.name = CONCAT(NEW.name, '!'); END",
        "CREATE EVENT nightly ON SCHEDULE EVERY 1 DAY DISABLE DO DELETE FROM big WHERE id < 0",
    ] {
        c.query_drop(stmt).await.unwrap_or_else(|e| panic!("{stmt}: {e}"));
    }
}

const TABLES: [&str; 5] = ["kitchen", "parent", "child", "odd name; DROP", "big"];

fn opts(db: &str) -> BackupOptions {
    serde_json::from_value(serde_json::json!({ "databases": [{ "name": db }] })).unwrap()
}

#[tokio::test]
async fn a_database_survives_backup_drop_and_restore() {
    with_env("rt", &["src"], |pool, names, dir| async move {
        let src = names[0].as_str();
        let mut c = pool.get_conn().await.unwrap();
        build_source(&mut c, src).await;
        let mut sums = Vec::new();
        for t in &TABLES {
            sums.push((t.to_string(), checksum(&mut c, src, t).await));
        }
        let mut creates: Vec<String> = Vec::new();
        for t in &TABLES {
            let r: Option<(String, String)> = c.query_first(format!("SHOW CREATE TABLE `{src}`.`{t}`")).await.unwrap();
            creates.push(r.unwrap().1);
        }
        let view_before: Option<i64> = c.query_first(format!("SELECT n FROM `{src}`.a_top")).await.unwrap();

        // ---- back up ----
        let mut events = Vec::new();
        let mut progress = |p: Progress| events.push(p);
        let cancel = AtomicBool::new(false);
        let target = BackupTarget { dir: dir.clone(), name: "rt".into() };
        let res = backup(&mut c, &opts(src), &target, &mut progress, &cancel).await.unwrap();
        assert!(res.path.extension().unwrap() == "sql" && res.path.exists());
        assert_eq!((res.tables, res.databases, res.files), (5, 1, 1));
        assert_eq!(res.rows, 3 + 2 + 3 + 2 + 20000, "every row was dumped");
        assert!(res.bytes > 1_000_000, "the big table's rows are in the dump");
        assert!(res.warnings.is_empty(), "{:?}", res.warnings);
        assert!(!events.is_empty() && events.iter().all(|e| e.phase == "Backing up"), "progress is reported");
        let text = std::fs::read_to_string(&res.path).unwrap();
        assert!(text.contains("SET FOREIGN_KEY_CHECKS = 0;") && text.contains("CREATE DATABASE IF NOT EXISTS"));
        assert!(!text.contains("DEFINER="), "definers are stripped by default");
        assert!(text.find("INSERT INTO `parent`").unwrap() < text.find("CREATE TRIGGER").unwrap(), "triggers come after the data");
        let (b, m, a) = (text.find("VIEW `z_base`").unwrap(), text.find("VIEW `m_mid`").unwrap(), text.find("VIEW `a_top`").unwrap());
        assert!(b < m && m < a, "views are written in dependency order");
        assert!(!text.contains("`gen`) VALUES") && !text.contains(", `gen`"), "generated columns are not inserted");

        // ---- drop the whole database and restore it from the file ----
        c.query_drop(format!("DROP DATABASE `{src}`")).await.unwrap();
        let mut log = noop();
        let run = run_source(&mut c, &res.path, &RunOptions::default(), &mut log, &cancel).await.unwrap();
        assert_eq!((run.failed, run.aborted), (0, false), "{:?}", run.errors);
        assert!(run.statements > 100);
        for (t, before) in &sums {
            assert_eq!(checksum(&mut c, src, t).await, *before, "table {t} must be identical after the restore");
        }
        for (i, t) in TABLES.iter().enumerate() {
            let r: Option<(String, String)> = c.query_first(format!("SHOW CREATE TABLE `{src}`.`{t}`")).await.unwrap();
            assert_eq!(r.unwrap().1, creates[i], "definition of {t}");
        }
        let view: Option<i64> = c.query_first(format!("SELECT n FROM `{src}`.a_top")).await.unwrap();
        assert_eq!(view, view_before);
        let f: Option<i64> = c.query_first(format!("SELECT `{src}`.add_one(41)")).await.unwrap();
        assert_eq!(f, Some(42));
        let names: Vec<String> = c.query(format!("SELECT name FROM `{src}`.parent ORDER BY id")).await.unwrap();
        assert_eq!(names, vec!["one", "two"], "the trigger did not fire while rows were restored");
        c.query_drop(format!("INSERT INTO `{src}`.parent VALUES (3, 'three')")).await.unwrap();
        let three: Option<String> = c.query_first(format!("SELECT name FROM `{src}`.parent WHERE id = 3")).await.unwrap();
        assert_eq!(three.as_deref(), Some("three!"), "…but it works afterwards");
        let ev: Option<(String, String)> = c.query_first(format!("SELECT EVENT_NAME, STATUS FROM information_schema.EVENTS WHERE EVENT_SCHEMA='{src}'")).await.unwrap();
        assert_eq!(ev.unwrap().0, "nightly");
        c.query_drop(format!("INSERT INTO `{src}`.child (parent_id, note) VALUES (1, 'z')")).await.unwrap();
        let next: Option<i64> = c.query_first("SELECT LAST_INSERT_ID()").await.unwrap();
        assert_eq!(next, Some(53), "AUTO_INCREMENT continues where it was");
        let bytes: Option<Vec<u8>> = c.query_first(format!("SELECT bl FROM `{src}`.kitchen WHERE id = 1")).await.unwrap();
        assert_eq!(bytes.unwrap(), (0..=255u8).collect::<Vec<u8>>(), "all 256 byte values survive");
    })
    .await;
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(walk(&p))
        } else {
            out.push(p)
        }
    }
    out
}

#[tokio::test]
async fn restore_into_another_database_and_the_other_output_layouts() {
    with_env("lay", &["src", "dst", "dst2", "dst3"], |pool, names, dir| async move {
        let (src, dst, dst2, dst3) = (names[0].as_str(), names[1].as_str(), names[2].as_str(), names[3].as_str());
        let mut c = pool.get_conn().await.unwrap();
        build_source(&mut c, src).await;
        for d in [dst, dst2, dst3] {
            c.query_drop(format!("CREATE DATABASE `{d}`")).await.unwrap();
        }
        let cancel = AtomicBool::new(false);
        let mut log = noop();
        let base: BackupOptions = serde_json::from_value(serde_json::json!({
            "databases": [{ "name": src, "tables": ["kitchen", "parent", "child", "big"] }], "create_database": false, "views": false
        }))
        .unwrap();
        let tgt = |n: &str| BackupTarget { dir: dir.clone(), name: n.into() };

        // single zip, restored into a different database name
        let zip_res = backup(&mut c, &BackupOptions { compress: true, ..base.clone() }, &tgt("z"), &mut log, &cancel).await.unwrap();
        assert_eq!(zip_res.path.extension().unwrap(), "zip");
        let z = zip::ZipArchive::new(std::fs::File::open(&zip_res.path).unwrap()).unwrap();
        assert_eq!(z.len(), 1, "one .sql inside");
        let run = run_source(&mut c, &zip_res.path, &RunOptions { database: Some(dst.into()), ..Default::default() }, &mut log, &cancel).await.unwrap();
        assert_eq!(run.failed, 0, "{:?}", run.errors);
        for t in ["kitchen", "parent", "child", "big"] {
            let (a, b) = (checksum(&mut c, dst, t).await, checksum(&mut c, src, t).await);
            assert_eq!(a, b, "{t} via zip:\n{}", table_diff(&mut c, src, dst, t).await);
        }
        let tables: Vec<String> = c.query(format!("SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA='{dst}' ORDER BY 1")).await.unwrap();
        assert_eq!(tables, vec!["big", "child", "kitchen", "parent"], "only the selected tables");

        // a file per table, in a folder
        let folder = backup(&mut c, &BackupOptions { file_per_table: true, ..base.clone() }, &tgt("f"), &mut log, &cancel).await.unwrap();
        assert!(folder.path.is_dir());
        let mut files: Vec<String> = walk(&folder.path).into_iter().map(|p| p.strip_prefix(&folder.path).unwrap().to_string_lossy().into_owned()).collect();
        files.sort();
        assert!(files.iter().any(|f| f.ends_with("10_kitchen.sql")) && files.iter().any(|f| f.ends_with("10_big.sql")), "{files:?}");
        assert_eq!(folder.files as usize, files.len());
        let one = std::fs::read_to_string(folder.path.join(files.iter().find(|f| f.ends_with("10_parent.sql")).unwrap())).unwrap();
        assert!(one.starts_with("-- MyLoDB backup") && one.contains("SET FOREIGN_KEY_CHECKS = 1;"), "each file is self-contained");
        let run = run_source(&mut c, &folder.path, &RunOptions { database: Some(dst2.into()), ..Default::default() }, &mut log, &cancel).await.unwrap();
        assert_eq!(run.failed, 0, "{:?}", run.errors);
        assert_eq!(run.files as usize, files.len());
        for t in ["kitchen", "parent", "child", "big"] {
            assert_eq!(checksum(&mut c, dst2, t).await, checksum(&mut c, src, t).await, "{t} via folder");
        }

        // a file per table, inside a zip
        let zf = backup(&mut c, &BackupOptions { file_per_table: true, compress: true, ..base.clone() }, &tgt("zf"), &mut log, &cancel).await.unwrap();
        let z = zip::ZipArchive::new(std::fs::File::open(&zf.path).unwrap()).unwrap();
        assert!(z.len() >= 4);
        let run = run_source(&mut c, &zf.path, &RunOptions { database: Some(dst3.into()), ..Default::default() }, &mut log, &cancel).await.unwrap();
        assert_eq!(run.failed, 0, "{:?}", run.errors);
        for t in ["kitchen", "parent", "child", "big"] {
            assert_eq!(checksum(&mut c, dst3, t).await, checksum(&mut c, src, t).await, "{t} via zip folder");
        }

        // structure only / data only
        let s_only = backup(&mut c, &BackupOptions { data: false, ..base.clone() }, &tgt("so"), &mut log, &cancel).await.unwrap();
        let t = std::fs::read_to_string(&s_only.path).unwrap();
        assert!(t.contains("CREATE TABLE") && !t.contains("INSERT INTO") && s_only.rows == 0);
        let d_only = backup(&mut c, &BackupOptions { structure: false, ..base.clone() }, &tgt("do"), &mut log, &cancel).await.unwrap();
        let t = std::fs::read_to_string(&d_only.path).unwrap();
        assert!(t.contains("INSERT INTO") && !t.contains("CREATE TABLE") && !t.contains("DROP TABLE"));
        for bad in [
            BackupOptions { structure: false, data: false, ..base.clone() },
            opts("no_such_database_xyz"),
            BackupOptions { databases: vec![], ..base.clone() },
        ] {
            assert!(matches!(backup(&mut c, &bad, &tgt("x"), &mut log, &cancel).await, Err(Error::Invalid(_))));
        }
    })
    .await;
}

#[tokio::test]
async fn a_cancelled_backup_leaves_no_files_and_the_connection_is_usable() {
    with_env("cx", &["src"], |pool, names, dir| async move {
        let src = names[0].as_str();
        let mut c = pool.get_conn().await.unwrap();
        build_source(&mut c, src).await;
        for (per_file, compress) in [(false, false), (true, false), (false, true), (true, true)] {
            let cancel = AtomicBool::new(false);
            let flag = &cancel;
            let mut seen = 0;
            let mut progress = move |_p: Progress| {
                seen += 1;
                if seen == 2 {
                    flag.store(true, Ordering::Relaxed);
                }
            };
            let o = BackupOptions { file_per_table: per_file, compress, ..opts(src) };
            let r = backup(&mut c, &o, &BackupTarget { dir: dir.clone(), name: "cx".into() }, &mut progress, &cancel).await;
            assert!(matches!(r, Err(Error::Cancelled)), "per_file={per_file} compress={compress}: {r:?}");
            assert!(std::fs::read_dir(&dir).unwrap().count() == 0, "per_file={per_file} compress={compress}: nothing is left behind");
            let one: Option<i64> = c.query_first("SELECT 1").await.unwrap();
            assert_eq!(one, Some(1));
        }
    })
    .await;
}

#[tokio::test]
async fn scripts_report_errors_with_lines_and_can_stop_or_continue() {
    with_env("sc", &["a"], |pool, names, dir| async move {
        let db = names[0].as_str();
        let mut c = pool.get_conn().await.unwrap();
        c.query_drop(format!("CREATE DATABASE `{db}`")).await.unwrap();
        let script = format!(
            "USE `{db}`;\nCREATE TABLE t (id INT PRIMARY KEY, v VARCHAR(20));\nINSERT INTO t VALUES (1,'a');\n\nINSERT INTO t VALUES (1,'dup');\n-- a comment ; with a semicolon\nINSERT INTO t VALUES (2, 'it''s; ok');\nSELECT * FROM missing_table;\nDELIMITER $$\nCREATE PROCEDURE p() BEGIN INSERT INTO t VALUES (3,'p'); INSERT INTO t VALUES (4,'q'); END$$\nDELIMITER ;\nCALL p();\n"
        );
        let file = dir.join("s.sql");
        std::fs::write(&file, &script).unwrap();
        let cancel = AtomicBool::new(false);
        let mut log = noop();

        let r = run_source(&mut c, &file, &RunOptions::default(), &mut log, &cancel).await.unwrap();
        assert_eq!((r.statements, r.failed, r.aborted), (8, 2, false), "{:?}", r.errors);
        assert_eq!(r.errors.iter().map(|e| e.line).collect::<Vec<_>>(), vec![5, 8], "failures are reported with their line");
        assert!(r.errors[0].message.contains("Duplicate") && r.errors[1].message.contains("missing_table"));
        assert_eq!(r.errors[0].file, "s.sql");
        let ids: Vec<i64> = c.query(format!("SELECT id FROM `{db}`.t ORDER BY id")).await.unwrap();
        assert_eq!(ids, vec![1, 2, 3, 4], "statements after a failure still ran, the DELIMITER block stayed whole");

        c.query_drop(format!("DROP TABLE `{db}`.t")).await.unwrap();
        c.query_drop(format!("DROP PROCEDURE `{db}`.p")).await.unwrap();
        let r = run_source(&mut c, &file, &RunOptions { stop_on_error: true, ..Default::default() }, &mut log, &cancel).await.unwrap();
        assert!(r.aborted && r.failed == 1 && r.statements == 4, "{r:?}");
        let ids: Vec<i64> = c.query(format!("SELECT id FROM `{db}`.t ORDER BY id")).await.unwrap();
        assert_eq!(ids, vec![1], "nothing after the error ran");

        // foreign-key checks are off by default (restores need that) and back on afterwards
        let fk = dir.join("fk.sql");
        std::fs::write(&fk, format!("USE `{db}`;\nCREATE TABLE c1 (id INT, p INT, FOREIGN KEY (p) REFERENCES p1(id));\nCREATE TABLE p1 (id INT PRIMARY KEY);\n")).unwrap();
        let r = run_source(&mut c, &fk, &RunOptions::default(), &mut log, &cancel).await.unwrap();
        assert_eq!(r.failed, 0, "{:?}", r.errors);
        let on: Option<i64> = c.query_first("SELECT @@FOREIGN_KEY_CHECKS").await.unwrap();
        assert_eq!(on, Some(1));
        c.query_drop(format!("DROP TABLE `{db}`.c1")).await.unwrap();
        c.query_drop(format!("DROP TABLE `{db}`.p1")).await.unwrap();
        let r = run_source(&mut c, &fk, &RunOptions { disable_fk_checks: false, ..Default::default() }, &mut log, &cancel).await.unwrap();
        assert_eq!(r.failed, 1, "with checks on, the forward reference fails");

        // a missing file and an empty archive are errors, not silent successes
        assert!(run_source(&mut c, &dir.join("nope.sql"), &RunOptions::default(), &mut log, &cancel).await.is_err());
        let empty_zip = dir.join("empty.zip");
        zip::ZipWriter::new(std::fs::File::create(&empty_zip).unwrap()).finish().unwrap();
        assert!(matches!(run_source(&mut c, &empty_zip, &RunOptions::default(), &mut log, &cancel).await, Err(Error::Invalid(_))));
    })
    .await;
}

#[tokio::test]
async fn a_large_script_is_streamed_with_progress_and_can_be_cancelled() {
    with_env("lg", &["a"], |pool, names, dir| async move {
        let db = names[0].as_str();
        let mut c = pool.get_conn().await.unwrap();
        c.query_drop(format!("CREATE DATABASE `{db}`")).await.unwrap();
        c.query_drop(format!("CREATE TABLE `{db}`.t (id INT PRIMARY KEY, pad VARCHAR(200))")).await.unwrap();
        let file = dir.join("large.sql");
        {
            use std::io::Write;
            let mut f = std::io::BufWriter::new(std::fs::File::create(&file).unwrap());
            writeln!(f, "USE `{db}`;").unwrap();
            for i in 0..40_000 {
                writeln!(f, "INSERT INTO t VALUES ({i}, '{}');", "x".repeat(100)).unwrap();
            }
        }
        let size = std::fs::metadata(&file).unwrap().len();
        assert!(size > 4_000_000);
        let cancel = AtomicBool::new(false);
        let mut reports: Vec<Progress> = Vec::new();
        let mut progress = |p: Progress| reports.push(p);
        let r = run_source(&mut c, &file, &RunOptions::default(), &mut progress, &cancel).await.unwrap();
        assert_eq!((r.statements, r.failed), (40_001, 0));
        assert!(reports.len() >= 3, "progress is reported while running");
        assert!(reports.windows(2).all(|w| w[0].done <= w[1].done), "progress never goes backwards");
        let last = reports.last().unwrap();
        assert_eq!((last.done, last.total), (size, Some(size)));
        let n: Option<i64> = c.query_first(format!("SELECT COUNT(*) FROM `{db}`.t")).await.unwrap();
        assert_eq!(n, Some(40_000));

        // cancel part-way
        c.query_drop(format!("TRUNCATE TABLE `{db}`.t")).await.unwrap();
        let flag = &cancel;
        let mut count = 0;
        let mut stop_early = move |_p: Progress| {
            count += 1;
            if count == 2 {
                flag.store(true, Ordering::Relaxed);
            }
        };
        let r = run_source(&mut c, &file, &RunOptions::default(), &mut stop_early, &cancel).await;
        assert!(matches!(r, Err(Error::Cancelled)), "{r:?}");
        let n: Option<i64> = c.query_first(format!("SELECT COUNT(*) FROM `{db}`.t")).await.unwrap();
        assert!(n.unwrap() > 0 && n.unwrap() < 40_000, "stopped part-way: {n:?}");
        let on: Option<i64> = c.query_first("SELECT @@FOREIGN_KEY_CHECKS").await.unwrap();
        assert_eq!(on, Some(1), "the session is restored after a cancel");
    })
    .await;
}

// ------------------------------------------------------------------ CSV import

fn plan(db: &str, table: &str, mapping: &[Option<&str>], mode: ImportMode) -> ImportPlan {
    ImportPlan {
        database: db.into(),
        table: table.into(),
        mapping: mapping.iter().map(|m| m.map(String::from)).collect(),
        mode,
        batch_rows: 2,
        stop_on_error: true,
        truncate_first: false,
        empty_as_null: true,
        disable_fk_checks: false,
    }
}

type Row6 = (i64, String, Option<i64>, Option<String>, Option<String>, Option<String>);

async fn rows(c: &mut Conn, db: &str) -> Vec<Row6> {
    c.query(format!("SELECT id, name, qty, price, note, day FROM `{db}`.p ORDER BY id")).await.unwrap()
}

#[tokio::test]
async fn csv_files_load_with_mapping_modes_errors_and_rollback() {
    with_env("csv", &["a"], |pool, names, dir| async move {
        let db = names[0].as_str();
        let mut c = pool.get_conn().await.unwrap();
        c.query_drop(format!("CREATE DATABASE `{db}`")).await.unwrap();
        c.query_drop(format!("CREATE TABLE `{db}`.p (id INT PRIMARY KEY, name VARCHAR(20) NOT NULL, qty INT NULL, price DECIMAL(8,2) NULL, note TEXT NULL, day DATE NULL, gen INT AS (qty * 2) STORED)")).await.unwrap();
        let cancel = AtomicBool::new(false);
        let mut log = noop();
        let write = |n: &str, c: &[u8]| {
            let p = dir.join(n);
            std::fs::write(&p, c).unwrap();
            p
        };

        // header, quotes, a newline inside a field, a NULL marker, empty numbers, a short row
        let f = write("a.csv", b"id,name,qty,price,note,day\n1,Ann,5,9.50,\"hello, \"\"world\"\"\nline2\",2024-01-02\n2,Bob,,\\N,,\n3,\"Cy\"\n");
        let fmt = CsvFormat { null_text: Some("\\N".into()), ..Default::default() };
        let all6 = [Some("id"), Some("name"), Some("qty"), Some("price"), Some("note"), Some("day")];
        let r = import_csv(&mut c, &f, &fmt, &plan(db, "p", &all6, ImportMode::Insert), &mut log, &cancel).await.unwrap();
        assert_eq!((r.rows_read, r.affected_rows, r.aborted, r.ragged_rows), (3, 3, false, 1), "{r:?}");
        assert!(r.warnings.iter().any(|w| w.contains("different number of fields")));
        let got = rows(&mut c, db).await;
        assert_eq!(got[0], (1, "Ann".into(), Some(5), Some("9.50".into()), Some("hello, \"world\"\nline2".into()), Some("2024-01-02".into())));
        assert_eq!(got[1], (2, "Bob".into(), None, None, Some(String::new()), None), "empty number = NULL, \\N = NULL, empty text stays empty");
        assert_eq!(got[2], (3, "Cy".into(), None, None, None, None), "a short row loads missing fields as NULL");

        // a duplicate key in Insert mode undoes everything
        let dup = write("d.csv", b"id,name\n10,New\n11,Newer\n1,Dup\n12,After\n");
        let r = import_csv(&mut c, &dup, &CsvFormat::default(), &plan(db, "p", &[Some("id"), Some("name")], ImportMode::Insert), &mut log, &cancel).await.unwrap();
        assert!(r.aborted && r.rolled_back && r.affected_rows == 0, "{r:?}");
        assert!(r.errors[0].message.contains("Duplicate"), "{:?}", r.errors);
        assert_eq!(rows(&mut c, db).await.len(), 3, "rows 10 and 11 were rolled back");

        // continue past bad rows: good ones load, bad ones are listed with their line numbers
        let mixed = write("m.csv", b"id,name,qty\n20,Good,1\n21,,2\n22,TooLong_name_that_will_not_fit_in_twenty,3\n23,Fine,x\n24,Ok,4\n");
        let mut p = plan(db, "p", &[Some("id"), Some("name"), Some("qty")], ImportMode::Insert);
        p.stop_on_error = false;
        let r = import_csv(&mut c, &mixed, &CsvFormat::default(), &p, &mut log, &cancel).await.unwrap();
        assert!(!r.aborted && r.rows_read == 5, "{r:?}");
        let ids: Vec<i64> = rows(&mut c, db).await.iter().map(|r| r.0).collect();
        // `21` has an empty (non-NULL) name: MySQL accepts '' for a NOT NULL varchar, so only 22 and 23 fail
        assert_eq!(r.errors.iter().map(|e| e.line).collect::<Vec<_>>(), vec![4, 5], "{:?}", r.errors);
        assert_eq!(ids, vec![1, 2, 3, 20, 21, 24], "only the good rows were loaded");

        // modes
        let up = write("u.csv", b"id,name,qty\n1,Renamed,99\n30,Brand new,7\n");
        let m = |mode| plan(db, "p", &[Some("id"), Some("name"), Some("qty")], mode);
        let r = import_csv(&mut c, &up, &CsvFormat::default(), &m(ImportMode::Ignore), &mut log, &cancel).await.unwrap();
        assert_eq!(r.affected_rows, 1, "{r:?}");
        assert_eq!(rows(&mut c, db).await[0].1, "Ann", "IGNORE keeps the existing row");
        import_csv(&mut c, &up, &CsvFormat::default(), &m(ImportMode::Update), &mut log, &cancel).await.unwrap();
        let after = rows(&mut c, db).await;
        assert_eq!((after[0].1.as_str(), after[0].2, after[0].3.clone(), after[0].4.is_some()), ("Renamed", Some(99), Some("9.50".into()), true), "UPDATE changes only the mapped columns");
        import_csv(&mut c, &up, &CsvFormat::default(), &m(ImportMode::Replace), &mut log, &cancel).await.unwrap();
        let after = rows(&mut c, db).await;
        assert_eq!((after[0].3.clone(), after[0].4.clone()), (None, None), "REPLACE replaces the whole row, so unmapped columns reset");

        // truncate first, mapping a subset of columns in a different order, skipping one
        let t = write("t.csv", b"x,n,i\nskip,Zed,500\nskip,Yan,501\n");
        let mut p = plan(db, "p", &[None, Some("name"), Some("id")], ImportMode::Insert);
        p.truncate_first = true;
        let r = import_csv(&mut c, &t, &CsvFormat::default(), &p, &mut log, &cancel).await.unwrap();
        assert_eq!(r.affected_rows, 2);
        assert_eq!(rows(&mut c, db).await.iter().map(|r| (r.0, r.1.clone())).collect::<Vec<_>>(), vec![(500, "Zed".into()), (501, "Yan".into())]);

        // semicolon-separated, Latin-1, no header
        let l = write("l.csv", b"600;caf\xe9;1\n601;na\xefve;2\n");
        let lfmt = CsvFormat { delimiter: ';', has_header: false, encoding: "latin1".into(), ..Default::default() };
        import_csv(&mut c, &l, &lfmt, &plan(db, "p", &[Some("id"), Some("name"), Some("qty")], ImportMode::Insert), &mut log, &cancel).await.unwrap();
        let names: Vec<String> = c.query(format!("SELECT name FROM `{db}`.p WHERE id >= 600 ORDER BY id")).await.unwrap();
        assert_eq!(names, vec!["café", "naïve"]);

        // bad plans are refused before anything is written
        let n_before = rows(&mut c, db).await.len();
        for (bad, why) in [
            (plan(db, "p", &[None, None], ImportMode::Insert), "no mapped column"),
            (plan(db, "p", &[Some("id"), Some("id")], ImportMode::Insert), "mapped twice"),
            (plan(db, "p", &[Some("nope")], ImportMode::Insert), "unknown column"),
            (plan(db, "p", &[Some("gen")], ImportMode::Insert), "generated column"),
            (plan(db, "nope", &[Some("id")], ImportMode::Insert), "unknown table"),
        ] {
            let r = import_csv(&mut c, &l, &lfmt, &bad, &mut log, &cancel).await;
            assert!(matches!(r, Err(Error::Invalid(_))), "{why}: {r:?}");
        }
        assert_eq!(rows(&mut c, db).await.len(), n_before);

        // cancelling mid-import undoes everything
        let mut big = String::from("id,name\n");
        for i in 1000..1400 {
            big.push_str(&format!("{i},n{i}\n"));
        }
        let bf = write("big.csv", big.as_bytes());
        let flag = &cancel;
        let mut stop = move |_p: Progress| flag.store(true, Ordering::Relaxed);
        let mut p = plan(db, "p", &[Some("id"), Some("name")], ImportMode::Insert);
        p.batch_rows = 20;
        let r = import_csv(&mut c, &bf, &CsvFormat::default(), &p, &mut stop, &cancel).await;
        assert!(matches!(r, Err(Error::Cancelled)), "{r:?}");
        assert_eq!(rows(&mut c, db).await.len(), n_before, "a cancelled import leaves nothing behind");
        let one: Option<i64> = c.query_first("SELECT 1").await.unwrap();
        assert_eq!(one, Some(1), "…and the connection is usable");
    })
    .await;
}

#[tokio::test]
async fn csv_import_can_switch_foreign_key_checks_off_and_restores_them() {
    with_env("csvfk", &["a"], |pool, names, dir| async move {
        let db = names[0].as_str();
        let mut c = pool.get_conn().await.unwrap();
        c.query_drop(format!("CREATE DATABASE `{db}`")).await.unwrap();
        c.query_drop(format!("CREATE TABLE `{db}`.parent (id INT PRIMARY KEY) ENGINE=InnoDB")).await.unwrap();
        c.query_drop(format!("CREATE TABLE `{db}`.child (id INT PRIMARY KEY, pid INT, FOREIGN KEY (pid) REFERENCES `{db}`.parent(id)) ENGINE=InnoDB")).await.unwrap();
        let f = dir.join("child.csv");
        std::fs::write(&f, "id,pid\n1,99\n").unwrap();
        let cancel = AtomicBool::new(false);
        let mut log = noop();
        let cols = [Some("id"), Some("pid")];
        let fmt = CsvFormat::default();

        // checks on (the default): the orphan row is refused and nothing is loaded
        let on = import_csv(&mut c, &f, &fmt, &plan(db, "child", &cols, ImportMode::Insert), &mut log, &cancel).await.unwrap();
        assert!(on.aborted && on.rolled_back, "{on:?}");

        // checks off: the same file loads, and the session flag is back to 1 afterwards
        let mut p = plan(db, "child", &cols, ImportMode::Insert);
        p.disable_fk_checks = true;
        let off = import_csv(&mut c, &f, &fmt, &p, &mut log, &cancel).await.unwrap();
        assert!(!off.aborted && off.affected_rows == 1, "{off:?}");
        let flag: Option<u8> = c.query_first("SELECT @@foreign_key_checks").await.unwrap();
        assert_eq!(flag, Some(1), "the switch must not leak into the pooled connection");
    })
    .await;
}
