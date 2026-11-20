//! Splitting a script into statements.

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

/// The statement the cursor is in or directly after (so `SELECT 1;|` still means `SELECT 1`).
/// `cursor` is a byte offset. Returns `None` when the cursor is not near any statement.
pub fn statement_at(sql: &str, cursor: usize) -> Option<Statement> {
    let cursor = cursor.min(sql.len());
    let all = split_statements(sql);
    let end = |s: &Statement| s.start + s.text.len();
    // inside (or at the end of) a statement
    if let Some(s) = all.iter().find(|s| cursor >= s.start && cursor <= end(s)) {
        return Some(s.clone());
    }
    // between statements: the nearest one before the cursor, otherwise the first after it
    all.iter().rev().find(|s| end(s) < cursor).or_else(|| all.first()).cloned()
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statement_at_cursor() {
        let sql = "SELECT 1;\nSELECT 2 FROM t;\n\nSELECT 3";
        let at = |c: usize| statement_at(sql, c).map(|s| s.text);
        assert_eq!(at(3).as_deref(), Some("SELECT 1"));
        assert_eq!(at(8).as_deref(), Some("SELECT 1"), "just before the semicolon");
        assert_eq!(at(9).as_deref(), Some("SELECT 1"), "right after the semicolon");
        assert_eq!(at(10).as_deref(), Some("SELECT 2 FROM t"));
        assert_eq!(at(26).as_deref(), Some("SELECT 2 FROM t"), "blank line after a statement");
        assert_eq!(at(sql.len()).as_deref(), Some("SELECT 3"));
        assert_eq!(statement_at("  ", 1), None);
    }


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
