#![cfg(feature = "derive")]
//! `rost::load` (generated code) must agree with `rost::from_str` (serde) on every document.

use rost::Rost;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};

#[derive(Rost, Deserialize, Debug, PartialEq)]
struct Item {
    id: u32,
    name: String,
    price: f64,
    active: bool,
    color: String,
}

#[derive(Rost, Deserialize, Debug, PartialEq)]
struct Window {
    width: u32,
    height: Option<u16>,
    scale: f64,
    #[rost(rename = "title")]
    #[serde(rename = "title")]
    caption: String,
}

#[derive(Rost, Deserialize, Debug, PartialEq)]
struct Config {
    debug: bool,
    version: i64,
    window: Window,
    #[rost(serde)]
    tags: Vec<String>,
    #[rost(default)]
    #[serde(default)]
    retries: u8,
    note: Option<String>,
}

fn same<'a, T>(src: &'a str)
where
    T: rost::Load<'a> + Deserialize<'a> + std::fmt::Debug + PartialEq,
{
    let fast = rost::load::<T>(src);
    let slow = rost::from_str::<T>(src);
    match (&fast, &slow) {
        (Ok(a), Ok(b)) => assert_eq!(a, b, "{src:?}"),
        (Err(_), Err(_)) => {}
        _ => panic!("{src:?}: load = {fast:?}, from_str = {slow:?}"),
    }
}

const CONFIG: &str = "debug: true\nversion: -3\ntags [a, \"b c\", d]\n#window {\n  width: 1280\n  scale: 1.25\n  title: \"Hi \\\"x\\\"\"\n}\n";

#[test]
fn a_config_loads_like_serde_reads_it() {
    let config = rost::load::<Config>(CONFIG).unwrap();
    assert_eq!(config.window.caption, "Hi \"x\"");
    assert_eq!(config.tags, ["a", "b c", "d"]);
    assert_eq!((config.retries, config.note, config.window.height), (0, None, None));
    same::<Config>(CONFIG);
}

#[test]
fn every_shape_of_document_agrees_with_the_serde_reader() {
    let documents = [
        CONFIG,
        // Reordered keys, extra keys, comments, separators, optional values given.
        "// c\nnote: \"n\"\nextra: 1\n#window { scale: 2; width: 3, title: t, height: 4 }\nversion: 1 debug: false\ntags []\nretries: 7\n",
        // JSON spelling.
        r#"{"debug": false, "version": 2, "tags": ["x"], "window": {"width": 1, "scale": 0.5, "title": "jé"}}"#,
        // Quoted and escaped keys.
        "\"debug\": true\n\"version\": 1\ntags []\n\"window\" { \"width\": 1, scale: 1, \"title\": \"a\" }\n",
        // Numbers in odd spellings fall back to the serde reader.
        "debug: true\nversion: 1_000\ntags []\n#window { width: 0x10, scale: 1e3, title: x }\n",
        "debug: true\nversion: 99999999999999999999\ntags []\n#window { width: 1, scale: 1, title: x }\n",
        "debug: true\nversion: 1\ntags []\n#window { width: 70000, height: 70000, scale: 1, title: x }\n",
        // Nulls.
        "debug: true\nversion: 1\ntags []\nnote: null\n#window { width: 1, height: null, scale: 1, title: x }\n",
        // Errors: missing, duplicate, wrong type, block where a scalar belongs, unclosed.
        "debug: true\nversion: 1\ntags []\n",
        "debug: true\ndebug: false\nversion: 1\ntags []\n#window { width: 1, scale: 1, title: x }\n",
        "debug: maybe\nversion: 1\ntags []\n#window { width: 1, scale: 1, title: x }\n",
        "debug: true\nversion: 1\ntags []\nwindow: 5\n",
        "debug: true\nversion { a: 1 }\ntags []\n#window { width: 1, scale: 1, title: x }\n",
        "debug: true\nversion: 1\ntags []\n#window { width: 1, scale: 1, title: x\n",
        "#window { width: 1 } }",
        "",
    ];
    for src in documents {
        same::<Config>(src);
    }
}

#[test]
fn maps_and_nested_structs_load_like_serde() {
    let doc: String = (0..200)
        .map(|i| format!("#item_{i} {{\n    id: {i}\n    name: \"item {i}\"\n    price: {i}.5\n    active: {}\n    color: #1e1e2e\n}}\n", i % 2 == 0))
        .collect();
    let map = rost::load::<HashMap<String, Item>>(&doc).unwrap();
    assert_eq!(map.len(), 200);
    assert_eq!(map["item_7"], Item { id: 7, name: "item 7".into(), price: 7.5, active: false, color: "#1e1e2e".into() });
    assert_eq!(map, rost::from_str::<HashMap<String, Item>>(&doc).unwrap());
    let ordered = rost::load::<BTreeMap<String, Item>>(&doc).unwrap();
    assert_eq!(ordered.keys().next().map(String::as_str), Some("item_0"));

    let compact: String = (0..50)
        .map(|i| format!("#i{i} {{ id: {i}, name: \"n\", price: {i}.25, active: true, color: red }}\n"))
        .collect();
    same::<HashMap<String, Item>>(&compact);
    same::<HashMap<String, Item>>("#a { id: 1, name: x, price: 1, active: true }");
}

#[derive(Rost, Deserialize, Debug, PartialEq)]
struct Borrowed<'a> {
    name: &'a str,
    #[rost(serde)]
    pair: (u8, u8),
}

#[test]
fn borrowed_strings_and_the_serde_escape_hatch() {
    let src = "name: plain\npair [1, 2]\n";
    let doc = rost::load::<Borrowed<'_>>(src).unwrap();
    assert_eq!(doc.name, "plain");
    assert_eq!(doc.pair, (1, 2));
    // The borrowed string is a slice of the source.
    assert_eq!(doc.name.as_ptr() as usize - src.as_ptr() as usize, 6);
    same::<Borrowed<'_>>(src);
    same::<Borrowed<'_>>("name: \"esc\\n\"\npair [1, 2]\n");
}

#[test]
fn a_top_level_list_is_not_a_struct() {
    assert!(rost::load::<Config>("[1, 2]").is_err());
}

#[derive(Rost, Debug)]
struct Outer {
    name: String,
    window: Window,
}

#[test]
fn load_errors_carry_the_line_and_column_of_the_entry() {
    let src = "name: \"a\"\nwindow {\n  width: 1\n  height: nope\n  scale: 1.0\n  title: \"t\"\n}\n";
    let err = rost::load::<Outer>(src).unwrap_err();
    assert!(err.message().starts_with("4:3: "), "{}", err.message());

    let err = rost::load::<Window>("width: wide\nscale: 1.0\ntitle: \"t\"\n").unwrap_err();
    assert!(err.message().starts_with("1:1: "), "{}", err.message());
}
