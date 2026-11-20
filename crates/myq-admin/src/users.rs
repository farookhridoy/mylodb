//! Accounts and privileges: read `SHOW GRANTS`, edit as a model, and generate the GRANT/REVOKE diff.

use crate::Result;
use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::Conn;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

fn lit(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "''"))
}

fn q(s: &str) -> String {
    format!("`{}`", s.replace('`', "``"))
}

fn account(user: &str, host: &str) -> String {
    format!("{}@{}", lit(user), lit(host))
}

/// A statement to execute, and a version that is safe to show (passwords masked).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Sql {
    pub run: String,
    pub shown: String,
}

impl Sql {
    fn plain(s: String) -> Sql {
        Sql { shown: s.clone(), run: s }
    }
}

// ---------- accounts ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub user: String,
    pub host: String,
    pub locked: bool,
    pub password_expired: bool,
    pub plugin: String,
}

pub async fn list_accounts(conn: &mut Conn) -> Result<Vec<Account>> {
    // MySQL 5.7+/8 expose lock and expiry columns; older servers and MariaDB may not.
    let rich: std::result::Result<Vec<(String, String, String, String, String)>, _> = conn
        .query("SELECT User, Host, plugin, account_locked, password_expired FROM mysql.user ORDER BY User, Host")
        .await;
    match rich {
        Ok(rows) => Ok(rows
            .into_iter()
            .map(|(user, host, plugin, locked, expired)| Account { user, host, plugin, locked: locked == "Y", password_expired: expired == "Y" })
            .collect()),
        Err(_) => {
            let rows: Vec<(String, String, String)> = conn.query("SELECT User, Host, plugin FROM mysql.user ORDER BY User, Host").await?;
            Ok(rows.into_iter().map(|(user, host, plugin)| Account { user, host, plugin, locked: false, password_expired: false }).collect())
        }
    }
}

/// What the user edits in the account form.
#[derive(Debug, Clone, Deserialize)]
pub struct AccountEdit {
    pub user: String,
    pub host: String,
    /// `None` keeps the current password (or creates a passwordless account).
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub plugin: Option<String>,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub expire_password: bool,
}

/// Statements for creating (`original` is `None`) or changing an account.
pub fn account_sql(original: Option<&Account>, e: &AccountEdit) -> std::result::Result<Vec<Sql>, String> {
    if e.user.is_empty() && original.is_none() {
        // The anonymous user ('') is legal in MySQL but almost always a mistake in a GUI form.
        return Err("user name is empty".into());
    }
    if e.host.trim().is_empty() {
        return Err("host is empty".into());
    }
    let mut out = Vec::new();
    let identified = |pw: &str, plugin: &Option<String>, mask: bool| {
        let secret = if mask { "'******'".to_string() } else { lit(pw) };
        match plugin.as_deref().filter(|p| !p.is_empty()) {
            Some(p) => format!("IDENTIFIED WITH {p} BY {secret}"),
            None => format!("IDENTIFIED BY {secret}"),
        }
    };
    match original {
        None => {
            let who = account(&e.user, &e.host);
            let (run, shown) = match e.password.as_deref().filter(|p| !p.is_empty()) {
                Some(pw) => (
                    format!("CREATE USER {who} {}", identified(pw, &e.plugin, false)),
                    format!("CREATE USER {who} {}", identified(pw, &e.plugin, true)),
                ),
                None => (format!("CREATE USER {who}"), format!("CREATE USER {who}")),
            };
            out.push(Sql { run, shown });
            if e.locked {
                out.push(Sql::plain(format!("ALTER USER {who} ACCOUNT LOCK")));
            }
            if e.expire_password {
                out.push(Sql::plain(format!("ALTER USER {who} PASSWORD EXPIRE")));
            }
        }
        Some(o) => {
            let mut who = account(&o.user, &o.host);
            if (o.user.as_str(), o.host.as_str()) != (e.user.as_str(), e.host.as_str()) {
                let to = account(&e.user, &e.host);
                out.push(Sql::plain(format!("RENAME USER {who} TO {to}")));
                who = to;
            }
            if let Some(pw) = e.password.as_deref().filter(|p| !p.is_empty()) {
                out.push(Sql {
                    run: format!("ALTER USER {who} {}", identified(pw, &e.plugin, false)),
                    shown: format!("ALTER USER {who} {}", identified(pw, &e.plugin, true)),
                });
            }
            if o.locked != e.locked {
                out.push(Sql::plain(format!("ALTER USER {who} ACCOUNT {}", if e.locked { "LOCK" } else { "UNLOCK" })));
            }
            if e.expire_password {
                out.push(Sql::plain(format!("ALTER USER {who} PASSWORD EXPIRE")));
            }
        }
    }
    Ok(out)
}

pub fn drop_account_sql(user: &str, host: &str) -> Sql {
    Sql::plain(format!("DROP USER {}", account(user, host)))
}

/// Run statements in order (stops at the first error, which is returned with its statement).
pub async fn run_sql(conn: &mut Conn, statements: &[Sql]) -> std::result::Result<usize, (usize, String)> {
    conn.query_drop("SET SESSION sql_mode = REPLACE(@@SESSION.sql_mode, 'NO_BACKSLASH_ESCAPES', '')").await.map_err(|e| (0, e.to_string()))?;
    for (i, s) in statements.iter().enumerate() {
        conn.query_drop(&s.run).await.map_err(|e| (i, format!("{}: {e}", s.shown)))?;
    }
    Ok(statements.len())
}

// ---------- privileges ----------

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "level", rename_all = "snake_case")]
pub enum Scope {
    Global,
    Database { db: String },
    Table { db: String, table: String },
}

impl Scope {
    fn sql(&self) -> String {
        match self {
            Scope::Global => "*.*".into(),
            // `_` and `%` are wildcards in database-level grants unless escaped.
            Scope::Database { db } => format!("{}.*", q(&db.replace('_', "\\_").replace('%', "\\%"))),
            Scope::Table { db, table } => format!("{}.{}", q(db), q(table)),
        }
    }
}

/// Privileges held on one object. `privileges` apply to the whole object, `columns` map a privilege
/// (SELECT/INSERT/UPDATE/REFERENCES) to the columns it is limited to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeGrant {
    #[serde(flatten)]
    pub scope: Option<Scope>,
    #[serde(default)]
    pub privileges: BTreeSet<String>,
    #[serde(default)]
    pub columns: BTreeMap<String, BTreeSet<String>>,
    #[serde(default)]
    pub grant_option: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Grants {
    pub scopes: Vec<ScopeGrant>,
    /// Grant lines the editor cannot represent (routines, roles, proxies); shown read-only and never changed.
    pub unsupported: Vec<String>,
}

/// Split `s` on `sep` outside backticks, quotes and parentheses.
fn split_top(s: &str, sep: char) -> Vec<String> {
    let (mut out, mut cur, mut depth, mut quote) = (Vec::new(), String::new(), 0i32, None::<char>);
    for c in s.chars() {
        match quote {
            Some(qc) => { if c == qc { quote = None; } cur.push(c); }
            None => match c {
                '`' | '\'' | '"' => { quote = Some(c); cur.push(c); }
                '(' => { depth += 1; cur.push(c); }
                ')' => { depth -= 1; cur.push(c); }
                c if c == sep && depth == 0 => out.push(std::mem::take(&mut cur)),
                c => cur.push(c),
            },
        }
    }
    out.push(cur);
    out.into_iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()
}

/// Position of the keyword (surrounded by spaces) outside quotes and parentheses.
fn find_kw(s: &str, kw: &str) -> Option<usize> {
    let upper = s.to_uppercase();
    let needle = format!(" {kw} ");
    let (mut depth, mut quote) = (0i32, None::<char>);
    for (i, c) in s.char_indices() {
        match quote {
            Some(qc) => { if c == qc { quote = None; } }
            None => match c {
                '`' | '\'' | '"' => quote = Some(c),
                '(' => depth += 1,
                ')' => depth -= 1,
                _ if depth == 0 && upper[i..].starts_with(&needle) => return Some(i),
                _ => {}
            },
        }
    }
    None
}

/// Backtick-quoted identifiers in `s`, in order (`` `a`, `b``c` `` -> ["a", "b`c"]).
fn idents(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '`' {
            let mut id = String::new();
            while let Some(n) = chars.next() {
                if n == '`' {
                    if chars.peek() == Some(&'`') { chars.next(); id.push('`'); } else { break; }
                } else {
                    id.push(n);
                }
            }
            out.push(id);
        }
    }
    out
}

/// Parse the lines of `SHOW GRANTS` into scope grants.
pub fn parse_grants(lines: &[String]) -> Grants {
    let mut by_scope: BTreeMap<Scope, ScopeGrant> = BTreeMap::new();
    let mut unsupported = Vec::new();
    for line in lines {
        let Some(body) = line.strip_prefix("GRANT ") else { unsupported.push(line.clone()); continue };
        let (Some(on), Some(to)) = (find_kw(body, "ON"), find_kw(body, "TO")) else { unsupported.push(line.clone()); continue };
        let privs = &body[..on];
        let obj = body[on + 4..to].trim();
        let tail = body[to..].to_uppercase();
        let scope = if obj == "*.*" {
            Scope::Global
        } else if obj.to_uppercase().starts_with("FUNCTION ") || obj.to_uppercase().starts_with("PROCEDURE ") {
            unsupported.push(line.clone());
            continue;
        } else {
            let obj = obj.strip_prefix("TABLE ").unwrap_or(obj);
            let ids = idents(obj);
            match (ids.as_slice(), obj.ends_with(".*")) {
                ([db], true) => Scope::Database { db: db.replace("\\_", "_").replace("\\%", "%") },
                ([db, t], false) => Scope::Table { db: db.clone(), table: t.clone() },
                _ => { unsupported.push(line.clone()); continue }
            }
        };
        let entry = by_scope.entry(scope.clone()).or_insert_with(|| ScopeGrant { scope: Some(scope), ..Default::default() });
        if tail.contains("WITH GRANT OPTION") {
            entry.grant_option = true;
        }
        for item in split_top(privs, ',') {
            let (name, cols) = match item.find('(') {
                Some(i) => (item[..i].trim().to_uppercase(), Some(idents(&item[i..]))),
                None => (item.to_uppercase(), None),
            };
            if name == "USAGE" {
                continue;
            }
            match cols {
                Some(c) => { entry.columns.entry(name).or_default().extend(c); }
                None => { entry.privileges.insert(name); }
            }
        }
    }
    // `GRANT USAGE` means "no privileges": a scope holding nothing is not a grant.
    let scopes = by_scope.into_values().filter(|g| !g.privileges.is_empty() || !g.columns.is_empty() || g.grant_option).collect();
    Grants { scopes, unsupported }
}

pub async fn read_grants(conn: &mut Conn, user: &str, host: &str) -> Result<Grants> {
    let lines: Vec<String> = conn.query(format!("SHOW GRANTS FOR {}", account(user, host))).await?;
    Ok(parse_grants(&lines))
}

const ALL: &str = "ALL PRIVILEGES";

/// GRANT/REVOKE statements that change `current` into `desired` for one account. REVOKEs come first.
pub fn grant_diff_sql(user: &str, host: &str, current: &[ScopeGrant], desired: &[ScopeGrant]) -> Vec<Sql> {
    let who = account(user, host);
    let empty = |s: &Scope| ScopeGrant { scope: Some(s.clone()), ..Default::default() };
    let scopes: BTreeSet<Scope> = current.iter().chain(desired).filter_map(|g| g.scope.clone()).collect();
    let (mut revokes, mut grants) = (Vec::new(), Vec::new());

    let col_list = |m: &BTreeMap<String, BTreeSet<String>>| -> Vec<String> {
        m.iter().filter(|(_, c)| !c.is_empty()).map(|(p, c)| format!("{p} ({})", c.iter().map(|x| q(x)).collect::<Vec<_>>().join(", "))).collect()
    };

    for scope in scopes {
        let cur = current.iter().find(|g| g.scope.as_ref() == Some(&scope)).cloned().unwrap_or_else(|| empty(&scope));
        let des = desired.iter().find(|g| g.scope.as_ref() == Some(&scope)).cloned().unwrap_or_else(|| empty(&scope));
        let on = scope.sql();

        // whole-object privileges
        let cur_all = cur.privileges.contains(ALL);
        let des_all = des.privileges.contains(ALL);
        let mut effective_current = cur.privileges.clone();
        if cur_all && !des_all {
            revokes.push(format!("REVOKE ALL PRIVILEGES ON {on} FROM {who}"));
            effective_current.clear();
        } else if !cur_all {
            let drop: Vec<&String> = cur.privileges.difference(&des.privileges).collect();
            if !drop.is_empty() && !des_all {
                revokes.push(format!("REVOKE {} ON {on} FROM {who}", drop.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
            }
        }
        if des_all && !cur_all {
            grants.push(format!("GRANT ALL PRIVILEGES ON {on} TO {who}"));
        } else if !des_all {
            let add: Vec<&String> = des.privileges.difference(&effective_current).collect();
            if !add.is_empty() {
                grants.push(format!("GRANT {} ON {on} TO {who}", add.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
            }
        }

        // column privileges
        let mut revoke_cols: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut grant_cols: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for p in cur.columns.keys().chain(des.columns.keys()).collect::<BTreeSet<_>>() {
            let (c, d) = (cur.columns.get(p).cloned().unwrap_or_default(), des.columns.get(p).cloned().unwrap_or_default());
            let r: BTreeSet<String> = c.difference(&d).cloned().collect();
            let g: BTreeSet<String> = d.difference(&c).cloned().collect();
            if !r.is_empty() { revoke_cols.insert(p.clone(), r); }
            if !g.is_empty() { grant_cols.insert(p.clone(), g); }
        }
        if !revoke_cols.is_empty() {
            revokes.push(format!("REVOKE {} ON {on} FROM {who}", col_list(&revoke_cols).join(", ")));
        }
        if !grant_cols.is_empty() {
            grants.push(format!("GRANT {} ON {on} TO {who}", col_list(&grant_cols).join(", ")));
        }

        // grant option
        if cur.grant_option && !des.grant_option {
            revokes.push(format!("REVOKE GRANT OPTION ON {on} FROM {who}"));
        }
        if des.grant_option && !cur.grant_option {
            grants.push(format!("GRANT USAGE ON {on} TO {who} WITH GRANT OPTION"));
        }
    }
    revokes.into_iter().chain(grants).map(Sql::plain).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }
    fn set(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }
    fn sg(scope: Scope, privs: &[&str]) -> ScopeGrant {
        ScopeGrant { scope: Some(scope), privileges: set(privs), ..Default::default() }
    }
    fn db(n: &str) -> Scope {
        Scope::Database { db: n.into() }
    }
    fn shown(v: Vec<Sql>) -> Vec<String> {
        v.into_iter().map(|s| s.shown).collect()
    }

    #[test]
    fn parses_every_grant_shape() {
        let g = parse_grants(&lines(&[
            "GRANT USAGE ON *.* TO `u`@`localhost`",
            "GRANT SELECT, RELOAD, APPLICATION_PASSWORD_ADMIN ON *.* TO `u`@`localhost` WITH GRANT OPTION",
            "GRANT ALL PRIVILEGES ON `my\\_app`.* TO `u`@`localhost`",
            "GRANT SELECT, INSERT (`a`, `b,c`), UPDATE (`a`) ON `shop`.`orders` TO `u`@`localhost`",
            "GRANT EXECUTE ON PROCEDURE `shop`.`p` TO `u`@`localhost`",
            "GRANT `role1`@`%` TO `u`@`localhost`",
        ]));
        assert_eq!(g.unsupported.len(), 2);
        let get = |s: Scope| g.scopes.iter().find(|x| x.scope.as_ref() == Some(&s)).unwrap().clone();
        let global = get(Scope::Global);
        assert_eq!(global.privileges, set(&["SELECT", "RELOAD", "APPLICATION_PASSWORD_ADMIN"]));
        assert!(global.grant_option);
        assert_eq!(get(db("my_app")).privileges, set(&["ALL PRIVILEGES"]), "escaped wildcards are unescaped");
        let t = get(Scope::Table { db: "shop".into(), table: "orders".into() });
        assert_eq!(t.privileges, set(&["SELECT"]));
        assert_eq!(t.columns["INSERT"], set(&["a", "b,c"]));
        assert_eq!(t.columns["UPDATE"], set(&["a"]));
    }

    #[test]
    fn usage_only_means_no_privileges() {
        let g = parse_grants(&lines(&["GRANT USAGE ON *.* TO `u`@`h`"]));
        assert!(g.scopes.is_empty() && g.unsupported.is_empty());
        let g = parse_grants(&lines(&["GRANT USAGE ON `d`.* TO `u`@`h` WITH GRANT OPTION"]));
        assert_eq!(g.scopes.len(), 1, "grant option alone is still a grant");
    }

    #[test]
    fn no_change_means_no_statements() {
        let g = parse_grants(&lines(&["GRANT SELECT ON `d`.* TO `u`@`h`", "GRANT SELECT (`a`) ON `d`.`t` TO `u`@`h`"]));
        assert!(grant_diff_sql("u", "h", &g.scopes, &g.scopes).is_empty());
    }

    #[test]
    fn diff_revokes_first_then_grants() {
        let cur = vec![sg(db("d"), &["SELECT", "INSERT", "DELETE"])];
        let mut des = vec![sg(db("d"), &["SELECT", "UPDATE"]), sg(Scope::Table { db: "d".into(), table: "t".into() }, &["SELECT"])];
        des[0].grant_option = true;
        assert_eq!(shown(grant_diff_sql("u", "h", &cur, &des)), vec![
            "REVOKE DELETE, INSERT ON `d`.* FROM 'u'@'h'",
            "GRANT UPDATE ON `d`.* TO 'u'@'h'",
            "GRANT USAGE ON `d`.* TO 'u'@'h' WITH GRANT OPTION",
            "GRANT SELECT ON `d`.`t` TO 'u'@'h'",
        ]);
    }

    #[test]
    fn all_privileges_transitions_and_wildcard_escaping() {
        let all = vec![sg(db("my_db"), &["ALL PRIVILEGES"])];
        let some = vec![sg(db("my_db"), &["SELECT"])];
        assert_eq!(shown(grant_diff_sql("u", "h", &all, &some)), vec!["REVOKE ALL PRIVILEGES ON `my\\_db`.* FROM 'u'@'h'", "GRANT SELECT ON `my\\_db`.* TO 'u'@'h'"]);
        assert_eq!(shown(grant_diff_sql("u", "h", &some, &all)), vec!["GRANT ALL PRIVILEGES ON `my\\_db`.* TO 'u'@'h'"]);
        assert_eq!(shown(grant_diff_sql("u", "h", &all, &[])), vec!["REVOKE ALL PRIVILEGES ON `my\\_db`.* FROM 'u'@'h'"]);
    }

    #[test]
    fn column_privileges_diff_per_column() {
        let t = || Scope::Table { db: "d".into(), table: "t".into() };
        let mut cur = sg(t(), &[]);
        cur.columns.insert("SELECT".into(), set(&["a", "b"]));
        let mut des = sg(t(), &[]);
        des.columns.insert("SELECT".into(), set(&["b", "c"]));
        des.columns.insert("UPDATE".into(), set(&["c"]));
        assert_eq!(shown(grant_diff_sql("u", "h", &[cur], &[des])), vec![
            "REVOKE SELECT (`a`) ON `d`.`t` FROM 'u'@'h'",
            "GRANT SELECT (`c`), UPDATE (`c`) ON `d`.`t` TO 'u'@'h'",
        ]);
    }

    #[test]
    fn account_statements_mask_passwords_and_escape_literals() {
        let e = AccountEdit { user: "o'neil".into(), host: "%".into(), password: Some("p'w\\d".into()), plugin: None, locked: true, expire_password: false };
        let s = account_sql(None, &e).unwrap();
        assert_eq!(s[0].run, "CREATE USER 'o''neil'@'%' IDENTIFIED BY 'p''w\\\\d'");
        assert_eq!(s[0].shown, "CREATE USER 'o''neil'@'%' IDENTIFIED BY '******'");
        assert_eq!(s[1].shown, "ALTER USER 'o''neil'@'%' ACCOUNT LOCK");

        let orig = Account { user: "a".into(), host: "h".into(), locked: true, password_expired: false, plugin: "x".into() };
        let edit = AccountEdit { user: "b".into(), host: "h2".into(), password: None, plugin: None, locked: false, expire_password: false };
        assert_eq!(shown(account_sql(Some(&orig), &edit).unwrap()), vec!["RENAME USER 'a'@'h' TO 'b'@'h2'", "ALTER USER 'b'@'h2' ACCOUNT UNLOCK"]);
        assert!(account_sql(None, &AccountEdit { user: "".into(), ..edit.clone() }).is_err());
        assert!(account_sql(None, &AccountEdit { host: " ".into(), ..edit }).is_err());
        assert_eq!(drop_account_sql("a", "h").shown, "DROP USER 'a'@'h'");
    }
}
