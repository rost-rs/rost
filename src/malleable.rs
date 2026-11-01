//! # Malleable Manipulation Engine ("Maleador") and Document Builder
//!
//! This module provides mutation, in-place patching, and programmatic building
//! capabilities for `.rost` files. It enables dynamic updates (such as live
//! property tweaks during 60/144 FPS game loops or UI theme hot-reloads)
//! with minimal or zero reallocations.

use super::parser::{parse_into_bytecode, read_u32};
use super::Template;
use std::fmt::Write;

/// A fluent builder for programmatically composing `.rost` documents.
///
/// # Examples
///
/// ```
/// use rost::malleable::RostBuilder;
///
/// let mut builder = RostBuilder::new();
/// builder
///     .directive("theme", "dark-nebula")
///     .block("button.primary", |b| {
///         b.property("width", "120px")
///          .property("height", "40px")
///          .enumeration("state", &["idle", "hover", "active"])
///          .binding("on_click", "emit_event()");
///     });
/// let doc = builder.finish();
///
/// assert!(doc.contains("@theme \"dark-nebula\""));
/// assert!(doc.contains("#button.primary {"));
/// ```
#[derive(Debug, Default, Clone)]
pub struct RostBuilder {
    buffer: String,
    indent_level: usize,
}

impl RostBuilder {
    /// Creates a new, empty [`RostBuilder`].
    pub fn new() -> Self {
        Self::default()
    }

    fn write_indent(&mut self) {
        for _ in 0..self.indent_level {
            self.buffer.push_str("    ");
        }
    }

    /// Adds a top-level or scoped metadata directive (e.g., `@theme "dark"`).
    pub fn directive(&mut self, name: &str, value: &str) -> &mut Self {
        self.write_indent();
        let _ = writeln!(self.buffer, "@{name} \"{value}\"");
        self
    }

    /// Adds a top-level section header (e.g., `#PlayerConfig`).
    pub fn title(&mut self, name: &str) -> &mut Self {
        self.write_indent();
        let _ = writeln!(self.buffer, "#{name}");
        self
    }

    /// Adds a secondary section header (e.g., `##state "hover"`).
    pub fn subtitle(&mut self, name: &str, value: &str) -> &mut Self {
        self.write_indent();
        let _ = writeln!(self.buffer, "##{name} \"{value}\"");
        self
    }

    /// Adds a key-value property (e.g., `width: 120px` or `title: "Hello"`).
    pub fn property(&mut self, key: &str, value: &str) -> &mut Self {
        self.write_indent();
        if value.contains(' ') && !value.starts_with('"') && !value.ends_with('"') {
            let _ = writeln!(self.buffer, "{key}: \"{value}\",");
        } else {
            let _ = writeln!(self.buffer, "{key}: {value},");
        }
        self
    }

    /// Adds an enum state set (e.g., `state | idle, hover, active |`).
    pub fn enumeration(&mut self, key: &str, variants: &[&str]) -> &mut Self {
        self.write_indent();
        let joined = variants.join(", ");
        let _ = writeln!(self.buffer, "{key} | {joined} |,");
        self
    }

    /// Adds an array or list of items (e.g., `palette [ #1e1e2e, #cdd6f4 ]`).
    pub fn list(&mut self, key: &str, items: &[&str]) -> &mut Self {
        self.write_indent();
        let joined = items.join(", ");
        let _ = writeln!(self.buffer, "{key} [ {joined} ],");
        self
    }

    /// Adds an expression or functional event binding (e.g., `on_click = emit_event()`).
    pub fn binding(&mut self, key: &str, expr: &str) -> &mut Self {
        self.write_indent();
        let _ = writeln!(self.buffer, "{key} = {expr}");
        self
    }

    /// Adds a scoped block with nested fields.
    pub fn block<F>(&mut self, selector: &str, build_inner: F) -> &mut Self
    where
        F: FnOnce(&mut RostBuilder),
    {
        self.write_indent();
        let _ = writeln!(self.buffer, "#{selector} {{");
        self.indent_level += 1;
        build_inner(self);
        self.indent_level -= 1;
        self.write_indent();
        self.buffer.push_str("}\n\n");
        self
    }

    /// Appends a comment line.
    pub fn comment(&mut self, comment: &str) -> &mut Self {
        self.write_indent();
        let _ = writeln!(self.buffer, "// {comment}");
        self
    }

    /// Finalizes the builder and returns the formatted `.rost` document string.
    pub fn finish(&self) -> String {
        self.buffer.clone()
    }

    /// Consumes the builder and returns the formatted `.rost` document string.
    pub fn into_string(self) -> String {
        self.buffer
    }

    /// Finalizes the builder and parses it immediately into a [`Template`].
    pub fn build(&self) -> Template {
        Template::from_string(self.finish())
    }
}

/// Encodes `value` so that writing it into a property slot cannot change the document
/// structure.
///
/// Inside an existing quoted slot only `\\` and `"` are escaped. In a bare slot the value is
/// written as-is when it is a single safe token sequence, and wrapped in quotes otherwise
/// (newlines, delimiters, unbalanced brackets, surrounding whitespace or empty values).
fn encode_value(value: &str, inside_quotes: bool) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;

    let escape = |v: &str| v.replace('\\', "\\\\").replace('"', "\\\"");
    if inside_quotes {
        return if value.contains(['\\', '"']) { Cow::Owned(escape(value)) } else { Cow::Borrowed(value) };
    }
    if is_bare_safe(value) {
        Cow::Borrowed(value)
    } else {
        Cow::Owned(format!("\"{}\"", escape(value)))
    }
}

/// Returns `true` if `value` round-trips as an unquoted property value.
fn is_bare_safe(value: &str) -> bool {
    if value.is_empty() || value.trim() != value {
        return false;
    }
    let mut depth: i32 = 0;
    for b in value.bytes() {
        match b {
            b'\n' | b'\r' | b';' | b'{' | b'}' | b'"' | b'|' => return false,
            b',' if depth == 0 => return false,
            b'(' | b'[' => depth += 1,
            b')' | b']' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

/// Mutates and patches a `.rost` document buffer.
pub struct MalleablePatcher;

impl MalleablePatcher {
    /// Patches a property value inside a [`Template`].
    ///
    /// `target_key` is a bare top-level key or a dot path into nested blocks
    /// (`"audio.volume"`), resolved exactly like [`Template::query_raw_value`].
    ///
    /// # Performance Guarantee
    /// - If the new value has the **exact same byte length** as the existing value,
    ///   the replacement occurs directly in-place in memory ($O(1)$) with **zero reallocations**
    ///   and preserves existing bytecode offsets.
    /// - If lengths differ, the buffer is updated and re-indexed.
    ///
    /// Returns `true` if the key was found and updated, or `false` otherwise.
    pub fn patch_property(template: &mut Template, target_key: &str, new_value: &str) -> bool {
        let Some(rec) = template.locate_property(target_key) else {
            return false;
        };
        let found_val_offset = read_u32(&template.fields, rec + 9) as usize;
        let found_val_len = read_u32(&template.fields, rec + 13) as usize;

        let quoted = found_val_offset > 0
            && template.data[found_val_offset - 1] == b'"'
            && template.data.get(found_val_offset + found_val_len) == Some(&b'"');
        let encoded = encode_value(new_value, quoted);

        if found_val_len == encoded.len() {
            // Fast in-place zero-copy rewrite:
            template.data[found_val_offset..found_val_offset + found_val_len]
                .copy_from_slice(encoded.as_bytes());
            true
        } else {
            // Re-slicing and re-indexing:
            let mut new_data = Vec::with_capacity(template.data.len() + encoded.len());
            new_data.extend_from_slice(&template.data[..found_val_offset]);
            new_data.extend_from_slice(encoded.as_bytes());
            new_data.extend_from_slice(&template.data[found_val_offset + found_val_len..]);

            let new_fields = parse_into_bytecode(&new_data);
            template.data = new_data;
            template.fields = new_fields;
            template.index = Default::default();
            true
        }
    }
}
