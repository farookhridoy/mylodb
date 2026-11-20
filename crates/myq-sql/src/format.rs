//! SQL formatter: indentation, line breaks and keyword case.

use serde::{Deserialize, Serialize};
use sqlformat::{Indent, QueryParams};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum KeywordCase {
    #[default]
    Upper,
    Lower,
    /// Leave keywords as typed.
    Preserve,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "style", content = "width", rename_all = "lowercase")]
pub enum IndentStyle {
    Spaces(u8),
    Tab,
}

impl Default for IndentStyle {
    fn default() -> Self {
        IndentStyle::Spaces(2)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatOptions {
    #[serde(default)]
    pub indent: IndentStyle,
    #[serde(default)]
    pub keyword_case: KeywordCase,
    /// Blank lines between statements.
    #[serde(default = "one")]
    pub lines_between_statements: u8,
}

fn one() -> u8 {
    1
}

impl Default for FormatOptions {
    fn default() -> Self {
        FormatOptions { indent: IndentStyle::default(), keyword_case: KeywordCase::default(), lines_between_statements: one() }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum FormatError {
    /// `DELIMITER` is a client command, not SQL; formatting around it could corrupt routine bodies.
    Delimiter,
    Empty,
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::Delimiter => write!(f, "this script uses DELIMITER; select a single statement to format"),
            FormatError::Empty => write!(f, "nothing to format"),
        }
    }
}

fn uses_delimiter(sql: &str) -> bool {
    sql.lines().any(|l| {
        let t = l.trim_start();
        t.len() > 9 && t[..9].eq_ignore_ascii_case("delimiter") && t.as_bytes()[9].is_ascii_whitespace()
    })
}

pub fn format_sql(sql: &str, opts: &FormatOptions) -> Result<String, FormatError> {
    if sql.trim().is_empty() {
        return Err(FormatError::Empty);
    }
    if uses_delimiter(sql) {
        return Err(FormatError::Delimiter);
    }
    let o = sqlformat::FormatOptions {
        indent: match opts.indent {
            IndentStyle::Spaces(n) => Indent::Spaces(n.clamp(1, 8)),
            IndentStyle::Tab => Indent::Tabs,
        },
        uppercase: match opts.keyword_case {
            KeywordCase::Upper => Some(true),
            KeywordCase::Lower => Some(false),
            KeywordCase::Preserve => None,
        },
        lines_between_queries: opts.lines_between_statements.min(4) + 1,
        ..Default::default()
    };
    Ok(sqlformat::format(sql, &QueryParams::None, &o).trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(sql: &str, o: FormatOptions) -> String {
        format_sql(sql, &o).unwrap()
    }

    #[test]
    fn breaks_clauses_onto_lines_and_uppercases_keywords() {
        let out = f("select a,b from t where x=1 and y='it''s' order by a limit 5", FormatOptions::default());
        assert_eq!(out, "SELECT\n  a,\n  b\nFROM\n  t\nWHERE\n  x = 1\n  AND y = 'it''s'\nORDER BY\n  a\nLIMIT\n  5");
    }

    #[test]
    fn keyword_case_options() {
        let q = "SELECT Name FROM Users WHERE id = 1";
        assert!(f(q, FormatOptions { keyword_case: KeywordCase::Lower, ..Default::default() }).contains("select\n  Name\nfrom\n  Users\nwhere"));
        assert!(f("select Name from Users", FormatOptions { keyword_case: KeywordCase::Preserve, ..Default::default() }).starts_with("select\n  Name\nfrom\n  Users"));
        // identifiers are never re-cased
        assert!(f("select Name from Users", FormatOptions::default()).contains("Name"));
    }

    #[test]
    fn indent_width_and_tabs() {
        let four = f("select a from t", FormatOptions { indent: IndentStyle::Spaces(4), ..Default::default() });
        assert!(four.contains("\n    a\n"), "{four:?}");
        let tab = f("select a from t", FormatOptions { indent: IndentStyle::Tab, ..Default::default() });
        assert!(tab.contains("\n\ta\n"), "{tab:?}");
    }

    #[test]
    fn strings_backticks_and_comments_survive() {
        let out = f("select `my col`, 'a;b' /* keep me */ from `t` -- trailing\nwhere x = \"q\"", FormatOptions::default());
        for needle in ["`my col`", "'a;b'", "/* keep me */", "-- trailing", "`t`", "\"q\""] {
            assert!(out.contains(needle), "{needle} lost in:\n{out}");
        }
    }

    #[test]
    fn formatting_is_idempotent_and_keeps_statements_apart() {
        let once = f("select 1;select 2 from t", FormatOptions::default());
        assert_eq!(once.matches(';').count(), 1);
        assert!(once.contains("SELECT\n  1;\n\nSELECT\n  2"), "{once}");
        assert_eq!(f(&once, FormatOptions::default()), once);
    }

    #[test]
    fn joins_and_subqueries() {
        let out = f("select * from a left join b on a.id=b.a_id where a.x in (select x from c)", FormatOptions::default());
        assert!(out.contains("LEFT JOIN"), "{out}");
        assert!(out.contains("ON a.id = b.a_id"), "{out}");
        assert!(out.contains("IN ("), "{out}");
    }

    #[test]
    fn refuses_scripts_with_delimiter_and_empty_input() {
        assert_eq!(format_sql("DELIMITER $$\nselect 1$$", &FormatOptions::default()), Err(FormatError::Delimiter));
        assert_eq!(format_sql("  \n", &FormatOptions::default()), Err(FormatError::Empty));
        assert!(format_sql("select delimiter_x from t", &FormatOptions::default()).is_ok(), "only a DELIMITER command counts");
    }
}
