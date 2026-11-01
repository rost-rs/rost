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

## Release checkpoints

Pre-release versions are cut from `main` so a given state can be downloaded and pinned
(`rost = { git = "https://github.com/rost-rs/rost.git", tag = "v0.2.0-alpha.1" }`).
Nothing is published to crates.io until `0.2.0` (Sprint 12).

| Checkpoint | Version | Reached when | Status |
|---|---|---|---|
| Alpha 1 | `0.2.0-alpha.1` | Sprint 5 merged: fast core, serde, JSON superset and strict JSON API, CI perf gate | **released** (2026-11-01) |
| Alpha 2 | `0.2.0-alpha.2` | Sprint 7 merged: indexed structure, `.rostc`, `#[derive(Rost)]` and validation | pending |
| Beta 1 | `0.2.0-beta.1` | Sprint 9 merged: CSS core and UI adapters (API may still change, features complete for styling) | pending |
| Beta 2 | `0.2.0-beta.2` | Sprint 11 merged: state, transactions, declarative bindings and API contracts | pending |
| RC / Release | `0.2.0-rc.1` → `0.2.0` | Sprint 12 merged: CLI, spec, docs; published on crates.io | pending |
| Stable | `1.0.0` | Spec frozen, no breaking changes planned | pending |

**Checkpoint ritual** (done as part of closing the sprint that reaches it, after the user approves it):

1. On `sprints`: bump `version` in `Cargo.toml` (and `Cargo.lock`), add the release to
   `CHANGELOG.md`, update the install snippet in `README.md`, and mark the row above.
   One commit: `chore(release): X.Y.Z-pre.N`.
2. Merge `sprints` into `main` with `--no-ff`.
3. Tag `main` with an annotated tag `vX.Y.Z-pre.N` (tests and clippy must pass on the tagged commit).
4. Nothing is pushed without an explicit request.

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

- [x] JSON input mode: parse standard JSON into the same bytecode arena (the grammar is a JSON superset: quoted keys, `key: { … }` / `key: [ … ]`, a document wrapped in `{ … }`; `Template::from_json` adds strict validation)
- [x] Lossless `rost ⇄ JSON` conversion through `rost::Value` (`rost::json::{from_str, to_string, to_string_pretty}`); `toml` converts through `Value` without a feature, `yaml` dropped (no maintained serde YAML crate worth a dependency)
- [x] Pass the JSONTestSuite conformance cases in JSON mode (all 95 `y_` accepted, all 188 `n_` rejected, no panic on the 35 `i_`; every `y_` value reads the same through the superset readers)
- [x] Extend the Sprint 3 benchmark to `simd-json`, `sonic-rs` (`lightningcss` moves to Sprint 8, where there is CSS to compare)
- [x] Larger benchmark corpus: canada / citm_catalog / twitter, fetched by `scripts/fetch-corpus.sh` (`benches/corpus.rs`)
- [x] Profile-driven parser optimizations: SWAR string scan in strict JSON, `memchr3` list delimiting, hot-path recovery after the grammar changes
- [ ] Typed loading clearly ahead of `serde_json` on compact layouts — **not reached**: on par (within ~7%), moved to Sprint 6
- [x] CI job that fails on performance regressions (`scripts/perf-gate.sh`: base vs PR in the same job, alternating rounds, >10% fails)

**Done when:** README shows reproducible numbers where `rost` leads or ties in its target workloads.
✅ Leads on small configs (load ~1.2×, write ~2×), pretty writing (~1.5×), dynamic trees vs
`serde_json`/`simd-json` and query-ready indexing (canada indexed in 0.9 ms, faster than
`serde_json` validates it); ties on large typed loads. Behind: `sonic-rs`'s arena DOM
(~4× faster than any owned tree) and strict validation of number-heavy JSON (canada 1.7×).

| Measure (pinned) | Sprint 4 tip | Sprint 5 |
|---|---|---|
| Lenient index, instructions | 1027M | 894M (−13%) |
| `from_str` typed, instructions | 1927M | 1977M (+2.6%: JSON grammar checks) |
| canada.json → `Value`, instructions per pass | 227M | 180M (−21%) |
| Perf gate, wall time (`benches/gate.rs`, min of 3 alternating rounds) | — | lenient −16%, typed `from_str` +4%, template +3.6%, `Value` −2.4%, small config +2% |

> Fixed on the way: strict JSON `-0` read as `Int(0)` while both rost readers kept
> `-0.0`; JSONTestSuite caught it.

## Sprint 6 — Indexed in-memory data structure

**Goal:** every lookup is O(1) or O(depth), with no rebuilding of views.

- [ ] Typed loading clearly ahead of `serde_json` on compact layouts (carried over from Sprint 5; still on par → Sprint 7)
- [ ] Arena-backed `Value` (borrowed keys/strings) to close the gap with `sonic_rs::Value` (→ Sprint 7)
- [ ] Block tree encoded in the bytecode (parent / first-child / next-sibling offsets) (→ Sprint 7)
- [ ] Path index built during parsing for `find_block` (`find_block` / `blocks()` still scan the document; the lazy index serves every `query_*`)
- [x] Allocation-free typed iterators for lists and enums (`query_list_iter`, `query_enum_iter`; verified with a counting allocator)
- [x] `query_list` / `query_enum` / `query_field` resolved through the path index (they rebuilt every block on each call)
- [ ] Key interning for repeated names (dropped: the index stores hashes, not strings, so there is nothing to intern yet)
- [x] Bytecode format version byte for forward compatibility (header of `.rostc`, unknown versions rejected)
- [x] `.rostc` precompiled binary (versioned header, checksum, structural validation) — `mmap` loading not done: `Template` owns its buffers
- [x] Document the 4 GiB `u32` offset limit, and reject larger documents instead of wrapping offsets (`u64` offsets behind a feature not done)

**Done when:** `find_block` / `query_*` no longer allocate and scale independently of document size.
*Status: met for list/enum/field queries (`list_queries_x100`: 43.6 ms → 22 µs on 5000 blocks); not met for `find_block`.*

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
- [ ] Benchmark against `lightningcss` (moved from Sprint 5)

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
