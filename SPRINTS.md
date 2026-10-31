# SPRINTS

Development plan to turn `rost` into **the ultra-fast JSON and CSS for Rust**.

## North star

1. **Speed + Rust compatibility as the hook.** `rost` must be the fastest way to load
   configuration, data and styles into Rust types — and prove it with public benchmarks.
2. **Structure data and program declaratively with near-zero complexity.** Typed schemas,
   derives, transactions and reactive state that feel like idiomatic Rust.
3. **Save developer time.** One format and one crate replace `serde_json` + `toml` +
   ad-hoc CSS parsers + hand-written state plumbing.

## Working agreement

- Branch `sprints` holds the plan; each sprint lives in its own branch (`sprint1`, `sprint2`, …)
  created from `sprints` and merged back when done.
- One commit per feature / fix / refactor ("do one thing and do it well"). Every fix ships
  with its regression test in the same commit.
- `cargo test --all-features` and `cargo clippy --all-features` must pass on every commit.
- The detailed backlog is in [`TODO.md`](TODO.md) under `UPGRADES`.

---

## Sprint 1 — Trustworthy core

**Goal:** nobody adopts a "fast" format that silently corrupts data. Close every soundness
hole and every silent data-loss bug found in the audit, without changing the format.

- [x] Centralize bytecode record sizes in named constants (single source of truth)
- [x] Validate UTF-8 when building a `Template` (no UB on arbitrary bytes)
- [x] Make raw slice resolvers crate-private (no safe public API over `from_utf8_unchecked`)
- [x] Make `Template::data` / `Template::fields` private behind read-only accessors
- [x] Reject decimal `u32` overflow instead of wrapping
- [x] Preserve the alpha channel of `#RRGGBBAA` / `0xRRGGBBAA` colors
- [x] Correctly rounded float parsing and a native `f64` path
- [x] Serde: full-width integers (`u64`/`i64`/`i8`…`u16`) and `f64` without narrowing
- [x] Unquoted values keep commas nested inside `()` / `[]` (`rgba(1, 2, 3)`)
- [x] Quote- and bracket-aware list/enum item splitting (`["a, b"]`)
- [x] Accurate line numbers across multi-line quoted strings
- [x] `patch_value` quotes values that would otherwise alter document structure
- [x] `patch_value` supports scoped dot paths (`block.key`)
- [x] Watcher: run callbacks/broadcasts outside the global lock (no deadlock)
- [x] Watcher: per-handle reload slots and handle ref-counting on `Drop`
- [x] Hot-reload is opt-in (`Template::load` no longer spawns a watcher)
- [x] Remove `Template::dump` (silently swallows IO errors) and the `utilrs_rost` alias
- [x] Zero clippy warnings

**Done when:** every bug listed in `TODO.md` Phase 0 that does not require a new error
model is fixed and covered by a regression test.

## Sprint 2 — Diagnostics and typed values

**Goal:** errors a developer can act on in seconds, and values that carry their type.

- [ ] `ParseError` with line, column, byte span and a human message (`miette`-style snippet behind a feature)
- [ ] `Template::parse(&str) -> Result<Template, ParseError>`; lenient mode kept as `parse_lenient`
- [ ] Detect unclosed strings / enums / lists, stray `}`, unclosed blocks, duplicate keys
- [ ] Typed scalar classification in the lexer: string, int, float, bool, null, color, dimension
- [ ] String escapes (`\n`, `\t`, `\"`, `\\`, `\u{…}`) and raw / multi-line strings
- [ ] Scope-correct lookups: top-level `get` only sees depth-0 fields
- [ ] `from_template` deserializes top-level properties, not only blocks
- [ ] Bytecode format version byte for forward compatibility
- [ ] Fuzz targets for the parser and the deserializer (`cargo-fuzz`)

**Done when:** malformed input never yields a partially-silent template in strict mode.

## Sprint 3 — Serde, first class (the drop-in experience)

**Goal:** switching from `serde_json` is a one-line change. Same mental model, same API shape.

- [ ] `rost::from_str`, `from_slice`, `from_reader` mirroring `serde_json`
- [ ] Serializer: `rost::to_string`, `to_string_pretty`, `to_writer`, `to_vec`
- [ ] Zero-copy borrowing (`&'de str`, `Cow<'de, str>`) straight from the buffer
- [ ] Enums (unit / newtype / tuple / struct variants), `Option`, `#[serde(default)]`, `flatten`, `rename`
- [ ] Nested structs ↔ nested blocks, `Vec<Struct>` ↔ list of blocks, `HashMap` ↔ block
- [ ] `rost::Value` dynamic tree + `rost!{}` literal macro (like `json!`)
- [ ] Round-trip property tests (`proptest`): `T → rost → T` is identity

**Done when:** the `serde_json` examples from its README port to `rost` by renaming the crate.

## Sprint 4 — JSON interop and proven speed

**Goal:** make the speed claim public and reproducible; read JSON too, so adoption is incremental.

- [ ] JSON input mode: parse standard JSON into the same bytecode arena
- [ ] Lossless `rost ⇄ JSON` conversion (+ `toml`/`yaml` behind features)
- [ ] Pass the JSONTestSuite conformance cases in JSON mode
- [ ] `criterion` benches vs `serde_json`, `simd-json`, `sonic-rs`, `toml`, `lightningcss`
- [ ] Benchmark corpus in-repo (configs, themes, large datasets) + results table in README
- [ ] Profile-driven parser optimizations (explicit SIMD for structural chars where it wins)
- [ ] CI job that fails on performance regressions

**Done when:** README shows reproducible numbers where `rost` leads or ties in its target workloads.

## Sprint 5 — Indexed in-memory data structure

**Goal:** every lookup is O(1) or O(depth), with no rebuilding of views.

- [ ] Block tree encoded in the bytecode (parent / first-child / next-sibling offsets)
- [ ] Path index (hash table) built during parsing for `query_*` and `find_block`
- [ ] Allocation-free typed iterators for lists, enums and blocks
- [ ] Key interning for repeated names
- [ ] `.rostc` precompiled binary (versioned header, checksum), loadable via `mmap`
- [ ] Document the 4 GiB `u32` offset limit or add `u64` offsets behind a feature

**Done when:** `find_block` / `query_*` no longer allocate and scale independently of document size.

## Sprint 6 — Rust ergonomics: derive and validation

**Goal:** describe data once as a Rust struct; get parsing, validation, defaults and docs for free.

- [ ] `rost-derive` crate with `#[derive(Rost)]` (replaces `rost_schema!` / `rost_block_schema!`)
- [ ] Field attributes: `default`, `rename`, `range(min, max)`, `regex`, `one_of`, `optional`
- [ ] Validation errors pointing to the exact source span
- [ ] JSON Schema / `.rost` schema generation from types (editor autocompletion)
- [ ] Typed newtypes out of the box: `Color`, `Dimension`, `Duration`, `Percentage`

**Done when:** a config struct needs only `#[derive(Rost)]` and one `rost::load` call.

## Sprint 7 — CSS core

**Goal:** a real styling language for Rust, not a key-value file with `#` in it.

- [ ] Selectors: type, `#id`, `.class`, descendant, child `>`, attribute, pseudo-states (`:hover`, `:focus`, `:disabled`)
- [ ] Specificity, cascade and inheritance with documented rules
- [ ] Variables / design tokens (`$primary`) resolved at load time
- [ ] `@import` / `@use` with cycle detection
- [ ] `@theme` variants (light/dark) and `@media` breakpoints
- [ ] Color functions (`rgb`, `hsl`, `oklch`, named colors), shadows, gradients, transforms
- [ ] `calc()` with typed units

**Done when:** a complete widget theme (states, tokens, themes) can be expressed and resolved.

## Sprint 8 — CSS for Rust UI frameworks

**Goal:** plug `rost` styles into real Rust UIs with live reload and zero frame drops.

- [ ] Computed-style cache per node with dependency-based invalidation
- [ ] `transition` and `@keyframes` with easing curves
- [ ] Event-driven hot reload via `notify` (feature flag), replacing polling
- [ ] Adapter crates: `rost-bevy`, `rost-egui`, `rost-iced` (others later)
- [ ] Example apps per adapter

**Done when:** editing a `.rost` theme updates a running Bevy/egui/iced app instantly.

## Sprint 9 — State and transactions

**Goal:** `rost` as an application state store with safe, atomic, observable changes.

- [ ] `State` store with typed get/set by path, validated against the schema
- [ ] Transactions: `state.transaction(|tx| { … })` → atomic commit, one notification
- [ ] Per-path observers / signals
- [ ] Undo / redo and snapshots
- [ ] Lock-free reader snapshots (`arc-swap`) for render / worker threads
- [ ] Incremental re-indexing of only the touched block
- [ ] Structural diff and patch (JSON-Patch-like) between documents

**Done when:** a game or UI can keep all its live state in `rost` with transactional updates.

## Sprint 10 — Declarative programming and API contracts

**Goal:** declare behaviour and API shapes in `rost`, get type-safe Rust out.

- [ ] Binding registry: `on_click = submit(form)` resolves to registered Rust functions (sandboxed, no eval)
- [ ] `@api` blocks describing requests / responses / errors
- [ ] `build.rs` code generation of Rust types and client/server stubs from `@api` blocks
- [ ] API transactions: request batching and atomic multi-operation payloads
- [ ] Lossless CST: edit and save while preserving comments and formatting
- [ ] Canonical formatter (`rost fmt`)

**Done when:** an HTTP API contract is defined in `.rost` and both sides compile against generated types.

## Sprint 11 — Tooling, ecosystem and release

**Goal:** frictionless adoption.

- [ ] `rost` CLI: `fmt`, `check`, `convert`, `diff`, `query`
- [ ] `rost-lsp` and `tree-sitter-rost` synced with the spec
- [ ] Formal `SPEC.md` grammar (EBNF) with format version `@rost 1`
- [ ] CI: tests, clippy `-D warnings`, `miri`, fuzzing, coverage, MSRV check
- [ ] `no_std + alloc` core, WASM build and JS bindings
- [ ] User guide, cookbook (config, UI theme, game state, API contract), CHANGELOG
- [ ] Publish `0.2` on crates.io, then `1.0` once the spec is frozen
