//! Strict and lenient parsing must never panic and must agree with each other.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rost::Template;

fuzz_target!(|src: &str| {
    let (bytecode, errors) = rost::parser::parse_with_diagnostics(src.as_bytes());
    assert_eq!(bytecode, rost::parser::parse_into_bytecode(src.as_bytes()));
    assert_eq!(Template::parse(src).is_ok(), errors.is_empty());
    for e in &errors {
        let _ = e.render(src);
    }

    let t = Template::from_str(src);
    for field in t.iter() {
        let _ = (field.scalar(), field.string_value(), field.list_items(), field.enum_variants());
        let _ = (t.query_scalar(field.name()), t.query_hex(field.name()));
    }
    for block in t.blocks() {
        let _ = t.find_block(block.name);
    }
});
