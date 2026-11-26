//! CSV import: detect the format, preview, infer a table definition, and load rows in batches inside
//! one transaction (so a failed or cancelled import leaves nothing behind).

use crate::{check_cancel, q, Error, Progress, ProgressFn, Result};
use encoding_rs_io::DecodeReaderBytesBuilder;
use myq_driver::mysql_async::{prelude::Queryable, Value};
use myq_driver::Conn;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

fn comma() -> char {
    ','
}
fn dquote() -> char {
    '"'
}
fn utf8() -> String {
    "utf-8".into()
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct CsvFormat {
    #[serde(default = "comma")]
    pub delimiter: char,
    #[serde(default = "dquote")]
    pub quote: char,
    #[serde(default = "yes")]
    pub has_header: bool,
    /// An encoding label: `utf-8`, `windows-1252`, `latin1`, `utf-16le`, `shift_jis` …
    #[serde(default = "utf8")]
    pub encoding: String,
    /// A cell equal to this text means SQL NULL (for example `NULL` or `\N`).
    #[serde(default)]
    pub null_text: Option<String>,
    /// Trim spaces around every cell.
    #[serde(default)]
    pub trim: bool,
}

impl Default for CsvFormat {
    fn default() -> Self {
        CsvFormat { delimiter: ',', quote: '"', has_header: true, encoding: utf8(), null_text: None, trim: false }
    }
}

type Reader = csv::Reader<Box<dyn Read + Send>>;

fn open(path: &Path, fmt: &CsvFormat) -> Result<Reader> {
    if !fmt.delimiter.is_ascii() || !fmt.quote.is_ascii() {
        return Err(Error::Invalid("the delimiter and quote must be single ASCII characters".into()));
    }
    let enc = encoding_rs::Encoding::for_label(fmt.encoding.trim().as_bytes()).ok_or_else(|| Error::Invalid(format!("unknown encoding `{}`", fmt.encoding)))?;
    let decoded = DecodeReaderBytesBuilder::new().encoding(Some(enc)).build(File::open(path)?);
    Ok(csv::ReaderBuilder::new()
        .delimiter(fmt.delimiter as u8)
        .quote(fmt.quote as u8)
        .has_headers(false) // the header is read as a normal record so both cases share one code path
        .flexible(true)
        .from_reader(Box::new(decoded) as Box<dyn Read + Send>))
}

fn clean(s: &str, fmt: &CsvFormat) -> String {
    if fmt.trim { s.trim().to_string() } else { s.to_string() }
}

// ---------- detection and preview ----------

/// Pick the delimiter that splits the first lines into the same, largest number of fields.
pub fn sniff_delimiter(path: &Path) -> Result<char> {
    let mut text = String::new();
    File::open(path)?.take(64 * 1024).read_to_string(&mut text).or_else(|_| -> std::io::Result<usize> { Ok(0) })?;
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).take(20).collect();
    let count = |line: &str, d: char| -> usize {
        let (mut n, mut in_q) = (0, false);
        for c in line.chars() {
            if c == '"' { in_q = !in_q } else if c == d && !in_q { n += 1 }
        }
        n
    };
    let mut best = (',', 0usize);
    for d in [',', ';', '\t', '|'] {
        let counts: Vec<usize> = lines.iter().map(|l| count(l, d)).collect();
        if let (Some(&first), true) = (counts.first(), counts.windows(2).all(|w| w[0] == w[1])) {
            if first > best.1 {
                best = (d, first);
            }
        }
    }
    Ok(best.0)
}

#[derive(Debug, Serialize)]
pub struct CsvPreview {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub column_count: usize,
    /// There are more rows than were read for the preview.
    pub more: bool,
}

pub fn preview_csv(path: &Path, fmt: &CsvFormat, limit: usize) -> Result<CsvPreview> {
    let mut rdr = open(path, fmt)?;
    let mut records = rdr.records();
    let mut first: Option<Vec<String>> = None;
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut more = false;
    for rec in &mut records {
        let rec: Vec<String> = rec?.iter().map(|c| clean(c, fmt)).collect();
        if fmt.has_header && first.is_none() {
            first = Some(rec);
        } else if rows.len() < limit {
            rows.push(rec);
        } else {
            more = true;
            break;
        }
    }
    let column_count = rows.iter().map(Vec::len).chain(first.iter().map(Vec::len)).max().unwrap_or(0);
    let headers = (0..column_count)
        .map(|i| first.as_ref().and_then(|h| h.get(i)).filter(|h| !h.is_empty()).cloned().unwrap_or_else(|| format!("column{}", i + 1)))
        .collect();
    Ok(CsvPreview { headers, rows, column_count, more })
}

// ---------- schema inference ----------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InferredColumn {
    pub name: String,
    /// A MySQL type such as `int`, `decimal(12,2)`, `datetime`, `varchar(255)`, `text`.
    pub column_type: String,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Kind {
    Empty,
    Int,
    BigInt,
    Decimal,
    Date,
    DateTime,
    Text,
}

fn classify(v: &str) -> (Kind, usize, usize) {
    // (kind, integer digits, fraction digits)
    let b = v.as_bytes();
    let unsigned = v.strip_prefix(['-', '+']).unwrap_or(v);
    let digits_only = !unsigned.is_empty() && unsigned.bytes().all(|c| c.is_ascii_digit());
    let leading_zero = unsigned.len() > 1 && unsigned.starts_with('0');
    if digits_only && !leading_zero {
        return match v.parse::<i32>() {
            Ok(_) => (Kind::Int, unsigned.len(), 0),
            Err(_) if v.parse::<i64>().is_ok() => (Kind::BigInt, unsigned.len(), 0),
            Err(_) => (Kind::Decimal, unsigned.len(), 0),
        };
    }
    if let Some((i, f)) = unsigned.split_once('.') {
        if !i.is_empty() && !f.is_empty() && i.bytes().all(|c| c.is_ascii_digit()) && f.bytes().all(|c| c.is_ascii_digit()) && !(i.len() > 1 && i.starts_with('0')) {
            return (Kind::Decimal, i.len(), f.len());
        }
    }
    let date = |s: &[u8]| s.len() == 10 && s[4] == b'-' && s[7] == b'-' && s.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    if date(b) {
        return (Kind::Date, 0, 0);
    }
    if b.len() >= 19 && date(&b[..10]) && (b[10] == b' ' || b[10] == b'T') && b[13] == b':' && b[16] == b':' {
        return (Kind::DateTime, 0, 0);
    }
    (Kind::Text, 0, 0)
}

fn merge(a: Kind, b: Kind) -> Kind {
    use Kind::*;
    match (a, b) {
        (x, y) if x == y => x,
        (Empty, y) => y,
        (x, Empty) => x,
        (Int, BigInt) | (BigInt, Int) => BigInt,
        (Int | BigInt, Decimal) | (Decimal, Int | BigInt) => Decimal,
        (Date, DateTime) | (DateTime, Date) => DateTime,
        _ => Text,
    }
}

/// Guess column types from the first `sample_rows` rows. Types are chosen to never reject a sampled value;
/// the user can edit the result before the table is created.
pub fn infer_schema(path: &Path, fmt: &CsvFormat, sample_rows: usize) -> Result<Vec<InferredColumn>> {
    let p = preview_csv(path, fmt, sample_rows)?;
    let n = p.column_count;
    let mut kinds = vec![Kind::Empty; n];
    let (mut int_digits, mut frac_digits, mut max_len) = (vec![0usize; n], vec![0usize; n], vec![0usize; n]);
    for row in &p.rows {
        for (i, cell) in row.iter().enumerate().take(n) {
            if cell.is_empty() || fmt.null_text.as_deref() == Some(cell.as_str()) {
                continue;
            }
            let (k, id, fd) = classify(cell);
            kinds[i] = merge(kinds[i], k);
            int_digits[i] = int_digits[i].max(id);
            frac_digits[i] = frac_digits[i].max(fd);
            max_len[i] = max_len[i].max(cell.chars().count());
        }
    }
    let mut used = HashSet::new();
    Ok((0..n)
        .map(|i| {
            let base: String = p.headers[i].trim().chars().take(60).collect();
            let mut name = if base.is_empty() { format!("column{}", i + 1) } else { base };
            while !used.insert(name.to_lowercase()) {
                name.push('_');
            }
            let column_type = match kinds[i] {
                Kind::Int => "int".to_string(),
                Kind::BigInt => "bigint".to_string(),
                Kind::Decimal => {
                    let scale = frac_digits[i].min(30);
                    format!("decimal({},{})", (int_digits[i] + scale).clamp(1, 65).max(scale), scale)
                }
                Kind::Date => "date".to_string(),
                Kind::DateTime => "datetime".to_string(),
                Kind::Empty | Kind::Text => match max_len[i] {
                    0..=50 => "varchar(255)".to_string(),
                    51..=255 => "varchar(500)".to_string(),
                    256..=1000 => "varchar(2000)".to_string(),
                    _ => "text".to_string(),
                },
            };
            InferredColumn { name, column_type }
        })
        .collect())
}

// ---------- import ----------

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImportMode {
    /// A duplicate key is an error.
    Insert,
    /// Rows that duplicate a key are skipped (`INSERT IGNORE`; other problems become warnings too).
    Ignore,
    /// A row with an existing key replaces it (`REPLACE`).
    Replace,
    /// A row with an existing key updates it (`ON DUPLICATE KEY UPDATE`).
    Update,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImportPlan {
    pub database: String,
    pub table: String,
    /// For each CSV column, the table column it fills (`None` skips it).
    pub mapping: Vec<Option<String>>,
    pub mode: ImportMode,
    #[serde(default = "default_batch")]
    pub batch_rows: usize,
    /// Stop at the first bad row and undo everything (otherwise load what can be loaded and list the rest).
    #[serde(default = "yes")]
    pub stop_on_error: bool,
    /// Empty the table first (TRUNCATE, which cannot be undone).
    #[serde(default)]
    pub truncate_first: bool,
    /// An empty cell is NULL for number, date and other non-text columns.
    #[serde(default = "yes")]
    pub empty_as_null: bool,
    /// Load with `foreign_key_checks = 0` (rows that point at parents loaded later, or not at all). Restored to 1 afterwards.
    #[serde(default)]
    pub disable_fk_checks: bool,
    /// Replace the table's rows: delete them inside the import's transaction, so a failed or empty import leaves them as they were.
    /// Refused for a table without transactions, or one that foreign keys point at (deleting could cascade into other tables).
    #[serde(default)]
    pub delete_first: bool,
}

fn default_batch() -> usize {
    500
}

#[derive(Debug, Clone, Serialize)]
pub struct RowError {
    /// Line of the CSV file (1-based).
    pub line: u64,
    pub message: String,
}

#[derive(Debug, Default, Serialize)]
pub struct ImportResult {
    pub rows_read: u64,
    /// Rows the server reports as affected (REPLACE and UPDATE count a replaced row twice).
    pub affected_rows: u64,
    pub errors: Vec<RowError>,
    /// Rows with more or fewer fields than the first row.
    pub ragged_rows: u64,
    pub aborted: bool,
    /// Everything was undone (the import stopped on an error).
    pub rolled_back: bool,
    pub warnings: Vec<String>,
    pub elapsed_ms: u64,
}

struct ColInfo {
    name: String,
    text_like: bool,
    binary: bool,
    bit: bool,
}

async fn column_info(conn: &mut Conn, db: &str, table: &str, wanted: &[String]) -> Result<(Vec<ColInfo>, String)> {
    let rows: Vec<(String, String, String)> = conn
        .exec("SELECT COLUMN_NAME, DATA_TYPE, EXTRA FROM information_schema.COLUMNS WHERE TABLE_SCHEMA=? AND TABLE_NAME=?", (db, table))
        .await?;
    if rows.is_empty() {
        return Err(Error::Invalid(format!("table `{db}`.`{table}` does not exist")));
    }
    let engine: Option<String> = conn.exec_first("SELECT ENGINE FROM information_schema.TABLES WHERE TABLE_SCHEMA=? AND TABLE_NAME=?", (db, table)).await?;
    let mut out = Vec::new();
    for w in wanted {
        let (_, ty, extra) = rows.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(w)).ok_or_else(|| Error::Invalid(format!("column `{w}` does not exist in `{table}`")))?;
        let e = extra.to_uppercase();
        if e.contains("GENERATED") && !e.contains("DEFAULT_GENERATED") {
            return Err(Error::Invalid(format!("column `{w}` is generated and cannot be loaded")));
        }
        let t = ty.to_lowercase();
        out.push(ColInfo {
            name: w.clone(),
            text_like: matches!(t.as_str(), "char" | "varchar" | "tinytext" | "text" | "mediumtext" | "longtext" | "enum" | "set" | "binary" | "varbinary" | "tinyblob" | "blob" | "mediumblob" | "longblob"),
            binary: matches!(t.as_str(), "binary" | "varbinary" | "tinyblob" | "blob" | "mediumblob" | "longblob"),
            bit: t == "bit",
        });
    }
    Ok((out, engine.unwrap_or_default()))
}

fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    let h = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))?;
    (h.len() % 2 == 0 && h.chars().all(|c| c.is_ascii_hexdigit())).then(|| (0..h.len()).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap()).collect())
}

fn to_value(cell: Option<&str>, col: &ColInfo, plan: &ImportPlan, fmt: &CsvFormat) -> Value {
    let Some(raw) = cell else { return Value::NULL };
    let v = clean(raw, fmt);
    if fmt.null_text.as_deref() == Some(v.as_str()) || (plan.empty_as_null && v.is_empty() && !col.text_like) {
        return Value::NULL;
    }
    if col.binary {
        if let Some(b) = hex_bytes(&v) {
            return Value::Bytes(b);
        }
    }
    if col.bit {
        if let Ok(n) = v.parse::<u64>() {
            return Value::UInt(n);
        }
    }
    Value::Bytes(v.into_bytes())
}

fn insert_sql(plan: &ImportPlan, cols: &[ColInfo], rows: usize) -> String {
    let names = cols.iter().map(|c| q(&c.name)).collect::<Vec<_>>().join(", ");
    let one = format!("({})", vec!["?"; cols.len()].join(","));
    let values = vec![one; rows].join(",");
    let target = format!("{}.{}", q(&plan.database), q(&plan.table));
    match plan.mode {
        ImportMode::Insert => format!("INSERT INTO {target} ({names}) VALUES {values}"),
        ImportMode::Ignore => format!("INSERT IGNORE INTO {target} ({names}) VALUES {values}"),
        ImportMode::Replace => format!("REPLACE INTO {target} ({names}) VALUES {values}"),
        ImportMode::Update => {
            let upd = cols.iter().map(|c| format!("{0} = VALUES({0})", q(&c.name))).collect::<Vec<_>>().join(", ");
            format!("INSERT INTO {target} ({names}) VALUES {values} ON DUPLICATE KEY UPDATE {upd}")
        }
    }
}

struct Batch {
    values: Vec<Vec<Value>>,
    lines: Vec<u64>,
}

/// Run one batch. On failure in continue-on-error mode the rows are retried one by one so only the
/// bad ones are reported.
async fn flush(conn: &mut Conn, plan: &ImportPlan, cols: &[ColInfo], b: &mut Batch, res: &mut ImportResult) -> Result<std::result::Result<(), String>> {
    if b.values.is_empty() {
        return Ok(Ok(()));
    }
    let params: Vec<Value> = b.values.iter().flatten().cloned().collect();
    match conn.exec_drop(insert_sql(plan, cols, b.values.len()), params).await {
        Ok(()) => res.affected_rows += conn.affected_rows(),
        Err(myq_driver::mysql_async::Error::Server(e)) if plan.stop_on_error => {
            return Ok(Err(format!("line {}: {} ({})", b.lines.first().copied().unwrap_or(0), e.message, e.code)));
        }
        Err(myq_driver::mysql_async::Error::Server(_)) => {
            for (row, line) in b.values.iter().zip(&b.lines) {
                match conn.exec_drop(insert_sql(plan, cols, 1), row.clone()).await {
                    Ok(()) => res.affected_rows += conn.affected_rows(),
                    Err(myq_driver::mysql_async::Error::Server(e)) => {
                        if res.errors.len() < 100 {
                            res.errors.push(RowError { line: *line, message: format!("{} ({})", e.message, e.code) });
                        }
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        }
        Err(e) => return Err(e.into()),
    }
    b.values.clear();
    b.lines.clear();
    Ok(Ok(()))
}

pub async fn import_csv(conn: &mut Conn, path: &Path, fmt: &CsvFormat, plan: &ImportPlan, progress: ProgressFn<'_>, cancel: &AtomicBool) -> Result<ImportResult> {
    if !plan.disable_fk_checks {
        return import_rows(conn, path, fmt, plan, progress, cancel).await;
    }
    conn.query_drop("SET SESSION foreign_key_checks = 0").await?;
    let result = import_rows(conn, path, fmt, plan, progress, cancel).await;
    // the connection goes back to the pool: leave the session as the next caller expects it
    let _ = conn.query_drop("SET SESSION foreign_key_checks = 1").await;
    result
}

async fn import_rows(conn: &mut Conn, path: &Path, fmt: &CsvFormat, plan: &ImportPlan, progress: ProgressFn<'_>, cancel: &AtomicBool) -> Result<ImportResult> {
    let started = Instant::now();
    let mapped: Vec<(usize, String)> = plan.mapping.iter().enumerate().filter_map(|(i, m)| m.clone().map(|m| (i, m))).collect();
    if mapped.is_empty() {
        return Err(Error::Invalid("map at least one CSV column to a table column".into()));
    }
    let mut seen = HashSet::new();
    if let Some((_, dup)) = mapped.iter().find(|(_, n)| !seen.insert(n.to_lowercase())) {
        return Err(Error::Invalid(format!("table column `{dup}` is mapped twice")));
    }
    let names: Vec<String> = mapped.iter().map(|(_, n)| n.clone()).collect();
    let (cols, engine) = column_info(conn, &plan.database, &plan.table, &names).await?;
    let per_batch = plan.batch_rows.clamp(1, 5000).min(60_000 / cols.len());
    let total = std::fs::metadata(path)?.len();

    let mut res = ImportResult::default();
    if !engine.eq_ignore_ascii_case("InnoDB") {
        res.warnings.push(format!("the table uses the {engine} engine, which has no transactions: a failed import cannot be undone"));
    }
    if plan.delete_first {
        if !engine.eq_ignore_ascii_case("InnoDB") {
            return Err(Error::Invalid(format!("the rows of `{}` cannot be replaced safely: the {engine} engine has no transactions", plan.table)));
        }
        let children: Option<u64> = conn.exec_first("SELECT COUNT(*) FROM information_schema.KEY_COLUMN_USAGE WHERE REFERENCED_TABLE_SCHEMA=? AND REFERENCED_TABLE_NAME=?", (&plan.database, &plan.table)).await?;
        if children.unwrap_or(0) > 0 {
            return Err(Error::Invalid(format!("the rows of `{}` cannot be replaced: foreign keys point at this table, so deleting them could change other tables", plan.table)));
        }
    }
    if plan.truncate_first {
        conn.query_drop(format!("TRUNCATE TABLE {}.{}", q(&plan.database), q(&plan.table))).await?;
    }
    conn.query_drop("START TRANSACTION").await?;
    if plan.delete_first {
        if let Err(e) = conn.query_drop(format!("DELETE FROM {}.{}", q(&plan.database), q(&plan.table))).await {
            let _ = conn.query_drop("ROLLBACK").await;
            return Err(e.into());
        }
    }

    let mut rdr = open(path, fmt)?;
    let mut batch = Batch { values: Vec::new(), lines: Vec::new() };
    let mut expected_fields: Option<usize> = None;
    let mut skip_header = fmt.has_header;
    let mut records = rdr.records();

    let outcome: Result<Option<String>> = async {
        while let Some(rec) = records.next() {
            let rec = rec?;
            if skip_header {
                skip_header = false;
                expected_fields = Some(rec.len());
                continue;
            }
            check_cancel(cancel)?;
            let line = rec.position().map_or(0, |p| p.line());
            res.rows_read += 1;
            let n = *expected_fields.get_or_insert(rec.len());
            if rec.len() != n {
                res.ragged_rows += 1;
            }
            batch.values.push(mapped.iter().zip(&cols).map(|((i, _), col)| to_value(rec.get(*i), col, plan, fmt)).collect());
            batch.lines.push(line);
            if batch.values.len() >= per_batch {
                if let Err(msg) = flush(conn, plan, &cols, &mut batch, &mut res).await? {
                    return Ok(Some(msg));
                }
                // once per batch; whoever displays progress decides how often to redraw
                progress(Progress { phase: "Importing".into(), done: records_position(&records).min(total), total: Some(total), message: format!("{} rows", res.rows_read) });
            }
        }
        if let Err(msg) = flush(conn, plan, &cols, &mut batch, &mut res).await? {
            return Ok(Some(msg));
        }
        if plan.delete_first && res.rows_read == 0 {
            return Ok(Some("the file has no rows, so the table was left as it was".into()));
        }
        Ok(None)
    }
    .await;

    match outcome {
        Ok(None) => {
            conn.query_drop("COMMIT").await?;
            progress(Progress { phase: "Importing".into(), done: total, total: Some(total), message: format!("{} rows", res.rows_read) });
        }
        Ok(Some(msg)) => {
            conn.query_drop("ROLLBACK").await?;
            let line = msg.strip_prefix("line ").and_then(|m| m.split(':').next()).and_then(|l| l.parse().ok()).unwrap_or(0);
            res.errors.push(RowError { line, message: msg });
            res.aborted = true;
            res.rolled_back = true;
            res.affected_rows = 0;
        }
        Err(e) => {
            let _ = conn.query_drop("ROLLBACK").await;
            return Err(e);
        }
    }
    if res.ragged_rows > 0 {
        res.warnings.push(format!("{} row(s) had a different number of fields than the first row; missing fields were loaded as NULL", res.ragged_rows));
    }
    res.elapsed_ms = started.elapsed().as_millis() as u64;
    Ok(res)
}

fn records_position<R: Read>(r: &csv::StringRecordsIter<'_, R>) -> u64 {
    r.reader().position().byte()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(name: &str, content: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("mylodb_csv_{}_{name}", std::process::id()));
        std::fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn sniffs_the_delimiter() {
        for (name, content, want) in [
            ("c", "a,b,c\n1,2,3\n4,5,6\n", ','),
            ("s", "a;b;c\n1;2;3\n", ';'),
            ("t", "a\tb\tc\n1\t2\t3\n", '\t'),
            ("p", "a|b\n1|2\n", '|'),
            ("q", "name;note\n\"x, y, z\";ok\n\"a, b\";fine\n", ';'),   // commas inside quotes do not count
            ("one", "single\nvalue\n", ','),
        ] {
            let p = write(name, content.as_bytes());
            assert_eq!(sniff_delimiter(&p).unwrap(), want, "{name}");
            std::fs::remove_file(p).unwrap();
        }
    }

    #[test]
    fn preview_handles_headers_quotes_ragged_rows_and_encodings() {
        let p = write("pv", "id,name,note\n1,\"Smith, J\",\"say \"\"hi\"\"\nnow\"\n2,Bob\n3,Cy,x,extra\n".as_bytes());
        let pv = preview_csv(&p, &CsvFormat::default(), 10).unwrap();
        assert_eq!(pv.headers, vec!["id", "name", "note", "column4"], "missing header names are generated");
        assert_eq!(pv.rows[0], vec!["1", "Smith, J", "say \"hi\"\nnow"]);
        assert_eq!(pv.rows.len(), 3);
        assert_eq!(pv.column_count, 4);
        let two = preview_csv(&p, &CsvFormat::default(), 2).unwrap();
        assert!(two.more && two.rows.len() == 2);
        let none = preview_csv(&p, &CsvFormat { has_header: false, ..Default::default() }, 1).unwrap();
        assert_eq!(none.rows[0], vec!["id", "name", "note"]);
        std::fs::remove_file(&p).unwrap();

        // Latin-1 and a UTF-8 byte-order mark
        let l = write("l1", b"n\ncaf\xe9\n");
        let r = preview_csv(&l, &CsvFormat { encoding: "latin1".into(), ..Default::default() }, 5).unwrap();
        assert_eq!(r.rows[0][0], "café");
        let b = write("bom", b"\xef\xbb\xbfid,v\n1,x\n");
        assert_eq!(preview_csv(&b, &CsvFormat::default(), 5).unwrap().headers[0], "id", "the BOM is not part of the first header");
        assert!(preview_csv(&b, &CsvFormat { encoding: "klingon".into(), ..Default::default() }, 5).is_err());
        assert!(preview_csv(&b, &CsvFormat { delimiter: 'é', ..Default::default() }, 5).is_err());
        for f in [l, b] { std::fs::remove_file(f).unwrap(); }
    }

    #[test]
    fn infers_types_that_accept_every_sampled_value() {
        let p = write("inf", "id,price,when,day,zip,name,big,mixed,empty,id\n1,10.5,2024-01-02 03:04:05,2024-01-02,01234,Ann,12345678901,1,,x\n2,3.25,2024-02-03T00:00:00,2024-02-03,99999,Bob,2,abc,,y\n".as_bytes());
        let cols = infer_schema(&p, &CsvFormat::default(), 100).unwrap();
        let t = |i: usize| cols[i].column_type.as_str();
        assert_eq!((t(0), t(1), t(2), t(3)), ("int", "decimal(4,2)", "datetime", "date"));
        assert_eq!(t(4), "varchar(255)", "a leading zero means an identifier, not a number");
        assert_eq!((t(5), t(6), t(7), t(8)), ("varchar(255)", "bigint", "varchar(255)", "varchar(255)"));
        assert_eq!((cols[0].name.as_str(), cols[9].name.as_str()), ("id", "id_"), "duplicate names are made unique");
        std::fs::remove_file(&p).unwrap();
        let long = write("long", format!("a,b\n{},{}\n", "x".repeat(300), "y".repeat(2000)).as_bytes());
        let c = infer_schema(&long, &CsvFormat::default(), 5).unwrap();
        assert_eq!((c[0].column_type.as_str(), c[1].column_type.as_str()), ("varchar(2000)", "text"));
        std::fs::remove_file(&long).unwrap();
    }

    #[test]
    fn classifies_values() {
        assert_eq!(classify("42").0, Kind::Int);
        assert_eq!(classify("-2147483648").0, Kind::Int);
        assert_eq!(classify("2147483648").0, Kind::BigInt);
        assert_eq!(classify("99999999999999999999").0, Kind::Decimal);
        assert_eq!(classify("0.5"), (Kind::Decimal, 1, 1));
        assert_eq!(classify("007").0, Kind::Text);
        assert_eq!(classify("1e5").0, Kind::Text);
        assert_eq!(classify("2024-13-45").0, Kind::Date, "shape only; the server validates dates");
        assert_eq!(merge(Kind::Int, Kind::Decimal), Kind::Decimal);
        assert_eq!(merge(Kind::Date, Kind::Int), Kind::Text);
    }

    #[test]
    fn insert_statements_for_each_mode() {
        let cols = vec![ColInfo { name: "a".into(), text_like: false, binary: false, bit: false }, ColInfo { name: "b`x".into(), text_like: true, binary: false, bit: false }];
        let plan = |mode| ImportPlan { database: "d".into(), table: "t".into(), mapping: vec![], mode, batch_rows: 1, stop_on_error: true, truncate_first: false, empty_as_null: true, disable_fk_checks: false, delete_first: false };
        assert_eq!(insert_sql(&plan(ImportMode::Insert), &cols, 2), "INSERT INTO `d`.`t` (`a`, `b``x`) VALUES (?,?),(?,?)");
        assert!(insert_sql(&plan(ImportMode::Ignore), &cols, 1).starts_with("INSERT IGNORE INTO"));
        assert!(insert_sql(&plan(ImportMode::Replace), &cols, 1).starts_with("REPLACE INTO"));
        assert!(insert_sql(&plan(ImportMode::Update), &cols, 1).ends_with("ON DUPLICATE KEY UPDATE `a` = VALUES(`a`), `b``x` = VALUES(`b``x`)"));
    }

    #[test]
    fn cell_conversion_rules() {
        let num = ColInfo { name: "n".into(), text_like: false, binary: false, bit: false };
        let txt = ColInfo { name: "t".into(), text_like: true, binary: false, bit: false };
        let bin = ColInfo { name: "b".into(), text_like: true, binary: true, bit: false };
        let bit = ColInfo { name: "x".into(), text_like: false, binary: false, bit: true };
        let plan = ImportPlan { database: "d".into(), table: "t".into(), mapping: vec![], mode: ImportMode::Insert, batch_rows: 1, stop_on_error: true, truncate_first: false, empty_as_null: true, disable_fk_checks: false, delete_first: false };
        let fmt = CsvFormat { null_text: Some("\\N".into()), trim: true, ..Default::default() };
        assert_eq!(to_value(Some(""), &num, &plan, &fmt), Value::NULL, "an empty number is NULL");
        assert_eq!(to_value(Some(""), &txt, &plan, &fmt), Value::Bytes(vec![]), "an empty string stays empty text");
        assert_eq!(to_value(Some("\\N"), &txt, &plan, &fmt), Value::NULL);
        assert_eq!(to_value(None, &txt, &plan, &fmt), Value::NULL, "a missing field is NULL");
        assert_eq!(to_value(Some("  7 "), &num, &plan, &fmt), Value::Bytes(b"7".to_vec()), "trim");
        assert_eq!(to_value(Some("0xCAFE"), &bin, &plan, &fmt), Value::Bytes(vec![0xCA, 0xFE]));
        assert_eq!(to_value(Some("0xCAFE"), &txt, &plan, &fmt), Value::Bytes(b"0xCAFE".to_vec()), "hex only for binary columns");
        assert_eq!(to_value(Some("5"), &bit, &plan, &fmt), Value::UInt(5), "BIT takes a number, not the character");
        let keep = ImportPlan { empty_as_null: false, ..plan };
        assert_eq!(to_value(Some(""), &num, &keep, &CsvFormat::default()), Value::Bytes(vec![]));
    }
}
