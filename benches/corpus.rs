//! Real-world JSON: the canada, citm_catalog and twitter files of serde-rs/json-benchmark.
//!
//! ```text
//! scripts/fetch-corpus.sh
//! cargo bench --features serde --bench corpus
//! ```
//!
//! Each figure is the median of several timed runs after a warm-up. `simd-json` parses
//! in place, so its rows include copying the input into a fresh buffer.

use std::hint::black_box;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const FILES: [&str; 3] = ["canada", "citm_catalog", "twitter"];

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

fn row(file: &str, contender: &str, bytes: usize, d: Duration) {
    println!(
        "| {file} | {contender} | {:.2} ms ({:.0} MB/s) |",
        d.as_secs_f64() * 1e3,
        bytes as f64 / d.as_secs_f64() / 1e6
    );
}

fn main() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("benches/data");
    let Ok(sources) = FILES
        .iter()
        .map(|name| std::fs::read_to_string(dir.join(format!("{name}.json"))))
        .collect::<Result<Vec<_>, _>>()
    else {
        eprintln!("corpus missing: run scripts/fetch-corpus.sh first");
        return;
    };

    println!("| File | Contender | Median |");
    println!("|---|---|---|");
    for (name, src) in FILES.iter().zip(&sources) {
        let len = src.len();

        // The superset reader must agree with the strict one before it is timed.
        let strict = rost::json::from_str(src).unwrap();
        assert!(rost::from_str::<rost::Value>(src).unwrap() == strict, "{name}: from_str differs");
        let t = rost::Template::from_json(src).unwrap();
        assert!(rost::from_template::<rost::Value>(&t).unwrap() == strict, "{name}: from_template differs");

        let d = median(11, || black_box(rost::json::validate(black_box(src))).unwrap());
        row(name, "rost::json::validate", len, d);
        let d = median(11, || {
            black_box(serde_json::from_str::<serde::de::IgnoredAny>(black_box(src)).unwrap());
        });
        row(name, "serde_json (IgnoredAny)", len, d);
        let d = median(11, || {
            black_box(sonic_rs::from_str::<serde::de::IgnoredAny>(black_box(src)).unwrap());
        });
        row(name, "sonic-rs (IgnoredAny)", len, d);

        let d = median(11, || {
            black_box(rost::Template::from_json(black_box(src)).unwrap());
        });
        row(name, "rost Template::from_json (validate + index)", len, d);
        let d = median(11, || {
            black_box(rost::Template::from_str(black_box(src)));
        });
        row(name, "rost Template::from_str (lenient index)", len, d);

        let d = median(11, || {
            black_box(rost::json::from_str(black_box(src)).unwrap());
        });
        row(name, "rost::json::from_str → Value", len, d);
        let d = median(11, || {
            black_box(rost::from_str::<rost::Value>(black_box(src)).unwrap());
        });
        row(name, "rost::from_str → Value", len, d);
        let d = median(11, || {
            black_box(rost::doc::Doc::from_json(black_box(src)).unwrap());
        });
        row(name, "rost::doc::Doc::from_json", len, d);
        let d = median(11, || {
            black_box(serde_json::from_str::<serde_json::Value>(black_box(src)).unwrap());
        });
        row(name, "serde_json::Value", len, d);
        let d = median(11, || {
            black_box(sonic_rs::from_str::<sonic_rs::Value>(black_box(src)).unwrap());
        });
        row(name, "sonic_rs::Value", len, d);
        let d = median(11, || {
            let mut buf = black_box(src).as_bytes().to_vec();
            black_box(simd_json::to_owned_value(&mut buf).unwrap());
        });
        row(name, "simd_json::OwnedValue", len, d);
    }
}
