//! Reproducible comparison of `rost` against `serde_json`, `sonic-rs`, `simd-json` and `toml`
//! on equivalent data.
//!
//! `simd-json` parses in place, so its timings include copying the input into a fresh
//! buffer, as a caller holding a `&str` would have to.
//!
//! ```text
//! cargo bench --features serde --bench compare
//! ROST_BENCH_ITEMS=200000 cargo bench --features serde --bench compare
//! ```
//!
//! Every figure is the median of several timed runs after a warm-up. Results are printed as
//! a Markdown table so they can be pasted into the README.

use std::collections::HashMap;
use std::hint::black_box;
use std::time::{Duration, Instant};

use rost::Template;

#[derive(serde::Deserialize, serde::Serialize)]
#[allow(dead_code)]
struct Item {
    id: u32,
    name: String,
    price: f64,
    active: bool,
    color: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[allow(dead_code)]
struct Config {
    title: String,
    version: u32,
    debug: bool,
    window: Window,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[allow(dead_code)]
struct Window {
    width: u32,
    height: u32,
    scale: f64,
    theme: String,
}

const ROST_CONFIG: &str = "title: \"My App\"\nversion: 3\ndebug: false\n#window {\n    width: 1280\n    height: 720\n    scale: 1.25\n    theme: \"dark\"\n}\n";
const JSON_CONFIG: &str = r#"{"title": "My App", "version": 3, "debug": false, "window": {"width": 1280, "height": 720, "scale": 1.25, "theme": "dark"}}"#;
const JSON_CONFIG_PRETTY: &str = "{\n    \"title\": \"My App\",\n    \"version\": 3,\n    \"debug\": false,\n    \"window\": {\n        \"width\": 1280,\n        \"height\": 720,\n        \"scale\": 1.25,\n        \"theme\": \"dark\"\n    }\n}\n";
const TOML_CONFIG: &str = "title = \"My App\"\nversion = 3\ndebug = false\n[window]\nwidth = 1280\nheight = 720\nscale = 1.25\ntheme = \"dark\"\n";

fn rost_doc(n: usize) -> String {
    (0..n)
        .map(|i| {
            format!(
                "#item_{i} {{\n    id: {i}\n    name: \"item {i}\"\n    price: {i}.5\n    active: true\n    color: #1e1e2e\n}}\n"
            )
        })
        .collect()
}

fn json_doc(n: usize) -> String {
    let body: Vec<String> = (0..n)
        .map(|i| {
            format!(
                "\n  \"item_{i}\": {{\"id\": {i}, \"name\": \"item {i}\", \"price\": {i}.5, \"active\": true, \"color\": \"#1e1e2e\"}}"
            )
        })
        .collect();
    format!("{{{}\n}}", body.join(","))
}

/// JSON with the same layout as the rost document: one field per line, indented.
fn json_pretty_doc(n: usize) -> String {
    let body: Vec<String> = (0..n)
        .map(|i| {
            format!(
                "\n    \"item_{i}\": {{\n        \"id\": {i},\n        \"name\": \"item {i}\",\n        \"price\": {i}.5,\n        \"active\": true,\n        \"color\": \"#1e1e2e\"\n    }}"
            )
        })
        .collect();
    format!("{{{}\n}}", body.join(","))
}

fn toml_doc(n: usize) -> String {
    (0..n)
        .map(|i| {
            format!(
                "[item_{i}]\nid = {i}\nname = \"item {i}\"\nprice = {i}.5\nactive = true\ncolor = \"#1e1e2e\"\n\n"
            )
        })
        .collect()
}

/// Median wall time of `runs` executions of `f`, after one warm-up run.
fn median(runs: usize, mut f: impl FnMut()) -> Duration {
    f();
    let mut times: Vec<Duration> = (0..runs)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed()
        })
        .collect();
    times.sort();
    times[runs / 2]
}

fn row(workload: &str, contender: &str, value: String) {
    println!("| {workload} | {contender} | {value} |");
}

fn throughput(bytes: usize, d: Duration) -> String {
    format!("{:.2} ms ({:.0} MB/s)", d.as_secs_f64() * 1e3, bytes as f64 / d.as_secs_f64() / 1e6)
}

fn per_op(d: Duration, ops: usize) -> String {
    let ns = d.as_secs_f64() * 1e9 / ops as f64;
    if ns >= 1000.0 { format!("{:.2} µs", ns / 1000.0) } else { format!("{ns:.0} ns") }
}

fn main() {
    let n: usize = std::env::var("ROST_BENCH_ITEMS").ok().and_then(|v| v.parse().ok()).unwrap_or(50_000);
    let rost_src = rost_doc(n);
    let json_src = json_doc(n);
    let json_pretty = json_pretty_doc(n);
    let toml_src = toml_doc(n);
    println!(
        "Documents: {n} records — rost {} B, JSON {} B (pretty {} B), TOML {} B\n",
        rost_src.len(),
        json_src.len(),
        json_pretty.len(),
        toml_src.len()
    );
    println!("| Workload | Contender | Median |");
    println!("|---|---|---|");

    let big = "Large document → `HashMap<String, Item>`";
    let d = median(9, || {
        black_box(rost::from_str::<HashMap<String, Item>>(black_box(&rost_src)).unwrap());
    });
    row(big, "**rost::from_str** (single pass)", throughput(rost_src.len(), d));
    let d = median(9, || {
        let t = Template::from_str(black_box(&rost_src));
        black_box(rost::from_template::<HashMap<String, Item>>(&t).unwrap());
    });
    row(big, "rost (Template + from_template)", throughput(rost_src.len(), d));
    let d = median(9, || {
        black_box(serde_json::from_str::<HashMap<String, Item>>(black_box(&json_src)).unwrap());
    });
    row(big, "serde_json (one record per line)", throughput(json_src.len(), d));
    let d = median(9, || {
        black_box(serde_json::from_str::<HashMap<String, Item>>(black_box(&json_pretty)).unwrap());
    });
    row(big, "serde_json (pretty, same layout as rost)", throughput(json_pretty.len(), d));
    let d = median(9, || {
        black_box(rost::from_str::<HashMap<String, Item>>(black_box(&json_src)).unwrap());
    });
    row(big, "**rost::from_str** on the JSON text", throughput(json_src.len(), d));
    let d = median(9, || {
        black_box(sonic_rs::from_str::<HashMap<String, Item>>(black_box(&json_src)).unwrap());
    });
    row(big, "sonic-rs (one record per line)", throughput(json_src.len(), d));
    let d = median(9, || {
        let mut buf = black_box(&json_src).as_bytes().to_vec();
        black_box(simd_json::serde::from_slice::<HashMap<String, Item>>(&mut buf).unwrap());
    });
    row(big, "simd-json (one record per line)", throughput(json_src.len(), d));
    let d = median(5, || {
        black_box(toml::from_str::<HashMap<String, Item>>(black_box(&toml_src)).unwrap());
    });
    row(big, "toml", throughput(toml_src.len(), d));

    let small = "Small config → struct (per load)";
    const LOADS: usize = 1000;
    let d = median(15, || {
        for _ in 0..LOADS {
            black_box(rost::from_str::<Config>(black_box(ROST_CONFIG)).unwrap());
        }
    });
    row(small, "**rost::from_str** (single pass)", per_op(d, LOADS));
    let d = median(15, || {
        for _ in 0..LOADS {
            let t = Template::from_str(black_box(ROST_CONFIG));
            black_box(rost::from_template::<Config>(&t).unwrap());
        }
    });
    row(small, "rost (Template + from_template)", per_op(d, LOADS));
    let d = median(15, || {
        for _ in 0..LOADS {
            black_box(serde_json::from_str::<Config>(black_box(JSON_CONFIG)).unwrap());
        }
    });
    row(small, "serde_json (compact)", per_op(d, LOADS));
    let d = median(15, || {
        for _ in 0..LOADS {
            black_box(serde_json::from_str::<Config>(black_box(JSON_CONFIG_PRETTY)).unwrap());
        }
    });
    row(small, "serde_json (pretty)", per_op(d, LOADS));
    let d = median(15, || {
        for _ in 0..LOADS {
            black_box(sonic_rs::from_str::<Config>(black_box(JSON_CONFIG)).unwrap());
        }
    });
    row(small, "sonic-rs (compact)", per_op(d, LOADS));
    let d = median(15, || {
        for _ in 0..LOADS {
            let mut buf = black_box(JSON_CONFIG).as_bytes().to_vec();
            black_box(simd_json::serde::from_slice::<Config>(&mut buf).unwrap());
        }
    });
    row(small, "simd-json (compact)", per_op(d, LOADS));
    let d = median(15, || {
        for _ in 0..LOADS {
            black_box(toml::from_str::<Config>(black_box(TOML_CONFIG)).unwrap());
        }
    });
    row(small, "toml", per_op(d, LOADS));

    let tree = "Large document → dynamic tree (`Value`)";
    let d = median(9, || {
        black_box(rost::from_str::<rost::Value>(black_box(&rost_src)).unwrap());
    });
    row(tree, "**rost::Value**", throughput(rost_src.len(), d));
    let d = median(9, || {
        black_box(serde_json::from_str::<serde_json::Value>(black_box(&json_src)).unwrap());
    });
    row(tree, "serde_json::Value (one record per line)", throughput(json_src.len(), d));
    let d = median(9, || {
        black_box(serde_json::from_str::<serde_json::Value>(black_box(&json_pretty)).unwrap());
    });
    row(tree, "serde_json::Value (pretty, same layout as rost)", throughput(json_pretty.len(), d));
    let d = median(9, || {
        black_box(rost::json::from_str(black_box(&json_src)).unwrap());
    });
    row(tree, "**rost::json::from_str** (strict JSON)", throughput(json_src.len(), d));
    let d = median(9, || {
        black_box(sonic_rs::from_str::<sonic_rs::Value>(black_box(&json_src)).unwrap());
    });
    row(tree, "sonic_rs::Value", throughput(json_src.len(), d));
    let d = median(9, || {
        let mut buf = black_box(&json_src).as_bytes().to_vec();
        black_box(simd_json::to_owned_value(&mut buf).unwrap());
    });
    row(tree, "simd_json::OwnedValue", throughput(json_src.len(), d));
    let d = median(5, || {
        black_box(toml::from_str::<toml::Table>(black_box(&toml_src)).unwrap());
    });
    row(tree, "toml::Table", throughput(toml_src.len(), d));

    let write = "Serialize `HashMap<String, Item>`";
    let items: HashMap<String, Item> = serde_json::from_str(&json_src).unwrap();
    let d = median(9, || {
        black_box(rost::to_string(black_box(&items)).unwrap());
    });
    row(write, "**rost::to_string**", throughput(rost::to_string(&items).unwrap().len(), d));
    let d = median(9, || {
        black_box(rost::to_string_pretty(black_box(&items)).unwrap());
    });
    row(write, "**rost::to_string_pretty**", throughput(rost::to_string_pretty(&items).unwrap().len(), d));
    let d = median(9, || {
        black_box(serde_json::to_string(black_box(&items)).unwrap());
    });
    row(write, "serde_json::to_string", throughput(serde_json::to_string(&items).unwrap().len(), d));
    let d = median(9, || {
        black_box(serde_json::to_string_pretty(black_box(&items)).unwrap());
    });
    row(write, "serde_json::to_string_pretty", throughput(serde_json::to_string_pretty(&items).unwrap().len(), d));
    let d = median(5, || {
        black_box(toml::to_string(black_box(&items)).unwrap());
    });
    row(write, "toml::to_string", throughput(toml::to_string(&items).unwrap().len(), d));

    let write_small = "Serialize small config (per call)";
    let config: Config = serde_json::from_str(JSON_CONFIG).unwrap();
    let d = median(15, || {
        for _ in 0..LOADS {
            black_box(rost::to_string_pretty(black_box(&config)).unwrap());
        }
    });
    row(write_small, "**rost::to_string_pretty**", per_op(d, LOADS));
    let d = median(15, || {
        for _ in 0..LOADS {
            black_box(serde_json::to_string_pretty(black_box(&config)).unwrap());
        }
    });
    row(write_small, "serde_json::to_string_pretty", per_op(d, LOADS));
    let d = median(15, || {
        for _ in 0..LOADS {
            black_box(toml::to_string(black_box(&config)).unwrap());
        }
    });
    row(write_small, "toml::to_string", per_op(d, LOADS));

    let scan = "Scan / index only";
    let d = median(15, || {
        black_box(rost::parser::parse_into_bytecode(black_box(rost_src.as_bytes())));
    });
    row(scan, "rost lenient index", throughput(rost_src.len(), d));
    let d = median(15, || {
        black_box(Template::parse(black_box(&rost_src)).unwrap());
    });
    row(scan, "rost strict parse (validated)", throughput(rost_src.len(), d));
    let d = median(15, || {
        black_box(serde_json::from_str::<serde::de::IgnoredAny>(black_box(&json_src)).unwrap());
    });
    row(scan, "serde_json validate (IgnoredAny)", throughput(json_src.len(), d));
    let d = median(15, || {
        black_box(rost::json::validate(black_box(&json_src)).unwrap());
    });
    row(scan, "rost::json::validate (strict JSON)", throughput(json_src.len(), d));
    let d = median(15, || {
        black_box(Template::from_json(black_box(&json_src)).unwrap());
    });
    row(scan, "rost Template::from_json (validate + index)", throughput(json_src.len(), d));
    let d = median(15, || {
        black_box(sonic_rs::from_str::<serde::de::IgnoredAny>(black_box(&json_src)).unwrap());
    });
    row(scan, "sonic-rs validate (IgnoredAny)", throughput(json_src.len(), d));

    let lookup = "Lookup by path (per query)";
    let t = Template::from_str(&rost_src);
    let mid = format!("item_{}.price", n / 2);
    const QUERIES: usize = 1000;
    let d = median(9, || {
        for _ in 0..QUERIES {
            black_box(t.query_raw_value(black_box(&mid)));
        }
    });
    row(lookup, "rost query_raw_value (mid document)", per_op(d, QUERIES));
    let value: serde_json::Value = serde_json::from_str(&json_src).unwrap();
    let key = format!("item_{}", n / 2);
    let d = median(9, || {
        for _ in 0..QUERIES {
            black_box(&value[black_box(key.as_str())]["price"]);
        }
    });
    row(lookup, "serde_json Value index (after DOM build)", per_op(d, QUERIES));
}
