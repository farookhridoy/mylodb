//! Statement splitting and script execution.

use crate::Result;
use mysql_async::{prelude::Queryable, Conn, Pool, Row};
use serde::Serialize;
use std::time::Instant;

/// Rows kept per result set; the rest are counted but dropped (the grid phase adds paging).
pub const MAX_ROWS: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    pub text: String,
    /// Byte offset of the statement in the original script.
    pub start: usize,
}

/// Split a script on the active delimiter, ignoring delimiters inside quotes, backticks and comments.
/// Understands the mysql-client `DELIMITER x` command so stored-routine bodies stay in one piece.
pub fn split_statements(sql: &str) -> Vec<Statement> {
    let b = sql.as_bytes();
    let mut out = Vec::new();
    let mut delim = ";".to_string();
    let (mut i, mut start) = (0usize, 0usize);

    let push = |out: &mut Vec<Statement>, from: usize, to: usize| {
        let text = sql[from..to].trim();
        if !text.is_empty() && !is_comment_only(text) {
            let lead = sql[from..to].len() - sql[from..to].trim_start().len();
            out.push(Statement { text: text.to_string(), start: from + lead });
        }
    };

    while i < b.len() {
        // DELIMITER command: only at the start of a line.
        let at_line_start = sql[start..i].trim().is_empty();
        if at_line_start && sql[i..].len() >= 10 && sql[i..i + 9].eq_ignore_ascii_case("delimiter") && b[i + 9].is_ascii_whitespace() {
            let line_end = sql[i..].find('\n').map_or(sql.len(), |p| i + p);
            let new = sql[i + 9..line_end].trim();
            if !new.is_empty() {
                delim = new.to_string();
            }
            i = (line_end + 1).min(sql.len());
            start = i;
            continue;
        }
        match b[i] {
            b'\'' | b'"' | b'`' => {
                let q = b[i];
                i += 1;
                while i < b.len() {
                    if b[i] == b'\\' && q != b'`' {
                        i += 2;
                        continue;
                    }
                    if b[i] == q {
                        if i + 1 < b.len() && b[i + 1] == q {
                            i += 2; // doubled quote
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'#' => i = sql[i..].find('\n').map_or(sql.len(), |p| i + p),
            b'-' if sql[i..].starts_with("--") && b.get(i + 2).map_or(true, |c| c.is_ascii_whitespace()) => {
                i = sql[i..].find('\n').map_or(sql.len(), |p| i + p)
            }
            b'/' if sql[i..].starts_with("/*") => i = sql[i + 2..].find("*/").map_or(sql.len(), |p| i + 2 + p + 2),
            _ if sql[i..].starts_with(delim.as_str()) => {
                push(&mut out, start, i);
                i += delim.len();
                start = i;
            }
            _ => i += 1,
        }
    }
    push(&mut out, start, sql.len().min(i.max(start)));
    out
}

fn is_comment_only(s: &str) -> bool {
    let mut rest = s.trim();
    loop {
        if let Some(r) = rest.strip_prefix("/*") {
            match r.find("*/") {
                Some(p) => rest = r[p + 2..].trim_start(),
                None => return true,
            }
        } else if rest.starts_with('#') || rest.starts_with("--") {
            match rest.find('\n') {
                Some(p) => rest = rest[p + 1..].trim_start(),
                None => return true,
            }
        } else {
            return rest.is_empty();
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ColumnInfo {
    pub name: String,
    pub type_name: String,
    /// BINARY/BLOB columns: cells are hex text, not characters.
    pub binary: bool,
}

/// `None` cells are SQL NULL (distinct from the empty string).
#[derive(Debug, Clone, Serialize)]
pub struct ResultSet {
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<Vec<Option<String>>>,
    pub total_rows: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct StatementResult {
    pub sql: String,
    pub sets: Vec<ResultSet>,
    pub affected_rows: u64,
    pub last_insert_id: Option<u64>,
    pub warnings: u16,
    pub elapsed_ms: u64,
    pub error: Option<String>,
}

async fn run_one(conn: &mut Conn, sql: &str) -> StatementResult {
    let started = Instant::now();
    let mut res = StatementResult { sql: sql.to_string(), ..Default::default() };
    let outcome: Result<()> = async {
        let mut qr = conn.query_iter(sql).await?;
        loop {
            let columns: Vec<ColumnInfo> = qr
                .columns()
                .map(|c| c.iter().map(|c| ColumnInfo { name: c.name_str().to_string(), type_name: format!("{:?}", c.column_type()), binary: crate::values::is_binary(c) }).collect())
                .unwrap_or_default();
            let (rows, total) = qr
                .reduce((Vec::<Vec<Option<String>>>::new(), 0u64), |(mut rows, n), row: Row| {
                    if rows.len() < MAX_ROWS {
                        rows.push(crate::values::row_cells(row, None).0);
                    }
                    (rows, n + 1)
                })
                .await?;
            if !columns.is_empty() {
                res.sets.push(ResultSet { columns, truncated: total > rows.len() as u64, total_rows: total, rows });
            }
            res.affected_rows = qr.affected_rows();
            res.last_insert_id = qr.last_insert_id();
            res.warnings = qr.warnings();
            if qr.is_empty() {
                break;
            }
        }
        Ok(())
    }
    .await;
    if let Err(e) = outcome {
        res.error = Some(e.to_string());
    }
    res.elapsed_ms = started.elapsed().as_millis() as u64;
    res
}

/// Run every statement of `sql` in order on `conn`, stopping after the first error.
/// `database` is selected first when given.
pub async fn execute_script(conn: &mut Conn, database: Option<&str>, sql: &str) -> Vec<StatementResult> {
    let mut out = Vec::new();
    if let Some(db) = database {
        let use_sql = format!("USE `{}`", db.replace('`', "``"));
        if let Err(e) = conn.query_drop(use_sql).await {
            return vec![StatementResult { sql: format!("USE {db}"), error: Some(e.to_string()), ..Default::default() }];
        }
    }
    for st in split_statements(sql) {
        let r = run_one(conn, &st.text).await;
        let failed = r.error.is_some();
        out.push(r);
        if failed {
            break;
        }
    }
    out
}

/// Interrupt the statement running on server connection `connection_id` (`KILL QUERY`).
pub async fn kill_query(pool: &Pool, connection_id: u32) -> Result<()> {
    let mut c = pool.get_conn().await?;
    c.query_drop(format!("KILL QUERY {connection_id}")).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(s: &str) -> Vec<String> {
        split_statements(s).into_iter().map(|s| s.text).collect()
    }

    #[test]
    fn splits_and_ignores_delimiters_in_strings_and_comments() {
        assert_eq!(
            texts("SELECT 'a;b'; -- c;d\nSELECT `x;y`; /* e;f */ SELECT \"q\\\";\"; SELECT 1 # z;\n"),
            ["SELECT 'a;b'", "-- c;d\nSELECT `x;y`", "/* e;f */ SELECT \"q\\\";\"", "SELECT 1 # z;"]
        );
    }

    #[test]
    fn last_statement_without_semicolon_and_empty_noise() {
        assert_eq!(texts(";;  SELECT 1;\n\n-- only comment\n;SELECT 2"), ["SELECT 1", "SELECT 2"]);
        assert!(texts("   \n-- nothing\n/* here */").is_empty());
    }

    #[test]
    fn delimiter_command_keeps_routine_bodies_whole() {
        let sql = "DELIMITER $$\nCREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END$$\nDELIMITER ;\nSELECT 3;";
        assert_eq!(texts(sql), ["CREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END", "SELECT 3"]);
    }

    #[test]
    fn offsets_point_at_statement_start() {
        let sql = "SELECT 1;  SELECT 2;";
        let s = split_statements(sql);
        assert_eq!(&sql[s[1].start..s[1].start + 8], "SELECT 2");
    }
}
