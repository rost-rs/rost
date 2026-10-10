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

extern crate self as utilrs_rost;

pub mod field;
pub mod malleable;
pub mod parser;
pub mod query;
pub mod schema;
pub mod watcher;

pub use field::{Dimension, ParsedField};
pub use malleable::{MalleablePatcher, RostBuilder};
pub use query::{parse_blocks, BlockView, FieldIter};
pub use watcher::{HotReloadWatcher, ReloadCallback};

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The primary parsed representation of a `.rost` document.
///
/// Contains the raw byte buffer (`data`) and the binary offset index arena (`fields`).
#[derive(Debug, Clone)]
pub struct Template {
    /// File path origin of the template, if loaded from filesystem.
    pub path: PathBuf,
    /// Raw unparsed byte content of the document.
    pub data: Vec<u8>,
    /// Compact bytecode array storing tags and Little-Endian `u32` offsets into `data`.
    pub fields: Vec<u8>,
    /// Native zero-CPU filesystem watcher if hot-reloading was enabled.
    pub(crate) watcher: Option<Arc<HotReloadWatcher>>,
}

impl Template {
    /// Dumps and parses a `.rost` document from the given file path.
    ///
    /// Preserves exact backward compatibility with prior versions of `Template::dump`.
    ///
    /// # Panics
    /// Does not panic; falls back to an empty template if the file cannot be read.
    pub fn dump(path: PathBuf) -> Self {
        let (data, fields) = Self::reader(&path);
        Self {
            path,
            data,
            fields,
            watcher: None,
        }
    }

    /// Loads a `.rost` document from disk with hot-reloading enabled by default (`hot_reload = true`).
    pub fn load<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        Self::load_with_options(path, true)
    }

    /// Loads a `.rost` document from disk, allowing explicit control over hot-reloading.
    pub fn load_with_options<P: AsRef<Path>>(path: P, hot_reload: bool) -> std::io::Result<Self> {
        let p = path.as_ref();
        let data = fs::read(p)?;
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
        })
    }

    /// Loads and parses a `.rost` document from disk (with hot-reloading enabled by default).
    pub fn from_file<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        Self::load(path)
    }

    /// Creates and parses a [`Template`] from an in-memory byte buffer.
    pub fn from_bytes(data: Vec<u8>) -> Self {
        let fields = parser::parse_into_bytecode(&data);
        Self {
            path: PathBuf::new(),
            data,
            fields,
            watcher: None,
        }
    }

    /// Creates and parses a [`Template`] from a string slice.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(content: &str) -> Self {
        Self::from_bytes(content.as_bytes().to_vec())
    }

    /// Creates and parses a [`Template`] from an owned `String`.
    pub fn from_string(content: String) -> Self {
        Self::from_bytes(content.into_bytes())
    }

    /// Starts a fluent [`RostBuilder`] to assemble a template.
    pub fn builder() -> RostBuilder {
        RostBuilder::new()
    }

    /// Reads a file from disk and parses it into data and field offset vectors.
    fn reader(path: &Path) -> (Vec<u8>, Vec<u8>) {
        let raw = fs::read(path).unwrap_or_default();
        let fields = parser::parse_into_bytecode(&raw);
        (raw, fields)
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

    /// Retrieves the first field matching the provided key or identifier.
    pub fn get(&self, key: &str) -> Option<ParsedField<'_>> {
        self.iter().find(|f| f.name() == key)
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

    /// Finds a specific block by its selector or name (e.g., `#button.primary`).
    pub fn find_block(&self, selector: &str) -> Option<BlockView<'_>> {
        let clean = selector.strip_prefix('#').unwrap_or(selector);
        self.blocks()
            .into_iter()
            .find(|b| b.name == selector || b.name == clean)
    }

    /// Patches an existing property in-place ("maleador").
    ///
    /// If the new value length matches the previous value length, replacement occurs
    /// in $O(1)$ directly in the underlying buffer with zero reallocations.
    pub fn patch_value(&mut self, key: &str, new_value: &str) -> bool {
        MalleablePatcher::patch_property(self, key, new_value)
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
        if let Some(w) = &self.watcher {
            if let Some(new_tmpl) = w.check_reload() {
                self.data = new_tmpl.data;
                self.fields = new_tmpl.fields;
                return true;
            }
        }
        false
    }

    /// Queries a field across scoped blocks using dot-notation (e.g. `"palette.bg_window"` or `"title"`).
    pub fn query_field(&self, path: &str) -> Option<ParsedField<'_>> {
        if let Some((block_sel, key)) = path.split_once('.') {
            self.find_block(block_sel).and_then(|b| b.get(key).cloned())
        } else {
            self.get(path)
        }
    }

    /// Queries a hex color (`u32`) using dot-notation or top-level key.
    pub fn query_hex(&self, path: &str) -> Option<u32> {
        if let Some((block_sel, key)) = path.split_once('.') {
            self.find_block(block_sel).and_then(|b| b.get_hex(key))
        } else {
            self.get(path).and_then(|f| f.as_hex_color())
        }
    }

    /// Queries a dimension/float (`f32`) using dot-notation or top-level key.
    pub fn query_dim(&self, path: &str) -> Option<f32> {
        if let Some((block_sel, key)) = path.split_once('.') {
            self.find_block(block_sel).and_then(|b| b.get_dim(key))
        } else {
            self.get(path)
                .and_then(|f| f.as_dimension())
                .map(|d| d.value as f32)
        }
    }

    /// Queries a string slice using dot-notation or top-level key.
    pub fn query_str(&self, path: &str) -> Option<&str> {
        if let Some((block_sel, key)) = path.split_once('.') {
            self.find_block(block_sel).and_then(|b| b.get_str(key))
        } else {
            self.get(path)
                .and_then(|f| f.value())
                .map(str::trim)
                .map(|s| s.trim_matches('"'))
        }
    }

    /// Queries an integer (`i64`) using dot-notation or top-level key.
    pub fn query_i64(&self, path: &str) -> Option<i64> {
        if let Some((block_sel, key)) = path.split_once('.') {
            self.find_block(block_sel).and_then(|b| b.get_i64(key))
        } else {
            self.get(path).and_then(|f| f.as_i64())
        }
    }

    /// Queries a float (`f64`) using dot-notation or top-level key.
    pub fn query_f64(&self, path: &str) -> Option<f64> {
        if let Some((block_sel, key)) = path.split_once('.') {
            self.find_block(block_sel).and_then(|b| b.get_f64(key))
        } else {
            self.get(path).and_then(|f| f.as_f64())
        }
    }

    /// Queries a boolean using dot-notation or top-level key.
    pub fn query_bool(&self, path: &str) -> Option<bool> {
        if let Some((block_sel, key)) = path.split_once('.') {
            self.find_block(block_sel).and_then(|b| b.get_bool(key))
        } else {
            self.get(path).and_then(|f| f.as_bool())
        }
    }

    /// Saves the current template buffer back to disk.
    pub fn save<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        fs::write(path, &self.data)
    }
}

impl std::str::FromStr for Template {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_str(s))
    }
}
