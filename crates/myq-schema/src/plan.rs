//! SQL plans for bulk and destructive operations: the statements a "Preview SQL, then run" dialog shows and then runs.
//! Pure functions: nothing here talks to a server. Every identifier is quoted, every refusal says why.

use crate::ddl::q;

pub const SYSTEM_DATABASES: [&str; 4] = ["information_schema", "performance_schema", "mysql", "sys"];

pub type PlanResult = Result<Vec<String>, String>;

fn qualified(db: &str, table: &str) -> String {
    format!("{}.{}", q(db), q(table))
}

fn nonempty(names: &[String], what: &str) -> Result<(), String> {
    if names.is_empty() {
        return Err(format!("choose at least one {what}"));
    }
    if names.iter().any(|n| n.trim().is_empty()) {
        return Err(format!("a {what} name is empty"));
    }
    let mut seen = std::collections::HashSet::new();
    if let Some(d) = names.iter().find(|n| !seen.insert(n.to_lowercase())) {
        return Err(format!("{what} `{d}` is listed twice"));
    }
    Ok(())
}

/// `DROP DATABASE` for each. The system databases are refused.
pub fn drop_databases(names: &[String]) -> PlanResult {
    nonempty(names, "database")?;
    if let Some(s) = names.iter().find(|n| SYSTEM_DATABASES.contains(&n.to_lowercase().as_str())) {
        return Err(format!("`{s}` is a system database and cannot be dropped"));
    }
    Ok(names.iter().map(|n| format!("DROP DATABASE {};", q(n))).collect())
}

/// `DROP TABLE` / `DROP VIEW` for the given `(name, is_view)` objects: one statement for all tables, one for all views.
pub fn drop_tables(db: &str, objects: &[(String, bool)]) -> PlanResult {
    nonempty(&objects.iter().map(|o| o.0.clone()).collect::<Vec<_>>(), "table")?;
    let list = |view: bool| objects.iter().filter(|o| o.1 == view).map(|o| qualified(db, &o.0)).collect::<Vec<_>>().join(", ");
    let mut out = Vec::new();
    let (tables, views) = (list(false), list(true));
    if !tables.is_empty() {
        out.push(format!("DROP TABLE {tables};"));
    }
    if !views.is_empty() {
        out.push(format!("DROP VIEW {views};"));
    }
    Ok(out)
}

/// `TRUNCATE TABLE` for each table. Views cannot be emptied.
pub fn empty_tables(db: &str, objects: &[(String, bool)]) -> PlanResult {
    nonempty(&objects.iter().map(|o| o.0.clone()).collect::<Vec<_>>(), "table")?;
    if let Some(v) = objects.iter().find(|o| o.1) {
        return Err(format!("`{}` is a view and cannot be emptied", v.0));
    }
    Ok(objects.iter().map(|o| format!("TRUNCATE TABLE {};", qualified(db, &o.0))).collect())
}

/// Copy each `(from, to)` table within `db`: structure with indexes, and the rows when `with_data`.
pub fn copy_tables(db: &str, copies: &[(String, String)], with_data: bool) -> PlanResult {
    nonempty(&copies.iter().map(|c| c.1.clone()).collect::<Vec<_>>(), "new table")?;
    if let Some(c) = copies.iter().find(|c| c.0 == c.1) {
        return Err(format!("`{}` cannot be copied onto itself", c.0));
    }
    let mut out = Vec::new();
    for (from, to) in copies {
        out.push(format!("CREATE TABLE {} LIKE {};", qualified(db, to), qualified(db, from)));
        if with_data {
            out.push(format!("INSERT INTO {} SELECT * FROM {};", qualified(db, to), qualified(db, from)));
        }
    }
    Ok(out)
}

/// One `RENAME TABLE` that adds `prefix` to every name.
pub fn add_prefix(db: &str, tables: &[String], prefix: &str) -> PlanResult {
    nonempty(tables, "table")?;
    if prefix.is_empty() {
        return Err("the prefix is empty".into());
    }
    Ok(vec![rename_all(db, tables.iter().map(|t| (t.clone(), format!("{prefix}{t}"))).collect())])
}

/// One `RENAME TABLE` that replaces `old` with `new` at the start of the names that have it. Tables without the prefix are left alone.
pub fn replace_prefix(db: &str, tables: &[String], old: &str, new: &str) -> PlanResult {
    nonempty(tables, "table")?;
    if old.is_empty() {
        return Err("the old prefix is empty".into());
    }
    let pairs: Vec<(String, String)> = tables.iter().filter_map(|t| t.strip_prefix(old).map(|rest| (t.clone(), format!("{new}{rest}")))).collect();
    if pairs.is_empty() {
        return Err(format!("none of the chosen tables starts with `{old}`"));
    }
    Ok(vec![rename_all(db, pairs)])
}

fn rename_all(db: &str, pairs: Vec<(String, String)>) -> String {
    format!("RENAME TABLE {};", pairs.iter().map(|(a, b)| format!("{} TO {}", qualified(db, a), qualified(db, b))).collect::<Vec<_>>().join(", "))
}

/// Copy the tables under new names made by adding `prefix`.
pub fn copy_with_prefix(db: &str, tables: &[String], prefix: &str, with_data: bool) -> PlanResult {
    nonempty(tables, "table")?;
    if prefix.is_empty() {
        return Err("the prefix is empty".into());
    }
    copy_tables(db, &tables.iter().map(|t| (t.clone(), format!("{prefix}{t}"))).collect::<Vec<_>>(), with_data)
}

pub fn checksum_tables(db: &str, tables: &[String]) -> PlanResult {
    nonempty(tables, "table")?;
    Ok(vec![format!("CHECKSUM TABLE {};", tables.iter().map(|t| qualified(db, t)).collect::<Vec<_>>().join(", "))])
}

fn charset_clause(charset: Option<&str>, collation: Option<&str>) -> Result<String, String> {
    let ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    let mut s = String::new();
    if let Some(c) = charset.filter(|c| !c.is_empty()) {
        if !ok(c) {
            return Err(format!("`{c}` is not a character set name"));
        }
        s.push_str(&format!(" CHARACTER SET {c}"));
    }
    if let Some(c) = collation.filter(|c| !c.is_empty()) {
        if !ok(c) {
            return Err(format!("`{c}` is not a collation name"));
        }
        s.push_str(&format!(" COLLATE {c}"));
    }
    if s.is_empty() {
        return Err("choose a character set or a collation".into());
    }
    Ok(s)
}

/// Change a database's default character set / collation (existing tables keep theirs).
pub fn alter_database(name: &str, charset: Option<&str>, collation: Option<&str>) -> PlanResult {
    if SYSTEM_DATABASES.contains(&name.to_lowercase().as_str()) {
        return Err(format!("`{name}` is a system database"));
    }
    Ok(vec![format!("ALTER DATABASE {}{};", q(name), charset_clause(charset, collation)?)])
}

/// Copy a database: create it, then every table (structure, and rows when `with_data`). Views, routines, triggers and events are not copied.
pub fn copy_database(src: &str, dst: &str, tables: &[String], with_data: bool, collation: Option<&str>) -> PlanResult {
    if dst.trim().is_empty() {
        return Err("the new database name is empty".into());
    }
    if dst.eq_ignore_ascii_case(src) {
        return Err("the copy needs a different name".into());
    }
    if SYSTEM_DATABASES.contains(&dst.to_lowercase().as_str()) {
        return Err(format!("`{dst}` is a system database"));
    }
    let mut out = vec![format!("CREATE DATABASE {}{};", q(dst), charset_clause(None, collation).unwrap_or_default())];
    for t in tables {
        out.push(format!("CREATE TABLE {} LIKE {};", qualified(dst, t), qualified(src, t)));
        if with_data {
            out.push(format!("INSERT INTO {} SELECT * FROM {};", qualified(dst, t), qualified(src, t)));
        }
    }
    Ok(out)
}

/// MySQL has no `RENAME DATABASE`. This creates the new database, moves every table into it with one `RENAME TABLE` (instant, data included),
/// and drops the old, now empty, database. Refused when `other_objects` (views, routines, triggers, events) exist: they would be lost.
pub fn rename_database(old: &str, new: &str, tables: &[String], other_objects: usize, collation: Option<&str>) -> PlanResult {
    if new.trim().is_empty() {
        return Err("the new database name is empty".into());
    }
    if new.eq_ignore_ascii_case(old) {
        return Err("the new name is the same as the old one".into());
    }
    if [old, new].iter().any(|n| SYSTEM_DATABASES.contains(&n.to_lowercase().as_str())) {
        return Err("system databases cannot be renamed".into());
    }
    if other_objects > 0 {
        return Err(format!("`{old}` has {other_objects} view(s), routine(s), trigger(s) or event(s); renaming by moving tables would lose them. Copy the database instead, or drop those objects first"));
    }
    let mut out = vec![format!("CREATE DATABASE {}{};", q(new), charset_clause(None, collation).unwrap_or_default())];
    if !tables.is_empty() {
        out.push(format!("RENAME TABLE {};", tables.iter().map(|t| format!("{} TO {}", qualified(old, t), qualified(new, t))).collect::<Vec<_>>().join(", ")));
    }
    out.push(format!("DROP DATABASE {};", q(old)));
    Ok(out)
}

/// The plan as it will run, with the foreign key switch spelled out when checks are off.
pub fn with_fk_checks(statements: &[String], checks_on: bool) -> Vec<String> {
    if checks_on {
        return statements.to_vec();
    }
    let mut out = vec!["SET FOREIGN_KEY_CHECKS = 0;".to_string()];
    out.extend(statements.iter().cloned());
    out.push("SET FOREIGN_KEY_CHECKS = 1;".to_string());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }
    fn objs(v: &[(&str, bool)]) -> Vec<(String, bool)> {
        v.iter().map(|(n, b)| (n.to_string(), *b)).collect()
    }

    #[test]
    fn dropping_databases_quotes_names_and_refuses_system_ones() {
        assert_eq!(drop_databases(&s(&["a", "we`ird"])).unwrap(), ["DROP DATABASE `a`;", "DROP DATABASE `we``ird`;"]);
        assert!(drop_databases(&s(&["a", "MySQL"])).unwrap_err().contains("system database"));
        assert!(drop_databases(&[]).is_err());
        assert!(drop_databases(&s(&["a", "A"])).unwrap_err().contains("twice"));
    }

    #[test]
    fn dropping_tables_is_one_statement_and_views_use_drop_view() {
        assert_eq!(drop_tables("shop", &objs(&[("a", false), ("b", false)])).unwrap(), ["DROP TABLE `shop`.`a`, `shop`.`b`;"]);
        assert_eq!(drop_tables("shop", &objs(&[("a", false), ("v", true)])).unwrap(), ["DROP TABLE `shop`.`a`;", "DROP VIEW `shop`.`v`;"]);
    }

    #[test]
    fn emptying_refuses_views() {
        assert_eq!(empty_tables("d", &objs(&[("a", false), ("b", false)])).unwrap(), ["TRUNCATE TABLE `d`.`a`;", "TRUNCATE TABLE `d`.`b`;"]);
        assert!(empty_tables("d", &objs(&[("v", true)])).unwrap_err().contains("view"));
    }

    #[test]
    fn copying_structure_and_data() {
        let c = vec![("a".to_string(), "a_copy".to_string())];
        assert_eq!(copy_tables("d", &c, false).unwrap(), ["CREATE TABLE `d`.`a_copy` LIKE `d`.`a`;"]);
        assert_eq!(copy_tables("d", &c, true).unwrap(), ["CREATE TABLE `d`.`a_copy` LIKE `d`.`a`;", "INSERT INTO `d`.`a_copy` SELECT * FROM `d`.`a`;"]);
        assert!(copy_tables("d", &[("a".into(), "a".into())], true).is_err());
    }

    #[test]
    fn prefix_operations_rename_in_one_statement() {
        assert_eq!(add_prefix("d", &s(&["a", "b"]), "old_").unwrap(), ["RENAME TABLE `d`.`a` TO `d`.`old_a`, `d`.`b` TO `d`.`old_b`;"]);
        assert_eq!(replace_prefix("d", &s(&["tmp_a", "tmp_b", "keep"]), "tmp_", "prod_").unwrap(), ["RENAME TABLE `d`.`tmp_a` TO `d`.`prod_a`, `d`.`tmp_b` TO `d`.`prod_b`;"]);
        assert!(replace_prefix("d", &s(&["a"]), "x_", "y_").unwrap_err().contains("starts with"));
        assert!(add_prefix("d", &s(&["a"]), "").is_err());
        assert_eq!(copy_with_prefix("d", &s(&["a"]), "bak_", true).unwrap().len(), 2);
    }

    #[test]
    fn checksum_and_alter_database() {
        assert_eq!(checksum_tables("d", &s(&["a", "b"])).unwrap(), ["CHECKSUM TABLE `d`.`a`, `d`.`b`;"]);
        assert_eq!(alter_database("shop", Some("utf8mb4"), Some("utf8mb4_unicode_ci")).unwrap(), ["ALTER DATABASE `shop` CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;"]);
        assert!(alter_database("shop", Some("utf8mb4; DROP"), None).is_err(), "names are checked, never pasted blindly");
        assert!(alter_database("shop", None, None).is_err());
        assert!(alter_database("mysql", Some("utf8mb4"), None).is_err());
    }

    #[test]
    fn copying_and_renaming_a_database() {
        let t = s(&["a", "b"]);
        assert_eq!(
            copy_database("src", "dst", &t, true, Some("utf8mb4_bin")).unwrap(),
            ["CREATE DATABASE `dst` COLLATE utf8mb4_bin;", "CREATE TABLE `dst`.`a` LIKE `src`.`a`;", "INSERT INTO `dst`.`a` SELECT * FROM `src`.`a`;", "CREATE TABLE `dst`.`b` LIKE `src`.`b`;", "INSERT INTO `dst`.`b` SELECT * FROM `src`.`b`;"]
        );
        assert!(copy_database("src", "SRC", &t, false, None).is_err());
        assert_eq!(
            rename_database("old", "new", &t, 0, None).unwrap(),
            ["CREATE DATABASE `new`;", "RENAME TABLE `old`.`a` TO `new`.`a`, `old`.`b` TO `new`.`b`;", "DROP DATABASE `old`;"]
        );
        assert!(rename_database("old", "new", &t, 2, None).unwrap_err().contains("would lose them"));
        assert!(rename_database("mysql", "x", &t, 0, None).is_err());
    }

    #[test]
    fn the_foreign_key_switch_is_spelled_out() {
        let plan = s(&["DROP TABLE `a`;"]);
        assert_eq!(with_fk_checks(&plan, true), plan);
        assert_eq!(with_fk_checks(&plan, false), ["SET FOREIGN_KEY_CHECKS = 0;", "DROP TABLE `a`;", "SET FOREIGN_KEY_CHECKS = 1;"]);
    }
}
