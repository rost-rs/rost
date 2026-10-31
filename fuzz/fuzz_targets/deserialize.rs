//! Serde deserialization of arbitrary documents must never panic.
#![no_main]

use std::collections::HashMap;

use libfuzzer_sys::fuzz_target;
use rost::Template;

#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
struct Typed {
    name: Option<String>,
    count: Option<i64>,
    ratio: Option<f64>,
    tags: Option<Vec<String>>,
    nested: Option<HashMap<String, String>>,
}

fuzz_target!(|src: &str| {
    let t = Template::from_str(src);
    let _ = rost::from_template::<Typed>(&t);
    let _ = rost::from_template::<HashMap<String, String>>(&t);
    for block in t.blocks() {
        let _ = rost::from_block::<Typed>(&block);
    }
});
