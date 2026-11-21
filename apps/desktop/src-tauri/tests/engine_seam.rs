//! Phase 15 guard (docs/ENGINE_ABSTRACTION.md, step 1): the application layer talks to the engine only through the `myq-*` crates.
//! If this fails, move the new query into the crate that owns the feature instead of importing `mysql_async` here.

#[test]
fn the_tauri_app_does_not_import_the_mysql_driver() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for entry in std::fs::read_dir(src).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(!text.contains("mysql_async"), "{} mentions mysql_async", path.display());
        }
    }
}
