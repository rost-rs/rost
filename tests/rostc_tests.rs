use rost::rostc::{FORMAT_VERSION, RostcError};
use rost::Template;

const DOC: &str = "\
@version 2\n#window {\n    width: 800\n    title: \"Café \\\"x\\\"\"\n    tags [a, b, c]\n    mode | on, off |\n    #inner { depth: 2 }\n}\n";

#[test]
fn roundtrip_keeps_text_bytecode_and_queries() {
    let doc = Template::parse(DOC).unwrap();
    let loaded = Template::from_rostc(&doc.to_rostc()).unwrap();
    assert_eq!(loaded.as_bytes(), doc.as_bytes());
    assert_eq!(loaded.bytecode(), doc.bytecode());
    assert_eq!(loaded.query_i64("window.width"), Some(800));
    assert_eq!(loaded.query_string("window.title").as_deref(), Some("Café \"x\""));
    assert_eq!(loaded.query_list("window.tags").unwrap(), ["a", "b", "c"]);
    assert_eq!(loaded.query_i64("window.inner.depth"), Some(2));
}

#[test]
fn every_json_test_suite_document_survives_validation() {
    // Real-world bytecode (unicode, escapes, deep nesting) must pass the structural check
    // the loader applies to untrusted files.
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/JSONTestSuite/test_parsing");
    let mut seen = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let doc = Template::parse_lenient(&text);
        let loaded = Template::from_rostc(&doc.to_rostc())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(loaded.bytecode(), doc.bytecode());
        seen += 1;
    }
    assert!(seen > 250, "only {seen} files read");
}

#[test]
fn damaged_files_are_errors_never_panics() {
    let bytes = Template::parse(DOC).unwrap().to_rostc();
    for len in 0..bytes.len() {
        assert!(Template::from_rostc(&bytes[..len]).is_err(), "truncated to {len}");
    }
    for i in 0..bytes.len() {
        let mut bad = bytes.clone();
        bad[i] ^= 0x40;
        assert!(Template::from_rostc(&bad).is_err(), "flipped byte {i}");
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(Template::from_rostc(&trailing).is_err());
}

#[test]
fn header_errors_are_specific() {
    let bytes = Template::parse(DOC).unwrap().to_rostc();
    let mut magic = bytes.clone();
    magic[0] = b'X';
    assert_eq!(Template::from_rostc(&magic).unwrap_err(), RostcError::BadMagic);

    let mut newer = bytes.clone();
    newer[5] = FORMAT_VERSION + 1;
    assert_eq!(
        Template::from_rostc(&newer).unwrap_err(),
        RostcError::UnsupportedVersion(FORMAT_VERSION + 1)
    );

    let mut payload = bytes.clone();
    *payload.last_mut().unwrap() ^= 1;
    assert_eq!(Template::from_rostc(&payload).unwrap_err(), RostcError::BadChecksum);
    assert_eq!(Template::from_rostc(&[]).unwrap_err(), RostcError::Truncated);
}

#[test]
fn files_roundtrip_through_disk() {
    let path = std::env::temp_dir().join(format!("rost-test-{}.rostc", std::process::id()));
    Template::parse(DOC).unwrap().save_rostc(&path).unwrap();
    let loaded = Template::load_rostc(&path).unwrap();
    assert_eq!(loaded.query_i64("window.width"), Some(800));
    std::fs::write(&path, b"garbage garbage garbage garbage garbage").unwrap();
    let err = Template::load_rostc(&path).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    std::fs::remove_file(&path).ok();
}
