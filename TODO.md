# TODO

## UPGRADES

Goal: make `.rost` simultaneously a data format (JSON), a styling language (CSS),
a state store (STATE MEMORY) and an in-memory data structure, all in one.
Order = priority: nothing else is adoptable until Phase 0 is done.
The execution plan lives in [`SPRINTS.md`](SPRINTS.md).

### Phase 0 — Correctness and soundness (blocking)

- [x] Validate UTF-8 in `Template::from_bytes` / `load` (`resolve_slice` uses `from_utf8_unchecked` over unvalidated bytes → UB on arbitrary input)
- [x] Make `Template::data` and `Template::fields` private (mutating them breaks the arena invariants the `unsafe` code relies on)
- [x] Replace silent parsing with `Result<Template, ParseError>` carrying line/column/span (unclosed quotes, unclosed `|`/`[`, stray `}`, duplicate keys)
- [x] Fix `parse_fast_u32` overflow via `wrapping_mul` (`4294967297` → `1`)
- [x] Fix 8-digit hex (`#RRGGBBAA`): alpha channel is dropped and the last two digits are not validated
- [x] Serde: `u64`/`i64`/`f64` must not go through `u32`/`f32` (`5000000000` → `705032704`, `-16777217` → `-16777216`, `pi` loses precision)
- [x] Correctly rounded float parsing (no `val += d / div` accumulation)
- [x] Unquoted values containing `,` (`rgba(1, 2, 3)`, `font: Inter, sans`) get truncated: respect nested parentheses/brackets when searching for the delimiter
- [x] Lists and enums: splitting on `,` ignores quotes (`["a, b"]` → 3 items); nested lists unsupported although the parser tracks depth
- [x] Count lines inside multi-line strings and `=` expressions (line numbers drift)
- [x] `patch_value`: escape/validate the new value (`"a\nevil: 1"` injects a new key) and honour scope (`block.key`) instead of the first global match
- [x] `Template::get` / `get_value` without a path return fields from inside blocks as if they were top-level
- [x] `from_template` (serde) ignores top-level properties: only blocks are iterated
- [x] Watcher: callbacks run while holding the global `Mutex` → deadlock if a callback calls `subscribe`/`on_reload`/`check_reload`
- [x] Watcher: `latest_reloaded` is a single slot shared by every handle of the same file (one handle steals another's update); `Drop` removes the state while other handles are alive
- [x] Watcher: `Template::load` enables hot-reload by default → should be opt-in
- [ ] Watcher: 100 ms polling + `sleep(15ms)` while holding the lock; migrate to `notify` (inotify/FSEvents/ReadDirectoryChangesW) behind a feature flag
- [x] Remove `Template::dump` (swallows IO errors by returning an empty template)
- [x] Unify bytecode record sizes (13/21/25/17/5) in constants; they are duplicated in `lib.rs`, `query.rs`, `de.rs`
- [ ] Fuzzing (`cargo-fuzz`) of parser and deserializer + round-trip property tests (`proptest`)
- [ ] `miri` in CI for all `unsafe` code

### Phase 1 — Format specification ("JSON" base)

- [ ] Formal `SPEC.md` (EBNF/PEG grammar) with a format version (`@rost 1`)
- [x] Explicit value types in the lexer: string, int, float, bool, null, color, dimension, list, map, reference — instead of "everything is `&str` and gets guessed at query time"
- [x] Defined string escapes (`\n`, `\t`, `\"`, `\u{...}`) and multi-line / raw strings (`"""` or `r"..."`)
- [ ] Inline maps/objects (`point { x: 1, y: 2 }` as a value) and lists of objects
- [ ] Block comments `/* */` and doc-comments `///` preserved in the AST
- [ ] Lossless bidirectional `rost ⇄ JSON` (and `TOML`/`YAML`) conversion with a conformance suite
- [ ] Serde serializer (`to_string`, `to_writer`) — only a deserializer exists today
- [ ] Real zero-copy deserialization (`&'de str` borrowed from the buffer, not `visit_string`)
- [ ] Serde enum support (unit, newtype, struct variants) mapped to `| a, b |`
- [ ] `u32` offset limit (4 GiB): document it or allow `u64` offsets behind a feature
- [ ] Canonical, idempotent formatter (`rost fmt`)

### Phase 2 — Styling layer ("CSS" base)

- [ ] Real selectors: type, `#id`, `.class`, descendant, child (`>`), attribute, pseudo-states (`:hover`, `:focus`, `:disabled`)
- [ ] Documented specificity and cascade + inheritance resolution
- [ ] Variables / design tokens (`$primary`, `var(--x)`) resolved at load time
- [ ] `@import` / `@use` across files with cycle detection
- [ ] `@media` / `@container` / breakpoints and `@theme` variants (dark/light)
- [ ] Typed arithmetic expressions (`calc(100% - 2 * $gap)`) with units
- [ ] Native CSS types: colors (`rgb()`, `hsl()`, `oklch()`, named), gradients, shadows, transforms, easing
- [ ] Declarative transitions/animations (`@keyframes`, `transition`)
- [ ] Cached computed styles per node (resolve the cascade once, invalidate by dependency)
- [ ] Official adapters: `rost-bevy`, `rost-egui`, `rost-iced`, `rost-gpui`, `rost-slint`

### Phase 3 — State store ("STATE MEMORY" base)

- [ ] Reactive store: `Template` → `State` with per-path signals/observers (`state.watch("player.hp", cb)`)
- [ ] Typed mutation by path (`set("player.hp", 90)`) validated against a schema
- [ ] Incremental patching: re-index only the affected block instead of the whole document when lengths change
- [ ] Structural diff between two templates (`rost diff`) and patch application (JSON-Patch-like)
- [ ] Change history: undo/redo, snapshots and time-travel debugging
- [ ] Atomic transactions (batch of changes → a single notification)
- [ ] Concurrency: shareable immutable snapshot (`Arc<Template>` + atomic swap à la `arc-swap`) for render-thread readers
- [ ] Persistence: save while preserving comments/formatting (lossless round-trip through a CST)
- [ ] Sync/merge (optional CRDT) for collaborative or networked editing
- [ ] `=` bindings: define evaluation semantics (function registry, sandbox, no arbitrary execution)

### Phase 4 — In-memory data structure ("DATA STRUCTURE" base)

- [ ] Key/path index (hash or perfect table) built at parse time: O(1) lookups instead of linear scans in `get`/`find_block`
- [ ] Precomputed block tree (parent/child/sibling offsets in the bytecode) so `blocks()`/`find_block()` stop rebuilding `Vec<BlockView>`
- [ ] Interning of repeated keys
- [ ] Precompiled, mmap-able binary format (`.rostc`) with versioned header and checksum for instant production loads
- [ ] `no_std` + `alloc` for embedded/WASM
- [ ] Bindings for other languages: WASM/JS, Python (`pyo3`), C ABI
- [ ] Typed iterators over lists/maps without intermediate `Vec` (`list_items()`/`get_list()` allocate today)
- [ ] Schemas: `#[derive(Rost)]` proc-macro (replacing `rost_schema!`), defaults, validation (ranges, regex, closed enums) and JSON Schema generation

### Phase 5 — Ecosystem and adoption

- [ ] Publish on crates.io with semver and an MSRV policy (today `edition = "2024"` and git-only dependency)
- [ ] Verify that `rost-lsp` and `tree-sitter-rost` exist and track the spec (the README links them)
- [ ] `rost` CLI: `fmt`, `check`, `convert` (json/toml/yaml), `diff`, `query`
- [ ] Reproducible `criterion` benchmarks against `serde_json`, `simd-json`, `toml`, `lightningcss` (back up the "500 MB/s" claim)
- [ ] CI: tests, clippy `-D warnings` (clippy is clean as of Sprint 1), `miri`, fuzz, coverage
- [ ] Docs: user guide, cookbook (UI, game config, linter), CHANGELOG
- [ ] Remove redundant aliases (`dump`/`load`/`from_file`, `finish`/`into_string`) before 1.0
- [x] Remove `extern crate self as utilrs_rost;` (leftover from a previous name)
