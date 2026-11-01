# SPRINTS

Development plan to turn `rost` into **the template and configuration language for Rust
applications that change while they run**, and a JSON-style data structure on the same engine.

## North star

**One audience:** Rust applications (tools, TUIs, GUIs, games, servers) that load configuration,
templates and themes from files people edit while the program is running.

1. **Hot reload is the product.** Edit a file, the running program has the new typed value in
   milliseconds; a broken edit never breaks the program (the last good value stays) and the error
   points at the exact line. No other crate gives this out of the box.
2. **Templates and configuration in one language.** Variables, imports, reusable templates and
   profiles resolved at load time, with no eval and no scripting engine.
3. **Typed with no ceremony.** `#[derive(Rost)]` and one call. Also usable as a JSON-style data
   structure (`Doc`, `Value`) on the same engine.
4. **Speed is a requirement, not the hook.** Stay level with or ahead of `serde_json` / `toml`
   (guarded by the CI perf gate), never trade the above for it.

Not the audience for now: CSS engines, API-contract codegen and UI-framework adapters.
They are kept under *Later*, after `0.2`.

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
| Alpha 2 | `0.2.0-alpha.2` | Sprint 8 merged: perfect hot reload and typed configuration (`derive`, validation, spans) | pending |
| Beta 1 | `0.2.0-beta.1` | Sprint 10 merged: templates, imports, profiles and author tooling (CLI, editor grammar) | pending |
| Beta 2 | `0.2.0-beta.2` | Sprint 11 merged: live state, transactions and write-back preserving comments | pending |
| RC / Release | `0.2.0-rc.1` → `0.2.0` | Sprint 12 merged: spec, docs, CI hardening, WASM; published on crates.io | pending |
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
- [x] Typed loading ahead of `serde_json` on compact layouts and level on multi-line layouts, via generated loaders (`#[derive(Rost)]`, `rost::load`): 427M vs 446M cycles compact (−4%), 440M vs 446M multi-line. *The bar was moved on 2026-11-01: parser speed is a requirement for adoption, not the reason to adopt (see the new priorities below).*
- [x] CI job that fails on performance regressions (`scripts/perf-gate.sh`; since Sprint 6 it counts instructions under valgrind, >3% fails)

**Done when:** README shows reproducible numbers where `rost` leads or ties in its target workloads.
✅ Leads on writing small configs (~2×), pretty writing (~1.5×), dynamic trees vs
`serde_json`/`simd-json` and query-ready indexing (canada indexed in 0.9 ms, faster than
`serde_json` validates it); ties on large typed loads and on small config loads (a Sprint 5 reading of 1.2× on
small configs was a cool-machine measurement; interleaved A/B in Sprint 6 shows parity). Behind: `sonic-rs`'s arena DOM
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

- [ ] Typed loading clearly ahead of `serde_json` on compact layouts — **not reached**: with generated loaders (`#[derive(Rost)]`, `rost::load`) the compact layout is 4-7% ahead of `serde_json` and the multi-line layout level with it (see below)
- [x] Arena-backed `Value`: `rost::doc::Doc` keeps a whole document on one flat tape of 16-byte nodes; keys and strings are offsets into the source. `Doc::from_json` 5.07–5.11 ms vs `sonic_rs::Value` 4.79–4.84 ms (1.06×; the owned `Value` takes 19 ms, 4× apart before); `Doc::from_rost` 8.1–8.4 ms vs `rost::Value` 21 ms
- [x] Block tree encoded in the bytecode: every block-start record carries parent, end, first child and next sibling (`Block`, `Template::block`, `root_blocks`, `block_path`)
- [x] Block lookup without scanning: `find_block` resolves through a name index and copies only the block it finds (100 calls on 5000 blocks: 497M → 3.1M instructions); `block_path` follows the tree links with no index at all
- [x] Allocation-free typed iterators for lists and enums (`query_list_iter`, `query_enum_iter`; verified with a counting allocator)
- [x] `query_list` / `query_enum` / `query_field` resolved through the path index (they rebuilt every block on each call)
- [x] Key interning for repeated names: a `Doc` stores no key at all (an offset per key, 8 bytes, no allocation). Interning the owned `Value`'s keys with `Arc<str>` was built and measured: −17% instructions but +14% cycles (atomic reference counts), so it was dropped
- [x] Bytecode format version byte for forward compatibility (header of `.rostc`, unknown versions rejected)
- [x] `.rostc` precompiled binary (versioned header, checksum, structural validation incl. block links) and `mmap` loading behind the `mmap` feature
- [x] Document the 4 GiB `u32` offset limit, and reject larger documents instead of wrapping offsets
- [x] Perf gate counts instructions under valgrind (a wall-clock gate flagged identical code as +22%); self-comparison differs by < 0.03%

**Done when:** `find_block` / `query_*` no longer allocate and scale independently of document size.
*Status: met. `list_queries_x100` 995M → 2.4M instructions, `find_block_x100` 497M → 3.1M, on 5000 blocks.*

**Typed loading, measured (pinned core, interleaved runs, 50k records → `HashMap<String, Item>`, cycles net of setup):**

| Path | multi-line layout | compact layout |
|---|---|---|
| `serde_json` (compact JSON) | 446M | 446M |
| `rost::from_str` (serde) | 481M (601M at the start of Sprint 6) | 460M |
| `rost::load` (`#[derive(Rost)]`, generated) | 440M | **427M (−4% vs `serde_json`)** |

About 40% of the work (allocating the `String`s, hashing, inserting into the `HashMap`) is identical for
every crate; a numeric-only record (no strings) loads in 186M for `serde_json` and 186M for `rost::load`.
The generated loader removes serde's visitor layer and no more: the parser itself runs at `serde_json`'s speed.

## Sprint 7 — Hot reload, done right

**Goal:** edit a `.rost` file and the running program has the new value, always safe, always fast.

- [x] Event-driven watching through `notify` (feature `watch`), polling kept as fallback; correct with editors that save by rename or temp file, with debouncing of bursts
- [x] `Reloader<T>`: typed live value (`T: Load`) behind a lock-free snapshot (`arc-swap`); readers never block and never see a half-written value
- [x] Last good value stays when an edit is invalid; the error (with file, line and column) is delivered to a callback / channel and kept in `reloader.last_error()`
- [x] Atomic swap: the new document is parsed, indexed and validated off to the side, then published in one step
- [x] Structural diff between old and new document: callbacks per path (`on_change("window.width", …)`), so apps update only what changed
- [x] Watch dependencies: a file and everything it imports reload together (hook ready for Sprint 9 imports)
- [x] Reload latency measured end to end (write → new value visible) and reported; stress test with thousands of files and rapid saves, no missed final state
- [x] Example: an app whose theme changes live while it runs (`examples/live_theme.rs`; a console loop, no TUI framework)

*Status: met. Median write → new value 21 ms (20 ms debounce); 16 integration tests incl. ten saves in a second with three broken ones (4 reader threads, zero torn reads) and 1000 files × 3 rapid saves.*

**Done when:** an app holds `Reloader<Config>`, the file is saved ten times in a second including three broken versions, and the app ends on the last valid one with no panic, no stall and no torn read.

## Sprint 8 — Typed configuration with no ceremony

**Goal:** describe the config once as a Rust struct; get loading, defaults, validation and good errors.

- [ ] `#[derive(Rost)]` complete: enums, tuple structs, type parameters, `rost::load_file` (*named-field structs, lifetimes, `rost::load`, `Load` exist since Sprint 6*)
- [ ] Field attributes: `range(min, max)`, `regex`, `one_of`, `optional`, `flatten` (*`default`, `rename` and `serde` exist*)
- [ ] Validation errors pointing to the exact source span, with the field path and what was expected
- [ ] Typed newtypes out of the box: `Color`, `Dimension`, `Duration`, `Percentage`, `Path`
- [ ] Environment and command-line overrides (`APP_WINDOW__WIDTH=800`) on top of the file
- [ ] `Reloader<T>` validates before it swaps (a value outside its range counts as a broken edit)
- [ ] Schema generation from types (JSON Schema and `.rost` schema) for editor autocompletion

**Done when:** a config struct needs only `#[derive(Rost)]`, one `Reloader::watch` call, and a bad value is reported at its line without touching the running config. **Reaches Alpha 2.**

## Sprint 9 — Templates

**Goal:** reuse and parameterise configuration, resolved at load time, with no eval.

- [ ] Variables / tokens (`$primary`) with scopes and typed values, resolved at load time
- [ ] `@import` / `@use` with cycle detection and clear errors; imports feed the reload dependency set
- [ ] Reusable templates with parameters (`@template card(title, accent = $primary) { … }` and `@use card(title = "Home")`)
- [ ] Overlays and profiles (`@profile dev`, `@profile release`, light / dark), selected at load time or switched live
- [ ] Conditionals and repetition limited to data (`@if $profile == dev`, `@each item in list`), always terminating
- [ ] Typed expressions with units (`calc(100% - $gap)`) evaluated at load time, never at run time
- [ ] Output of a template is an ordinary document: `Doc`, `Value` and typed loading work unchanged on it
- [ ] Benchmark: load plus resolve of a templated document against hand-expanded TOML / JSON

**Done when:** an application theme with tokens, imports, two profiles and a parameterised component is one small set of files, and editing any of them live-reloads the resolved result.

## Sprint 10 — Author experience and tooling

**Goal:** people who write `.rost` get editor help and CI checks from day one.

- [ ] `rost` CLI: `check` (parse, validate against a schema, resolve templates), `fmt`, `convert` (JSON / TOML ↔ rost), `diff`, `query`, `watch` (print the resolved document on every change)
- [ ] Lossless CST: edit and save while preserving comments and formatting; canonical formatter
- [ ] `tree-sitter-rost` grammar and `rost-lsp` (diagnostics with spans, completion from the schema, go to definition for tokens and imports)
- [ ] Formal `SPEC.md` grammar (EBNF) with format version `@rost 1`, including templates
- [ ] Migration guide and converters from `toml`, `serde_json` and `.env` configs

**Done when:** a new user gets syntax highlighting, inline errors and `rost check` in CI without reading the source. **Reaches Beta 1.**

## Sprint 11 — Live state and write-back

**Goal:** the running program can also change the file and its own state safely.

- [ ] `State` store with typed get/set by path, validated against the schema
- [ ] Transactions: `state.transaction(|tx| { … })` → atomic commit, one notification
- [ ] Per-path observers / signals, shared with the reload diff callbacks
- [ ] Write-back: save a change from the program (a settings screen) to the file preserving comments and formatting, without triggering a reload loop
- [ ] Undo / redo and snapshots
- [ ] Incremental re-indexing of only the touched block
- [ ] Structural diff and patch (JSON-Patch-like) between documents

**Done when:** a settings screen edits values, the file is updated in place with its comments intact, and an external editor change merges with it.  **Reaches Beta 2.**

## Sprint 12 — Hardening and release

**Goal:** frictionless adoption and a stable base.

- [ ] CI: tests, clippy `-D warnings`, `miri`, fuzzing of parser, templates and `.rostc`, coverage, MSRV check
- [ ] `no_std + alloc` core, WASM build and JS bindings
- [ ] User guide and cookbook (app config, live theme, game balance tuning, templated service config), CHANGELOG
- [ ] Final benchmark report against `serde_json`, `toml`, `sonic-rs`, `simd-json`
- [ ] Publish `0.2` on crates.io, then `1.0` once the spec is frozen

## Later (after 0.2)

Kept from the earlier plan; none of it blocks the audience above.

- CSS core: selectors, specificity and cascade, pseudo-states, `@media`, color functions, gradients, transforms, benchmark against `lightningcss`
- UI adapters (`rost-bevy`, `rost-egui`, `rost-iced`), computed-style cache, `transition` and `@keyframes`
- Binding registry (`on_click = submit(form)`) resolved to registered Rust functions
- `@api` blocks and `build.rs` codegen of types and client / server stubs
