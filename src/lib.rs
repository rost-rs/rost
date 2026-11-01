//! # Rost: Blazing-Fast Zero-Copy Document Parser, Indexer, and Malleable Engine
//!
//! The `.rost` format is designed to bridge the gap between human-readable,
//! expressive text documents and near-instant zero-copy memory parsing.
//! By utilizing SIMD-vectorized token scanners and indexing raw byte offsets
//! into a flat binary arena, `rost` processes over **70,000 tokens in ~1.3 ms**
//! (sustained throughput of ~500 MB/s).
//!
//! ## Architectural Principles
//!
//! 1. **Modular Linux Philosophy:** Decoupled submodules with single, focused responsibilities:
//!    - [`parser`]: SIMD-accelerated low-level scanner and bytecode generator.
//!    - [`field`]: Zero-copy parsed node representation and scalar conversion helpers.
//!    - [`query`]: Zero-allocation field iterator and scoped block query engine.
//!    - [`malleable`]: Mutation, in-place patching, and programmatic document builder.
//! 2. **Zero-Copy Memory Layout:** Slices (`&str`) point directly into the underlying
//!    data buffer without heap allocation.
//! 3. **Malleability ("Maleador"):** Supports both reading and in-place mutation of values,
//!    enabling live game balancing or UI hot-reloading without dropping below 60/144 FPS.
//!
//! ## Target Scopes
//!
//! ### 1. CSS & Declarative UI Styling for Rust
//! Rust UI engines (e.g., Slint, Iced, GPUI, Dioxus, Bevy UI) lack an open, ultra-fast,
//! decoupled styling format. `.rost` allows defining themes, selectors, states, and properties:
//!
//! ```rost
//! @theme "dark-nebula"
//!
//! #button.primary {
//!     width: 120px,
//!     height: 40px,
//!     bg: #1e1e2e,
//!     state | idle, hover, active |,
//!     on_click = emit_event(action: "submit")
//! }
//! ```
//!
//! ### 2. Game Engines & High-Frequency Configuration
//! Eliminates frame drops during live asset reloads and levels streaming:
//!
//! ```rost
//! #PlayerEntity
//! name: "Valkyrie",
//! speed: 350.0,
//! max_health: 100,
//! inventory [ "spear", "shield", "elixir" ],
//! difficulty | easy, normal, nightmare |
//! ```
//!
//! ### 3. Linters, Static Analyzers, and Code Generation Tooling
//! High-throughput AST pattern matching rules and schemas (similar to `ast-grep` or `ripgrep` configurations):
//!
//! ```rost
//! @rule "no-eval"
//! pattern: "eval($$expr)"
//! severity | error |
//! message: "Arbitrary code execution is prohibited"
//! fix = replace_with_safe_parser()
//! ```
//!
//! ## Quick Start
//!
//! ```
//! use rost::{Template, RostBuilder};
//!
//! // 1. Programmatic creation
//! let template = RostBuilder::new()
//!     .directive("theme", "cyberpunk")
//!     .property("font_size", "14px")
//!     .enumeration("modes", &["light", "dark"])
//!     .build();
//!
//! // 2. Querying fields with zero allocations
//! assert_eq!(template.get_directive("theme"), Some("cyberpunk"));
//! assert_eq!(template.get_value("font_size"), Some("14px"));
//!
//! // 3. Type parsing helpers
//! let field = template.get("font_size").unwrap();
//! let dim = field.as_dimension().unwrap();
//! assert_eq!(dim.value, 14.0);
//! assert_eq!(dim.unit, "px");
//! ```

pub mod error;
pub mod field;
mod index;
pub mod malleable;
pub mod parser;
pub mod query;
pub mod schema;
pub mod watcher;
pub mod object;

#[cfg(feature = "serde")]
pub mod de;
#[cfg(feature = "serde")]
mod ser;
#[cfg(feature = "serde")]
mod text;

pub use error::{ParseError, ParseErrorKind};
pub use field::{Dimension, ParsedField, QuoteStyle, Scalar};
pub use malleable::{MalleablePatcher, RostBuilder};
pub use query::{parse_blocks, BlockView, FieldIter};
pub use watcher::{CentralWatcher, HotReloadWatcher, ReloadCallback};
pub use object::RostObject;

#[cfg(feature = "serde")]
pub use de::{from_block, from_template, DeError};
/// The error type of serialization and deserialization, named like `serde_json::Error`.
#[cfg(feature = "serde")]
pub use de::DeError as Error;
#[cfg(feature = "serde")]
pub use ser::{to_string, to_string_pretty, to_vec, to_vec_pretty, to_writer, to_writer_pretty};
#[cfg(feature = "serde")]
pub use text::{from_reader, from_slice, from_str};

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

/// The primary parsed representation of a `.rost` document.
///
/// Contains the raw byte buffer (`data`) and the binary offset index arena (`fields`).
#[derive(Debug, Clone)]
pub struct Template {
    /// File path origin of the template, if loaded from filesystem.
    pub path: PathBuf,
    /// Raw unparsed byte content of the document. Always valid UTF-8.
    pub(crate) data: Vec<u8>,
    /// Compact bytecode array storing tags and Little-Endian `u32` offsets into `data`.
    pub(crate) fields: Vec<u8>,
    /// Native zero-CPU filesystem watcher if hot-reloading was enabled.
    pub(crate) watcher: Option<Arc<HotReloadWatcher>>,
    /// Path index built on the first dot-path query; must be reset whenever `fields` changes.
    pub(crate) index: OnceLock<index::PathIndex>,
}

impl Template {
    /// Loads a `.rost` document from disk without hot-reloading.
    ///
    /// Use [`Template::load_watched`] to opt into hot-reloading.
    pub fn load<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        Self::load_with_options(path, false)
    }

    /// Loads a `.rost` document from disk and registers it with the shared hot-reload watcher.
    pub fn load_watched<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        Self::load_with_options(path, true)
    }

    /// Loads a `.rost` document from disk, allowing explicit control over hot-reloading.
    pub fn load_with_options<P: AsRef<Path>>(path: P, hot_reload: bool) -> std::io::Result<Self> {
        let p = path.as_ref();
        let data = read_utf8(p)?;
        let fields = parser::parse_into_bytecode(&data);
        let watcher = if hot_reload {
            HotReloadWatcher::new(p).ok()
        } else {
            None
        };
        Ok(Self {
            path: p.to_path_buf(),
            data,
            fields,
            watcher,
            index: OnceLock::new(),
        })
    }

    /// Loads and parses a `.rost` document from disk (without hot-reloading).
    pub fn from_file<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        Self::load(path)
    }

    /// Creates and parses a [`Template`] from an in-memory byte buffer.
    ///
    /// # Errors
    /// Returns [`std::str::Utf8Error`] if `data` is not valid UTF-8.
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, std::str::Utf8Error> {
        std::str::from_utf8(&data)?;
        Ok(Self::from_utf8_unchecked(data))
    }

    /// Builds a [`Template`] from bytes the caller has already validated as UTF-8.
    ///
    /// Every zero-copy slice handed out by the query engine relies on this invariant.
    fn from_utf8_unchecked(data: Vec<u8>) -> Self {
        debug_assert!(std::str::from_utf8(&data).is_ok());
        let fields = parser::parse_into_bytecode(&data);
        Self {
            path: PathBuf::new(),
            data,
            fields,
            watcher: None,
            index: OnceLock::new(),
        }
    }

    /// Parses a document strictly, failing on the first malformed construct.
    ///
    /// Use this for configuration and data you want validated: unterminated strings,
    /// enums or lists, unbalanced braces, keys without an operator and duplicate keys are
    /// all rejected with a [`ParseError`] pointing at the exact line and column.
    ///
    /// # Examples
    ///
    /// ```
    /// use rost::Template;
    ///
    /// let ok = Template::parse("#window { width: 500px }").unwrap();
    /// assert_eq!(ok.query_dim("window.width"), Some(500.0));
    ///
    /// let src = "#window {\n    width 500px\n}";
    /// let err = Template::parse(src).unwrap_err();
    /// assert_eq!(err.to_string(), "2:5: expected `:`, `=`, `|`, `[` or `{` after key");
    /// eprintln!("{}", err.render(src));
    /// ```
    pub fn parse(src: &str) -> Result<Self, ParseError> {
        let (fields, errors) = parser::parse_with_diagnostics(src.as_bytes());
        if let Some(first) = errors.into_iter().min_by_key(|e| e.span.start) {
            return Err(first);
        }
        Ok(Self {
            path: PathBuf::new(),
            data: src.as_bytes().to_vec(),
            fields,
            watcher: None,
            index: OnceLock::new(),
        })
    }

    /// Parses a document leniently: never fails, silently skipping malformed constructs.
    ///
    /// Equivalent to [`Template::from_str`]; prefer [`Template::parse`] unless you need
    /// best-effort reading of partially broken input (e.g. while a file is being edited).
    pub fn parse_lenient(src: &str) -> Self {
        Self::from_str(src)
    }

    /// Returns every diagnostic for `src`, ordered by position (empty when well-formed).
    ///
    /// # Examples
    ///
    /// ```
    /// use rost::Template;
    ///
    /// let errors = Template::check("a: 1\na: 2\n}\n");
    /// assert_eq!(errors.len(), 2);
    /// assert_eq!(errors[0].line, 2);
    /// ```
    pub fn check(src: &str) -> Vec<ParseError> {
        let mut errors = parser::parse_with_diagnostics(src.as_bytes()).1;
        errors.sort_by_key(|e| e.span.start);
        errors
    }

    /// Creates and parses a [`Template`] from a string slice (lenient, see [`Template::parse_lenient`]).
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(content: &str) -> Self {
        Self::from_utf8_unchecked(content.as_bytes().to_vec())
    }

    /// Creates and parses a [`Template`] from an owned `String`.
    pub fn from_string(content: String) -> Self {
        Self::from_utf8_unchecked(content.into_bytes())
    }

    /// Starts a fluent [`RostBuilder`] to assemble a template.
    pub fn builder() -> RostBuilder {
        RostBuilder::new()
    }

    /// Returns a zero-allocation iterator over all [`ParsedField`] items.
    #[inline]
    pub fn iter(&self) -> FieldIter<'_> {
        FieldIter::new(&self.data, &self.fields)
    }

    /// Resolves all fields into an allocated vector of [`ParsedField`] references.
    ///
    /// Preserves exact backward compatibility with previous versions.
    pub fn read_fields(&self) -> Vec<ParsedField<'_>> {
        self.iter().collect()
    }

    /// Retrieves the first top-level field matching the provided key or identifier.
    ///
    /// Fields nested inside blocks are not visible here; address them with a dot path
    /// through [`Template::query_field`] (e.g. `"button.width"`).
    pub fn get(&self, key: &str) -> Option<ParsedField<'_>> {
        let mut depth: usize = 0;
        for field in self.iter() {
            if depth == 0 && field.name() == key {
                return Some(field);
            }
            match field {
                ParsedField::BlockStart { .. } => depth += 1,
                ParsedField::BlockEnd { .. } => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        None
    }

    /// Retrieves the string value of the first property matching `key`.
    pub fn get_value(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(|f| f.value())
    }

    /// Retrieves the value of a metadata directive (e.g., `@theme`).
    pub fn get_directive(&self, name: &str) -> Option<&str> {
        self.iter().find_map(|f| match f {
            ParsedField::Directive { name: n, value, .. } if n == name => Some(value),
            _ => None,
        })
    }

    /// Returns all scoped blocks extracted from this template.
    pub fn blocks(&self) -> Vec<BlockView<'_>> {
        parse_blocks(&self.data, &self.fields)
    }

    /// Finds a specific block by its selector or name (e.g., `#button.primary` or `avatar`).
    pub fn find_block(&self, selector: &str) -> Option<BlockView<'_>> {
        let clean = selector.strip_prefix('#').unwrap_or(selector);
        self.blocks().into_iter().find(|b| {
            let b_clean = b.name.strip_prefix('#').unwrap_or(b.name);
            if b.name == selector || b_clean == clean {
                return true;
            }
            if let Some(parent) = b.parent {
                let p_clean = parent.strip_prefix('#').unwrap_or(parent);
                if clean.starts_with(p_clean)
                    && clean[p_clean.len()..].starts_with('.')
                    && &clean[p_clean.len() + 1..] == b_clean
                {
                    return true;
                }
            }
            false
        })
    }

    /// Internal helper to resolve a dot-path into a [BlockView] and property key.
    fn resolve_block_and_key<'p>(&self, path: &'p str) -> Option<(BlockView<'_>, &'p str)> {
        if let Some((block_sel, key)) = path.rsplit_once('.') {
            if let Some(b) = self.find_block(block_sel) {
                return Some((b, key));
            }
            if let Some((_, sub_sel)) = block_sel.rsplit_once('.')
                && let Some(b) = self.find_block(sub_sel)
            {
                return Some((b, key));
            }
        }
        None
    }

    /// Patches an existing property in-place ("maleador").
    ///
    /// `key` is a bare top-level key (`"volume"`) or a dot path into nested blocks
    /// (`"audio.volume"`).
    ///
    /// If the new value length matches the previous value length, replacement occurs
    /// in $O(1)$ directly in the underlying buffer with zero reallocations.
    pub fn patch_value(&mut self, key: &str, new_value: &str) -> bool {
        MalleablePatcher::patch_property(self, key, new_value)
    }

    /// Returns the raw content of the document as bytes (always valid UTF-8).
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Returns the compact bytecode index produced by [`parser::parse_into_bytecode`].
    #[inline]
    pub fn bytecode(&self) -> &[u8] {
        &self.fields
    }

    /// Returns the raw content of the document as a UTF-8 string slice.
    #[inline]
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.data).unwrap_or("")
    }

    /// Checks if hot-reloading is currently active for this template.
    pub fn is_hot_reload_enabled(&self) -> bool {
        self.watcher.is_some()
    }

    /// Registers a callback closure to run on a background thread whenever the file updates.
    pub fn on_reload<F>(&self, cb: F)
    where
        F: Fn(Template) + Send + Sync + 'static,
    {
        if let Some(w) = &self.watcher {
            w.on_reload(cb);
        }
    }

    /// Subscribes to document updates via a channel receiver.
    pub fn subscribe(&self) -> Option<std::sync::mpsc::Receiver<Template>> {
        self.watcher.as_ref().map(|w| w.subscribe())
    }

    /// Checks if the template has been updated on disk. If so, updates this template in-place and returns `true`.
    pub fn check_reload(&mut self) -> bool {
        if let Some(w) = &self.watcher
            && let Some(new_tmpl) = w.check_reload()
        {
            self.data = new_tmpl.data;
            self.fields = new_tmpl.fields;
            self.index = OnceLock::new();
            return true;
        }
        false
    }

    /// Queries a field across scoped blocks using dot-notation (e.g. `"palette.bg_window"` or `"title"`).
    pub fn query_field(&self, path: &str) -> Option<ParsedField<'_>> {
        if let Some((b, key)) = self.resolve_block_and_key(path) {
            b.get(key).cloned()
        } else {
            self.get(path)
        }
    }

    /// Queries the raw string value of a property directly from the zero-copy bytecode arena.
    ///
    /// Zero heap allocations and blazing fast (under 100 ns).
    pub fn query_raw_value(&self, path: &str) -> Option<&str> {
        self.locate_property(path)
            .map(|rec| parser::resolve_slice(&self.data, &self.fields, rec + 9))
    }

    /// Returns the bytecode index of the property record addressed by `path`.
    ///
    /// A bare key (`"volume"`) only matches top-level properties; a dot path
    /// (`"audio.volume"`, `"car.dimensions.width"`) matches the property inside that
    /// exact block nesting.
    pub(crate) fn locate_property(&self, path: &str) -> Option<usize> {
        let clean_path = path.strip_prefix('#').unwrap_or(path);
        self.index
            .get_or_init(|| index::PathIndex::build(&self.data, &self.fields))
            .get(&self.data, &self.fields, clean_path)
    }

    /// Linear-scan equivalent of [`Template::locate_property`], kept as the reference
    /// implementation the index is tested against.
    #[cfg(test)]
    pub(crate) fn locate_property_linear(&self, path: &str) -> Option<usize> {
        let clean_path = path.strip_prefix('#').unwrap_or(path);

        // Fast-path: single key (no dots)
        if !clean_path.contains('.') {
            let mut idx = 0;
            let mut depth: u32 = 0;
            while idx < self.fields.len() {
                let tag = self.fields[idx];
                match tag {
                    parser::TAG_COMMON_TEXT => {
                        let key = parser::resolve_slice(&self.data, &self.fields, idx + 1);
                        if key == clean_path && depth == 0 {
                            return Some(idx);
                        }
                        idx += parser::REC_PAIR;
                    }
                    parser::TAG_BLOCK_START => {
                        depth += 1;
                        idx += parser::REC_NAMED;
                    }
                    parser::TAG_BLOCK_END => {
                        depth = depth.saturating_sub(1);
                        idx += parser::REC_BLOCK_END;
                    }
                    other => idx += parser::record_len(other),
                }
            }
            return None;
        }

        // Dot-separated path: e.g. "car.dimensions.top_width"
        let mut segments = [""; 8];
        let mut seg_count = 0;
        for s in clean_path.split('.') {
            if seg_count < 8 {
                segments[seg_count] = s;
                seg_count += 1;
            }
        }
        if seg_count == 0 {
            return None;
        }
        let target_key = segments[seg_count - 1];
        let block_path = &segments[..seg_count - 1];

        let mut idx = 0;
        let mut stack_depth: usize = 0;
        let mut matched_depth: usize = 0;

        while idx < self.fields.len() {
            let tag = self.fields[idx];
            match tag {
                parser::TAG_BLOCK_START => {
                    let name = parser::resolve_slice(&self.data, &self.fields, idx + 1);
                    let name_clean = name.strip_prefix('#').unwrap_or(name);
                    idx += parser::REC_NAMED;
                    if stack_depth < block_path.len() && stack_depth == matched_depth && name_clean == block_path[stack_depth] {
                        matched_depth += 1;
                    }
                    stack_depth += 1;
                }
                parser::TAG_BLOCK_END => {
                    stack_depth = stack_depth.saturating_sub(1);
                    if stack_depth < matched_depth {
                        matched_depth = stack_depth;
                    }
                    idx += parser::REC_BLOCK_END;
                }
                parser::TAG_COMMON_TEXT => {
                    if stack_depth == block_path.len() && matched_depth == block_path.len() {
                        let key = parser::resolve_slice(&self.data, &self.fields, idx + 1);
                        if key == target_key {
                            return Some(idx);
                        }
                    }
                    idx += parser::REC_PAIR;
                }
                other => idx += parser::record_len(other),
            }
        }

        None
    }

    /// Queries a hex color (`u32`) using dot-notation or top-level key.
    pub fn query_hex(&self, path: &str) -> Option<u32> {
        if let Some(val) = self.query_raw_value(path) {
            return field::parse_fast_u32(val);
        }
        if let Some((b, key)) = self.resolve_block_and_key(path) {
            b.get_hex(key)
        } else {
            self.get(path).and_then(|f| f.as_hex_color())
        }
    }

    /// Queries a dimension/float (`f32`) using dot-notation or top-level key.
    pub fn query_dim(&self, path: &str) -> Option<f32> {
        if let Some(val) = self.query_raw_value(path) {
            return field::parse_fast_f32(val);
        }
        if let Some((b, key)) = self.resolve_block_and_key(path) {
            b.get_dim(key)
        } else {
            self.get(path)
                .and_then(|f| f.as_dimension())
                .map(|d| d.value as f32)
        }
    }

    /// Queries a string slice using dot-notation or top-level key.
    pub fn query_str(&self, path: &str) -> Option<&str> {
        if let Some(val) = self.query_raw_value(path) {
            return Some(val.trim().trim_end_matches(';').trim_matches('"'));
        }
        if let Some((b, key)) = self.resolve_block_and_key(path) {
            b.get_str(key)
        } else {
            self.get(path)
                .and_then(|f| f.value())
                .map(str::trim)
                .map(|s| s.trim_matches('"'))
        }
    }

    /// Queries a string using dot-notation or top-level key, decoding escapes in quoted values.
    ///
    /// # Examples
    ///
    /// ```
    /// use rost::Template;
    ///
    /// let t = Template::parse(r#"#msg { text: "line 1\nline \"2\"" }"#).unwrap();
    /// assert_eq!(t.query_string("msg.text").as_deref(), Some("line 1\nline \"2\""));
    /// ```
    pub fn query_string(&self, path: &str) -> Option<std::borrow::Cow<'_, str>> {
        self.query_field(path).and_then(|f| f.string_value())
    }

    /// Queries a property as a typed [`Scalar`] using dot-notation or top-level key.
    ///
    /// # Examples
    ///
    /// ```
    /// use rost::{Scalar, Template};
    ///
    /// let t = Template::parse("#p { hp: 100, speed: 3.5, tint: #ff0000, w: 12px, id: \"007\" }").unwrap();
    /// assert_eq!(t.query_scalar("p.hp"), Some(Scalar::Int(100)));
    /// assert_eq!(t.query_scalar("p.speed"), Some(Scalar::Float(3.5)));
    /// assert_eq!(t.query_scalar("p.tint"), Some(Scalar::Color(0xff0000)));
    /// assert_eq!(t.query_scalar("p.id"), Some(Scalar::Str("007".into())));
    /// ```
    pub fn query_scalar(&self, path: &str) -> Option<Scalar<'_>> {
        self.query_field(path).and_then(|f| f.scalar())
    }

    /// Queries an integer (`i64`) using dot-notation or top-level key.
    pub fn query_i64(&self, path: &str) -> Option<i64> {
        if let Some(val) = self.query_raw_value(path) {
            let s = val.trim().trim_end_matches(';');
            let num_str = s.trim_end_matches(|c: char| c.is_alphabetic() || c == '%');
            return num_str.parse::<i64>().ok();
        }
        if let Some((b, key)) = self.resolve_block_and_key(path) {
            b.get_i64(key)
        } else {
            self.get(path).and_then(|f| f.as_i64())
        }
    }

    /// Queries a float (`f64`) using dot-notation or top-level key.
    pub fn query_f64(&self, path: &str) -> Option<f64> {
        if let Some(val) = self.query_raw_value(path) {
            let s = val.trim().trim_end_matches(';');
            let num_str = s.trim_end_matches(|c: char| c.is_alphabetic() || c == '%');
            return num_str.parse::<f64>().ok();
        }
        if let Some((b, key)) = self.resolve_block_and_key(path) {
            b.get_f64(key)
        } else {
            self.get(path).and_then(|f| f.as_f64())
        }
    }

    /// Queries a boolean using dot-notation or top-level key.
    pub fn query_bool(&self, path: &str) -> Option<bool> {
        if let Some(val) = self.query_raw_value(path) {
            return match val.trim().trim_end_matches(';') {
                "true" | "1" | "yes" | "on" => Some(true),
                "false" | "0" | "no" | "off" => Some(false),
                _ => None,
            };
        }
        if let Some((b, key)) = self.resolve_block_and_key(path) {
            b.get_bool(key)
        } else {
            self.get(path).and_then(|f| f.as_bool())
        }
    }

    /// Queries an action or function binding expression (e.g. `"counter_button.on_click"` -> `Some("increment_counter()")`).
    pub fn query_binding(&self, path: &str) -> Option<&str> {
        if let Some((b, key)) = self.resolve_block_and_key(path) {
            b.get_binding(key)
        } else {
            self.iter().find_map(|f| match f {
                ParsedField::VarFunc { var_name, signature } if var_name == path => Some(signature),
                _ => None,
            })
        }
    }

    /// Queries all items from a list field using dot-notation or top-level key (e.g. `"inventory.items"`).
    pub fn query_list(&self, path: &str) -> Option<Vec<&str>> {
        if let Some((b, key)) = self.resolve_block_and_key(path) {
            b.get_list(key)
        } else {
            self.get(path).and_then(|f| match f {
                ParsedField::List { .. } => Some(f.list_items()),
                _ => None,
            })
        }
    }

    /// Queries a specific element by index from a list field (e.g. `query_list_item("inventory.items", 0)`).
    pub fn query_list_item(&self, path: &str, index: usize) -> Option<&str> {
        if let Some((b, key)) = self.resolve_block_and_key(path) {
            b.get_list_item(key, index)
        } else {
            self.get(path).and_then(|f| f.list_item(index))
        }
    }

    /// Queries all variants from an enum field using dot-notation or top-level key (e.g. `"profile.status"`).
    pub fn query_enum(&self, path: &str) -> Option<Vec<&str>> {
        if let Some((b, key)) = self.resolve_block_and_key(path) {
            b.get_enum(key)
        } else {
            self.get(path).and_then(|f| match f {
                ParsedField::Enum { .. } => Some(f.enum_variants()),
                _ => None,
            })
        }
    }

    /// Queries a specific variant by index from an enum field (e.g. `query_enum_variant("profile.status", 0)`).
    pub fn query_enum_variant(&self, path: &str, index: usize) -> Option<&str> {
        if let Some((b, key)) = self.resolve_block_and_key(path) {
            b.get_enum_variant(key, index)
        } else {
            self.get(path).and_then(|f| f.enum_variant(index))
        }
    }

    /// Saves the current template buffer back to disk.
    pub fn save<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        fs::write(path, &self.data)
    }
}

/// Reads a file and verifies it is valid UTF-8 before it can back a [`Template`].
pub(crate) fn read_utf8(path: &Path) -> std::io::Result<Vec<u8>> {
    let data = fs::read(path)?;
    if let Err(e) = std::str::from_utf8(&data) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e));
    }
    Ok(data)
}

impl std::str::FromStr for Template {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_str(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_index_agrees_with_linear_lookup() {
        let src = "a: 1\n#x { a: 2, b: 3\n y { a: 4 }\n list [ 1, 2 ] }\n#x { b: 5, c: 6 }\n\
                   z { #q { k: 7 } }\nb: 8\na: 9\n";
        let t = Template::from_str(src);
        let paths = [
            "a", "b", "c", "k", "x", "x.a", "x.b", "x.c", "x.y.a", "x.list", "y.a", "z.q.k",
            "#z.q.k", "z.k", "x.y", "", ".", "a.", ".a", "x..a",
        ];
        for p in paths {
            assert_eq!(t.locate_property(p), t.locate_property_linear(p), "path {p:?}");
        }
    }
}
