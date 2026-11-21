/// How an engine spells SQL: quoting and paging. Everything that builds SQL text for an engine goes through its dialect.
pub trait Dialect: Send + Sync {
    /// A quoted identifier (a database, table, column or routine name).
    fn quote_ident(&self, ident: &str) -> String;
    /// A quoted string literal that survives a round trip.
    fn quote_string(&self, s: &str) -> String;
    /// The clause that returns `limit` rows starting at `offset` (`LIMIT 10 OFFSET 20`).
    fn limit_offset(&self, limit: u64, offset: u64) -> String;
}

/// MySQL and MariaDB.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MySql;

impl Dialect for MySql {
    fn quote_ident(&self, ident: &str) -> String {
        format!("`{}`", ident.replace('`', "``"))
    }

    /// Backslash, quote, NUL and Ctrl-Z are escaped (the last two would otherwise travel as raw control characters).
    fn quote_string(&self, s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('\'');
        for c in s.chars() {
            match c {
                '\\' => out.push_str("\\\\"),
                '\'' => out.push_str("''"),
                '\0' => out.push_str("\\0"),
                '\u{1a}' => out.push_str("\\Z"),
                c => out.push(c),
            }
        }
        out.push('\'');
        out
    }

    fn limit_offset(&self, limit: u64, offset: u64) -> String {
        format!("LIMIT {limit} OFFSET {offset}")
    }
}

/// A quoted MySQL identifier.
pub fn q(ident: &str) -> String {
    MySql.quote_ident(ident)
}

/// A quoted MySQL string literal.
pub fn lit(s: &str) -> String {
    MySql.quote_string(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mysql_quoting_is_byte_for_byte_what_the_crates_used_to_write() {
        assert_eq!(q("orders"), "`orders`");
        assert_eq!(q("we`ird"), "`we``ird`");
        assert_eq!(q(""), "``");
        assert_eq!(q("odd name; DROP"), "`odd name; DROP`");
        assert_eq!(lit("plain"), "'plain'");
        assert_eq!(lit("O'Neil"), "'O''Neil'");
        assert_eq!(lit("a\\b"), "'a\\\\b'");
        assert_eq!(lit("日本語 ✓"), "'日本語 ✓'");
    }

    #[test]
    fn control_characters_in_a_literal_are_escaped() {
        assert_eq!(lit("a\0b"), "'a\\0b'");
        assert_eq!(lit("a\u{1a}b"), "'a\\Zb'");
    }

    #[test]
    fn paging_clause() {
        assert_eq!(MySql.limit_offset(501, 0), "LIMIT 501 OFFSET 0");
        assert_eq!(MySql.limit_offset(10, 40), "LIMIT 10 OFFSET 40");
    }
}
