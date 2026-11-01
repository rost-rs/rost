//! Serializer output and `T → .rost → T` round trips through both deserializers.
#![cfg(feature = "serde")]

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
enum Shape {
    Empty,
    Circle(f64),
    Rect(u32, u32),
    Poly { points: Vec<(i32, i32)>, closed: bool },
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct Item {
    id: u64,
    name: String,
    price: f64,
    tags: Vec<String>,
    shape: Shape,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct Doc {
    title: String,
    version: u32,
    debug: bool,
    ratio: f32,
    offset: i64,
    letter: char,
    nothing: Option<String>,
    something: Option<Vec<u8>>,
    unit: (),
    items: Vec<Item>,
    grid: Vec<Vec<i16>>,
    ports: BTreeMap<u16, String>,
    env: HashMap<String, String>,
    mode: Shape,
    pair: (String, bool),
}

fn sample() -> Doc {
    Doc {
        title: "quotes \" back\\slash ]bracket[ {brace} , ; : = | // not a comment".into(),
        version: 3,
        debug: false,
        ratio: 0.1,
        offset: i64::MIN,
        letter: '🦀',
        nothing: None,
        something: Some(vec![0, 255]),
        unit: (),
        items: vec![
            Item { id: 1, name: "line\nbreak\ttab\r\u{1}".into(), price: 1.0, tags: vec![], shape: Shape::Empty },
            Item {
                id: u64::MAX,
                name: "".into(),
                price: -0.0,
                tags: vec!["a, b".into(), "[c]".into()],
                shape: Shape::Poly { points: vec![(0, 0), (-1, 2)], closed: true },
            },
            Item { id: 2, name: "null".into(), price: 1e300, tags: vec!["true".into()], shape: Shape::Rect(3, 4) },
        ],
        grid: vec![vec![1, -2], vec![], vec![3]],
        ports: BTreeMap::from([(80, "http".into()), (443, "https".into())]),
        env: HashMap::from([("PATH".into(), "/bin".into())]),
        mode: Shape::Circle(2.5),
        pair: ("x".into(), true),
    }
}

fn round_trip<T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug>(value: &T) {
    for text in [rost::to_string(value).unwrap(), rost::to_string_pretty(value).unwrap()] {
        let back: T = rost::from_str(&text).unwrap_or_else(|e| panic!("from_str failed: {e}\n{text}"));
        assert_eq!(&back, value, "from_str round trip of\n{text}");
        let template = rost::Template::parse(&text).unwrap_or_else(|e| panic!("Template::parse failed: {e:?}\n{text}"));
        let back: T = rost::from_template(&template).unwrap_or_else(|e| panic!("from_template failed: {e}\n{text}"));
        assert_eq!(&back, value, "from_template round trip of\n{text}");
    }
}

#[test]
fn every_shape_round_trips_through_both_layouts() {
    round_trip(&sample());
    round_trip(&Shape::Rect(1, 2));
    round_trip(&Shape::Poly { points: vec![], closed: false });
    round_trip(&BTreeMap::from([("a".to_string(), vec![Shape::Empty, Shape::Circle(1.5)])]));
}

#[test]
fn compact_and_pretty_layouts() {
    #[derive(Serialize)]
    struct Window {
        title: &'static str,
        size: (u32, u32),
        mode: Shape,
        shapes: Vec<Shape>,
        inner: Inner,
        empty: Vec<u8>,
    }
    #[derive(Serialize)]
    struct Inner {
        a: Option<u8>,
    }
    let w = Window {
        title: "Main",
        size: (1280, 720),
        mode: Shape::Empty,
        shapes: vec![Shape::Circle(1.0), Shape::Empty],
        inner: Inner { a: None },
        empty: vec![],
    };
    assert_eq!(
        rost::to_string(&w).unwrap(),
        "title: \"Main\", size [1280, 720], mode: Empty, shapes [{ Circle: 1.0 }, Empty], inner { a: null }, empty []"
    );
    assert_eq!(
        rost::to_string_pretty(&w).unwrap(),
        "title: \"Main\"\nsize [1280, 720]\nmode: Empty\nshapes [\n    {\n        Circle: 1.0\n    },\n    Empty\n]\ninner {\n    a: null\n}\nempty []\n"
    );
    assert_eq!(rost::to_string_pretty(&Shape::Rect(1, 2)).unwrap(), "Rect [1, 2]\n");
    assert_eq!(String::from_utf8(rost::to_vec(&Shape::Circle(0.5)).unwrap()).unwrap(), "Circle: 0.5");

    let mut buf = Vec::new();
    rost::to_writer_pretty(&mut buf, &BTreeMap::from([(1u8, true)])).unwrap();
    assert_eq!(buf, b"1: true\n");
}

#[test]
fn unrepresentable_values_are_errors() {
    let err = |r: Result<String, rost::Error>| r.unwrap_err().to_string();
    assert!(err(rost::to_string(&42)).contains("document root"));
    assert!(err(rost::to_string(&vec![1])).contains("not a list"));
    assert!(err(rost::to_string(&Shape::Empty)).contains("document root"));
    assert!(err(rost::to_string(&HashMap::from([("two words", 1)]))).contains("`two words` cannot be written as a .rost key"));
    assert!(err(rost::to_string(&HashMap::from([("#x", 1)]))).contains("cannot be written"));
    assert!(err(rost::to_string(&HashMap::from([("v", f64::NAN)]))).contains("non-finite"));
    assert!(err(rost::to_string(&HashMap::from([(vec![1], 1)]))).contains("map keys must be"));
}

#[test]
fn value_tree_api() {
    use rost::{Map, Value};

    let src = "title: \"demo\"\ncount: 3\nratio: 0.5\nok: true\nnone: null\ncolor: #ff0000\nsize: 140px\ntags [a, \"b\"]\n#window {\n    width: 1280\n    inner { deep: 1 }\n}\nshape { Circle: 2.0 }\n";
    let v: Value = rost::from_str(src).unwrap();
    assert_eq!(v, rost::from_template::<Value>(&rost::Template::parse(src).unwrap()).unwrap());

    assert_eq!(v["title"], "demo");
    assert_eq!(v["count"], 3);
    assert_eq!(v["ratio"], 0.5);
    assert_eq!(v["ok"], true);
    assert!(v["none"].is_null());
    assert_eq!(v["color"].as_i64(), Some(0xff0000));
    assert_eq!(v["size"], "140px");
    assert_eq!(v["tags"][1], "b");
    assert!(v["tags"][9].is_null());
    assert_eq!(v["window"]["inner"]["deep"].as_u64(), Some(1));
    assert!(v["window"]["missing"]["deeper"].is_null());
    assert_eq!(v.as_block().unwrap().keys().next().map(String::as_str), Some("title"));

    // Order is kept when written back.
    let written = rost::to_string_pretty(&v).unwrap();
    assert!(written.starts_with("title: \"demo\"\ncount: 3\nratio: 0.5\nok: true\nnone: null\n"), "{written}");
    assert_eq!(rost::from_str::<Value>(&written).unwrap(), v);

    // Typed extraction, including enums.
    #[derive(Deserialize, Debug, PartialEq)]
    struct Window {
        width: u32,
        inner: BTreeMap<String, u8>,
    }
    let w: Window = rost::from_value(v["window"].clone()).unwrap();
    assert_eq!(w, Window { width: 1280, inner: BTreeMap::from([("deep".into(), 1)]) });
    assert_eq!(rost::from_value::<Shape>(v["shape"].clone()).unwrap(), Shape::Circle(2.0));
    assert_eq!(rost::from_value::<Shape>(Value::from("Empty")).unwrap(), Shape::Empty);
    assert_eq!(rost::from_value::<Option<u8>>(Value::Null).unwrap(), None);

    // Mutation through IndexMut builds blocks on demand.
    let mut m = Value::Null;
    m["a"]["b"] = Value::from(vec![1, 2]);
    m["a"]["b"][0] = Value::from("x");
    m["n"] = Value::from(u64::MAX);
    assert_eq!(m.to_string(), "a { b [\"x\", 2] }, n: 1.8446744073709552e19");
    assert_eq!(format!("{m:#}"), "a {\n    b [\"x\", 2]\n}\nn: 1.8446744073709552e19\n");
    // A block displays as a document of its own.
    assert_eq!(m["a"].to_string(), "b [\"x\", 2]");

    // Large maps switch to a hash index and keep insertion order and replacement semantics.
    let mut big: Map = (0..100).map(|i| (format!("k{i}"), i)).collect();
    assert_eq!(big.insert("k50", Value::from("new")), Some(Value::from(50)));
    assert_eq!(big.remove("k0"), Some(Value::from(0)));
    assert_eq!(big.len(), 99);
    assert_eq!(big["k50"], "new");
    assert_eq!(big.keys().next().map(String::as_str), Some("k1"));
    assert_eq!(big["k99"], 99);
    assert!(!big.contains_key("k0"));
}

#[test]
#[should_panic(expected = "cannot index an integer with a key")]
fn value_index_mut_on_scalar_panics() {
    let mut v = rost::Value::from(1);
    v["a"] = rost::Value::Null;
}

#[test]
fn rost_macro_builds_values() {
    use rost::{rost, Value};

    let n = 7;
    let v = rost!({
        a: 1,
        "b-c": [1, -2, 3.5, "x", null, [true], { k: n * 2 }],
        nested: { deep: { deeper: "yes" } },
        empty_list: [],
        empty_block: {},
        computed: format!("{n}!"),
        opt: Some(1u8),
    });
    assert_eq!(v["a"], 1);
    assert_eq!(v["b-c"][1], -2);
    assert_eq!(v["b-c"][2], 3.5);
    assert!(v["b-c"][4].is_null());
    assert_eq!(v["b-c"][5][0], true);
    assert_eq!(v["b-c"][6]["k"], 14);
    assert_eq!(v["nested"]["deep"]["deeper"], "yes");
    assert_eq!(v["empty_list"], Value::List(vec![]));
    assert_eq!(v["empty_block"], Value::Block(rost::Map::new()));
    assert_eq!(v["computed"], "7!");
    assert_eq!(v["opt"], 1);
    assert_eq!(rost!(null), Value::Null);
    assert_eq!(rost!([1, 2]), Value::from(vec![1, 2]));

    // The literal agrees with the text form.
    let text = "a: 1\nb-c [1, -2, 3.5, \"x\", null, [true], { k: 14 }]\nnested { deep { deeper: \"yes\" } }\nempty_list []\nempty_block {}\ncomputed: \"7!\"\nopt: 1\n";
    assert_eq!(rost::from_str::<Value>(text).unwrap(), v);
    assert_eq!(rost::from_str::<Value>(&rost::to_string_pretty(&v).unwrap()).unwrap(), v);
}
