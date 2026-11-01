//! Fixed workloads for the CI performance gate (`scripts/perf-gate.sh`).
//!
//! * no argument: prints one `name nanoseconds` line per workload (fastest of 15 runs);
//! * `list`: prints the workload names;
//! * `run NAME`: runs the document setup and then workload `NAME` once (`none` = setup
//!   only). The script measures this under valgrind, where instruction counts are
//!   deterministic, so the gate has no timing noise to report as a regression.
//!
//! Only APIs that already exist on the base branch are used, so the script can copy this
//! file into a checkout of the base and measure both builds with the same code.

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

struct Data {
    doc: String,
    lists: String,
    tagged: rost::Template,
}

fn setup() -> Data {
    let doc: String = (0..20_000)
        .map(|i| format!("#item_{i} {{\n    id: {i}\n    name: \"item {i}\"\n    price: {i}.5\n    active: true\n    color: #1e1e2e\n}}\n"))
        .collect();
    let lists: String = (0..20_000).map(|i| format!("p{i} [[{i}.5, -{i}.25], [1, 2], \"s\"]\n")).collect();
    let tagged: String = (0..5_000).map(|i| format!("#node_{i} {{\n    tags [a{i}, b, c]\n    kind | x, y |\n}}\n")).collect();
    Data { doc, lists, tagged: rost::Template::from_str(&tagged) }
}

fn lenient_index(d: &Data) {
    black_box(rost::parser::parse_into_bytecode(black_box(d.doc.as_bytes())));
}

fn from_str_typed(d: &Data) {
    black_box(rost::from_str::<HashMap<String, Item>>(black_box(&d.doc)).unwrap());
}

fn template_typed(d: &Data) {
    let t = rost::Template::from_str(black_box(&d.doc));
    black_box(rost::from_template::<HashMap<String, Item>>(&t).unwrap());
}

fn from_str_value(d: &Data) {
    black_box(rost::from_str::<rost::Value>(black_box(&d.lists)).unwrap());
}

fn small_config_x1000(_: &Data) {
    for _ in 0..1000 {
        black_box(rost::from_str::<Config>(black_box(CONFIG)).unwrap());
    }
}

fn list_queries_x100(d: &Data) {
    for i in (0..5_000).step_by(50) {
        let path = format!("node_{i}.tags");
        black_box(d.tagged.query_list(black_box(&path)));
        let path = format!("node_{i}.kind");
        black_box(d.tagged.query_enum(black_box(&path)));
    }
}

type Workload = (&'static str, fn(&Data));

fn find_block_x100(d: &Data) {
    for i in (0..5_000).step_by(50) {
        let selector = format!("node_{i}");
        black_box(d.tagged.find_block(black_box(&selector)));
    }
}

const WORKLOADS: &[Workload] = &[
    ("lenient_index", lenient_index),
    ("from_str_typed", from_str_typed),
    ("template_typed", template_typed),
    ("from_str_value", from_str_value),
    ("small_config_x1000", small_config_x1000),
    ("list_queries_x100", list_queries_x100),
    ("find_block_x100", find_block_x100),
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["list"] => WORKLOADS.iter().for_each(|(name, _)| println!("{name}")),
        ["run", "none"] => {
            black_box(setup());
        }
        ["run", name] => {
            let (_, f) = WORKLOADS.iter().find(|(n, _)| *n == name).expect("unknown workload");
            let data = setup();
            f(&data);
        }
        _ => {
            let data = setup();
            for (name, f) in WORKLOADS {
                fastest(name, || f(&data));
            }
        }
    }
}
