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
- **🧩 Ecosystem Integration:** Directly powers [`rost-lsp`](https://github.com/rost-rs/rost-lsp) and [`tree-sitter-rost`](https://github.com/rost-rs/tree-sitter-rost).

---

## Quick Start

Add `rost` to your `Cargo.toml`:

```toml
[dependencies]
rost = { git = "https://github.com/rost-rs/rost.git" }
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
map keys work. Writing fails, instead of producing text that would not read back, for
keys the format cannot hold and for NaN/infinite floats.

### 6. Programmatic Document Building

```rust
use rost::RostBuilder;

let document = RostBuilder::new()
    .directive("theme", "cyberpunk")
    .property("font_size", "14px")
    .enumeration("modes", &["light", "dark"])
    .finish();

assert!(document.contains("@theme \"cyberpunk\""));
```

---

## Testing & Benchmarks

Run the test suite (unit, integration, doc tests and a deterministic stress test that
also checks `from_str` against `from_template` on thousands of generated documents):

```bash
cargo test --all-features
```

Reproduce the comparison against `serde_json` and `toml` (equivalent data, same machine):

```bash
cargo bench --features serde --bench compare
```

Results for 50,000 records on an Intel i7-13650HX (pinned to one core, medians of two
runs; absolute times vary with machine load, the ratios are what is stable):

| Workload | Contender | Median |
|---|---|---|
| 5 MB document → `HashMap<String, Item>` | **`rost::from_str`** | 9.8 ms |
| | `serde_json`, one record per line (5.3 MB) | 9.7 ms |
| | `serde_json`, pretty-printed like the rost file (7.7 MB) | 10.8 ms |
| | `toml` | 68–70 ms |
| Small config → struct | **`rost::from_str`** | 137 ns |
| | `serde_json` compact / pretty | 139–143 ns / 171 ns |
| | `toml` | 1.0 µs |
| 5 MB document → dynamic tree | **`rost::Value`** | 19–26 ms |
| | `serde_json::Value` | 23–32 ms |
| | `toml::Table` | 83–86 ms |
| Serialize the 50k-record map | **`rost::to_string`** / **`to_string_pretty`** | 3.2–3.3 ms / 3.2–3.3 ms |
| | `serde_json::to_string` / `to_string_pretty` | 3.2–3.3 ms / 5.0–5.1 ms |
| | `toml::to_string` | 27–28 ms |
| Serialize small config (pretty) | **`rost::to_string_pretty`** | 70 ns |
| | `serde_json::to_string_pretty` | 146 ns |
| Load a queryable document | **rost lenient index** / strict (validated) | 3.7 ms / 5.2 ms |
| Lookup by dot path | **rost `query_raw_value`** (after one ~3 ms index build) | 43 ns |
| | `serde_json` `Value` indexing (after the DOM build) | 57 ns |

Typed loading and compact writing are on par with `serde_json`; readable output, the
dynamic tree and small configs are faster (pretty writing ~1.5×, small config writes
~2×, `Value` ~1.2×), and rost files are ~30% smaller than equivalent pretty JSON.

---

## License

MIT License. Copyright (c) 2026 The Rost Project Contributors.
