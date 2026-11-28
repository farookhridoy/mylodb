//! Cutting a PostgreSQL script into statements. Semicolons inside strings (`'…'`, `E'…'`), quoted identifiers, dollar-quoted bodies
//! (`$$ … $$`, `$tag$ … $tag$`) and comments (`--`, nested `/* … */`) do not end a statement.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Statement {
    /// Byte range in the script, from the first character that is not blank or comment to just before the `;`.
    pub start: usize,
    pub end: usize,
    pub text: String,
}

fn ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

/// The `$tag$` that starts at `i`, if one does. `$1` (a parameter) is not one.
fn dollar_tag(b: &[u8], i: usize) -> Option<usize> {
    if i > 0 && (ident_char(b[i - 1]) || b[i - 1] == b'$') {
        return None;
    }
    let mut j = i + 1;
    while j < b.len() && ident_char(b[j]) {
        if j == i + 1 && b[j].is_ascii_digit() {
            return None;
        }
        j += 1;
    }
    (j < b.len() && b[j] == b'$').then_some(j + 1 - i)
}

pub fn split(sql: &str) -> Vec<Statement> {
    let b = sql.as_bytes();
    let n = b.len();
    let mut out = Vec::new();
    let (mut i, mut start, mut end) = (0, None::<usize>, 0);
    let mut push = |start: &mut Option<usize>, end: usize| {
        if let Some(s) = start.take() {
            out.push(Statement { start: s, end, text: sql[s..end].to_string() });
        }
    };
    while i < n {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == b'-' && b.get(i + 1) == Some(&b'-') {
            while i < n && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'*') {
            let mut depth = 0;
            while i < n {
                if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                    depth += 1;
                    i += 2;
                } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if c == b';' {
            push(&mut start, end);
            i += 1;
            continue;
        }
        start.get_or_insert(i);
        if c == b'\'' || c == b'"' {
            // E'…' also escapes with a backslash
            let escapes = c == b'\'' && i > 0 && matches!(b[i - 1], b'e' | b'E') && (i < 2 || !ident_char(b[i - 2]));
            i += 1;
            while i < n {
                if escapes && b[i] == b'\\' {
                    i += 2;
                } else if b[i] == c {
                    if b.get(i + 1) == Some(&c) {
                        i += 2;
                    } else {
                        i += 1;
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else if let Some(len) = (c == b'$').then(|| dollar_tag(b, i)).flatten() {
            let tag = &b[i..i + len];
            i += len;
            while i < n && !b[i..].starts_with(tag) {
                i += 1;
            }
            i += len;
        } else {
            i += 1;
        }
        i = i.min(n);
        while !sql.is_char_boundary(i) {
            i += 1;
        }
        end = i;
    }
    push(&mut start, end);
    out
}

/// The statement the cursor (a byte offset) is in or just after; before the first one, the first.
pub fn statement_at(sql: &str, cursor: usize) -> Option<Statement> {
    let all = split(sql);
    all.iter().rev().find(|s| s.start <= cursor).or_else(|| all.first()).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(sql: &str) -> Vec<String> {
        split(sql).into_iter().map(|s| s.text).collect()
    }

    #[test]
    fn semicolons_end_statements_only_outside_quotes_and_comments() {
        assert_eq!(texts("select 1; select 2;"), ["select 1", "select 2"]);
        assert_eq!(texts("select 'a;b', \"c;d\" from t -- x;y\n; /* a /* nested; */ ; */ select 2"), ["select 'a;b', \"c;d\" from t", "select 2"]);
        assert_eq!(texts("select 'it''s;'; select E'a\\';b'; select 'c\\'; select 4"), ["select 'it''s;'", "select E'a\\';b'", "select 'c\\'", "select 4"]);
        assert_eq!(texts("select 5 # 3; ;; -- only a comment"), ["select 5 # 3"], "# is an operator, and empty statements vanish");
    }

    #[test]
    fn dollar_quoted_bodies_stay_whole() {
        let f = "create function f() returns int language plpgsql as $$ begin return 1; end; $$";
        assert_eq!(texts(&format!("{f}; select 2")), [f, "select 2"]);
        assert_eq!(texts("do $x$ begin perform '$$'; end $x$; select $1; select a$b from t"), ["do $x$ begin perform '$$'; end $x$", "select $1", "select a$b from t"]);
        assert_eq!(texts("select $$ unclosed; "), ["select $$ unclosed; "]);
        assert_eq!(texts("select 'é;'; select \"ü\""), ["select 'é;'", "select \"ü\""]);
    }

    #[test]
    fn the_statement_at_the_cursor() {
        let sql = "select 1;\nselect 2;\n";
        assert_eq!(statement_at(sql, 0).unwrap().text, "select 1");
        assert_eq!(statement_at(sql, sql.len()).unwrap().text, "select 2");
        assert!(statement_at(" -- nothing", 2).is_none());
    }
}
