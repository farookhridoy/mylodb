//! What `information_schema.COLUMNS.COLUMN_DEFAULT` means. MySQL and MariaDB spell it differently, so this is the one place that reads it:
//! everything else (the designer, the overview, the data grid) takes the parsed value.
//!
//! * MySQL: `NULL` is "no default"; a literal is its raw text; an expression is flagged by `DEFAULT_GENERATED` in `EXTRA`.
//! * MariaDB: `NULL` is the four letters `NULL` for a nullable column without a default (or `DEFAULT NULL`); string literals come quoted
//!   (`'it''s'`); numbers are bare; expressions are bare text such as `current_timestamp()`.

use mysql_async::prelude::Queryable;
use mysql_async::Conn;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedDefault {
    /// No DEFAULT clause.
    None,
    /// `DEFAULT NULL`.
    Null,
    Literal(String),
    Expression(String),
}

impl ParsedDefault {
    /// Text for display: the literal or the expression; nothing for "none" and NULL.
    pub fn display(&self) -> Option<String> {
        match self {
            ParsedDefault::Literal(v) | ParsedDefault::Expression(v) => Some(v.clone()),
            ParsedDefault::None | ParsedDefault::Null => None,
        }
    }
}

/// True when the server is MariaDB (its `VERSION()` says so).
pub async fn is_mariadb(conn: &mut Conn) -> Result<bool, mysql_async::Error> {
    let v: Option<String> = conn.query_first("SELECT VERSION()").await?;
    Ok(v.unwrap_or_default().to_lowercase().contains("mariadb"))
}

fn unquote(v: &str) -> String {
    v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')).map_or_else(|| v.to_string(), |s| s.replace("''", "'").replace("\\'", "'"))
}

/// Parse one `COLUMN_DEFAULT` value. `extra` is the column's `EXTRA` text.
pub fn parse_column_default(mariadb: bool, raw: Option<&str>, extra: &str) -> ParsedDefault {
    let Some(v) = raw else { return ParsedDefault::None };
    if extra.to_lowercase().contains("default_generated") {
        return ParsedDefault::Expression(v.to_string());
    }
    if !mariadb {
        return ParsedDefault::Literal(v.to_string());
    }
    if v.eq_ignore_ascii_case("NULL") {
        ParsedDefault::Null
    } else if v.starts_with('\'') {
        ParsedDefault::Literal(unquote(v))
    } else if v.contains('(') {
        ParsedDefault::Expression(v.to_string())
    } else {
        ParsedDefault::Literal(v.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mysql_defaults() {
        assert_eq!(parse_column_default(false, None, ""), ParsedDefault::None);
        assert_eq!(parse_column_default(false, Some("NULL"), ""), ParsedDefault::Literal("NULL".into()), "a MySQL text default 'NULL' is a literal");
        assert_eq!(parse_column_default(false, Some("it's"), ""), ParsedDefault::Literal("it's".into()));
        assert_eq!(parse_column_default(false, Some("CURRENT_TIMESTAMP"), "DEFAULT_GENERATED"), ParsedDefault::Expression("CURRENT_TIMESTAMP".into()));
    }

    #[test]
    fn mariadb_defaults() {
        assert_eq!(parse_column_default(true, None, ""), ParsedDefault::None);
        assert_eq!(parse_column_default(true, Some("NULL"), ""), ParsedDefault::Null);
        assert_eq!(parse_column_default(true, Some("'it''s'"), ""), ParsedDefault::Literal("it's".into()));
        assert_eq!(parse_column_default(true, Some("0.00"), ""), ParsedDefault::Literal("0.00".into()));
        assert_eq!(parse_column_default(true, Some("current_timestamp(3)"), ""), ParsedDefault::Expression("current_timestamp(3)".into()));
        assert_eq!(ParsedDefault::Null.display(), None);
    }
}
