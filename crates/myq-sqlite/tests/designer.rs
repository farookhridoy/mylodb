//! The SQLite table designer against real files: reading a definition, creating, altering in place, rebuilding, and rolling back.

use myq_schema::ddl::{ColumnDef, ColumnDefault, ForeignKeyDef, IndexColumn, IndexDef, IndexKind, RefAction, TableDef};
use myq_sqlite::{apply_ddl, ddl_statements, read_table_def};
use rusqlite::Connection;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mylodb_itest_designer_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("shop.db");
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE customers (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, email VARCHAR(80) DEFAULT 'none');
             CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL REFERENCES customers(id) ON DELETE CASCADE, total REAL DEFAULT 0, placed TEXT);
             CREATE INDEX idx_orders_customer ON orders (customer_id, placed DESC);
             CREATE UNIQUE INDEX idx_customers_email ON customers (email);
             INSERT INTO customers (name, email) VALUES ('Ann', 'a@x'), ('Bo', 'b@x');
             INSERT INTO orders VALUES (1, 1, 10.5, '2026-01-01'), (2, 1, 20, '2026-01-02'), (3, 2, 5, '2026-01-03');",
        )
        .unwrap();
    path
}

fn count(path: &PathBuf, table: &str) -> i64 {
    Connection::open(path).unwrap().query_row(&format!("SELECT count(*) FROM \"{table}\""), [], |r| r.get(0)).unwrap()
}

fn col(name: &str, ty: &str) -> ColumnDef {
    ColumnDef { name: name.into(), original_name: None, data_type: ty.into(), nullable: true, ..Default::default() }
}

#[test]
fn a_definition_is_read_with_keys_indexes_and_defaults() {
    let path = fixture("read");
    let c = read_table_def(&path, "customers").unwrap();
    assert_eq!(c.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["id", "name", "email"]);
    assert!(c.columns[0].auto_increment, "AUTOINCREMENT is recognised");
    assert!(!c.columns[1].nullable);
    assert_eq!(c.columns[2].data_type, "varchar");
    assert_eq!(c.columns[2].params.as_deref(), Some("80"));
    assert_eq!(c.columns[2].default, ColumnDefault::Literal("none".into()));
    assert_eq!(c.indexes.iter().map(|i| (i.name.as_str(), i.kind)).collect::<Vec<_>>(), vec![("PRIMARY", IndexKind::Primary), ("idx_customers_email", IndexKind::Unique)]);
    let o = read_table_def(&path, "orders").unwrap();
    assert_eq!(o.foreign_keys.len(), 1);
    assert_eq!((o.foreign_keys[0].ref_table.as_str(), o.foreign_keys[0].columns.clone(), o.foreign_keys[0].ref_columns.clone(), o.foreign_keys[0].on_delete), ("customers", vec!["customer_id".to_string()], vec!["id".to_string()], RefAction::Cascade));
    let idx = o.indexes.iter().find(|i| i.name == "idx_orders_customer").unwrap();
    assert!(idx.columns[1].desc && !idx.columns[0].desc);
    assert!(read_table_def(&path, "nope").is_err());
    // an unchanged definition needs no statements
    assert_eq!(ddl_statements(Some(&o), &o).unwrap(), Vec::<String>::new());
}

#[test]
fn a_new_table_is_created_with_its_key_foreign_key_and_index() {
    let path = fixture("create");
    let mut id = col("id", "integer");
    id.nullable = false;
    id.auto_increment = true;
    let mut cid = col("customer_id", "integer");
    cid.nullable = false;
    let t = TableDef {
        name: "notes".into(),
        columns: vec![id, cid, { let mut c = col("body", "text"); c.default = ColumnDefault::Literal("it's".into()); c }],
        indexes: vec![
            IndexDef { name: "PRIMARY".into(), original_name: None, kind: IndexKind::Primary, columns: vec![IndexColumn { name: "id".into(), length: None, desc: false }] },
            IndexDef { name: "idx_notes_customer".into(), original_name: None, kind: IndexKind::Index, columns: vec![IndexColumn { name: "customer_id".into(), length: None, desc: false }] },
        ],
        foreign_keys: vec![ForeignKeyDef { name: "fk_notes_customer".into(), original_name: None, columns: vec!["customer_id".into()], ref_schema: None, ref_table: "customers".into(), ref_columns: vec!["id".into()], on_delete: RefAction::Cascade, on_update: RefAction::Restrict }],
        ..Default::default()
    };
    let stmts = ddl_statements(None, &t).unwrap();
    assert!(stmts[0].contains("\"id\" integer PRIMARY KEY AUTOINCREMENT") && stmts[0].contains("DEFAULT 'it''s'") && stmts[0].contains("FOREIGN KEY"), "{}", stmts[0]);
    apply_ddl(&path, &stmts).unwrap();
    let back = read_table_def(&path, "notes").unwrap();
    assert_eq!(back.columns.len(), 3);
    assert!(back.columns[0].auto_increment);
    assert_eq!(back.foreign_keys.len(), 1);
    assert!(back.indexes.iter().any(|i| i.name == "idx_notes_customer"));
    // a bad definition is explained, not sent
    let mut bad = t.clone();
    bad.columns.push(col("ID", "text"));
    bad.indexes[1].columns[0].name = "missing".into();
    let errs = ddl_statements(None, &bad).unwrap_err();
    assert!(errs.iter().any(|e| e.contains("appears twice")) && errs.iter().any(|e| e.contains("missing column")), "{errs:?}");
}

#[test]
fn adding_renaming_and_dropping_columns_and_indexes_alters_in_place() {
    let path = fixture("inplace");
    let before = read_table_def(&path, "orders").unwrap();
    let mut d = before.clone();
    d.columns[3].name = "placed_on".into(); // rename
    d.columns.remove(2); // drop total
    d.columns.push(col("note", "text")); // add
    d.indexes.retain(|i| i.name != "idx_orders_customer"); // drop index
    d.indexes.push(IndexDef { name: "idx_orders_placed".into(), original_name: None, kind: IndexKind::Index, columns: vec![IndexColumn { name: "placed_on".into(), length: None, desc: false }] });
    let stmts = ddl_statements(Some(&before), &d).unwrap();
    assert!(!stmts.iter().any(|s| s.contains("DROP TABLE")), "no rebuild is needed: {stmts:?}");
    assert!(stmts[0].starts_with("DROP INDEX"), "an index goes before its column is dropped: {stmts:?}");
    apply_ddl(&path, &stmts).unwrap();
    assert_eq!(count(&path, "orders"), 3, "the rows are untouched");
    let after = read_table_def(&path, "orders").unwrap();
    assert_eq!(after.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["id", "customer_id", "placed_on", "note"]);
    assert!(after.indexes.iter().any(|i| i.name == "idx_orders_placed") && !after.indexes.iter().any(|i| i.name == "idx_orders_customer"));
    // renaming the table
    let mut r = after.clone();
    r.name = "purchases".into();
    apply_ddl(&path, &ddl_statements(Some(&after), &r).unwrap()).unwrap();
    assert_eq!(count(&path, "purchases"), 3);
}

#[test]
fn changing_a_type_or_a_key_rebuilds_the_table_and_keeps_rows_and_indexes() {
    let path = fixture("rebuild");
    let before = read_table_def(&path, "orders").unwrap();
    let mut d = before.clone();
    d.columns[2].data_type = "numeric".into();
    d.columns[2].params = Some("10,2".into());
    d.columns[3].nullable = false; // placed becomes NOT NULL
    d.columns[3].default = ColumnDefault::Literal("unknown".into());
    d.foreign_keys[0].on_delete = RefAction::Restrict;
    let stmts = ddl_statements(Some(&before), &d).unwrap();
    assert!(stmts[0].starts_with("CREATE TABLE \"__new_orders\"") && stmts.iter().any(|s| s.starts_with("INSERT INTO")) && stmts.iter().any(|s| s.starts_with("DROP TABLE \"orders\"")), "{stmts:?}");
    apply_ddl(&path, &stmts).unwrap();
    let after = read_table_def(&path, "orders").unwrap();
    assert_eq!(after.columns[2].data_type, "numeric");
    assert!(!after.columns[3].nullable);
    assert_eq!(after.foreign_keys[0].on_delete, RefAction::Restrict);
    assert!(after.indexes.iter().any(|i| i.name == "idx_orders_customer" && i.columns[1].desc), "the index is back");
    assert_eq!(count(&path, "orders"), 3);
    let c = Connection::open(&path).unwrap();
    assert_eq!(c.query_row::<f64, _, _>("SELECT total FROM orders WHERE id = 2", [], |r| r.get(0)).unwrap(), 20.0);
    assert!(c.query_row::<i64, _, _>("SELECT count(*) FROM sqlite_master WHERE name LIKE '__new_%'", [], |r| r.get(0)).unwrap() == 0, "no temporary table is left");
}

#[test]
fn a_failing_change_leaves_the_table_as_it_was() {
    let path = fixture("rollback");
    let before = read_table_def(&path, "customers").unwrap();
    // a NOT NULL column without a default cannot be filled for the existing rows
    let mut d = before.clone();
    d.columns.push({ let mut c = col("tier", "text"); c.nullable = false; c });
    let stmts = ddl_statements(Some(&before), &d).unwrap();
    assert!(stmts[0].starts_with("CREATE TABLE \"__new_"), "a NOT NULL column with no default needs a rebuild: {stmts:?}");
    let err = apply_ddl(&path, &stmts).unwrap_err();
    assert!(err.to_lowercase().contains("not null"), "{err}");
    let again = read_table_def(&path, "customers").unwrap();
    assert_eq!(again.columns.len(), 3, "the original table is intact");
    assert_eq!(count(&path, "customers"), 2);
    // a change that would orphan rows is refused too (the parent key is removed while orders point at it)
    let orders = read_table_def(&path, "orders").unwrap();
    let mut d2 = read_table_def(&path, "customers").unwrap();
    d2.columns[0].auto_increment = false;
    d2.indexes.retain(|i| i.kind != IndexKind::Primary);
    d2.columns[0].name = "cid".into();
    d2.columns[0].original_name = None; // the old id column is dropped and a new one added: the rows lose their ids
    let _ = orders;
    let stmts2 = ddl_statements(Some(&again), &d2).unwrap();
    assert!(apply_ddl(&path, &stmts2).is_err(), "orders would point at missing customers");
    assert_eq!(read_table_def(&path, "customers").unwrap().columns[0].name, "id");
}

#[test]
fn generated_columns_are_read_kept_through_a_rebuild_and_can_be_added() {
    let path = fixture("generated");
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE lines (id INTEGER PRIMARY KEY, qty INTEGER NOT NULL, price REAL NOT NULL,
               total REAL GENERATED ALWAYS AS (qty * price) STORED,
               label TEXT AS (printf('%d x %.2f', qty, price)) VIRTUAL);
             INSERT INTO lines (qty, price) VALUES (2, 1.5), (3, 2.0);",
        )
        .unwrap();
    let before = read_table_def(&path, "lines").unwrap();
    assert_eq!(before.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["id", "qty", "price", "total", "label"], "generated columns are listed");
    let total = before.columns.iter().find(|c| c.name == "total").unwrap();
    assert_eq!((total.generated.as_ref().unwrap().expression.as_str(), total.generated.as_ref().unwrap().stored), ("qty * price", true));
    let label = before.columns.iter().find(|c| c.name == "label").unwrap().generated.clone().unwrap();
    assert!(!label.stored && label.expression.contains("printf('%d x %.2f', qty, price)"), "{label:?}");

    // a change that rebuilds the table keeps the generated columns and computes them again
    let mut d = before.clone();
    d.columns[1].data_type = "numeric".into();
    let stmts = ddl_statements(Some(&before), &d).unwrap();
    assert!(stmts[0].contains("GENERATED ALWAYS AS (qty * price) STORED"), "{}", stmts[0]);
    let insert = stmts.iter().find(|s| s.starts_with("INSERT INTO")).unwrap();
    assert!(!insert.contains("total") && !insert.contains("label"), "generated columns are not copied: {insert}");
    apply_ddl(&path, &stmts).unwrap();
    let c = Connection::open(&path).unwrap();
    assert_eq!(c.query_row::<f64, _, _>("SELECT total FROM lines WHERE id = 2", [], |r| r.get(0)).unwrap(), 6.0);
    assert_eq!(read_table_def(&path, "lines").unwrap().columns.len(), 5);

    // a virtual generated column is added in place; a stored one rebuilds the table
    let cur = read_table_def(&path, "lines").unwrap();
    let mut add = cur.clone();
    add.columns.push({ let mut c = col("double_qty", "integer"); c.generated = Some(myq_schema::ddl::Generated { expression: "qty * 2".into(), stored: false }); c });
    let s = ddl_statements(Some(&cur), &add).unwrap();
    assert!(s.len() == 1 && s[0].starts_with("ALTER TABLE") && s[0].contains("ADD COLUMN"), "{s:?}");
    apply_ddl(&path, &s).unwrap();
    let cur = read_table_def(&path, "lines").unwrap();
    let mut stored = cur.clone();
    stored.columns.push({ let mut c = col("triple", "integer"); c.generated = Some(myq_schema::ddl::Generated { expression: "qty * 3".into(), stored: true }); c });
    let s = ddl_statements(Some(&cur), &stored).unwrap();
    assert!(s[0].starts_with("CREATE TABLE \"__new_"), "a stored column needs a rebuild: {s:?}");
    apply_ddl(&path, &s).unwrap();
    assert_eq!(Connection::open(&path).unwrap().query_row::<i64, _, _>("SELECT triple FROM lines WHERE id = 2", [], |r| r.get(0)).unwrap(), 9);

    // mistakes are explained
    let mut bad = cur.clone();
    bad.columns[3].generated.as_mut().unwrap().expression = " ".into();
    assert!(ddl_statements(Some(&cur), &bad).unwrap_err().iter().any(|e| e.contains("expression is empty")));
}
