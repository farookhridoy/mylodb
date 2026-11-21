//! Designer + privilege round trips against a local MySQL. Uses its own database and its own test
//! account, both removed afterwards even if an assertion fails.

use myq_admin::*;
use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::{pool, ConnConfig, Pool, SslMode};
use myq_schema::ddl::*;
use std::collections::{BTreeMap, BTreeSet};

fn cfg() -> ConnConfig {
    ConnConfig {
        host: std::env::var("MYLODB_TEST_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port: std::env::var("MYLODB_TEST_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3306),
        user: std::env::var("MYLODB_TEST_USER").unwrap_or_else(|_| "root".into()),
        password: std::env::var("MYLODB_TEST_PASSWORD").ok(),
        database: None,
        ssl: SslMode::Disabled,
        ..Default::default()
    }
}

fn col(name: &str, ty: &str, params: Option<&str>) -> ColumnDef {
    ColumnDef { name: name.into(), data_type: ty.into(), params: params.map(String::from), nullable: true, ..Default::default() }
}
fn index(name: &str, kind: IndexKind, cols: &[(&str, Option<u32>, bool)]) -> IndexDef {
    IndexDef { name: name.into(), original_name: None, kind, columns: cols.iter().map(|(n, l, d)| IndexColumn { name: n.to_string(), length: *l, desc: *d }).collect() }
}
/// The state a definition is in once it exists on the server.
fn existing(mut t: TableDef) -> TableDef {
    for c in &mut t.columns { c.original_name = Some(c.name.clone()); }
    for i in &mut t.indexes { i.original_name = Some(i.name.clone()); }
    for f in &mut t.foreign_keys { f.original_name = Some(f.name.clone()); }
    t
}

#[tokio::test]
async fn designer_and_privileges_end_to_end() {
    let pool = pool(&cfg());
    let mut admin = match pool.get_conn().await {
        Ok(c) => c,
        Err(e) => return eprintln!("SKIP: no MySQL reachable: {e}"),
    };
    let pid = std::process::id();
    let db = format!("mylodb_itest_admin_{pid}");
    let user = format!("mylodb_itest_u_{pid}");
    admin.query_drop(format!("CREATE DATABASE `{db}`")).await.unwrap();
    let outcome = tokio::spawn(body(pool.clone(), db.clone(), user.clone())).await;
    // cleanup regardless of the outcome
    for u in [user.clone(), format!("{user}_renamed")] {
        let _ = admin.query_drop(format!("DROP USER IF EXISTS '{u}'@'localhost'")).await;
        let _ = admin.query_drop(format!("DROP USER IF EXISTS '{u}'@'127.0.0.1'")).await;
    }
    admin.query_drop(format!("DROP DATABASE `{db}`")).await.unwrap();
    outcome.unwrap();
}

async fn exec(conn: &mut myq_driver::Conn, stmts: &[String]) {
    run_statements(conn, stmts).await.unwrap_or_else(|e| panic!("{e}"));
}

async fn body(pool: Pool, db: String, user: String) {
    let db = db.as_str();
    let mut c = pool.get_conn().await.unwrap();

    // ---------- designer: create ----------
    exec(&mut c, &[format!("CREATE TABLE `{db}`.parent (id INT PRIMARY KEY, label VARCHAR(10))")]).await;

    let mut id = col("id", "int", None);
    id.unsigned = true; id.nullable = false; id.auto_increment = true;
    let mut name = col("name", "varchar", Some("40"));
    name.nullable = false; name.default = ColumnDefault::Literal("it's".into()); name.comment = "the \\ name".into();
    let mut email = col("email", "varchar", Some("80"));
    email.charset = Some("latin1".into()); email.collation = Some("latin1_bin".into());
    let mut price = col("price", "decimal", Some("10,2"));
    price.nullable = false; price.default = ColumnDefault::Literal("0.00".into());
    let mut status = col("status", "enum", Some("'a','b','it''s'"));
    status.default = ColumnDefault::Literal("a".into());
    let mut created = col("created", "datetime", Some("3"));
    created.nullable = false; created.default = ColumnDefault::Expression("CURRENT_TIMESTAMP(3)".into()); created.on_update = Some("CURRENT_TIMESTAMP(3)".into());
    let mut gen = col("double_id", "int", None);
    gen.generated = Some(Generated { expression: "`parent_id` * 2".into(), stored: false });
    let parent_id = col("parent_id", "int", None);
    let d0 = TableDef {
        name: "items".into(),
        engine: Some("InnoDB".into()),
        comment: "demo".into(),
        columns: vec![id, name, email, price, status, created, col("doc", "json", None), col("body", "text", None), gen, parent_id],
        indexes: vec![
            index("PRIMARY", IndexKind::Primary, &[("id", None, false)]),
            index("uq_email", IndexKind::Unique, &[("email", Some(20), false)]),
            index("by_name", IndexKind::Index, &[("name", None, false), ("price", None, true)]),
            index("ft_body", IndexKind::Fulltext, &[("body", None, false)]),
            index("by_parent", IndexKind::Index, &[("parent_id", None, false)]),
        ],
        foreign_keys: vec![ForeignKeyDef { name: "fk_parent".into(), original_name: None, columns: vec!["parent_id".into()], ref_schema: None, ref_table: "parent".into(), ref_columns: vec!["id".into()], on_delete: RefAction::Cascade, on_update: RefAction::Restrict }],
        ..Default::default()
    };
    exec(&mut c, &[create_table_sql(Some(db), &d0).unwrap()]).await;

    // ---------- designer: read back ----------
    let (r0, warnings) = read_table_def(&mut c, db, "items").await.unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let by = |t: &TableDef, n: &str| t.columns.iter().find(|c| c.name == n).unwrap().clone();
    assert_eq!(by(&r0, "id").type_sql(), "int unsigned");
    assert!(by(&r0, "id").auto_increment && !by(&r0, "id").nullable);
    assert_eq!(by(&r0, "name").default, ColumnDefault::Literal("it's".into()));
    assert_eq!(by(&r0, "name").comment, "the \\ name");
    assert_eq!(by(&r0, "email").collation.as_deref(), Some("latin1_bin"), "a column differing from the table keeps its collation");
    assert_eq!(by(&r0, "name").collation, None, "an inheriting column carries none");
    assert_eq!(by(&r0, "price").default, ColumnDefault::Literal("0.00".into()));
    assert_eq!(by(&r0, "status").params.as_deref(), Some("'a','b','it''s'"));
    // MariaDB spells the function in lower case (`current_timestamp(3)`); it is the same expression
    match &by(&r0, "created").default {
        ColumnDefault::Expression(e) => assert!(e.eq_ignore_ascii_case("CURRENT_TIMESTAMP(3)"), "{e}"),
        other => panic!("{other:?}"),
    }
    assert!(by(&r0, "created").on_update.as_deref().is_some_and(|u| u.eq_ignore_ascii_case("CURRENT_TIMESTAMP(3)")));
    assert!(by(&r0, "double_id").generated.is_some() && !by(&r0, "double_id").generated.unwrap().stored);
    assert_eq!(r0.comment, "demo");
    let ix = |t: &TableDef, n: &str| t.indexes.iter().find(|i| i.name == n).unwrap().clone();
    assert_eq!(ix(&r0, "PRIMARY").kind, IndexKind::Primary);
    assert_eq!(ix(&r0, "uq_email").kind, IndexKind::Unique);
    assert_eq!(ix(&r0, "uq_email").columns[0].length, Some(20));
    assert_eq!(ix(&r0, "ft_body").kind, IndexKind::Fulltext);
    assert!(ix(&r0, "by_name").columns[1].desc);
    assert_eq!(r0.foreign_keys.len(), 1);
    assert_eq!((r0.foreign_keys[0].on_delete, r0.foreign_keys[0].ref_schema.clone()), (RefAction::Cascade, None));

    // The invariant the designer relies on: an unedited table produces no statements.
    assert_eq!(alter_table_sql(Some(db), &r0, &r0).unwrap(), Vec::<String>::new(), "spurious diff on an untouched table");

    // Round trip: re-creating from what we read gives a table that reads back identically.
    let mut copy = r0.clone();
    copy.name = "items_copy".into();
    for c in &mut copy.columns { c.original_name = None; }
    for i in &mut copy.indexes { i.original_name = None; }
    for f in &mut copy.foreign_keys { f.original_name = None; f.name = "fk_copy".into(); }
    exec(&mut c, &[create_table_sql(Some(db), &copy).unwrap()]).await;
    let (r_copy, _) = read_table_def(&mut c, db, "items_copy").await.unwrap();
    let mut expect = r0.clone();
    expect.name = "items_copy".into();
    expect.foreign_keys[0].name = "fk_copy".into();
    expect.foreign_keys[0].original_name = Some("fk_copy".into());
    assert_eq!(r_copy, expect, "create(read(t)) must read back equal");

    // ---------- designer: alter ----------
    exec(&mut c, &[format!("INSERT INTO `{db}`.parent VALUES (1,'p')"), format!("INSERT INTO `{db}`.items (name, email, price, parent_id, body) VALUES ('a','x@y',1.5,1,'hello world')")]).await;
    let mut d1 = r0.clone();
    d1.columns.retain(|c| c.name != "doc");                                           // drop a column
    let body_col = d1.columns.remove(d1.columns.iter().position(|c| c.name == "body").unwrap());
    d1.columns.insert(1, body_col);                                                    // move a column up
    d1.columns.iter_mut().find(|c| c.name == "name").unwrap().name = "title".into();   // rename
    d1.columns.iter_mut().find(|c| c.name == "title").unwrap().params = Some("120".into()); // widen
    d1.indexes.iter_mut().find(|i| i.name == "by_name").unwrap().columns[0].name = "title".into(); // index follows the rename
    d1.indexes.retain(|i| i.name != "uq_email");                                       // drop an index
    d1.indexes.push(index("uq_title", IndexKind::Unique, &[("title", None, false)]));  // add an index
    let mut added = col("score", "int", None);
    added.nullable = false; added.default = ColumnDefault::Literal("7".into());
    d1.columns.insert(0, added);                                                       // add a column first
    d1.foreign_keys[0].on_delete = RefAction::SetNull;                                 // change the FK
    d1.comment = "changed".into();
    let stmts = alter_table_sql(Some(db), &r0, &d1).unwrap();
    assert!(stmts.len() >= 3, "{stmts:#?}");
    assert!(stmts[0].contains("DROP FOREIGN KEY") && stmts.last().unwrap().contains("ADD CONSTRAINT"), "{stmts:#?}");
    exec(&mut c, &stmts).await;
    let (r1, _) = read_table_def(&mut c, db, "items").await.unwrap();
    assert_eq!(r1.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["score", "id", "body", "title", "email", "price", "status", "created", "double_id", "parent_id"]);
    assert_eq!(alter_table_sql(Some(db), &r1, &existing(d1.clone())).unwrap(), Vec::<String>::new(), "after applying, the server matches the design exactly");
    assert_eq!(r1.foreign_keys[0].on_delete, RefAction::SetNull);
    assert_eq!(r1.comment, "changed");
    let rows: Vec<(i32, String)> = c.query(format!("SELECT score, title FROM `{db}`.items")).await.unwrap();
    assert_eq!(rows, vec![(7, "a".to_string())], "existing rows keep their data through rename/move/add");

    // rename the table and change the primary key
    let mut d2 = existing(r1.clone());
    d2.name = "things".into();
    d2.indexes.iter_mut().find(|i| i.kind == IndexKind::Primary).unwrap().columns.push(IndexColumn { name: "score".into(), length: None, desc: false });
    // (id stays AUTO_INCREMENT; it is still the first column of the key)
    exec(&mut c, &alter_table_sql(Some(db), &r1, &d2).unwrap()).await;
    let (r2, _) = read_table_def(&mut c, db, "things").await.unwrap();
    assert_eq!(ix(&r2, "PRIMARY").columns.len(), 2);
    assert!(read_table_def(&mut c, db, "items").await.is_err());

    // a failing change reports which statement and keeps earlier ones applied
    let bad = vec![format!("ALTER TABLE `{db}`.things COMMENT='ok'"), format!("ALTER TABLE `{db}`.things DROP COLUMN no_such")];
    let err = run_statements(&mut c, &bad).await.unwrap_err();
    assert_eq!(err.index, 1);
    assert!(err.to_string().contains("earlier statement"));
    assert_eq!(read_table_def(&mut c, db, "things").await.unwrap().0.comment, "ok");

    // views are rejected; table operations work
    exec(&mut c, &[format!("CREATE VIEW `{db}`.v AS SELECT 1 AS x")]).await;
    assert!(matches!(read_table_def(&mut c, db, "v").await, Err(Error::NotATable(_))));
    assert!(matches!(read_table_def(&mut c, db, "nope").await, Err(Error::NotFound(_))));
    let copy_cols: Vec<String> = r2.columns.iter().filter(|c| c.generated.is_none()).map(|c| c.name.clone()).collect();
    exec(&mut c, &duplicate_table_sql(db, "things", "things_dup", Some(&copy_cols))).await;
    let n: Option<i64> = c.query_first(format!("SELECT COUNT(*) FROM `{db}`.things_dup")).await.unwrap();
    assert_eq!(n, Some(1));
    exec(&mut c, &[truncate_table_sql(db, "things_dup"), rename_table_sql(db, "things_dup", "dup2")]).await;
    let n: Option<i64> = c.query_first(format!("SELECT COUNT(*) FROM `{db}`.dup2")).await.unwrap();
    assert_eq!(n, Some(0));
    exec(&mut c, &[drop_table_sql(db, "dup2")]).await;

    // ---------- users and privileges ----------
    let edit = AccountEdit { user: user.clone(), host: "localhost".into(), password: Some("s3cr'et\\pw".into()), plugin: None, locked: false, expire_password: false };
    let created: Vec<Sql> = account_sql(None, &edit).unwrap();
    assert!(!created[0].shown.contains("s3cr"), "preview never shows the password");
    run_sql(&mut c, &created).await.unwrap();
    let acct = list_accounts(&mut c).await.unwrap().into_iter().find(|a| a.user == user && a.host == "localhost").expect("account listed");
    assert!(!acct.locked);

    // grant: db-level + table-level + column-level + grant option
    let tbl = Scope::Table { db: db.to_string(), table: "things".into() };
    let dbs = Scope::Database { db: db.to_string() };
    let mut cols = BTreeMap::new();
    cols.insert("UPDATE".to_string(), BTreeSet::from(["title".to_string(), "comment_x".to_string()]));
    cols.retain(|_, v| { v.remove("comment_x"); true });
    let desired = vec![
        ScopeGrant { scope: Some(dbs.clone()), privileges: BTreeSet::from(["SELECT".into(), "INSERT".into()]), grant_option: true, ..Default::default() },
        ScopeGrant { scope: Some(tbl.clone()), privileges: BTreeSet::from(["DELETE".into()]), columns: cols, ..Default::default() },
    ];
    let current = read_grants(&mut c, &user, "localhost").await.unwrap();
    assert!(current.scopes.is_empty(), "a new account has only USAGE");
    run_sql(&mut c, &grant_diff_sql(&user, "localhost", &current.scopes, &desired)).await.unwrap();
    let after = read_grants(&mut c, &user, "localhost").await.unwrap();
    let find = |g: &Grants, s: &Scope| g.scopes.iter().find(|x| x.scope.as_ref() == Some(s)).cloned().unwrap_or_else(|| panic!("no grant for {s:?}: {g:?}"));
    assert_eq!(find(&after, &dbs).privileges, BTreeSet::from(["SELECT".to_string(), "INSERT".to_string()]));
    assert!(find(&after, &dbs).grant_option);
    assert_eq!(find(&after, &tbl).privileges, BTreeSet::from(["DELETE".to_string()]));
    assert_eq!(find(&after, &tbl).columns["UPDATE"], BTreeSet::from(["title".to_string()]));
    assert!(grant_diff_sql(&user, "localhost", &after.scopes, &after.scopes).is_empty());

    // change: drop INSERT + grant option, add UPDATE, remove the table scope entirely, add ALL on another db
    let mut desired2 = vec![ScopeGrant { scope: Some(dbs.clone()), privileges: BTreeSet::from(["SELECT".into(), "UPDATE".into()]), ..Default::default() }];
    desired2.push(ScopeGrant { scope: Some(Scope::Database { db: format!("{db}_other") }), privileges: BTreeSet::from(["ALL PRIVILEGES".into()]), ..Default::default() });
    let diff = grant_diff_sql(&user, "localhost", &after.scopes, &desired2);
    assert!(diff.iter().position(|s| s.run.starts_with("REVOKE")).unwrap() < diff.iter().position(|s| s.run.starts_with("GRANT")).unwrap());
    run_sql(&mut c, &diff).await.unwrap();
    let after2 = read_grants(&mut c, &user, "localhost").await.unwrap();
    assert_eq!(find(&after2, &dbs).privileges, BTreeSet::from(["SELECT".to_string(), "UPDATE".to_string()]));
    assert!(!find(&after2, &dbs).grant_option);
    assert!(after2.scopes.iter().all(|s| s.scope.as_ref() != Some(&tbl)), "table scope fully revoked");
    assert_eq!(find(&after2, &Scope::Database { db: format!("{db}_other") }).privileges, BTreeSet::from(["ALL PRIVILEGES".to_string()]));
    // ALL -> a subset
    let desired3 = vec![ScopeGrant { scope: Some(Scope::Database { db: format!("{db}_other") }), privileges: BTreeSet::from(["SELECT".into()]), ..Default::default() }];
    run_sql(&mut c, &grant_diff_sql(&user, "localhost", &after2.scopes, &desired3)).await.unwrap();
    let after3 = read_grants(&mut c, &user, "localhost").await.unwrap();
    assert_eq!(after3.scopes.len(), 1);
    assert_eq!(after3.scopes[0].privileges, BTreeSet::from(["SELECT".to_string()]));

    // lock + rename + password change, then drop
    let acct = list_accounts(&mut c).await.unwrap().into_iter().find(|a| a.user == user).unwrap();
    let renamed = AccountEdit { user: format!("{user}_renamed"), host: "localhost".into(), password: Some("new".into()), plugin: None, locked: true, expire_password: false };
    run_sql(&mut c, &account_sql(Some(&acct), &renamed).unwrap()).await.unwrap();
    let listed = list_accounts(&mut c).await.unwrap();
    assert!(listed.iter().all(|a| a.user != user));
    assert!(listed.iter().find(|a| a.user == format!("{user}_renamed")).unwrap().locked);
    run_sql(&mut c, &[drop_account_sql(&format!("{user}_renamed"), "localhost")]).await.unwrap();
    assert!(list_accounts(&mut c).await.unwrap().iter().all(|a| !a.user.starts_with(&user)));
}
