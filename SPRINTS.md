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
  created from the current tip of `sprints` and merged back when done, so sprints stack.
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

- [x] `ParseError` with line, column, byte span and a human message (`miette`-style snippet behind a feature)
- [x] `Template::parse(&str) -> Result<Template, ParseError>`; lenient mode kept as `parse_lenient`
- [x] Detect unclosed strings / enums / lists, stray `}`, unclosed blocks, duplicate keys
- [x] Typed scalar classification in the lexer: string, int, float, bool, null, color, dimension
- [x] String escapes (`\n`, `\t`, `\"`, `\\`, `\u{…}`) and raw / multi-line strings
- [x] Scope-correct lookups: top-level `get` only sees depth-0 fields
- [x] `from_template` deserializes top-level properties, not only blocks
- [x] Fuzz targets for the parser and the deserializer (`cargo-fuzz`)

**Done when:** malformed input never yields a partially-silent template in strict mode.

> The bytecode format version byte moved to Sprint 6, where the precompiled `.rostc`
> format makes it meaningful. Fuzz targets live in `fuzz/` (`cargo +nightly fuzz run parse`);
> `tests/robustness.rs` is their always-on stable counterpart.

## Sprint 3 — Speed first: beat serde_json

**Goal:** `rost` is the fastest way to get data into Rust types. Measure in-repo, remove the
regressions introduced by the correctness work, and change the architecture where the
two-pass design (index, then walk) cannot win.

Targets, against `serde_json` on equivalent data on the same machine (50k records,
i7-13650HX, pinned to one core; `cargo bench --features serde --bench compare`):

| Workload | Sprint 2 | Target | Sprint 3 result |
|---|---|---|---|
| 5 MB document → typed structs | 1.5× slower | faster than `serde_json` | **on par**: 1.00–1.08× vs one-record-per-line JSON, **0.90–0.94×** vs JSON with the same layout (rost file 30% smaller) |
| Small config → struct | 2.3× slower | faster than `serde_json` | **on par**: 0.97–1.06× vs compact JSON, **0.88×** vs pretty JSON |
| Lookup by dot path | O(n), 274 µs mid-document | O(1) after the first query | ✅ **60 ns** after a one-time ~3 ms index build (`serde_json::Value`: 84 ns after a 44 ms DOM build) |
| Strict parse vs lenient | 2.4× | ≤ 1.5× | ✅ **1.2–1.3×** (15.9 ms → 7.3 ms) |
| Lenient indexing | regressed | ≤ develop baseline | ✅ **17.2M vs 16.8M cycles** per parse (develop), within noise |

- [x] In-repo reproducible comparison benchmark (`cargo bench --bench compare`) vs `serde_json` and `toml`
- [x] Recover the Sprint 1 indexing regression
- [x] Strict duplicate-key tracking without per-block hash sets
- [x] Lazy path index: O(1) `query_*` after the first lookup
- [x] Single-pass serde deserializer straight from text: `rost::from_str` / `from_slice`
- [x] Zero-copy borrowed strings (`&'de str`) in the single-pass deserializer (and in `from_template`)
- [x] Results table in README

**Done when:** every target in the table is met and reproducible with `cargo bench`.

> Typed loading reached parity rather than a clear lead: half of that workload is
> `HashMap` hashing and `String` allocation, identical for both crates, and the scanners
> now execute fewer instructions than `serde_json`'s. Branch-free SIMD / table scans were
> measured and rejected (the parse is a dependency chain through the position, where
> predicted byte loops win). Explicit SIMD for long runs moves to Sprint 5.

## Sprint 4 — Serde, first class (the drop-in experience)

**Goal:** switching from `serde_json` is a one-line change. Same mental model, same API shape.

- [x] `rost::from_reader` and API parity with `serde_json` (`from_str` / `from_slice` land in Sprint 3; `rost::Error`, `rost::Result`)
- [x] Serializer: `rost::to_string`, `to_string_pretty`, `to_writer`, `to_vec` (+ `_pretty` variants)
- [x] Zero-copy borrowing (`&'de str`, `Cow<'de, str>`) straight from the buffer (done in Sprint 3)
- [x] Enums (unit / newtype / tuple / struct variants), `Option`, `#[serde(default)]`, `flatten`, `rename` (+ tagged / untagged enums, integer / bool / enum map keys)
- [x] Nested structs ↔ nested blocks, `Vec<Struct>` ↔ list of blocks, `HashMap` ↔ block (list items may be inline blocks `{ … }` or nested lists)
- [x] `rost::Value` dynamic tree + `rost!{}` literal macro (like `json!`), `from_value`
- [x] Round-trip property tests: `T → rost → T` is identity (seeded in-repo generator, like the stress test, instead of a `proptest` dependency)

**Done when:** the `serde_json` examples from its README port to `rost` by renaming the crate.
✅ `tests/serde_json_readme.rs` (the data is written in `.rost`; `json!` becomes `rost!`).

| Workload (50k records, pinned, medians) | `rost` | `serde_json` |
|---|---|---|
| Serialize, compact | 3.2–3.3 ms | 3.2–3.3 ms (**on par**) |
| Serialize, pretty | 3.2–3.3 ms | 5.0–5.1 ms (**rost ~1.5× faster**) |
| Serialize small config, pretty | 70 ns | 146 ns (**rost ~2× faster**) |
| Text → dynamic tree (`Value`) | 19–26 ms | 23–32 ms (**rost ~1.2× faster**) |
| Typed loading (Sprint 3 workloads) | unchanged: instruction counts −0.7% (`from_str`) / −1.2% (small config) vs Sprint 3 | |

> Fixed on the way: a `]` inside a quoted list item ended the list early (data loss in
> both parsers), and untyped targets (`serde_json::Value`, `rost::Value`) dropped units
> (`140px` → `140.0`).

## Sprint 5 — JSON interop and proven speed

**Goal:** make the speed claim public and reproducible; read JSON too, so adoption is incremental.

- [ ] JSON input mode: parse standard JSON into the same bytecode arena
- [ ] Lossless `rost ⇄ JSON` conversion (+ `toml`/`yaml` behind features)
- [ ] Pass the JSONTestSuite conformance cases in JSON mode
- [ ] Extend the Sprint 3 benchmark to `simd-json`, `sonic-rs`, `lightningcss`
- [ ] Larger benchmark corpus (real configs, themes, datasets)
- [ ] Profile-driven parser optimizations (explicit SIMD for long runs: strings, comments, indentation)
- [ ] Typed loading clearly ahead of `serde_json` on compact layouts (Sprint 3 reached parity)
- [ ] CI job that fails on performance regressions

**Done when:** README shows reproducible numbers where `rost` leads or ties in its target workloads.

## Sprint 6 — Indexed in-memory data structure

**Goal:** every lookup is O(1) or O(depth), with no rebuilding of views.

- [ ] Block tree encoded in the bytecode (parent / first-child / next-sibling offsets)
- [ ] Path index built during parsing for `find_block` (lazy `query_*` index lands in Sprint 3)
- [ ] Allocation-free typed iterators for lists, enums and blocks
- [ ] Key interning for repeated names
- [ ] Bytecode format version byte for forward compatibility (moved from Sprint 2)
- [ ] `.rostc` precompiled binary (versioned header, checksum), loadable via `mmap`
- [ ] Document the 4 GiB `u32` offset limit or add `u64` offsets behind a feature

**Done when:** `find_block` / `query_*` no longer allocate and scale independently of document size.

## Sprint 7 — Rust ergonomics: derive and validation

**Goal:** describe data once as a Rust struct; get parsing, validation, defaults and docs for free.

- [ ] `rost-derive` crate with `#[derive(Rost)]` (replaces `rost_schema!` / `rost_block_schema!`)
- [ ] Field attributes: `default`, `rename`, `range(min, max)`, `regex`, `one_of`, `optional`
- [ ] Validation errors pointing to the exact source span
- [ ] JSON Schema / `.rost` schema generation from types (editor autocompletion)
- [ ] Typed newtypes out of the box: `Color`, `Dimension`, `Duration`, `Percentage`

**Done when:** a config struct needs only `#[derive(Rost)]` and one `rost::load` call.

## Sprint 8 — CSS core

**Goal:** a real styling language for Rust, not a key-value file with `#` in it.

- [ ] Selectors: type, `#id`, `.class`, descendant, child `>`, attribute, pseudo-states (`:hover`, `:focus`, `:disabled`)
- [ ] Specificity, cascade and inheritance with documented rules
- [ ] Variables / design tokens (`$primary`) resolved at load time
- [ ] `@import` / `@use` with cycle detection
- [ ] `@theme` variants (light/dark) and `@media` breakpoints
- [ ] Color functions (`rgb`, `hsl`, `oklch`, named colors), shadows, gradients, transforms
- [ ] `calc()` with typed units

**Done when:** a complete widget theme (states, tokens, themes) can be expressed and resolved.

## Sprint 9 — CSS for Rust UI frameworks

**Goal:** plug `rost` styles into real Rust UIs with live reload and zero frame drops.

- [ ] Computed-style cache per node with dependency-based invalidation
- [ ] `transition` and `@keyframes` with easing curves
- [ ] Event-driven hot reload via `notify` (feature flag), replacing polling
- [ ] Adapter crates: `rost-bevy`, `rost-egui`, `rost-iced` (others later)
- [ ] Example apps per adapter

**Done when:** editing a `.rost` theme updates a running Bevy/egui/iced app instantly.

## Sprint 10 — State and transactions

**Goal:** `rost` as an application state store with safe, atomic, observable changes.

- [ ] `State` store with typed get/set by path, validated against the schema
- [ ] Transactions: `state.transaction(|tx| { … })` → atomic commit, one notification
- [ ] Per-path observers / signals
- [ ] Undo / redo and snapshots
- [ ] Lock-free reader snapshots (`arc-swap`) for render / worker threads
- [ ] Incremental re-indexing of only the touched block
- [ ] Structural diff and patch (JSON-Patch-like) between documents

**Done when:** a game or UI can keep all its live state in `rost` with transactional updates.

## Sprint 11 — Declarative programming and API contracts

**Goal:** declare behaviour and API shapes in `rost`, get type-safe Rust out.

- [ ] Binding registry: `on_click = submit(form)` resolves to registered Rust functions (sandboxed, no eval)
- [ ] `@api` blocks describing requests / responses / errors
- [ ] `build.rs` code generation of Rust types and client/server stubs from `@api` blocks
- [ ] API transactions: request batching and atomic multi-operation payloads
- [ ] Lossless CST: edit and save while preserving comments and formatting
- [ ] Canonical formatter (`rost fmt`)

**Done when:** an HTTP API contract is defined in `.rost` and both sides compile against generated types.

## Sprint 12 — Tooling, ecosystem and release

**Goal:** frictionless adoption.

- [ ] `rost` CLI: `fmt`, `check`, `convert`, `diff`, `query`
- [ ] `rost-lsp` and `tree-sitter-rost` synced with the spec
- [ ] Formal `SPEC.md` grammar (EBNF) with format version `@rost 1`
- [ ] CI: tests, clippy `-D warnings`, `miri`, fuzzing, coverage, MSRV check
- [ ] `no_std + alloc` core, WASM build and JS bindings
- [ ] User guide, cookbook (config, UI theme, game state, API contract), CHANGELOG
- [ ] Publish `0.2` on crates.io, then `1.0` once the spec is frozen
