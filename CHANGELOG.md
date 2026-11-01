# Changelog

All notable changes per release checkpoint. Checkpoints are defined in
[`SPRINTS.md`](SPRINTS.md#release-checkpoints). Versions below `1.0.0` may break the API.

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
