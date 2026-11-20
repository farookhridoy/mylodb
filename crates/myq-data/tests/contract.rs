//! The UI and the Rust side talk JSON. These payloads are the exact shapes the React grid sends
//! (captured from a real UI run), so a serde rename on either side breaks this test.

use myq_data::{Change, CellValue, ExportOptions, ExportFormat, Op, PageSpec};

#[test]
fn page_spec_from_the_grid() {
    let spec: PageSpec = serde_json::from_str(
        r#"{"filters":[{"column":"total","op":"gt","values":["1"]},{"column":"note","op":"contains","values":["x"]},
                       {"column":"note","op":"is_null","values":[]},{"column":"id","op":"not_contains","values":["9"]}],
            "where_sql":"total > 6","sort":[{"column":"total","desc":true},{"column":"id","desc":false}],"limit":500,"offset":0}"#,
    )
    .unwrap();
    assert_eq!(spec.filters.len(), 4);
    assert_eq!(spec.filters[0].op, Op::Gt);
    assert_eq!(spec.filters[2].op, Op::IsNull);
    assert_eq!(spec.where_sql.as_deref(), Some("total > 6"));
    assert!(spec.sort[0].desc && !spec.sort[1].desc);
    // where_sql may be null
    let spec: PageSpec = serde_json::from_str(r#"{"filters":[],"where_sql":null,"sort":[],"limit":100,"offset":200}"#).unwrap();
    assert_eq!((spec.where_sql, spec.offset), (None, 200));
}

#[test]
fn every_filter_operator_name_the_ui_can_send_parses() {
    for op in ["contains", "not_contains", "eq", "ne", "starts_with", "ends_with", "lt", "le", "gt", "ge", "between", "in", "regexp", "is_null", "not_null", "is_empty"] {
        serde_json::from_str::<Op>(&format!("\"{op}\"")).unwrap_or_else(|e| panic!("{op}: {e}"));
    }
}

#[test]
fn changes_from_the_grid() {
    let changes: Vec<Change> = serde_json::from_str(
        r#"[{"op":"update","key":[{"column":"id","value":{"kind":"text","value":"1"}}],
                "set":[{"column":"note","value":{"kind":"text","value":"edited note"}},
                       {"column":"a","value":{"kind":"null"}},{"column":"b","value":{"kind":"default"}},
                       {"column":"img","value":{"kind":"file","value":"/tmp/x.png"}}]},
            {"op":"delete","key":[{"column":"id","value":{"kind":"text","value":"3"}}]},
            {"op":"insert","values":[{"column":"customer_id","value":{"kind":"text","value":"2"}}]}]"#,
    )
    .unwrap();
    assert_eq!(changes.len(), 3);
    match &changes[0] {
        Change::Update { set, .. } => {
            assert_eq!(set[1].value, CellValue::Null);
            assert_eq!(set[2].value, CellValue::Default);
            assert_eq!(set[3].value, CellValue::File("/tmp/x.png".into()));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn export_options_from_the_grid() {
    for (name, want) in [("csv", ExportFormat::Csv), ("json", ExportFormat::Json), ("sql_insert", ExportFormat::SqlInsert)] {
        let o: ExportOptions = serde_json::from_str(&format!(r#"{{"format":"{name}","null_text":"NULL"}}"#)).unwrap();
        assert_eq!(o.format, want);
    }
}
