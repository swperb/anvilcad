//! Damaged and hand-edited part files must load as an error or as a usable part, never
//! panic. Each case starts from a valid saved part and breaks one thing.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use anvil_model::{Document, FileError};
use serde_json::{json, Value};

fn saved() -> Value {
    serde_json::from_str(&Document::new().to_json().unwrap()).unwrap()
}

fn load(v: &Value) -> Result<Document, FileError> {
    Document::from_json(&v.to_string())
}

/// Loads, rebuilds, walks the configuration tree and saves again: the paths the app runs
/// right after opening a file.
fn exercise(mut doc: Document) -> Document {
    let _ = doc.rebuild();
    let _ = doc.configuration_tree();
    let _ = doc.active_configuration_name();
    let back = Document::from_json(&doc.to_json().unwrap()).unwrap();
    assert_eq!(back.configurations.len(), doc.configurations.len());
    doc
}

#[test]
fn truncated_and_non_json_files_are_errors() {
    let text = Document::new().to_json().unwrap();
    for cut in [0, 1, text.len() / 2, text.len() - 1] {
        assert!(matches!(Document::from_json(&text[..cut]), Err(FileError::Json(_))), "cut at {cut}");
    }
    assert!(matches!(Document::from_json("\u{0}\u{1}binary"), Err(FileError::Json(_))));
    assert!(matches!(Document::from_json("[1, 2, 3]"), Err(FileError::WrongFormat(_))));
}

#[test]
fn wrong_types_are_errors() {
    let mut v = saved();
    v["features"] = json!("not a list");
    assert!(matches!(load(&v), Err(FileError::Json(_))));

    let mut v = saved();
    v["features"] = json!([{ "id": "one", "name": 5 }]);
    assert!(matches!(load(&v), Err(FileError::Json(_))));
}

#[test]
fn newer_format_is_refused() {
    let mut v = saved();
    v["format_version"] = json!(u32::MAX);
    assert!(matches!(load(&v), Err(FileError::TooNew(u32::MAX))));
}

#[test]
fn empty_configuration_table_gets_a_default() {
    let mut v = saved();
    v["configurations"] = json!([]);
    v["active_configuration"] = json!(77);
    let doc = exercise(load(&v).unwrap());
    assert_eq!(doc.configurations.len(), 1);
    assert_eq!(doc.active_configuration, doc.configurations[0].id);
}

#[test]
fn dangling_active_configuration_falls_back() {
    let mut v = saved();
    let first = v["configurations"][0]["id"].as_u64().unwrap();
    v["active_configuration"] = json!(first + 1000);
    let doc = exercise(load(&v).unwrap());
    assert_eq!(u64::from(doc.active_configuration), first);
}

#[test]
fn configuration_parent_cycle_loads() {
    let mut v = saved();
    let mut a = v["configurations"][0].clone();
    a["id"] = json!(10);
    a["name"] = json!("A");
    a["parent"] = json!(11);
    let mut b = a.clone();
    b["id"] = json!(11);
    b["name"] = json!("B");
    b["parent"] = json!(10);
    v["configurations"].as_array_mut().unwrap().extend([a, b]);
    let doc = exercise(load(&v).unwrap());
    assert_eq!(doc.configurations.len(), 3);
}
