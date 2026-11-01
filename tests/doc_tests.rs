#![cfg(feature = "serde")]

//! `Doc` must hold exactly what the owned readers produce.

use rost::doc::Doc;
use rost::Value;

fn y_files() -> Vec<(String, String)> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/JSONTestSuite/test_parsing");
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if let (true, Ok(text)) = (name.starts_with("y_"), std::fs::read_to_string(&path)) {
            files.push((name, text));
        }
    }
    files.sort();
    files
}

#[test]
fn json_docs_equal_the_owned_value_for_every_accepted_suite_file() {
    let files = y_files();
    assert!(files.len() > 90);
    for (name, text) in &files {
        let doc = Doc::from_json(text).unwrap_or_else(|e| panic!("{name}: {e}"));
        let owned = rost::json::from_str(text).unwrap();
        assert_eq!(doc.root().to_value(), owned, "{name}");
    }
}

#[test]
fn invalid_json_is_rejected_like_the_strict_reader() {
    for bad in ["", "{", "[1,]", "{\"a\" 1}", "01", "\"\\x\"", "[1] 2", "1e999"] {
        assert!(Doc::from_json(bad).is_err(), "{bad:?}");
        assert!(rost::json::validate(bad).is_err());
    }
}

#[test]
fn rost_docs_equal_from_str_value() {
    let sources = [
        "title: \"My App\"\nversion: 3\nratio: 1.5\nneg: -0\ndebug: false\nnothing: null\n#window {\n  width: 1280\n  tags [a, \"b c\", 3]\n  #inner { deep: true }\n}\n",
        "{\"a\": [1, 2, {\"b\": null}], \"c\": \"x\\ny\"}",
        "k: \"esc \\\" \\t \\u{e9}\"\nraw: \"\"\"two\nlines\"\"\"\nbig: 18446744073709551615\n",
        "",
        "// only a comment\n",
    ];
    for src in sources {
        let doc = Doc::from_rost(src).unwrap_or_else(|e| panic!("{src:?}: {e}"));
        let owned: Value = rost::from_str(src).unwrap();
        assert_eq!(doc.root().to_value(), owned, "{src:?}");
    }
    assert!(Doc::from_rost("a: {").is_err());
}

#[test]
fn navigation_reads_values_without_copying_them() {
    let text = r#"{"name": "rost", "n": 3, "pi": 1.5, "ok": true, "none": null,
        "list": [10, [20, 21], {"k": "v"}], "esc": "a\tb"}"#;
    let doc = Doc::from_json(text).unwrap();
    let root = doc.root();
    assert!(root.is_object() && root.len() == 7);
    let name = root.get("name").unwrap().as_str().unwrap();
    // Plain strings are slices of the source text itself.
    let start = name.as_ptr() as usize - text.as_ptr() as usize;
    assert_eq!(&text[start..start + 4], "rost");
    assert_eq!(root.get("n").unwrap().as_i64(), Some(3));
    assert_eq!(root.get("n").unwrap().as_f64(), Some(3.0));
    assert_eq!(root.get("pi").unwrap().as_f64(), Some(1.5));
    assert_eq!(root.get("pi").unwrap().as_i64(), None);
    assert_eq!(root.get("ok").unwrap().as_bool(), Some(true));
    assert!(root.get("none").unwrap().is_null());
    assert_eq!(root.get("esc").unwrap().as_str(), Some("a\tb"));
    let list = root.get("list").unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list.at(0).unwrap().as_i64(), Some(10));
    assert_eq!(list.at(1).unwrap().at(1).unwrap().as_i64(), Some(21));
    assert_eq!(list.at(2).unwrap().get("k").unwrap().as_str(), Some("v"));
    assert!(list.at(3).is_none());
    let keys: Vec<&str> = root.entries().map(|(k, _)| k).collect();
    assert_eq!(keys, ["name", "n", "pi", "ok", "none", "list", "esc"]);
}

#[test]
fn large_objects_are_indexed_and_keep_the_last_of_a_repeated_key() {
    let mut text = String::from("{");
    for i in 0..1000 {
        text.push_str(&format!("\"key{i}\": {i}, "));
    }
    text.push_str("\"key7\": \"again\", \"last\": 0}");
    let doc = Doc::from_json(&text).unwrap();
    let root = doc.root();
    for i in (0..1000).filter(|&i| i != 7) {
        assert_eq!(root.get(&format!("key{i}")).unwrap().as_i64(), Some(i), "key{i}");
    }
    assert_eq!(root.get("key7").unwrap().as_str(), Some("again"));
    assert!(root.get("nope").is_none());
    // The owned reader agrees.
    assert_eq!(doc.root().to_value(), rost::json::from_str(&text).unwrap());
    // The small-object path keeps the last duplicate too.
    let small = Doc::from_json(r#"{"a": 1, "a": 2}"#).unwrap();
    assert_eq!(small.root().get("a").unwrap().as_i64(), Some(2));
}
