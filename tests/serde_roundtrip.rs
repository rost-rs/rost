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

#[test]
fn serde_attributes_work_on_both_paths() {
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    #[serde(rename_all = "kebab-case")]
    struct Config {
        #[serde(rename = "app")]
        name: String,
        #[serde(default)]
        retries: u8,
        #[serde(default = "default_port")]
        port: u16,
        #[serde(skip_serializing_if = "Option::is_none")]
        proxy: Option<String>,
        max_conn: u32,
        #[serde(flatten)]
        extra: BTreeMap<String, rost::Value>,
        event: Event,
        payload: Payload,
        #[serde(skip)]
        cache: Vec<u8>,
    }

    fn default_port() -> u16 {
        8080
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    #[serde(tag = "type")]
    enum Event {
        Click { x: i32, y: i32 },
        Key { code: u32 },
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    #[serde(untagged)]
    enum Payload {
        Number(i64),
        Text(String),
    }

    let src = "app: \"svc\"\nmax-conn: 10\nevent { type: \"Key\", code: 13 }\npayload: \"hi\"\ncolor: \"red\"\nlevel: 3\n";
    let expected = Config {
        name: "svc".into(),
        retries: 0,
        port: 8080,
        proxy: None,
        max_conn: 10,
        extra: BTreeMap::from([("color".into(), rost::Value::from("red")), ("level".into(), rost::Value::from(3))]),
        event: Event::Key { code: 13 },
        payload: Payload::Text("hi".into()),
        cache: vec![],
    };
    assert_eq!(rost::from_str::<Config>(src).unwrap(), expected);
    assert_eq!(rost::from_template::<Config>(&rost::Template::parse(src).unwrap()).unwrap(), expected);

    let written = rost::to_string_pretty(&expected).unwrap();
    assert_eq!(
        written,
        "app: \"svc\"\nretries: 0\nport: 8080\nmax-conn: 10\ncolor: \"red\"\nlevel: 3\nevent {\n    type: \"Key\"\n    code: 13\n}\npayload: \"hi\"\n"
    );
    assert_eq!(rost::from_str::<Config>(&written).unwrap(), expected);

    let clicked = Config { event: Event::Click { x: -1, y: 2 }, payload: Payload::Number(5), ..expected };
    round_trip_owned(&clicked);
}

/// `round_trip` for types that are not `Clone`.
fn round_trip_owned<T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug>(value: &T) {
    round_trip(value);
}

/// xorshift64*: deterministic generator, so a failing case is reproducible from its seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn string(&mut self) -> String {
        const PIECES: &[&str] = &[
            "a", "Z", "0", " ", "\"", "\\", "\n", "\t", "\r", "]", "[", "{", "}", ",", ";", ":", "=", "|", "#", "@",
            "//", "\"\"\"", "é", "🦀", "\u{1}", "\u{7f}", "null", "true", "1.5", "-", "px",
        ];
        (0..self.below(6)).map(|_| PIECES[self.below(PIECES.len())]).collect()
    }

    fn key(&mut self) -> String {
        const CHARS: &[u8] = b"abcXYZ019_-.$%!?'";
        let len = 1 + self.below(6);
        let mut key: String = (0..len).map(|_| CHARS[self.below(CHARS.len())] as char).collect();
        if key.starts_with('/') {
            key.insert(0, 'k');
        }
        key
    }

    fn float(&mut self) -> f64 {
        match self.below(4) {
            0 => f64::from_bits(self.next()),
            1 => self.below(1000) as f64 / 8.0,
            2 => -(self.next() as f64),
            _ => [0.0, -0.0, f64::MIN_POSITIVE, f64::MAX, f64::EPSILON, 1e-300][self.below(6)],
        }
    }

    fn value(&mut self, depth: usize) -> rost::Value {
        use rost::Value;
        let leaf = depth == 0 || self.below(3) > 0;
        match self.below(if leaf { 5 } else { 7 }) {
            0 => Value::Null,
            1 => Value::Bool(self.below(2) == 1),
            2 => Value::Int(match self.below(3) {
                0 => self.next() as i64,
                1 => self.below(100) as i64 - 50,
                _ => [i64::MIN, i64::MAX, 0][self.below(3)],
            }),
            3 => {
                let f = self.float();
                Value::Float(if f.is_finite() { f } else { 0.25 })
            }
            4 => Value::String(self.string()),
            5 => Value::List((0..self.below(5)).map(|_| self.value(depth - 1)).collect()),
            _ => Value::Block(self.block(depth - 1)),
        }
    }

    fn block(&mut self, depth: usize) -> rost::Map {
        (0..self.below(6)).map(|_| (self.key(), self.value(depth))).collect()
    }
}

#[test]
fn generated_values_round_trip() {
    let mut rng = Rng(0x5E71_A11E_E000_0001);
    for case in 0..3_000 {
        let value = rost::Value::Block(rng.block(4));
        for text in [rost::to_string(&value).unwrap(), rost::to_string_pretty(&value).unwrap()] {
            let back: rost::Value = rost::from_str(&text).unwrap_or_else(|e| panic!("case {case}: {e}\n{text}"));
            assert_eq!(back, value, "case {case}: from_str of\n{text}");
            let template = rost::Template::parse(&text).unwrap_or_else(|e| panic!("case {case}: {e:?}\n{text}"));
            let back: rost::Value = rost::from_template(&template).unwrap_or_else(|e| panic!("case {case}: {e}\n{text}"));
            assert_eq!(back, value, "case {case}: from_template of\n{text}");
        }
    }
}

#[test]
fn generated_typed_documents_round_trip() {
    let mut rng = Rng(0x7EED_0000_0000_0042);
    for _ in 0..1_000 {
        let mut doc = sample();
        doc.title = rng.string();
        doc.version = rng.next() as u32;
        doc.ratio = rng.float() as f32;
        if !doc.ratio.is_finite() {
            doc.ratio = 1.5;
        }
        doc.offset = rng.next() as i64;
        doc.letter = char::from_u32(rng.below(0x11_0000) as u32).unwrap_or('x');
        doc.nothing = (rng.below(2) == 0).then(|| rng.string());
        doc.items = (0..rng.below(4))
            .map(|i| Item {
                id: rng.next(),
                name: rng.string(),
                price: {
                    let f = rng.float();
                    if f.is_finite() { f } else { 2.0 }
                },
                tags: (0..rng.below(3)).map(|_| rng.string()).collect(),
                shape: match i % 4 {
                    0 => Shape::Empty,
                    1 => Shape::Circle(rng.below(100) as f64),
                    2 => Shape::Rect(rng.next() as u32, rng.next() as u32),
                    _ => Shape::Poly { points: vec![(rng.next() as i32, -1)], closed: rng.below(2) == 0 },
                },
            })
            .collect();
        doc.env = (0..rng.below(4)).map(|_| (rng.key(), rng.string())).collect();
        round_trip(&doc);
    }
}

#[test]
fn numbers_are_written_exactly() {
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct N {
        f: f64,
        i: i64,
        u: u64,
    }
    let mut rng = Rng(0xF10A_7000_0000_0003);
    let check = |f: f64, i: i64, u: u64| {
        let text = rost::to_string(&N { f, i, u }).unwrap();
        let back: N = rost::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"));
        assert_eq!(back.f.to_bits(), f.to_bits(), "{text}");
        assert_eq!((back.i, back.u), (i, u), "{text}");
        // The fast path writes the same digits as `Debug` whenever `Debug` is positional.
        let debug = format!("{f:?}");
        if !debug.contains('e') {
            assert_eq!(text, format!("f: {debug}, i: {i}, u: {u}"));
        }
    };
    for f in [0.0, -0.0, 0.5, -0.5, 1.0, 10.0, 0.1, 0.05, 1234.5, 1e-6, 0.000123, 123456789012345.0, 999999999999999.9] {
        check(f, 0, 0);
    }
    check(1.5, i64::MIN, u64::MAX);
    check(2.5, i64::MAX, 0);
    for _ in 0..200_000 {
        let f = match rng.below(4) {
            0 => f64::from_bits(rng.next()),
            1 => (rng.next() % 10_000_000) as f64 / [1.0, 10.0, 100.0, 1000.0, 1e6][rng.below(5)],
            2 => rng.next() as f64 / (rng.next() | 1) as f64,
            _ => -((rng.next() % 1_000_000_000_000_000) as f64) / 1e7,
        };
        if f.is_finite() {
            check(f, rng.next() as i64, rng.next());
        }
    }
}
