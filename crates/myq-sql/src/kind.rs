//! Classifying a statement by its first keyword.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementKind {
    /// SELECT / WITH / TABLE / VALUES: reads only.
    Read,
    /// INSERT / UPDATE / DELETE / REPLACE.
    Write,
    Other,
}

/// Classify by the first keyword, after comments and opening parentheses.
pub fn statement_kind(sql: &str) -> StatementKind {
    let mut s = sql.trim_start();
    loop {
        if let Some(r) = s.strip_prefix("/*") {
            match r.find("*/") {
                Some(p) => s = r[p + 2..].trim_start(),
                None => return StatementKind::Other,
            }
        } else if s.starts_with("--") || s.starts_with('#') {
            match s.find('\n') {
                Some(p) => s = s[p + 1..].trim_start(),
                None => return StatementKind::Other,
            }
        } else if let Some(r) = s.strip_prefix('(') {
            s = r.trim_start();
        } else {
            break;
        }
    }
    let word: String = s.chars().take_while(|c| c.is_ascii_alphabetic()).collect::<String>().to_uppercase();
    match word.as_str() {
        "SELECT" | "WITH" | "TABLE" | "VALUES" => StatementKind::Read,
        "INSERT" | "UPDATE" | "DELETE" | "REPLACE" => StatementKind::Write,
        _ => StatementKind::Other,
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_statements() {
        for (q, k) in [
            ("select 1", StatementKind::Read), ("  /* c */ WITH x AS (SELECT 1) SELECT * FROM x", StatementKind::Read), ("(select 1) union (select 2)", StatementKind::Read),
            ("-- note\nSELECT 1", StatementKind::Read), ("TABLE t", StatementKind::Read), ("update t set a=1", StatementKind::Write), ("DELETE FROM t", StatementKind::Write),
            ("insert into t values (1)", StatementKind::Write), ("DROP TABLE t", StatementKind::Other), ("SHOW TABLES", StatementKind::Other), ("/* open", StatementKind::Other),
        ] {
            assert_eq!(statement_kind(q), k, "{q}");
        }
    }

}
