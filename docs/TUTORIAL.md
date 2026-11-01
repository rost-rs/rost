# Rost tutorial

Rost is a configuration and template format for Rust apps that you can edit while the app runs. This tutorial
goes from the file format to hot reload in six small steps. Every program below is in `examples/` and is
compiled by `cargo test`; run any of them with

```sh
cargo run --all-features --example tutorial_1_load
```

## 0. Set up

```toml
[dependencies]
rost = { git = "https://github.com/rost-rs/rost.git", tag = "v0.2.0-alpha.2", features = ["derive", "watch"] }
serde = "1"   # only needed for lists (`Vec`), see step 1
```

`derive` gives you `#[derive(Rost)]`; `watch` gives you `Reloader` (step 5).

## 1. The format, and loading it into a struct

A `.rost` file is `key: value` lines. Text goes in quotes, numbers and `true`/`false` stand alone, lists use
`[...]`, and `name { ... }` opens a nested block. Your struct has one field per key.

```rust
use rost::Rost;

// Every struct you load into derives `Rost`. Field names are the keys of the file.
#[derive(Rost, Debug)]
struct Window {
    width: u32,
    height: u32,
}

#[derive(Rost, Debug)]
struct Config {
    title: String,
    scale: f64,
    fullscreen: bool,
    #[rost(default)] // may be missing from the file
    nickname: String,
    #[rost(serde)] // lists go through serde: add `serde = "1"` to your Cargo.toml
    tags: Vec<String>,
    window: Window, // a nested block
}

const FILE: &str = r#"
// Comments start with two slashes and go on their own line.
title: "My app"
scale: 1.25
fullscreen: false
tags: ["fast", "small"]

window {
    width: 1280
    height: 720
}
"#;

fn main() {
    let config: Config = rost::load(FILE).unwrap();
    assert_eq!(config.title, "My app");
    assert_eq!(config.window.width, 1280);
    assert_eq!(config.nickname, ""); // the default
    assert_eq!(config.tags, ["fast", "small"]);
    println!("{config:#?}");
}
```

Things to know:

- A field that may be absent gets `#[rost(default)]` (or is an `Option<T>`).
- `Vec<T>` fields use `#[rost(serde)]` and need `serde` in your `Cargo.toml`.
- **Put comments on their own line.** After a value that is not in quotes, `// text` becomes part of the
  value: `width: 800 // pixels` fails to load as a number, and `name: hello // note` loads as the string
  `hello // note`. Quoted strings (`name: "hello" // note`) are fine.
- For a list of repeated items, use a list of objects: `items [ { id: 1 }, { id: 2 } ]`. Nested `#name { }`
  blocks inside another block were ignored when loading through serde in our tests.

## 2. Typed values

Instead of strings you parse yourself, use the types in `rost::types`. A closed set of words is an enum.

```rust
use rost::types::{Color, Dimension, Duration, Percentage};
use rost::Rost;

// A closed set of words becomes an enum. `rename_all` picks how the words are spelled.
#[derive(Rost, Debug, PartialEq)]
#[rost(rename_all = "lowercase")]
enum Mode {
    Dark,
    Light,
}

#[derive(Rost, Debug)]
struct Style {
    mode: Mode,
    accent: Color,       // #rgb or #rrggbb
    timeout: Duration,   // 250ms, 1.5s, 2m
    opacity: Percentage, // 80%
    gap: Dimension,      // 12px, 1.5rem
}

fn main() {
    let style: Style = rost::load("mode: dark\naccent: #ff8800\ntimeout: 1.5s\nopacity: 80%\ngap: 12px\n").unwrap();
    assert_eq!(style.mode, Mode::Dark);
    assert_eq!((style.accent.r, style.accent.g, style.accent.b), (255, 136, 0));
    assert_eq!(style.timeout.get().as_millis(), 1500);
    assert_eq!(style.opacity.fraction(), 0.8);
    assert_eq!(style.gap.to_string(), "12px");

    // Each type explains itself when the value is wrong.
    let err = rost::load::<Style>("mode: dark\naccent: salmon\ntimeout: 1s\nopacity: 80%\ngap: 1px\n").unwrap_err();
    println!("{err}");
}
```

## 3. Validation

Attributes on a field check the value while it loads. Errors say where and why.

```rust
use rost::Rost;

#[derive(Rost, Debug)]
struct Window {
    #[rost(range(min = 100, max = 4096))]
    width: u32,
    #[rost(one_of("main", "tool", "popup"), default)]
    kind: String,
}

fn main() {
    let ok: Window = rost::load("width: 800\nkind: tool\n").unwrap();
    assert_eq!(ok.width, 800);

    // Errors carry line, column and the field.
    let err = rost::load::<Window>("width: 5000\n").unwrap_err();
    assert_eq!(err.message(), "1:1: field `width`: 5000 is out of range: expected 100 to 4096");
    println!("{err}");

    let err = rost::load::<Window>("width: 800\nkind: dialog\n").unwrap_err();
    println!("{err}");
}
```

The position is the one of the **key** (`1:1`), not of the value. Other attributes: `optional`, `flatten`,
`regex = "^v[0-9]+$"` (feature `regex`), `rename = "..."`, `rename_all = "kebab-case"`.

## 4. Overrides

Change a value without editing the file: from the command line (`--set window.width=1024`) or the
environment (`MYAPP_WINDOW__WIDTH=1024`). Overrides patch the text, so comments and layout survive, and an
invalid override is reported as the override.

```rust
use rost::{Overrides, Rost};

#[derive(Rost, Debug)]
struct Window {
    width: u32,
}

#[derive(Rost, Debug)]
struct Config {
    title: String,
    window: Window,
}

const FILE: &str = "// my settings\ntitle: \"My app\"\nwindow {\n    // pixels\n    width: 800\n}\n";

fn main() {
    // `--set window.width=1024` on the command line, or MYAPP_WINDOW__WIDTH=1024 in the environment.
    let overrides = Overrides::new().set("window.width", "1024").set("title", "Other");
    let config: Config = rost::load_with(FILE, &overrides).unwrap();
    assert_eq!((config.title.as_str(), config.window.width), ("Other", 1024));

    // An override patches the text: comments and layout are kept.
    let patched = overrides.patch(FILE).unwrap().unwrap();
    assert!(patched.contains("// my settings") && patched.contains("// pixels"));
    println!("{patched}");

    // In a real program:
    //     let overrides = Overrides::from_env("MYAPP").merge(Overrides::from_args(std::env::args()));
}
```

## 5. Hot reload

This is the point of the format. `Reloader` loads the file, watches it, and swaps in each new valid version.
An invalid file never replaces the running value: you keep the last good one and can show the error.

```rust
use std::time::{Duration, Instant};

use rost::{Reloader, Rost};

#[derive(Rost, Debug)]
struct Config {
    width: u32,
}

fn wait_until(mut done: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !done() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn main() {
    let path = std::env::temp_dir().join(format!("rost-tutorial-{}.rost", std::process::id()));
    std::fs::write(&path, "width: 800\n").unwrap();

    // One call: load, watch the file, and keep the value up to date.
    let live = Reloader::<Config>::watch_load(&path).unwrap();
    assert_eq!(live.get().width, 800);

    // Someone edits the file: the new value appears, no restart.
    std::fs::write(&path, "width: 1024\n").unwrap();
    wait_until(|| live.get().width == 1024);
    assert_eq!(live.get().width, 1024);

    // Someone saves a broken file: it is rejected, the last good value stays.
    std::fs::write(&path, "width: banana\n").unwrap();
    wait_until(|| live.last_error().is_some());
    assert_eq!(live.get().width, 1024);
    let error = live.last_error().unwrap();
    println!("rejected at {}:{}: {}", error.line.unwrap(), error.column.unwrap(), error.message);

    // Fixing the file recovers.
    std::fs::write(&path, "width: 640\n").unwrap();
    wait_until(|| live.get().width == 640);
    assert_eq!(live.get().width, 640);

    // Rules between fields: checked before a new value replaces the old one.
    let strict = Reloader::<Config>::builder(&path, |text| rost::load(text).map_err(rost::ReloadError::from_display))
        .validate(|c| if c.width % 2 == 0 { Ok(()) } else { Err("width must be even".into()) })
        .build()
        .unwrap();
    std::fs::write(&path, "width: 641\n").unwrap();
    wait_until(|| strict.last_error().is_some());
    assert_eq!(strict.get().width, 640);
    println!("rejected: {}", strict.last_error().unwrap().message);

    let _ = std::fs::remove_file(&path);
}
```

`live.get()` is cheap (an `Arc` clone): call it every frame. `live.version()` counts accepted reloads.
`watch_load_with(path, overrides)` applies overrides on every reload.

## 6. Schema for editors

`#[derive(Describe)]` produces a JSON Schema from your structs, with `///` doc comments as descriptions.

```rust
use rost::{Describe, Rost};

#[derive(Rost, Describe)]
struct Window {
    /// Width in pixels.
    #[rost(range(min = 100, max = 4096))]
    width: u32,
}

#[derive(Rost, Describe)]
struct Config {
    /// Shown in the title bar.
    title: String,
    window: Window,
}

fn main() {
    // A JSON Schema, so editors can autocomplete and check your .rost files...
    let schema = Config::describe().to_json_schema();
    assert!(schema.contains("Width in pixels."));
    println!("{schema}");
    // ...and a readable one.
    println!("{}", Config::describe().to_rost_schema());
}
```

## Where the demo fits

`scripts/rost-init.sh` builds a terminal app on these same pieces: a `Reloader` for `app.rost`, a
`validate` hook for the rules between fields, `Overrides` for `--set`, and `Color` and `Duration` in the
themes. What makes its `app.rost` long is not the format: the demo draws a whole interface from a widget
tree, so `main.rs` contains a small interpreter for that tree. Most apps only need the structs of steps 1 to 5.
`mi-demo/src/main_vanilla.rs` (same screen, no rost) is there to compare.
