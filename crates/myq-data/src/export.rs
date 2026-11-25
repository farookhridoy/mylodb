//! Exporting rows to files. Every format is a `RowSink`; the table export and the "export this query"
//! export both stream rows into one, so a new format is written once.

use crate::{meta::TableMeta, query::{select_sql, PageSpec, Select}, q, Error, Result};
use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::{is_binary, is_numeric, row_cells, Conn};
use myq_driver::mysql_async::Column;
use rust_xlsxwriter::{Format, Workbook};
use serde::Deserialize;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    #[default]
    Csv,
    Json,
    /// One JSON object per line: streams, appends and loads into other tools without parsing the whole file.
    JsonLines,
    SqlInsert,
    Xml,
    Html,
    Xlsx,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Csv => "csv",
            ExportFormat::Json => "json",
            ExportFormat::JsonLines => "jsonl",
            ExportFormat::SqlInsert => "sql",
            ExportFormat::Xml => "xml",
            ExportFormat::Html => "html",
            ExportFormat::Xlsx => "xlsx",
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ExportOptions {
    pub format: ExportFormat,
    /// Compress the output with gzip (the file name should end in `.gz`). Not for XLSX, which is a zip container already.
    #[serde(default)]
    pub gzip: bool,
    /// Text written for SQL NULL in CSV (JSON, SQL and XML always have their own null).
    #[serde(default)]
    pub null_text: String,
    /// Table name used in INSERT statements and as the title of XML/HTML/XLSX output.
    #[serde(default)]
    pub name: Option<String>,
}

/// Where an export goes. XLSX is a zip container and needs a real file.
pub enum Dest<'a> {
    Writer(&'a mut (dyn Write + Send)),
    File(&'a Path),
}

#[derive(Debug, Default)]
pub struct ExportOutcome {
    pub rows: u64,
    /// Things the user should know (cells cut to fit a format's limit, characters XML cannot hold).
    pub warnings: Vec<String>,
}

/// Called every 1,000 rows and at the end with the number of rows written so far. Return false to cancel the export.
pub type Tick<'a> = &'a mut (dyn FnMut(u64) -> bool + Send);

/// A `Tick` that never cancels and reports nothing.
pub fn no_progress(_rows: u64) -> bool {
    true
}

/// How often the tick is called (and so how quickly a cancel takes effect).
const TICK_EVERY: u64 = 1_000;

#[derive(Debug, Clone)]
pub struct ExportColumn {
    pub name: String,
    pub numeric: bool,
    pub binary: bool,
}

fn columns_of(cols: &[Column]) -> Vec<ExportColumn> {
    cols.iter().map(|c| ExportColumn { name: c.name_str().to_string(), numeric: is_numeric(c), binary: is_binary(c) }).collect()
}

trait RowSink: Send {
    fn start(&mut self, cols: &[ExportColumn]) -> Result<()>;
    fn row(&mut self, cells: &[Option<String>]) -> Result<()>;
    fn finish(&mut self) -> Result<Vec<String>>;
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

fn sql_literal(v: &Option<String>, numeric: bool, binary: bool) -> String {
    match v {
        None => "NULL".into(),
        Some(s) if numeric => s.clone(),
        Some(s) if binary && s.starts_with("0x") => format!("X'{}'", &s[2..]),
        Some(s) => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "''")),
    }
}

fn json_string(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

fn xml_escape(s: &str, stripped: &mut u64) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // XML 1.0 cannot represent most control characters at all
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => *stripped += 1,
            c => out.push(c),
        }
    }
    out
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

// ---------- text sinks ----------

struct CsvSink<'a> {
    out: Box<dyn Write + Send + 'a>,
    null_text: String,
    /// One reusable line buffer: a row is built here and written once, without a String per cell.
    line: String,
}

fn push_csv(buf: &mut String, s: &str) {
    if s.contains([',', '"', '\n', '\r']) {
        buf.push('"');
        for c in s.chars() {
            if c == '"' { buf.push('"'); }
            buf.push(c);
        }
        buf.push('"');
    } else {
        buf.push_str(s);
    }
}

impl RowSink for CsvSink<'_> {
    fn start(&mut self, cols: &[ExportColumn]) -> Result<()> {
        writeln!(self.out, "{}", cols.iter().map(|c| csv_field(&c.name)).collect::<Vec<_>>().join(","))?;
        Ok(())
    }
    fn row(&mut self, cells: &[Option<String>]) -> Result<()> {
        self.line.clear();
        for (i, c) in cells.iter().enumerate() {
            if i > 0 {
                self.line.push(',');
            }
            push_csv(&mut self.line, c.as_deref().unwrap_or(&self.null_text));
        }
        self.line.push('\n');
        self.out.write_all(self.line.as_bytes())?;
        Ok(())
    }
    fn finish(&mut self) -> Result<Vec<String>> {
        self.out.flush()?;
        Ok(vec![])
    }
}

struct JsonSink<'a> {
    out: Box<dyn Write + Send + 'a>,
    cols: Vec<ExportColumn>,
    names: Vec<String>,
    line: String,
    rows: u64,
}

impl RowSink for JsonSink<'_> {
    fn start(&mut self, cols: &[ExportColumn]) -> Result<()> {
        self.cols = cols.to_vec();
        self.names = cols.iter().map(|c| json_string(&c.name)).collect();
        write!(self.out, "[")?;
        Ok(())
    }
    fn row(&mut self, cells: &[Option<String>]) -> Result<()> {
        self.line.clear();
        self.line.push_str(if self.rows == 0 { "\n  {" } else { ",\n  {" });
        for (i, (c, col)) in cells.iter().zip(&self.cols).enumerate() {
            if i > 0 {
                self.line.push_str(", ");
            }
            self.line.push_str(&self.names[i]);
            self.line.push_str(": ");
            push_json_cell(&mut self.line, c, col);
        }
        self.line.push('}');
        self.out.write_all(self.line.as_bytes())?;
        self.rows += 1;
        Ok(())
    }
    fn finish(&mut self) -> Result<Vec<String>> {
        writeln!(self.out, "{}]", if self.rows == 0 { "" } else { "\n" })?;
        self.out.flush()?;
        Ok(vec![])
    }
}

/// The cell as a JSON value: numbers stay numbers, everything else is a string.
fn push_json_cell(buf: &mut String, c: &Option<String>, col: &ExportColumn) {
    match c {
        None => buf.push_str("null"),
        Some(s) if col.numeric && s.parse::<f64>().is_ok() => buf.push_str(s),
        Some(s) => buf.push_str(&json_string(s)),
    }
}

struct JsonLinesSink<'a> {
    out: Box<dyn Write + Send + 'a>,
    cols: Vec<ExportColumn>,
    names: Vec<String>,
    line: String,
}

impl RowSink for JsonLinesSink<'_> {
    fn start(&mut self, cols: &[ExportColumn]) -> Result<()> {
        self.cols = cols.to_vec();
        self.names = cols.iter().map(|c| json_string(&c.name)).collect();
        Ok(())
    }
    fn row(&mut self, cells: &[Option<String>]) -> Result<()> {
        self.line.clear();
        self.line.push('{');
        for (i, (c, col)) in cells.iter().zip(&self.cols).enumerate() {
            if i > 0 {
                self.line.push(',');
            }
            self.line.push_str(&self.names[i]);
            self.line.push(':');
            push_json_cell(&mut self.line, c, col);
        }
        self.line.push_str("}\n");
        self.out.write_all(self.line.as_bytes())?;
        Ok(())
    }
    fn finish(&mut self) -> Result<Vec<String>> {
        self.out.flush()?;
        Ok(vec![])
    }
}

struct SqlSink<'a> {
    out: Box<dyn Write + Send + 'a>,
    table: String,
    cols: Vec<ExportColumn>,
    head: String,
    rows: u64,
}

impl RowSink for SqlSink<'_> {
    fn start(&mut self, cols: &[ExportColumn]) -> Result<()> {
        self.cols = cols.to_vec();
        self.head = format!("INSERT INTO {} ({}) VALUES\n", q(&self.table), cols.iter().map(|c| q(&c.name)).collect::<Vec<_>>().join(", "));
        Ok(())
    }
    fn row(&mut self, cells: &[Option<String>]) -> Result<()> {
        // batches of 100 rows per statement
        if self.rows % 100 == 0 {
            if self.rows > 0 {
                writeln!(self.out, ";")?;
            }
            write!(self.out, "{}", self.head)?;
        } else {
            writeln!(self.out, ",")?;
        }
        let vals: Vec<String> = cells.iter().zip(&self.cols).map(|(c, col)| sql_literal(c, col.numeric, col.binary)).collect();
        write!(self.out, "({})", vals.join(", "))?;
        self.rows += 1;
        Ok(())
    }
    fn finish(&mut self) -> Result<Vec<String>> {
        if self.rows > 0 {
            writeln!(self.out, ";")?;
        }
        self.out.flush()?;
        Ok(vec![])
    }
}

struct XmlSink<'a> {
    out: Box<dyn Write + Send + 'a>,
    name: String,
    cols: Vec<ExportColumn>,
    stripped: u64,
}

impl RowSink for XmlSink<'_> {
    fn start(&mut self, cols: &[ExportColumn]) -> Result<()> {
        self.cols = cols.to_vec();
        writeln!(self.out, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<resultset name=\"{}\">", xml_escape(&self.name, &mut self.stripped))?;
        Ok(())
    }
    fn row(&mut self, cells: &[Option<String>]) -> Result<()> {
        writeln!(self.out, "  <row>")?;
        for (c, col) in cells.iter().zip(&self.cols) {
            let name = xml_escape(&col.name, &mut self.stripped);
            match c {
                None => writeln!(self.out, "    <field name=\"{name}\" null=\"true\"/>")?,
                Some(v) => {
                    let text = xml_escape(v, &mut self.stripped);
                    writeln!(self.out, "    <field name=\"{name}\">{text}</field>")?;
                }
            }
        }
        writeln!(self.out, "  </row>")?;
        Ok(())
    }
    fn finish(&mut self) -> Result<Vec<String>> {
        writeln!(self.out, "</resultset>")?;
        self.out.flush()?;
        Ok(if self.stripped > 0 { vec![format!("{} control character(s) that XML cannot represent were removed", self.stripped)] } else { vec![] })
    }
}

struct HtmlSink<'a> {
    out: Box<dyn Write + Send + 'a>,
    name: String,
    cols: Vec<ExportColumn>,
}

impl RowSink for HtmlSink<'_> {
    fn start(&mut self, cols: &[ExportColumn]) -> Result<()> {
        self.cols = cols.to_vec();
        let title = html_escape(&self.name);
        writeln!(
            self.out,
            "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>{title}</title>\n<style>\nbody{{font:14px -apple-system,Helvetica,Arial,sans-serif;margin:24px}}\n\
             table{{border-collapse:collapse}}th,td{{border:1px solid #ccc;padding:4px 10px;text-align:left;vertical-align:top}}\nth{{background:#f0f0f0}}td.null{{color:#999;font-style:italic}}td.num{{text-align:right}}\n</style>\n</head>\n<body>\n<h2>{title}</h2>\n<table>\n<thead><tr>{}</tr></thead>\n<tbody>",
            cols.iter().map(|c| format!("<th>{}</th>", html_escape(&c.name))).collect::<String>()
        )?;
        Ok(())
    }
    fn row(&mut self, cells: &[Option<String>]) -> Result<()> {
        let tds: String = cells
            .iter()
            .zip(&self.cols)
            .map(|(c, col)| match c {
                None => "<td class=\"null\">NULL</td>".to_string(),
                Some(v) => format!("<td{}>{}</td>", if col.numeric { " class=\"num\"" } else { "" }, html_escape(v)),
            })
            .collect();
        writeln!(self.out, "<tr>{tds}</tr>")?;
        Ok(())
    }
    fn finish(&mut self) -> Result<Vec<String>> {
        writeln!(self.out, "</tbody>\n</table>\n</body>\n</html>")?;
        self.out.flush()?;
        Ok(vec![])
    }
}

// ---------- XLSX ----------

/// Excel's hard limits.
const XLSX_MAX_ROWS: u32 = 1_048_575; // plus the header row
const XLSX_MAX_CELL_CHARS: usize = 32_767;

struct XlsxSink {
    book: Workbook,
    path: std::path::PathBuf,
    name: String,
    cols: Vec<ExportColumn>,
    next_row: u32,
    truncated: u64,
}

impl XlsxSink {
    fn new(path: &Path, name: &str) -> Self {
        XlsxSink { book: Workbook::new(), path: path.to_path_buf(), name: name.to_string(), cols: vec![], next_row: 1, truncated: 0 }
    }
}

fn xe(e: rust_xlsxwriter::XlsxError) -> Error {
    Error::Invalid(format!("xlsx: {e}"))
}

/// A number only when Excel can hold it exactly (it keeps 15 significant digits); otherwise text,
/// so identifiers and large DECIMALs are never silently rounded.
fn exact_number(s: &str) -> Option<f64> {
    let digits = s.chars().filter(|c| c.is_ascii_digit()).count();
    let plain = !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || matches!(c, '-' | '.' | '+')) && !(s.len() > 1 && s.starts_with('0') && !s.starts_with("0."));
    if plain && digits <= 15 { s.parse::<f64>().ok().filter(|f| f.is_finite()) } else { None }
}

impl RowSink for XlsxSink {
    fn start(&mut self, cols: &[ExportColumn]) -> Result<()> {
        self.cols = cols.to_vec();
        let title: String = self.name.chars().filter(|c| !matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\')).take(31).collect();
        let sheet = self.book.add_worksheet_with_constant_memory().map_err(xe)?;
        if !title.is_empty() {
            sheet.set_name(&title).map_err(xe)?;
        }
        let bold = Format::new().set_bold();
        for (i, c) in cols.iter().enumerate() {
            sheet.write_string_with_format(0, i as u16, &c.name, &bold).map_err(xe)?;
        }
        Ok(())
    }
    fn row(&mut self, cells: &[Option<String>]) -> Result<()> {
        if self.next_row > XLSX_MAX_ROWS {
            return Err(Error::Invalid(format!("an Excel sheet holds at most {XLSX_MAX_ROWS} data rows; export to CSV instead")));
        }
        let r = self.next_row;
        let numeric: Vec<bool> = self.cols.iter().map(|c| c.numeric).collect();
        let mut cut = 0;
        let sheet = self.book.worksheet_from_index(0).map_err(xe)?;
        for (i, c) in cells.iter().enumerate() {
            let Some(v) = c else { continue }; // NULL stays an empty cell
            match (numeric[i], exact_number(v)) {
                (true, Some(n)) => {
                    sheet.write_number(r, i as u16, n).map_err(xe)?;
                }
                _ if v.chars().count() > XLSX_MAX_CELL_CHARS => {
                    let short: String = v.chars().take(XLSX_MAX_CELL_CHARS).collect();
                    sheet.write_string(r, i as u16, &short).map_err(xe)?;
                    cut += 1;
                }
                _ => {
                    sheet.write_string(r, i as u16, v).map_err(xe)?;
                }
            }
        }
        self.truncated += cut;
        self.next_row += 1;
        Ok(())
    }
    fn finish(&mut self) -> Result<Vec<String>> {
        self.book.save(&self.path).map_err(xe)?;
        Ok(if self.truncated > 0 { vec![format!("{} cell(s) longer than {XLSX_MAX_CELL_CHARS} characters were cut to Excel's limit", self.truncated)] } else { vec![] })
    }
}

// ---------- driving a sink ----------

/// gzip on top of a writer. The stream is finished (its trailer written) when the sink flushes, which every sink does once, at the end.
struct GzWriter<W: Write> {
    inner: flate2::write::GzEncoder<W>,
}

impl<W: Write> Write for GzWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        // finish the gzip stream (its trailer), then flush what is under it; the encoder itself must not be flushed after finishing
        self.inner.try_finish()?;
        self.inner.get_mut().flush()
    }
}

/// The sink for `opts.format`, writing to `dest`.
fn make_sink<'a>(opts: &ExportOptions, dest: Dest<'a>, default_name: &str) -> Result<Box<dyn RowSink + Send + 'a>> {
    let name = opts.name.clone().unwrap_or_else(|| default_name.to_string());
    if opts.format == ExportFormat::Xlsx {
        if opts.gzip {
            return Err(Error::Invalid("XLSX is already a compressed file; leave gzip off".into()));
        }
        let Dest::File(path) = dest else { return Err(Error::Invalid("XLSX needs a file destination".into())) };
        return Ok(Box::new(XlsxSink::new(path, &name)));
    }
    let out: Box<dyn Write + Send + 'a> = match dest {
        Dest::Writer(w) => Box::new(w), // `&mut dyn Write` is itself a Write
        Dest::File(p) => Box::new(BufWriter::with_capacity(256 * 1024, File::create(p)?)),
    };
    // a buffer above the encoder: the compressor gets 256 KB chunks instead of one small write per row
    let out: Box<dyn Write + Send + 'a> = if opts.gzip { Box::new(BufWriter::with_capacity(256 * 1024, GzWriter { inner: flate2::write::GzEncoder::new(out, flate2::Compression::fast()) })) } else { out };
    Ok(match opts.format {
        ExportFormat::Csv => Box::new(CsvSink { out, null_text: opts.null_text.clone(), line: String::new() }),
        ExportFormat::Json => Box::new(JsonSink { out, cols: vec![], names: vec![], line: String::new(), rows: 0 }),
        ExportFormat::JsonLines => Box::new(JsonLinesSink { out, cols: vec![], names: vec![], line: String::new() }),
        ExportFormat::SqlInsert => Box::new(SqlSink { out, table: name, cols: vec![], head: String::new(), rows: 0 }),
        ExportFormat::Xml => Box::new(XmlSink { out, name, cols: vec![], stripped: 0 }),
        ExportFormat::Html => Box::new(HtmlSink { out, name, cols: vec![] }),
        ExportFormat::Xlsx => unreachable!("handled above"),
    })
}

/// Stream every row matching the filters/sort of `spec` (paging is ignored) to `dest`.
pub async fn export_rows(conn: &mut Conn, meta: &TableMeta, spec: &PageSpec, opts: &ExportOptions, dest: Dest<'_>, tick: Tick<'_>) -> Result<ExportOutcome> {
    let all = PageSpec { limit: u32::MAX, offset: 0, ..spec.clone() };
    let Select { sql, params, .. } = select_sql(meta, &all)?;
    // `select_sql` appends LIMIT/OFFSET for paging; exports want everything, so drop that tail.
    let sql = sql.rsplit_once(" LIMIT ").map(|(head, _)| head.to_string()).unwrap_or(sql);
    let mut sink = make_sink(opts, dest, &meta.table)?;
    let mut qr = conn.exec_iter(sql, params).await?;
    let cols = columns_of(&qr.columns().map(|c| c.to_vec()).unwrap_or_default());
    sink.start(&cols)?;
    let mut rows = 0u64;
    while let Some(row) = qr.next().await? {
        sink.row(&row_cells(row, None).0)?;
        rows += 1;
        if rows % TICK_EVERY == 0 && !tick(rows) {
            return Err(Error::Cancelled);
        }
    }
    tick(rows);
    Ok(ExportOutcome { rows, warnings: sink.finish()? })
}

/// A writer for one export that takes its rows from anywhere (the SQLite engine streams its rows through this): `start` with the
/// columns, `row` for each row, `finish` for the warnings.
pub struct Exporter<'a> {
    sink: Box<dyn RowSink + Send + 'a>,
    rows: u64,
}

impl<'a> Exporter<'a> {
    pub fn new(opts: &ExportOptions, dest: Dest<'a>, default_name: &str, cols: &[ExportColumn]) -> Result<Exporter<'a>> {
        let mut sink = make_sink(opts, dest, default_name)?;
        sink.start(cols)?;
        Ok(Exporter { sink, rows: 0 })
    }

    pub fn row(&mut self, cells: &[Option<String>]) -> Result<()> {
        self.sink.row(cells)?;
        self.rows += 1;
        Ok(())
    }

    pub fn rows(&self) -> u64 {
        self.rows
    }

    pub fn finish(mut self) -> Result<ExportOutcome> {
        Ok(ExportOutcome { rows: self.rows, warnings: self.sink.finish()? })
    }
}

/// Export the result of a read-only statement.
pub async fn export_query(conn: &mut Conn, database: Option<&str>, sql: &str, opts: &ExportOptions, dest: Dest<'_>, tick: Tick<'_>) -> Result<ExportOutcome> {
    let stmts = myq_sql::split_statements(sql);
    let [stmt] = stmts.as_slice() else { return Err(Error::Invalid("export one statement at a time".into())) };
    if myq_sql::statement_kind(&stmt.text) != myq_sql::StatementKind::Read {
        return Err(Error::Invalid("only SELECT statements can be exported".into()));
    }
    if let Some(db) = database {
        conn.query_drop(format!("USE {}", q(db))).await?;
    }
    let mut sink = make_sink(opts, dest, "query")?;
    // typed (binary) protocol: a FLOAT keeps its full precision, which the text protocol rounds to ~6 digits
    let mut qr = conn.exec_iter(stmt.text.clone(), ()).await?;
    let cols = columns_of(&qr.columns().map(|c| c.to_vec()).unwrap_or_default());
    sink.start(&cols)?;
    let mut rows = 0u64;
    while let Some(row) = qr.next().await? {
        sink.row(&row_cells(row, None).0)?;
        rows += 1;
        if rows % TICK_EVERY == 0 && !tick(rows) {
            return Err(Error::Cancelled);
        }
    }
    tick(rows);
    Ok(ExportOutcome { rows, warnings: sink.finish()? })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_quotes_only_when_needed() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\"\nnow"), "\"say \"\"hi\"\"\nnow\"");
    }

    #[test]
    fn sql_literals_escape_and_keep_numbers_and_binary() {
        assert_eq!(sql_literal(&None, false, false), "NULL");
        assert_eq!(sql_literal(&Some("12.50".into()), true, false), "12.50");
        assert_eq!(sql_literal(&Some("it's \\".into()), false, false), "'it''s \\\\'");
        assert_eq!(sql_literal(&Some("0xCAFE".into()), false, true), "X'CAFE'");
        assert_eq!(sql_literal(&Some("0xCAFE".into()), false, false), "'0xCAFE'", "only binary columns are hex literals");
    }

    #[test]
    fn xml_and_html_escape_markup_and_xml_drops_control_characters() {
        let mut n = 0;
        assert_eq!(xml_escape("a<b>&\"c\"\u{1}\u{0}\t", &mut n), "a&lt;b&gt;&amp;&quot;c&quot;\t");
        assert_eq!(n, 2);
        assert_eq!(html_escape("<script>alert(\"x\")</script> & co"), "&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt; &amp; co");
    }

    #[test]
    fn excel_numbers_are_only_used_when_exact() {
        assert_eq!(exact_number("42"), Some(42.0));
        assert_eq!(exact_number("-3.25"), Some(-3.25));
        assert_eq!(exact_number("123456789012345"), Some(123456789012345.0));
        assert_eq!(exact_number("1234567890123456"), None, "16 digits would lose precision");
        assert_eq!(exact_number("007"), None, "leading zeros are text (zip codes, ids)");
        assert_eq!(exact_number("0.5"), Some(0.5));
        assert_eq!(exact_number("12abc"), None);
        assert_eq!(exact_number(""), None);
    }

    /// Writer speed without a database: `cargo test -p myq-data --lib --release writer_speed -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn writer_speed() {
        let cols: Vec<ExportColumn> = ["id", "customer_id", "status", "amount", "note", "created_at"].iter().enumerate()
            .map(|(i, n)| ExportColumn { name: n.to_string(), numeric: i < 2 || i == 3, binary: false }).collect();
        let rows: Vec<Vec<Option<String>>> = (0..200_000u64)
            .map(|i| vec![Some(i.to_string()), Some((i * 7919 % 100000).to_string()), Some("shipped".into()), Some(format!("{:.2}", i as f64 / 7.0)), Some(format!("order note {i}")), Some("2020-03-04 05:06:07".into())])
            .collect();
        for (name, mut sink) in [
            ("csv", Box::new(CsvSink { out: Box::new(std::io::sink()), null_text: String::new(), line: String::new() }) as Box<dyn RowSink>),
            ("jsonl", Box::new(JsonLinesSink { out: Box::new(std::io::sink()), cols: vec![], names: vec![], line: String::new() })),
        ] {
            sink.start(&cols).unwrap();
            let t = std::time::Instant::now();
            for r in &rows {
                sink.row(r).unwrap();
            }
            sink.finish().unwrap();
            let s = t.elapsed().as_secs_f64();
            println!("{name:<8} {:>7.1} ms for {} rows  ({:.0} rows/s)", s * 1000.0, rows.len(), rows.len() as f64 / s);
        }
    }
}
