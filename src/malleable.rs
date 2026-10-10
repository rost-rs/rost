//! # Malleable Manipulation Engine ("Maleador") and Document Builder
//!
//! This module provides mutation, in-place patching, and programmatic building
//! capabilities for `.rost` files. It enables dynamic updates (such as live
//! property tweaks during 60/144 FPS game loops or UI theme hot-reloads)
//! with minimal or zero reallocations.

use super::parser::parse_into_bytecode;
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

/// Mutates and patches a `.rost` document buffer.
pub struct MalleablePatcher;

impl MalleablePatcher {
    /// Patches a property value inside a [`Template`].
    ///
    /// # Performance Guarantee
    /// - If the new value has the **exact same byte length** as the existing value,
    ///   the replacement occurs directly in-place in memory ($O(1)$) with **zero reallocations**
    ///   and preserves existing bytecode offsets.
    /// - If lengths differ, the buffer is updated and re-indexed.
    ///
    /// Returns `true` if the key was found and updated, or `false` otherwise.
    pub fn patch_property(template: &mut Template, target_key: &str, new_value: &str) -> bool {
        let (found_val_offset, found_val_len) = {
            let mut found = None;
            for field in template.iter() {
                if let super::field::ParsedField::CommonText { name, value, .. } = field
                    && name == target_key
                {
                    // Locate byte offset of `value` inside `template.data`
                    let base_ptr = template.data.as_ptr() as usize;
                    let val_ptr = value.as_ptr() as usize;
                    if val_ptr >= base_ptr && val_ptr + value.len() <= base_ptr + template.data.len() {
                        let off = val_ptr - base_ptr;
                        found = Some((off, value.len()));
                        break;
                    }
                }
            }
            match found {
                Some(pair) => pair,
                None => return false,
            }
        };

        if found_val_len == new_value.len() {
            // Fast in-place zero-copy rewrite:
            template.data[found_val_offset..found_val_offset + found_val_len]
                .copy_from_slice(new_value.as_bytes());
            true
        } else {
            // Re-slicing and re-indexing:
            let mut new_data = Vec::with_capacity(template.data.len() + new_value.len());
            new_data.extend_from_slice(&template.data[..found_val_offset]);
            new_data.extend_from_slice(new_value.as_bytes());
            new_data.extend_from_slice(&template.data[found_val_offset + found_val_len..]);

            let new_fields = parse_into_bytecode(&new_data);
            template.data = new_data;
            template.fields = new_fields;
            true
        }
    }
}
