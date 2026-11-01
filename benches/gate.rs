//! Fixed workloads for the CI performance gate (`scripts/perf-gate.sh`).
//!
//! Prints one `name nanoseconds` line per workload, the fastest of several runs. Only
//! APIs that already exist on the base branch are used, so the script can copy this file
//! into a checkout of the base and time both builds with the same code.

use std::collections::HashMap;
use std::hint::black_box;
use std::time::Instant;

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct Item {
    id: u32,
    name: String,
    price: f64,
    active: bool,
    color: String,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct Config {
    title: String,
    version: u32,
    debug: bool,
    window: Window,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct Window {
    width: u32,
    height: u32,
    scale: f64,
    theme: String,
}

const CONFIG: &str = "title: \"My App\"\nversion: 3\ndebug: false\n#window {\n    width: 1280\n    height: 720\n    scale: 1.25\n    theme: \"dark\"\n}\n";

fn fastest(name: &str, mut f: impl FnMut()) {
    f();
    let best = (0..15)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed()
        })
        .min()
        .unwrap();
    println!("{name} {}", best.as_nanos());
}

fn main() {
    let doc: String = (0..20_000)
        .map(|i| format!("#item_{i} {{\n    id: {i}\n    name: \"item {i}\"\n    price: {i}.5\n    active: true\n    color: #1e1e2e\n}}\n"))
        .collect();
    let lists: String = (0..20_000).map(|i| format!("p{i} [[{i}.5, -{i}.25], [1, 2], \"s\"]\n")).collect();

    fastest("lenient_index", || {
        black_box(rost::parser::parse_into_bytecode(black_box(doc.as_bytes())));
    });
    fastest("from_str_typed", || {
        black_box(rost::from_str::<HashMap<String, Item>>(black_box(&doc)).unwrap());
    });
    fastest("template_typed", || {
        let t = rost::Template::from_str(black_box(&doc));
        black_box(rost::from_template::<HashMap<String, Item>>(&t).unwrap());
    });
    fastest("from_str_value", || {
        black_box(rost::from_str::<rost::Value>(black_box(&lists)).unwrap());
    });
    fastest("small_config_x1000", || {
        for _ in 0..1000 {
            black_box(rost::from_str::<Config>(black_box(CONFIG)).unwrap());
        }
    });
}
