//! Advisory review of SQL scripts: which statements can destroy data, and which write at all.
//!
//! This is a safety net for people, not a security boundary: a parser can be fooled (dynamic SQL, stored procedures), so read-only
//! mode is enforced by the database session too. Nothing here sends SQL anywhere.

use crate::split::split_statements;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Worth a second look.
    Warning,
    /// Can lose data or lock people out.
    Danger,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskKind {
    DropObject,
    Truncate,
    DeleteWithoutWhere,
    UpdateWithoutWhere,
    DropColumn,
    DropPartition,
    Replace,
    Access,
    ServerSetting,
    Rename,
    Kill,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Finding {
    pub kind: RiskKind,
    pub severity: Severity,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct StatementRisk {
    /// Position of the statement in the script (0-based).
    pub index: usize,
    pub sql: String,
    pub findings: Vec<Finding>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    /// An unquoted word, upper-cased.
    Word(String),
    Num(String),
    Punct(char),
}

/// Words and punctuation with their parenthesis depth; strings, quoted identifiers and comments are skipped.
fn tokens(sql: &str) -> Vec<(Tok, usize)> {
    let b: Vec<char> = sql.chars().collect();
    let (mut i, mut depth) = (0usize, 0usize);
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i];
        match c {
            '\'' | '"' | '`' => {
                i += 1;
                while i < b.len() {
                    if b[i] == '\\' && c != '`' { i += 2; continue; }
                    if b[i] == c {
                        if b.get(i + 1) == Some(&c) { i += 2; continue; }
                        break;
                    }
                    i += 1;
                }
                i += 1;
                // a quoted identifier still counts as "something"; the word itself is irrelevant to every rule
                out.push((Tok::Word("<quoted>".into()), depth));
            }
            '-' if b.get(i + 1) == Some(&'-') && b.get(i + 2).is_none_or(|c| c.is_whitespace()) => { while i < b.len() && b[i] != '\n' { i += 1; } }
            '#' => { while i < b.len() && b[i] != '\n' { i += 1; } }
            '/' if b.get(i + 1) == Some(&'*') => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == '*' && b[i + 1] == '/') { i += 1; }
                i += 2;
            }
            c if c.is_alphabetic() || c == '_' || c == '@' => {
                let start = i;
                while i < b.len() && (b[i].is_alphanumeric() || matches!(b[i], '_' | '$' | '@' | '.')) { i += 1; }
                out.push((Tok::Word(b[start..i].iter().collect::<String>().to_uppercase()), depth));
            }
            c if c.is_ascii_digit() => {
                let start = i;
                while i < b.len() && (b[i].is_ascii_digit() || b[i] == '.') { i += 1; }
                out.push((Tok::Num(b[start..i].iter().collect()), depth));
            }
            '(' => { out.push((Tok::Punct('('), depth)); depth += 1; i += 1; }
            ')' => { depth = depth.saturating_sub(1); out.push((Tok::Punct(')'), depth)); i += 1; }
            c if c.is_whitespace() => i += 1,
            c => { out.push((Tok::Punct(c), depth)); i += 1; }
        }
    }
    out
}

fn word<'a>(t: &'a [(Tok, usize)], i: usize) -> Option<&'a str> {
    match t.get(i) {
        Some((Tok::Word(w), _)) => Some(w.as_str()),
        _ => None,
    }
}

/// True when the statement has a WHERE at the top level that actually narrows the rows.
fn has_restrictive_where(t: &[(Tok, usize)]) -> bool {
    let Some(at) = t.iter().position(|(tok, d)| *d == 0 && matches!(tok, Tok::Word(w) if w == "WHERE")) else { return false };
    let cond: Vec<&Tok> = t[at + 1..]
        .iter()
        .take_while(|(tok, d)| !(*d == 0 && matches!(tok, Tok::Word(w) if matches!(w.as_str(), "ORDER" | "LIMIT" | "RETURNING"))))
        .map(|(tok, _)| tok)
        .collect();
    let trivially_true = match cond.as_slice() {
        [] => true,
        [Tok::Word(w)] => w == "TRUE",
        [Tok::Num(n)] => n != "0",
        [Tok::Num(a), Tok::Punct('='), Tok::Num(b)] => a == b,
        _ => false,
    };
    !trivially_true
}

fn review_statement(text: &str) -> Vec<Finding> {
    let t = tokens(text);
    let mut out = Vec::new();
    let mut add = |kind, severity, reason: &str| out.push(Finding { kind, severity, reason: reason.to_string() });
    match word(&t, 0) {
        Some("DROP") => {
            let obj = match word(&t, 1) { Some("TEMPORARY") => word(&t, 2), o => o };
            if word(&t, 1) != Some("TEMPORARY") {
                match obj {
                    Some("DATABASE" | "SCHEMA") => add(RiskKind::DropObject, Severity::Danger, "DROP DATABASE deletes the database with every table in it"),
                    Some("TABLE") => add(RiskKind::DropObject, Severity::Danger, "DROP TABLE deletes the table and all its rows"),
                    Some("VIEW" | "TRIGGER" | "PROCEDURE" | "FUNCTION" | "EVENT" | "INDEX") => add(RiskKind::DropObject, Severity::Warning, "DROP removes the object"),
                    Some("USER" | "ROLE") => add(RiskKind::Access, Severity::Danger, "DROP USER removes an account and its privileges"),
                    _ => {}
                }
            }
        }
        Some("TRUNCATE") => add(RiskKind::Truncate, Severity::Danger, "TRUNCATE deletes every row and cannot be rolled back"),
        Some("DELETE") if !has_restrictive_where(&t) => add(RiskKind::DeleteWithoutWhere, Severity::Danger, "DELETE without a WHERE condition deletes every row"),
        Some("UPDATE") if !has_restrictive_where(&t) => add(RiskKind::UpdateWithoutWhere, Severity::Danger, "UPDATE without a WHERE condition changes every row"),
        Some("REPLACE") => add(RiskKind::Replace, Severity::Warning, "REPLACE deletes the existing row when the key matches"),
        Some("ALTER") if word(&t, 1) == Some("USER") => add(RiskKind::Access, Severity::Warning, "changes an account"),
        Some("ALTER") => {
            for i in 0..t.len() {
                if word(&t, i) == Some("DROP") && t[i].1 == 0 {
                    match word(&t, i + 1) {
                        Some("PARTITION") => add(RiskKind::DropPartition, Severity::Danger, "DROP PARTITION deletes the rows in it"),
                        Some("INDEX" | "KEY" | "FOREIGN" | "CONSTRAINT" | "CHECK" | "PRIMARY") => {}
                        Some(_) => add(RiskKind::DropColumn, Severity::Danger, "DROP COLUMN deletes the column and its data"),
                        None => {}
                    }
                }
                if word(&t, i) == Some("TRUNCATE") && word(&t, i + 1) == Some("PARTITION") { add(RiskKind::DropPartition, Severity::Danger, "TRUNCATE PARTITION deletes the rows in it"); }
            }
            out.dedup_by(|a, b| a.kind == b.kind);
        }
        Some("GRANT" | "REVOKE") => add(RiskKind::Access, Severity::Warning, "changes who can do what on the server"),
        Some("CREATE") if word(&t, 1) == Some("USER") => add(RiskKind::Access, Severity::Warning, "changes an account"),
        Some("RENAME") => add(RiskKind::Rename, Severity::Warning, "renaming breaks anything that still uses the old name"),
        Some("KILL") => add(RiskKind::Kill, Severity::Warning, "ends another session's work"),
        Some("SET") if matches!(word(&t, 1), Some("GLOBAL" | "PERSIST" | "PERSIST_ONLY")) => add(RiskKind::ServerSetting, Severity::Warning, "changes a server-wide setting"),
        _ => {}
    }
    out
}

/// The statements of `script` that deserve a second look, with the reasons. Statements without findings are left out.
pub fn review(script: &str) -> Vec<StatementRisk> {
    split_statements(script)
        .iter()
        .enumerate()
        .filter_map(|(index, s)| {
            let findings = review_statement(&s.text);
            (!findings.is_empty()).then(|| StatementRisk { index, sql: excerpt(&s.text), findings })
        })
        .collect()
}

fn excerpt(sql: &str) -> String {
    let t = sql.trim();
    if t.chars().count() > 400 { format!("{}…", t.chars().take(400).collect::<String>()) } else { t.to_string() }
}

/// Why a statement may not run on a read-only connection, or None when it may.
/// Covers writes and attempts to switch the read-only session back to read-write.
pub fn read_only_violation(statement: &str) -> Option<&'static str> {
    let t = tokens(statement);
    let w = |i| word(&t, i);
    let any = |name: &str| t.iter().any(|(tok, _)| matches!(tok, Tok::Word(x) if x == name || x.ends_with(&format!(".{name}")) || x.ends_with(&format!("@@{name}"))));
    match w(0) {
        Some("INSERT" | "UPDATE" | "DELETE" | "REPLACE") => Some("it changes data"),
        Some("CREATE" | "ALTER" | "DROP" | "TRUNCATE" | "RENAME") => Some("it changes the schema"),
        Some("GRANT" | "REVOKE") => Some("it changes privileges"),
        Some("LOAD" | "IMPORT") => Some("it loads data"),
        Some("OPTIMIZE" | "REPAIR" | "ANALYZE") => Some("it rewrites table data or statistics"),
        Some("FLUSH" | "RESET" | "PURGE") => Some("it changes server state"),
        Some("SET") if any("TRANSACTION_READ_ONLY") || any("TX_READ_ONLY") || (w(1) == Some("TRANSACTION") || w(2) == Some("TRANSACTION")) && any("WRITE") => Some("it would switch the session to read-write"),
        Some("SET") if matches!(w(1), Some("GLOBAL" | "PERSIST" | "PERSIST_ONLY")) => Some("it changes a server setting"),
        Some("START" | "BEGIN") if any("WRITE") => Some("it would start a read-write transaction"),
        _ => None,
    }
}

/// Statements of a script that a read-only connection must refuse: `(index, text, reason)`.
pub fn read_only_violations(script: &str) -> Vec<(usize, String, &'static str)> {
    split_statements(script)
        .iter()
        .enumerate()
        .filter_map(|(i, s)| read_only_violation(&s.text).map(|r| (i, excerpt(&s.text), r)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(sql: &str) -> Vec<RiskKind> {
        review(sql).into_iter().flat_map(|r| r.findings.into_iter().map(|f| f.kind)).collect()
    }

    #[test]
    fn drops_and_truncates_are_dangerous() {
        assert_eq!(kinds("DROP TABLE users"), [RiskKind::DropObject]);
        assert_eq!(kinds("drop database shop"), [RiskKind::DropObject]);
        assert_eq!(kinds("TRUNCATE TABLE t"), [RiskKind::Truncate]);
        assert_eq!(review("DROP TABLE t")[0].findings[0].severity, Severity::Danger);
        assert_eq!(review("DROP VIEW v")[0].findings[0].severity, Severity::Warning);
        assert!(kinds("DROP TEMPORARY TABLE scratch").is_empty(), "temporary tables are not worth a warning");
    }

    #[test]
    fn delete_and_update_need_a_real_where() {
        assert_eq!(kinds("DELETE FROM t"), [RiskKind::DeleteWithoutWhere]);
        assert_eq!(kinds("UPDATE t SET a = 1"), [RiskKind::UpdateWithoutWhere]);
        assert_eq!(kinds("DELETE FROM t WHERE 1=1"), [RiskKind::DeleteWithoutWhere]);
        assert_eq!(kinds("DELETE FROM t WHERE 1"), [RiskKind::DeleteWithoutWhere]);
        assert_eq!(kinds("UPDATE t SET a = 1 WHERE TRUE"), [RiskKind::UpdateWithoutWhere]);
        assert!(kinds("DELETE FROM t WHERE id = 5").is_empty());
        assert!(kinds("UPDATE t SET a = 1 WHERE id IN (SELECT id FROM u WHERE x = 1)").is_empty());
        assert!(kinds("DELETE FROM t WHERE 1 = 1 AND id = 5").is_empty(), "narrowed by the second condition");
        assert!(kinds("DELETE FROM t WHERE id = 5 LIMIT 1").is_empty());
        // a WHERE inside a subquery of the SET does not count for the outer statement
        assert_eq!(kinds("UPDATE t SET a = (SELECT 1 FROM u WHERE u.id = 2)"), [RiskKind::UpdateWithoutWhere]);
    }

    #[test]
    fn strings_comments_and_quoted_names_are_not_code() {
        assert!(kinds("SELECT 'DROP TABLE x; DELETE FROM y' AS note").is_empty());
        assert!(kinds("SELECT 1 -- DELETE FROM t\n").is_empty());
        assert!(kinds("/* TRUNCATE t */ SELECT 1").is_empty());
        assert!(kinds("INSERT INTO `drop` VALUES ('delete from t')").is_empty());
        assert_eq!(kinds("/* note */ DELETE FROM t"), [RiskKind::DeleteWithoutWhere]);
        assert_eq!(kinds("DELETE FROM t WHERE name = 'x' OR 1 = 1"), Vec::<RiskKind>::new(), "advisory: not every wide condition is detected");
    }

    #[test]
    fn schema_changes_that_lose_data() {
        assert_eq!(kinds("ALTER TABLE t DROP COLUMN note"), [RiskKind::DropColumn]);
        assert_eq!(kinds("ALTER TABLE t DROP note"), [RiskKind::DropColumn]);
        assert_eq!(kinds("ALTER TABLE t DROP PARTITION p1"), [RiskKind::DropPartition]);
        assert!(kinds("ALTER TABLE t DROP INDEX i").is_empty());
        assert!(kinds("ALTER TABLE t DROP FOREIGN KEY fk").is_empty());
        assert!(kinds("ALTER TABLE t ADD COLUMN x INT").is_empty());
    }

    #[test]
    fn access_and_server_changes_are_warnings() {
        assert_eq!(kinds("GRANT ALL ON *.* TO 'a'@'%'"), [RiskKind::Access]);
        assert_eq!(kinds("SET GLOBAL max_connections = 10"), [RiskKind::ServerSetting]);
        assert_eq!(kinds("ALTER USER 'a'@'%' IDENTIFIED BY 'x'"), [RiskKind::Access]);
        assert_eq!(kinds("CREATE USER 'a'@'%'"), [RiskKind::Access]);
        assert_eq!(kinds("DROP USER 'a'@'%'"), [RiskKind::Access]);
        assert_eq!(kinds("RENAME TABLE a TO b"), [RiskKind::Rename]);
        assert_eq!(kinds("KILL 12"), [RiskKind::Kill]);
        assert_eq!(kinds("REPLACE INTO t VALUES (1)"), [RiskKind::Replace]);
        assert!(kinds("SET NAMES utf8mb4").is_empty());
        assert!(kinds("SELECT * FROM t").is_empty());
    }

    #[test]
    fn a_script_reports_each_risky_statement_with_its_position() {
        let r = review("SELECT 1; DELETE FROM a; UPDATE b SET x = 1 WHERE id = 2; TRUNCATE c");
        assert_eq!(r.iter().map(|x| x.index).collect::<Vec<_>>(), [1, 3]);
        assert_eq!(r[0].sql, "DELETE FROM a");
    }

    #[test]
    fn read_only_refuses_writes_and_attempts_to_leave_read_only() {
        for sql in [
            "INSERT INTO t VALUES (1)", "update t set a=1 where id=1", "DELETE FROM t WHERE id=1", "REPLACE INTO t VALUES (1)", "CREATE TABLE x (a int)",
            "ALTER TABLE t ADD c int", "DROP TABLE t", "TRUNCATE t", "RENAME TABLE a TO b", "GRANT SELECT ON *.* TO u", "LOAD DATA INFILE 'x' INTO TABLE t",
            "OPTIMIZE TABLE t", "ANALYZE TABLE t", "FLUSH TABLES", "SET GLOBAL x = 1",
            "SET SESSION TRANSACTION READ WRITE", "SET TRANSACTION READ WRITE", "SET SESSION transaction_read_only = 0", "SET @@session.tx_read_only = OFF",
            "SET @@transaction_read_only = 0", "START TRANSACTION READ WRITE",
        ] {
            assert!(read_only_violation(sql).is_some(), "{sql} must be refused");
        }
        for sql in [
            "SELECT 1", "  /* c */ select * from t", "WITH x AS (SELECT 1) SELECT * FROM x", "SHOW TABLES", "DESCRIBE t", "EXPLAIN SELECT 1", "USE shop", "SET NAMES utf8mb4",
            "SET @a = 1", "START TRANSACTION READ ONLY", "BEGIN", "COMMIT", "ROLLBACK", "SET SESSION TRANSACTION READ ONLY", "CALL report()", "SELECT 'INSERT INTO t' AS s",
        ] {
            assert!(read_only_violation(sql).is_none(), "{sql} is fine on a read-only connection: {:?}", read_only_violation(sql));
        }
    }

    #[test]
    fn read_only_violations_name_the_statement() {
        let v = read_only_violations("SELECT 1; UPDATE t SET a = 1 WHERE id = 1; SELECT 2");
        assert_eq!(v.len(), 1);
        assert_eq!((v[0].0, v[0].1.as_str(), v[0].2), (1, "UPDATE t SET a = 1 WHERE id = 1", "it changes data"));
    }
}
