//! Schema compare: the statements that make a **target** database's tables look like a **source** database's.
//! Pure: it takes table definitions as read from the two databases and builds statements with the same generators the table designer uses
//! (`create_table_sql`, `alter_table_sql`), so a comparison can never produce SQL the designer would not.

use crate::ddl::{alter_table_sql, create_table_sql, drop_table_sql, ForeignKeyDef, TableDef};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    /// In the source, not in the target: `CREATE TABLE`.
    Added,
    /// In both and different: `ALTER TABLE`.
    Changed,
    /// In the target, not in the source: `DROP TABLE`. Destructive.
    Removed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TableChange {
    pub table: String,
    pub kind: ChangeKind,
    /// The statements for this table (a new table is created without its foreign keys).
    pub statements: Vec<String>,
    /// Statements that must run after every table exists: the foreign keys of new tables.
    pub deferred: Vec<String>,
    pub destructive: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SchemaDiff {
    pub changes: Vec<TableChange>,
    /// Tables that are the same in both.
    pub identical: usize,
}

impl SchemaDiff {
    /// The script for the chosen tables, in an order that works: creates and alters, then the new tables' foreign keys, then drops.
    pub fn script(&self, chosen: &[String]) -> Vec<String> {
        let picked: Vec<&TableChange> = self.changes.iter().filter(|c| chosen.contains(&c.table)).collect();
        let mut out: Vec<String> = picked.iter().filter(|c| c.kind != ChangeKind::Removed).flat_map(|c| c.statements.clone()).collect();
        out.extend(picked.iter().flat_map(|c| c.deferred.clone()));
        out.extend(picked.iter().filter(|c| c.kind == ChangeKind::Removed).flat_map(|c| c.statements.clone()));
        out.into_iter().map(|s| if s.ends_with(';') { s } else { format!("{s};") }).collect()
    }
}

fn own_schema(fk: &ForeignKeyDef, source_db: &str) -> ForeignKeyDef {
    let mut f = fk.clone();
    if f.ref_schema.as_deref() == Some(source_db) {
        f.ref_schema = None; // a reference into the source database means "this database" in the target
    }
    f
}

/// The source table as an edit of the target table: columns, indexes and keys with the same name are the same object, so a column that
/// differs becomes a `CHANGE COLUMN` and a column that only the source has becomes an `ADD COLUMN`.
fn as_edit_of(source: &TableDef, target: &TableDef, source_db: &str) -> TableDef {
    let mut t = source.clone();
    t.name = target.name.clone();
    for c in &mut t.columns {
        c.original_name = target.columns.iter().any(|x| x.name == c.name).then(|| c.name.clone());
    }
    for i in &mut t.indexes {
        i.original_name = target.indexes.iter().any(|x| x.name == i.name).then(|| i.name.clone());
    }
    t.foreign_keys = t.foreign_keys.iter().map(|f| own_schema(f, source_db)).collect();
    for f in &mut t.foreign_keys {
        f.original_name = target.foreign_keys.iter().any(|x| x.name == f.name).then(|| f.name.clone());
    }
    t
}

/// A new table, created without its foreign keys (they may point at tables created later); the keys come back as `deferred`.
fn as_new(source: &TableDef, source_db: &str, target_db: &str) -> Result<(String, Vec<String>), String> {
    let mut bare = source.clone();
    for c in &mut bare.columns {
        c.original_name = None;
    }
    let fks: Vec<ForeignKeyDef> = bare.foreign_keys.iter().map(|f| own_schema(f, source_db)).collect();
    bare.foreign_keys.clear();
    let create = create_table_sql(Some(target_db), &bare).map_err(|e| format!("table `{}`: {}", source.name, e.0.join("; ")))?;
    let deferred = fks
        .iter()
        .map(|f| format!("ALTER TABLE {}.{} ADD {}", crate::ddl::q(target_db), crate::ddl::q(&source.name), f.constraint_sql(Some(target_db))))
        .collect();
    Ok((create, deferred))
}

/// What has to change in `target` (tables as read from `target_db`) to match `source` (tables as read from `source_db`).
/// Views, routines, triggers and events are not compared.
pub fn compare(source: &[TableDef], source_db: &str, target: &[TableDef], target_db: &str) -> Result<SchemaDiff, String> {
    let mut changes = Vec::new();
    let mut identical = 0;
    for s in source {
        match target.iter().find(|t| t.name == s.name) {
            None => {
                let (create, deferred) = as_new(s, source_db, target_db)?;
                changes.push(TableChange { table: s.name.clone(), kind: ChangeKind::Added, statements: vec![create], deferred, destructive: false });
            }
            Some(t) => {
                let edit = as_edit_of(s, t, source_db);
                let statements = alter_table_sql(Some(target_db), t, &edit).map_err(|e| format!("table `{}`: {}", s.name, e.0.join("; ")))?;
                if statements.is_empty() {
                    identical += 1;
                } else {
                    changes.push(TableChange { table: s.name.clone(), kind: ChangeKind::Changed, statements, deferred: vec![], destructive: false });
                }
            }
        }
    }
    for t in target.iter().filter(|t| !source.iter().any(|s| s.name == t.name)) {
        changes.push(TableChange { table: t.name.clone(), kind: ChangeKind::Removed, statements: vec![drop_table_sql(target_db, &t.name)], deferred: vec![], destructive: true });
    }
    changes.sort_by(|a, b| a.table.cmp(&b.table));
    Ok(SchemaDiff { changes, identical })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ddl::{ColumnDef, IndexColumn, IndexDef, IndexKind, RefAction};

    fn col(name: &str, ty: &str, params: Option<&str>, nullable: bool) -> ColumnDef {
        ColumnDef { name: name.into(), data_type: ty.into(), params: params.map(String::from), nullable, ..Default::default() }
    }

    fn pk(cols: &[&str]) -> IndexDef {
        IndexDef { name: "PRIMARY".into(), original_name: None, kind: IndexKind::Primary, columns: cols.iter().map(|c| IndexColumn { name: c.to_string(), length: None, desc: false }).collect() }
    }

    fn table(name: &str, columns: Vec<ColumnDef>) -> TableDef {
        TableDef { name: name.into(), engine: Some("InnoDB".into()), columns, indexes: vec![pk(&["id"])], ..Default::default() }
    }

    fn users() -> TableDef {
        table("users", vec![col("id", "int", None, false), col("name", "varchar", Some("40"), false)])
    }

    #[test]
    fn identical_schemas_have_no_changes() {
        let d = compare(&[users()], "a", &[users()], "b").unwrap();
        assert_eq!((d.changes.len(), d.identical), (0, 1));
        assert!(d.script(&["users".into()]).is_empty());
    }

    #[test]
    fn a_new_table_is_created_in_the_target_database_and_a_missing_one_is_a_destructive_drop() {
        let d = compare(&[users(), table("posts", vec![col("id", "int", None, false)])], "a", &[users(), table("old_log", vec![col("id", "int", None, false)])], "b").unwrap();
        let by = |t: &str| d.changes.iter().find(|c| c.table == t).unwrap();
        assert_eq!(by("posts").kind, ChangeKind::Added);
        assert!(by("posts").statements[0].starts_with("CREATE TABLE `b`.`posts`"), "{:?}", by("posts").statements);
        assert_eq!(by("old_log").kind, ChangeKind::Removed);
        assert!(by("old_log").destructive);
        assert_eq!(by("old_log").statements, ["DROP TABLE `b`.`old_log`"]);
        let script = d.script(&["posts".into(), "old_log".into()]);
        assert!(script[0].starts_with("CREATE TABLE") && script.last().unwrap().starts_with("DROP TABLE") && script.iter().all(|s| s.ends_with(';')));
        assert!(d.script(&["posts".into()]).iter().all(|s| !s.contains("DROP")), "drops only run when chosen");
    }

    #[test]
    fn a_changed_column_and_an_added_column_become_alter_clauses() {
        let mut src = users();
        src.columns[1] = col("name", "varchar", Some("100"), true);
        src.columns.push(col("email", "varchar", Some("255"), true));
        let d = compare(&[src], "a", &[users()], "b").unwrap();
        assert_eq!(d.changes.len(), 1);
        let sql = d.changes[0].statements.join("\n");
        assert!(sql.contains("ALTER TABLE `b`.`users`") && sql.contains("CHANGE COLUMN `name` `name` varchar(100)") && sql.contains("ADD COLUMN `email` varchar(255)"), "{sql}");
        assert!(!sql.contains("DROP COLUMN") && !sql.contains("RENAME"), "{sql}");
    }

    #[test]
    fn a_column_only_the_target_has_is_dropped_and_an_index_difference_is_rebuilt() {
        let mut tgt = users();
        tgt.columns.push(col("legacy", "int", None, true));
        tgt.indexes.push(IndexDef { name: "idx_name".into(), original_name: None, kind: IndexKind::Index, columns: vec![IndexColumn { name: "name".into(), length: None, desc: false }] });
        let d = compare(&[users()], "a", &[tgt], "b").unwrap();
        let sql = d.changes[0].statements.join("\n");
        assert!(sql.contains("DROP COLUMN `legacy`") && sql.contains("DROP INDEX `idx_name`"), "{sql}");
    }

    #[test]
    fn new_tables_defer_their_foreign_keys_so_creation_order_does_not_matter() {
        let mut posts = table("posts", vec![col("id", "int", None, false), col("user_id", "int", None, false)]);
        posts.foreign_keys.push(ForeignKeyDef { name: "fk_user".into(), original_name: None, columns: vec!["user_id".into()], ref_schema: Some("a".into()), ref_table: "users".into(), ref_columns: vec!["id".into()], on_delete: RefAction::Cascade, on_update: RefAction::Restrict });
        let d = compare(&[posts, users()], "a", &[], "b").unwrap();
        let p = d.changes.iter().find(|c| c.table == "posts").unwrap();
        assert!(!p.statements[0].contains("FOREIGN KEY"), "created without keys: {}", p.statements[0]);
        assert_eq!(p.deferred.len(), 1);
        assert!(p.deferred[0].contains("ALTER TABLE `b`.`posts` ADD CONSTRAINT `fk_user` FOREIGN KEY (`user_id`) REFERENCES `users`"), "{}", p.deferred[0]);
        assert!(!p.deferred[0].contains("`a`"), "a reference into the source database means the target database: {}", p.deferred[0]);
        let script = d.script(&["posts".into(), "users".into()]);
        let last_create = script.iter().rposition(|s| s.starts_with("CREATE TABLE")).unwrap();
        let fk = script.iter().position(|s| s.contains("FOREIGN KEY")).unwrap();
        assert!(fk > last_create, "keys come after every create");
    }
}
