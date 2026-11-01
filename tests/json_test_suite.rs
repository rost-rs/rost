//! Runs JSONTestSuite's `test_parsing` against the strict JSON mode, and checks that the
//! rost grammar reads every valid JSON object to the same tree.

use std::fs;
use std::path::PathBuf;

fn cases() -> Vec<(String, Vec<u8>)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/JSONTestSuite/test_parsing");
    let mut cases: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let path = e.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (name, fs::read(&path).unwrap())
        })
        .collect();
    cases.sort();
    assert!(cases.len() > 300);
    cases
}

fn strict(bytes: &[u8]) -> Result<(), String> {
    let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    rost::json::validate(text).map_err(|e| e.to_string())
}

#[test]
fn accepts_every_y_and_rejects_every_n() {
    let mut wrong = Vec::new();
    for (name, bytes) in cases() {
        let result = std::panic::catch_unwind(|| strict(&bytes));
        match (name.as_bytes()[0], result) {
            (_, Err(_)) => wrong.push(format!("{name}: panicked")),
            (b'y', Ok(Err(e))) => wrong.push(format!("{name}: rejected ({e})")),
            (b'n', Ok(Ok(()))) => wrong.push(format!("{name}: accepted")),
            _ => {}
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[cfg(feature = "serde")]
#[test]
fn builds_the_same_tree_it_validates() {
    for (name, bytes) in cases() {
        assert_eq!(
            rost::json::from_slice(&bytes).is_ok(),
            strict(&bytes).is_ok(),
            "{name}"
        );
    }
}

#[cfg(feature = "serde")]
#[test]
fn valid_json_objects_read_the_same_as_rost() {
    let mut wrong = Vec::new();
    let mut objects = 0;
    for (name, bytes) in cases() {
        if !name.starts_with('y') {
            continue;
        }
        let text = std::str::from_utf8(&bytes).unwrap();
        // Values that are not objects are checked as the field of one.
        let wrapped;
        let text = if text.trim_start().starts_with('{') {
            text
        } else {
            wrapped = format!("{{\"v\": {text}}}");
            &wrapped
        };
        objects += 1;
        let json = rost::json::from_str(text).unwrap();
        match rost::from_str::<rost::Value>(text) {
            Ok(v) if v == json => {}
            other => wrong.push(format!("{name}: from_str gave {other:?}, JSON is {json:?}")),
        }
        let template = rost::Template::from_json(text).unwrap();
        match rost::from_template::<rost::Value>(&template) {
            Ok(v) if v == json => {}
            other => wrong.push(format!("{name}: from_template gave {other:?}, JSON is {json:?}")),
        }
    }
    assert!(objects > 90);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
