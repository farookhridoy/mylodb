//! JSON shapes exchanged with the designer and users UI. A serde rename on either side breaks this.

use myq_admin::{Account, AccountEdit, Scope, ScopeGrant};
use myq_schema::ddl::*;

#[test]
fn table_def_as_the_designer_sends_it() {
    let t: TableDef = serde_json::from_str(r#"{
        "name":"items","engine":"InnoDB","charset":"utf8mb4","collation":null,"comment":"",
        "columns":[
          {"name":"id","original_name":null,"data_type":"int","params":null,"unsigned":true,"zerofill":false,"nullable":false,
           "default":{"kind":"none"},"auto_increment":true,"charset":null,"collation":null,"comment":"","on_update":null,"generated":null,"invisible":false},
          {"name":"n","original_name":"n","data_type":"varchar","params":"255","unsigned":false,"zerofill":false,"nullable":true,
           "default":{"kind":"literal","value":"x"},"auto_increment":false,"charset":null,"collation":null,"comment":"c","on_update":null,"generated":null,"invisible":false},
          {"name":"d","original_name":null,"data_type":"datetime","params":"3","unsigned":false,"zerofill":false,"nullable":false,
           "default":{"kind":"expression","value":"CURRENT_TIMESTAMP(3)"},"auto_increment":false,"charset":null,"collation":null,"comment":"",
           "on_update":"CURRENT_TIMESTAMP(3)","generated":null,"invisible":false},
          {"name":"g","original_name":null,"data_type":"int","params":null,"unsigned":false,"zerofill":false,"nullable":true,
           "default":{"kind":"null"},"auto_increment":false,"charset":null,"collation":null,"comment":"","on_update":null,
           "generated":{"expression":"id * 2","stored":true},"invisible":false}],
        "indexes":[{"name":"PRIMARY","original_name":null,"kind":"primary","columns":[{"name":"id","length":null,"desc":false}]},
                   {"name":"ft","original_name":null,"kind":"fulltext","columns":[{"name":"n","length":10,"desc":true}]}],
        "foreign_keys":[{"name":"fk","original_name":null,"columns":["id"],"ref_schema":null,"ref_table":"p","ref_columns":["id"],
                         "on_delete":"set_null","on_update":"no_action"}]}"#).unwrap();
    assert_eq!(t.columns[0].default, ColumnDefault::None);
    assert_eq!(t.columns[1].default, ColumnDefault::Literal("x".into()));
    assert_eq!(t.columns[3].generated.as_ref().unwrap().stored, true);
    assert_eq!(t.indexes[1].kind, IndexKind::Fulltext);
    assert_eq!((t.foreign_keys[0].on_delete, t.foreign_keys[0].on_update), (RefAction::SetNull, RefAction::NoAction));
    // and it produces SQL, i.e. the UI payload is usable end to end
    let sql = create_table_sql(Some("d"), &t).unwrap();
    assert!(sql.contains("ON DELETE SET NULL ON UPDATE NO ACTION"), "{sql}");

    // what the backend sends back deserializes into the same TypeScript-facing shape
    let v = serde_json::to_value(&t).unwrap();
    assert_eq!(v["columns"][1]["default"], serde_json::json!({"kind":"literal","value":"x"}));
    assert_eq!(v["columns"][0]["default"], serde_json::json!({"kind":"none"}));
    assert_eq!(v["foreign_keys"][0]["on_delete"], "set_null");
}

#[test]
fn scope_grants_use_a_flat_level_tag() {
    let g: Vec<ScopeGrant> = serde_json::from_str(r#"[
        {"level":"global","privileges":["SELECT"],"columns":{},"grant_option":false},
        {"level":"database","db":"shop","privileges":["ALL PRIVILEGES"],"columns":{},"grant_option":true},
        {"level":"table","db":"shop","table":"orders","privileges":[],"columns":{"UPDATE":["a","b"]},"grant_option":false}]"#).unwrap();
    assert_eq!(g[0].scope, Some(Scope::Global));
    assert_eq!(g[1].scope, Some(Scope::Database { db: "shop".into() }));
    assert!(g[1].grant_option);
    assert_eq!(g[2].scope, Some(Scope::Table { db: "shop".into(), table: "orders".into() }));
    assert_eq!(g[2].columns["UPDATE"].len(), 2);
    let back = serde_json::to_value(&g[1]).unwrap();
    assert_eq!(back["level"], "database");
    assert_eq!(back["db"], "shop");
}

#[test]
fn account_payloads() {
    let a: Account = serde_json::from_str(r#"{"user":"u","host":"h","locked":true,"password_expired":false,"plugin":"x"}"#).unwrap();
    assert!(a.locked);
    let e: AccountEdit = serde_json::from_str(r#"{"user":"u","host":"h","password":null,"plugin":null,"locked":false,"expire_password":true}"#).unwrap();
    assert!(e.expire_password && e.password.is_none());
}
