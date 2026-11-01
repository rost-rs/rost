# Changelog

## Unreleased

- `scripts/rost-init.sh`: the demo is now driven by `app.rost` (widget tree with areas and sizes, blocks, texts, lists,
  gauge, palette, status, text templates, key bindings, themes list), validated across fields before each reload.
  `scripts/demo-test.py` reads the screen through a small terminal emulator and runs 24 checks.

All notable changes per release checkpoint. Checkpoints are defined in
[`SPRINTS.md`](SPRINTS.md#release-checkpoints). Versions below `1.0.0` may break the API.

## 0.2.0-alpha.2 — 2026-11-01

Second downloadable checkpoint (Sprints 6–8): indexed structure, `.rostc`, `Doc`, generated loaders, live reload and typed
configuration. Tag: `v0.2.0-alpha.2`. Try it with `scripts/rost-init.sh`.

### Added (Sprint 8)
- `scripts/rost-init.sh`: builds a themed terminal app (three themes, live edits, rejected edits, `t` patching
  `app.rost`); `scripts/demo-test.py` drives it in a pseudo-terminal and runs in CI.
- `Overrides::merge`.
- `#[derive(Rost)]` for unit enums, newtype structs (and, through serde, data enums and tuple
  structs); `rename_all`; field attributes `optional`, `flatten`, `range`, `one_of`, `regex`.
- `rost::validate`, with errors at the line and column of the key and the field path.
- `rost::types::{Color, Dimension, Duration, Percentage, Path}`.
- `#[derive(Describe)]`, `TypeDesc::{to_json_schema, to_rost_schema}`.
- `rost::{load_file, Overrides, load_with, load_file_with}`: environment and command-line overrides.
- `ReloaderBuilder::validate`, `Reloader::watch_load_with`.
- Feature `regex`.

### Changed (Sprint 8)
- Typed-loader errors read `L:C: field `path`: message`; a missing field is `field `x`: missing`.


### Added (Sprint 7, feature `watch`)
- `rost::Reloader<T>`: a typed value that follows its file. Lock-free `get()`, atomic swap,
  the last good value stays when an edit is invalid (`last_error()`, `on_error`), `on_change`,
  `on_change_path("window.width", …)`, `depends_on(file)`, `wait_for_version`. Entry points
  `watch`, `watch_serde`, `watch_load` (`#[derive(Rost)]`), `builder`.
- One shared dispatcher thread fed by `notify` file events, debounced (20 ms), with a stat poll
  as safety net; editors that save by rename work; identical content is not reloaded.
- `rost::diff::{diff, Change}`: structural diff between two `Value` trees, as dotted paths.
- `examples/live_theme.rs`.

### Fixed (Sprint 7)
- Typed loaders (`rost::load`, `#[derive(Rost)]`) now report `line:column` on value errors
  (`cannot parse 'height' as u32` had no position).


### Added
- `rost::doc::{Doc, ValueRef}`: a borrowed document tree on a flat tape. `Doc::from_json`,
  `Doc::from_rost`; navigation by `get`, `at`, `iter`, `entries`; `to_value` for the owned `Value`.
- Block tree in the bytecode: `Block`, `Template::{block, root_blocks, block_path}`,
  `Block::{parent, first_child, next_sibling, children, fields, get, child, descend}`.
- `.rostc` precompiled documents: `Template::{to_rostc, from_rostc, save_rostc, load_rostc}` with a
  magic, a format version byte, a checksum and structural validation of untrusted files; and
  `unsafe Template::mmap_rostc` behind the `mmap` feature.
- `Template::query_list_iter` / `query_enum_iter` return an allocation-free `Items` iterator.
- `#[derive(Rost)]` (crate `rost-derive`, feature `derive`) and `rost::load`: generated loaders that skip
  serde's visitor protocol and fall back to it for anything unusual, so they agree with `rost::from_str`
  on every document. Attributes `rename`, `default`, `serde`. On the compact layout: 4% faster than
  `serde_json`, 7% faster than `rost::from_str` (50k records).
- `ParseErrorKind::TooLarge` and `parser::MAX_DOCUMENT_LEN`; `DeError::{new, message}`.

### Changed
- `query_list`, `query_enum`, `query_field` and `find_block` use the path index: O(depth) instead of
  rebuilding every block per call. `find_block` copies only the fields of the block it finds.
- Block-start records are 29 bytes (were 13): the lenient index executes 2.1% more instructions on a
  block-heavy document.
- The perf gate (`scripts/perf-gate.sh`) counts instructions under valgrind, threshold 3%.
- **Breaking:** `DeError`'s field is no longer public (`DeError::new`, `DeError::message`). The error is
  one pointer wide, which makes the typed loaders 6-8% faster in cycles on large documents.

### Fixed
- Documents over 4 GiB were silently indexed with wrapped `u32` offsets; they are now rejected.
- `scripts/perf-gate.sh` could run a stale `gate-*` binary, and reported timing noise as regressions.

## 0.2.0-alpha.1 — 2026-11-01

First downloadable checkpoint (Sprints 1–5). Tag: `v0.2.0-alpha.1`.

### Added
- **JSON superset:** every JSON object is a valid `.rost` document (quoted keys,
  `key: {…}` / `[…]`, a document wrapped in `{…}`), so existing `.json` files load
  through `rost::from_str`, `Template::parse` and all queries.
- **Strict JSON API:** `rost::json::{validate, from_str, from_slice, to_string, to_string_pretty}`
  and `Template::from_json`. Passes all accept/reject cases of JSONTestSuite
  (vendored in `tests/data/JSONTestSuite`), nesting limited to 128 levels.
- **Serde, first class:** `rost::from_str`, `from_slice`, `from_reader`, `from_template`,
  `to_string`, `to_string_pretty`, `to_vec`, `to_writer`, enums of every shape,
  `rost::Value` dynamic tree and the `rost!` literal macro.
- **Diagnostics:** `ParseError` with span, line/column and snippet; strict
  `Template::parse` / `Template::check` collecting every error.
- **Typed values:** typed scalars, string escapes, raw multi-line strings, units kept.
- **Tooling:** CI (tests and clippy with/without `serde`), a perf gate comparing a PR
  against its base branch (`scripts/perf-gate.sh`), and a real-world corpus benchmark
  (`scripts/fetch-corpus.sh`, `cargo bench --bench corpus`).

### Changed
- **Breaking:** hot reload is opt-in; `Template::get` only matches top-level fields;
  `Template` internals are private.
- Keys that are not plain words are quoted when serializing.

### Performance (i7-13650HX, one pinned core)
- Small config → struct: 125 ns (serde_json 145 ns, sonic-rs 151 ns, simd-json 272 ns).
- Small config serialize: 78 ns (serde_json 157 ns).
- 5 MB typed `HashMap`: 11.2 ms (serde_json 10.5 ms, sonic-rs 10.7 ms, toml 66 ms).
- Lookup by path: 40 ns (serde_json 60 ns).
- canada.json indexed for queries: 0.9 ms.

### Fixed
- Float parsing is correctly rounded; `u32` overflow and 8-digit hex alpha handled;
  line numbers, nested commas, quoted separators, unterminated lists and watcher locking
  fixed (see `git log` for each regression test).

### Known gaps
- Typed loading of big documents is on par with `serde_json`, not ahead (Sprint 6).
- `sonic_rs::Value` (arena DOM) is about 4x faster than any owned tree (Sprint 6).
- Strict validation of number-heavy JSON is 1.7x slower than `serde_json` on canada.json.
- No CLI/TUI yet (Sprint 12).
