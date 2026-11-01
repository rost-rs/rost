# rost

<p align="center">
  <img src="assets/rost.svg" alt="Rost Logo" width="128" height="128">
</p>

<p align="center">
  <strong>Blazing-Fast Zero-Copy Document Parser, Indexer, and Malleable Engine for the <code>.rost</code> Format</strong>
</p>

---

## Overview

The **`.rost`** declarative specification format bridges the gap between human-readable text documents and near-instant zero-copy memory parsing. By pairing SIMD-vectorized token scanners with raw byte-offset arenas, `rost` processes over **70,000 tokens in ~1.3 ms** (~500 MB/s sustained throughput).

`rost` serves as the core language foundation for:
1. **Declarative UI & Stylesheet Engines:** Ultra-fast, decoupled styling for Rust graphical frameworks (GPUI, egui, Iced, Slint, Bevy UI).
2. **Game Engines & Real-Time Configs:** High-frequency balance tuning and level asset reloading with zero frame drops.
3. **Linters & Static Code Analysis:** AST pattern matching schemas and lint rules.

---

## Features

- **⚡ Zero-Copy Architecture:** String slices (`&str`) point directly into the underlying buffer without intermediate heap allocations.
- **🛠️ Malleable Engine:** Supports both zero-allocation querying and in-place document patching for live hot-reloading.
- **🔁 JSON Superset:** every JSON object is a valid document; `rost::json` adds a strict
  RFC 8259 mode and JSON ⇄ rost conversion.
- **🧩 Ecosystem Integration:** Directly powers [`rost-lsp`](https://github.com/rost-rs/rost-lsp) and [`tree-sitter-rost`](https://github.com/rost-rs/tree-sitter-rost).

---

## Quick Start

Add `rost` to your `Cargo.toml`:

```toml
[dependencies]
rost = { git = "https://github.com/rost-rs/rost.git", tag = "v0.2.0-alpha.1" }
```

### 1. Document Parsing & Querying

```rust
use rost::Template;

let raw = r#"
@theme "cyber-dark"

#button.primary {
    width: 140px,
    height: 44px,
    bg: #1e1e2e,
    state | idle, hover, active |
}
"#;

let template = Template::from_str(raw);
assert_eq!(template.get_directive("theme"), Some("cyber-dark"));

let blocks = template.blocks();
assert_eq!(blocks[0].name, "button.primary");
assert_eq!(blocks[0].get_value("width"), Some("140px"));
```

### 2. Strict Parsing and Diagnostics

`Template::parse` validates the document and points at the exact problem;
`Template::from_str` / `parse_lenient` keep best-effort reading for files being edited.

```rust
use rost::Template;

let src = "#window {\n    width 500px\n}";
let err = Template::parse(src).unwrap_err();
assert_eq!(err.to_string(), "2:5: expected `:`, `=`, `|`, `[` or `{` after key");
eprintln!("{}", err.render(src));
// error: 2:5: expected `:`, `=`, `|`, `[` or `{` after key
//   |
// 2 |     width 500px
//   |     ^^^^^
```

Detected: unterminated strings / enums / lists, unbalanced braces, keys without an
operator and duplicate keys. `Template::check` returns every diagnostic at once.

### 3. Typed Values, Escapes and Raw Strings

Bare tokens are typed by shape; quoted strings always stay strings and support escapes;
`"""..."""` holds raw multi-line text.

```rust
use rost::{Scalar, Template};

let t = Template::parse(r#"
#player {
    hp: 100
    speed: 3.5
    tint: #ff0000
    width: 12px
    id: "007"
    motto: "never \"yield\"\n"
    bio: """
Raw text with "quotes" and {braces}.
"""
    pet: null
}
"#).unwrap();

assert_eq!(t.query_scalar("player.hp"), Some(Scalar::Int(100)));
assert_eq!(t.query_scalar("player.tint"), Some(Scalar::Color(0xff0000)));
assert_eq!(t.query_scalar("player.id"), Some(Scalar::Str("007".into())));
assert_eq!(t.query_string("player.motto").as_deref(), Some("never \"yield\"\n"));
assert_eq!(t.query_scalar("player.pet"), Some(Scalar::Null));
```

With the `serde` feature, top-level properties, escapes and `null` (→ `None`) map
straight onto your structs via `rost::from_template` / `Template::deserialize`.

### 4. Typed Loading in a Single Pass (`serde`)

`rost::from_str` / `rost::from_slice` deserialize text straight into your types without
building an index first: one scan, strings without escapes borrowed from the input, and
integers, decimals and booleans converted while scanning. Errors carry line and column.

```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Config<'a> {
    title: &'a str,
    version: u32,
    window: Window,
}

#[derive(Debug, Deserialize)]
struct Window {
    width: u32,
    scale: f64,
}

let src = "title: \"My App\"\nversion: 3\n#window {\n    width: 1280\n    scale: 1.25\n}\n";
let config: Config = rost::from_str(src).unwrap();
assert_eq!((config.title, config.window.width), ("My App", 1280));

let err = rost::from_str::<Config>("title: \"open\n").unwrap_err();
assert_eq!(err.to_string(), "1:8: unterminated string literal");
```

Use `Template` + `query_*` when you need to look values up by path instead: after a
one-time ~3 ms index build on a 5 MB document, each lookup takes ~60 ns.

### 5. Writing, Dynamic Values and Enums (`serde`)

The API mirrors `serde_json`: `to_string` / `to_string_pretty` / `to_vec` / `to_writer`,
`from_reader`, a `Value` tree with a `rost!` literal macro, and `rost::Error` /
`rost::Result`. Code written against `serde_json` ports by renaming the crate (see
`tests/serde_json_readme.rs`).

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum Shape {
    Empty,
    Circle(f64),
    Rect { w: u32, h: u32 },
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Scene {
    name: String,
    shapes: Vec<Shape>,
}

let scene = Scene {
    name: "demo".into(),
    shapes: vec![Shape::Empty, Shape::Circle(1.5), Shape::Rect { w: 2, h: 3 }],
};
let text = rost::to_string(&scene).unwrap();
assert_eq!(text, "name: \"demo\", shapes [Empty, { Circle: 1.5 }, { Rect { w: 2, h: 3 } }]");
assert_eq!(rost::from_str::<Scene>(&text).unwrap(), scene);

// Untyped access, like serde_json::Value.
let v: rost::Value = rost::from_str(&text).unwrap();
assert_eq!(v["shapes"][1]["Circle"], 1.5);

let patch = rost::rost!({ name: "copy", tags: ["a", "b"] });
assert_eq!(patch.to_string(), "name: \"copy\", tags [\"a\", \"b\"]");
```

How Rust maps to `.rost`: structs and maps are blocks, `Vec`s and tuples are lists
(items may be inline blocks `{ … }` or nested lists), unit variants are bare names and
variants with data are single-entry blocks (`{ Circle: 1.5 }`). Integer, `bool` and enum
map keys work; keys that are not plain words (`"user name"`, `"a.b"`, `""`) are written
quoted. Writing fails, instead of producing text that would not read back, for NaN and
infinite floats.

### 6. JSON In, JSON Out

Every JSON object is already a rost document, so `rost::from_str`, `Template::parse` and
every query read existing `.json` files as they are; migrating can happen one file at a
time. For input that must be strict RFC 8259 JSON, `rost::json` rejects the rost extensions
(comments, trailing commas, bare words) with a line and column, and converts in both
directions:

```rust
use rost::Template;

let src = r#"{"window": {"width": 800, "title": "Editor"}, "tags": ["a", "b"]}"#;

// The same zero-copy index and queries as a .rost file, after strict validation.
let doc = Template::from_json(src).unwrap();
assert_eq!(doc.query_i64("window.width"), Some(800));
assert!(Template::from_json("{width: 800}").is_err());

// JSON -> rost -> JSON.
let value = rost::json::from_str(src).unwrap();
let rost_text = rost::to_string_pretty(&value).unwrap();
let back: rost::Value = rost::from_str(&rost_text).unwrap();
assert_eq!(rost::json::to_string(&back), rost::json::to_string(&value));
```

`rost::json` passes every accept/reject case of
[JSONTestSuite](https://github.com/nst/JSONTestSuite) (95 valid, 188 invalid, vendored in
`tests/data`), and caps nesting at 128 levels. Any serde format converts through
`rost::Value` too, e.g. `toml::from_str::<rost::Value>(src)` then `rost::to_string`.

### 7. Programmatic Document Building

```rust
use rost::RostBuilder;

let document = RostBuilder::new()
    .directive("theme", "cyberpunk")
    .property("font_size", "14px")
    .enumeration("modes", &["light", "dark"])
    .finish();

assert!(document.contains("@theme \"cyberpunk\""));
```

### 8. Precompiled Documents, Block Trees and Borrowed Trees

```rust
use rost::{doc::Doc, Template};

let doc = Template::parse("#window { width: 800\n  #title { text: \"Hi\" } }")?;

// Blocks know their parent, children and siblings: navigation never scans or allocates.
let window = doc.block("window").unwrap();
for child in window.children() {
    println!("{} → {:?}", child.name(), child.get("text").map(|f| f.value()));
}

// Save the document with its bytecode; loading it again skips parsing.
doc.save_rostc("window.rostc")?;
let doc = Template::load_rostc("window.rostc")?;
for item in doc.query_list_iter("inventory.items").into_iter().flatten() {
    println!("{item}"); // borrowed from the document, no allocation
}

// A whole JSON or .rost document as one flat tape: ~4x faster to build than `Value`.
let text = r#"{"name": "rost", "tags": ["fast", "small"]}"#;
let tape = Doc::from_json(text)?;
assert_eq!(tape.root().get("name").and_then(|v| v.as_str()), Some("rost"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

A `.rostc` file is validated on load (magic, format version, checksum, UTF-8, every offset
and block link of the bytecode), so a damaged or hostile file is an error, never a panic or
an invalid `&str`. With the `mmap` feature, `unsafe Template::mmap_rostc` uses the file in
place instead of copying it. Documents over 4 GiB are rejected: the bytecode uses `u32` offsets.

### 9. Generated Loaders (`derive`)

```rust
use rost::Rost;

#[derive(Rost)]
struct Window {
    width: u32,
    #[rost(rename = "title")]
    caption: String,
}

#[derive(Rost)]
struct Config {
    debug: bool,
    window: Window,
    #[rost(serde)] // any field type that implements serde's `Deserialize`
    tags: Vec<String>,
    note: Option<String>,
}

let config: Config = rost::load("debug: true\ntags [a, b]\n#window { width: 1280, title: \"Hi\" }")?;
# Ok::<(), rost::de::DeError>(())
```

With the `derive` feature, `#[derive(Rost)]` writes the loop that reads a struct's entries, so
`rost::load` skips serde's visitor layer. Whatever the generated code does not recognise falls back
to the serde reader, so `rost::load` and `rost::from_str` return the same value for every document.

---

## Testing & Benchmarks

Run the test suite (unit, integration, doc tests and a deterministic stress test that
also checks `from_str` against `from_template` on thousands of generated documents):

```bash
cargo test --all-features
```

Reproduce the comparison against `serde_json`, `sonic-rs`, `simd-json` and `toml`
(equivalent data, same machine), and the real-world JSON corpus:

```bash
cargo bench --features serde --bench compare
scripts/fetch-corpus.sh && cargo bench --features serde --bench corpus
```

Results for 50,000 records on an Intel i7-13650HX (pinned to one core, medians of two
runs; absolute times vary with machine load, the ratios are what is stable). `simd-json`
timings include copying the input, since it parses in place:

| Workload | Contender | Median |
|---|---|---|
| 5 MB document → `HashMap<String, Item>` | **`rost::load`** (`#[derive(Rost)]`), compact layout / multi-line | **10.3–10.7 ms** / 10.95–11.4 ms |
| | **`rost::from_str`**, compact layout / multi-line | 11.3–11.6 ms / 11.6–12.1 ms |
| | **`rost::from_str`** reading the JSON text itself | 12.1–12.2 ms |
| | `serde_json`, one record per line (5.3 MB) | 10.95–11.3 ms |
| | `sonic-rs` / `simd-json` | 11.2–11.8 ms / 12.1–12.6 ms |
| | `toml` | 66–76 ms |
| Small config → struct | **`rost::from_str`** | 125–152 ns |
| | `serde_json` compact / pretty | 144–155 ns / 167–189 ns |
| | `sonic-rs` / `simd-json` | 151–160 ns / 272–310 ns |
| | `toml` | 1.1 µs |
| 5 MB document → dynamic tree | **`rost::Value`** / **`rost::json::from_str`** | 21.0 ms / 18.5–18.9 ms |
| | **`Doc::from_json`** / **`Doc::from_rost`** (borrowed flat tape) | 5.1 ms / 8.1–8.4 ms |
| | `serde_json::Value` | 25–27 ms |
| | `sonic_rs::Value` (arena-backed DOM) | 4.8 ms |
| | `simd_json::OwnedValue` | 22–23 ms |
| Serialize the 50k-record map | **`rost::to_string`** / **`to_string_pretty`** | 3.8 ms / 3.6–3.7 ms |
| | `serde_json::to_string` / `to_string_pretty` | 3.5 ms / 5.3 ms |
| | `toml::to_string` | 29–30 ms |
| Serialize small config (pretty) | **`rost::to_string_pretty`** | 78–80 ns |
| | `serde_json::to_string_pretty` | 157–160 ns |
| Validate 5.3 MB of JSON | **`rost::json::validate`** | 3.9 ms |
| | `serde_json` / `sonic-rs` (`IgnoredAny`) | 3.3–3.5 ms / 5.3–5.5 ms |
| Load a queryable document | **rost lenient index** / strict (validated) | 4.4–4.6 ms / 5.2–5.5 ms |
| | **`Template::from_rostc`** (precompiled: checksum + validation, no parsing) / **`mmap_rostc`** | 4.1 ms / 3.6–3.8 ms |
| | **`Template::from_json`** (strict JSON, validated + indexed) | 9.6–10.1 ms |
| Lookup by dot path | **rost `query_raw_value`** (after one index build) | 40 ns |
| | `serde_json` `Value` indexing (after the DOM build) | 60–67 ns |

Real-world JSON (serde-rs/json-benchmark files, same machine, medians):

| File | `json::validate` | `serde_json` validate | rost lenient index | `json::from_str` → `Value` | `Doc::from_json` | `serde_json::Value` | `sonic_rs::Value` |
|---|---|---|---|---|---|---|---|
| canada (2.2 MB, numbers) | 2.1–2.8 ms | 1.3 ms | **0.9 ms** | 6.4–7.0 ms | 3.1 ms | 6.6–7.0 ms | 1.8–2.0 ms |
| citm_catalog (1.7 MB) | **0.9 ms** | 0.9 ms | **0.6 ms** | 2.1–2.2 ms | 1.14 ms | 2.3–2.5 ms | 0.96 ms |
| twitter (0.6 MB, strings) | 0.3 ms | 0.25 ms | 0.27 ms | 1.36–1.43 ms | **0.40 ms** | 1.30–1.38 ms | 0.45 ms |

Small configs load on par with `serde_json` (125–152 ns vs 144–155 ns, depending on the state of the machine) and write ~2× faster; typed loading of
large documents with `#[derive(Rost)]` is 4–7% ahead of `serde_json` on the compact layout and level on the multi-line one, and `rost::from_str` is within ~7% of it; the dynamic tree
beats `serde_json::Value` and `simd-json`, and `Doc`, the borrowed flat tape, builds in
1.06× the time of `sonic-rs`'s arena DOM on the 5 MB document (ahead of it on twitter; 1.5× behind on
number-heavy canada). Indexing a JSON file for queries is faster than `serde_json` merely
validating it. rost files are ~30% smaller than equivalent pretty JSON.

---

## License

MIT License. Copyright (c) 2026 The Rost Project Contributors.
