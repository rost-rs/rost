//! # Query Engine and Zero-Allocation Field Iterators
//!
//! This module provides facilities to query, search, and iterate over indexed
//! `.rost` documents with zero heap allocations during traversal.

use super::field::ParsedField;
use super::parser::*;

/// An iterator yielding [`ParsedField`] items on-the-fly directly from the
/// compact bytecode arena without intermediate heap allocations.
#[derive(Debug, Clone)]
pub struct FieldIter<'a> {
    data: &'a [u8],
    bytes: &'a [u8],
    idx: usize,
}

impl<'a> FieldIter<'a> {
    /// Creates a new field iterator over raw data and bytecode buffers.
    #[inline]
    pub fn new(data: &'a [u8], bytes: &'a [u8]) -> Self {
        Self {
            data,
            bytes,
            idx: 0,
        }
    }
}

impl<'a> Iterator for FieldIter<'a> {
    type Item = ParsedField<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx >= self.bytes.len() {
            return None;
        }

        let tag = self.bytes[self.idx];
        self.idx += 1;

        match tag {
            TAG_TITLE => {
                let name = resolve_slice(self.data, self.bytes, self.idx);
                let line = read_u32(self.bytes, self.idx + 8);
                self.idx += REC_NAMED - 1;
                Some(ParsedField::Title { name, line })
            }
            TAG_SUBTITLE => {
                let name = resolve_slice(self.data, self.bytes, self.idx);
                let value = resolve_slice(self.data, self.bytes, self.idx + 8);
                let line = read_u32(self.bytes, self.idx + 16);
                self.idx += REC_PAIR - 1;
                Some(ParsedField::Subtitle { name, value, line })
            }
            TAG_COMMON_TEXT => {
                let name = resolve_slice(self.data, self.bytes, self.idx);
                let value = resolve_slice(self.data, self.bytes, self.idx + 8);
                let line = read_u32(self.bytes, self.idx + 16);
                self.idx += REC_PAIR - 1;
                Some(ParsedField::CommonText { name, value, line })
            }
            TAG_ENUM => {
                let name = resolve_slice(self.data, self.bytes, self.idx);
                let raw_variants = resolve_slice(self.data, self.bytes, self.idx + 8);
                let start_line = read_u32(self.bytes, self.idx + 16);
                let end_line = read_u32(self.bytes, self.idx + 20);
                self.idx += REC_RANGE - 1;
                Some(ParsedField::Enum {
                    name,
                    raw_variants,
                    start_line,
                    end_line,
                })
            }
            TAG_LIST => {
                let name = resolve_slice(self.data, self.bytes, self.idx);
                let raw_items = resolve_slice(self.data, self.bytes, self.idx + 8);
                let start_line = read_u32(self.bytes, self.idx + 16);
                let end_line = read_u32(self.bytes, self.idx + 20);
                self.idx += REC_RANGE - 1;
                Some(ParsedField::List {
                    name,
                    raw_items,
                    start_line,
                    end_line,
                })
            }
            TAG_VAR_FUNC => {
                let var_name = resolve_slice(self.data, self.bytes, self.idx);
                let signature = resolve_slice(self.data, self.bytes, self.idx + 8);
                self.idx += REC_BINDING - 1;
                Some(ParsedField::VarFunc {
                    var_name,
                    signature,
                })
            }
            TAG_DIRECTIVE => {
                let name = resolve_slice(self.data, self.bytes, self.idx);
                let value = resolve_slice(self.data, self.bytes, self.idx + 8);
                let line = read_u32(self.bytes, self.idx + 16);
                self.idx += REC_PAIR - 1;
                Some(ParsedField::Directive { name, value, line })
            }
            TAG_BLOCK_START => {
                let name = resolve_slice(self.data, self.bytes, self.idx);
                let line = read_u32(self.bytes, self.idx + 8);
                self.idx += REC_NAMED - 1;
                Some(ParsedField::BlockStart { name, line })
            }
            TAG_BLOCK_END => {
                let line = read_u32(self.bytes, self.idx);
                self.idx += REC_BLOCK_END - 1;
                Some(ParsedField::BlockEnd { line })
            }
            _ => None,
        }
    }
}

/// Represents a scoped block view (e.g., `#button.primary { ... }` or `@theme { ... }`).
#[derive(Debug, Clone)]
pub struct BlockView<'a> {
    /// Selector or identifier of the block.
    pub name: &'a str,
    /// Parent block name if this is a nested block scope.
    pub parent: Option<&'a str>,
    /// Starting line of the block.
    pub line: u32,
    /// Fields contained inside this block scope.
    pub fields: Vec<ParsedField<'a>>,
}

impl<'a> BlockView<'a> {
    /// Searches for a property or field within this scoped block.
    pub fn get(&self, key: &str) -> Option<&ParsedField<'a>> {
        self.fields.iter().find(|f| f.name() == key)
    }

    /// Searches for a string value within this scoped block.
    pub fn get_value(&self, key: &str) -> Option<&'a str> {
        self.get(key).and_then(|f| f.value())
    }

    /// Retrieves and parses a property as a hex color (`u32`).
    pub fn get_hex(&self, key: &str) -> Option<u32> {
        self.get(key).and_then(|f| f.as_hex_color())
    }

    /// Retrieves and parses a property as a dimension/scalar float (`f32`).
    pub fn get_dim(&self, key: &str) -> Option<f32> {
        self.get(key)
            .and_then(|f| f.as_dimension())
            .map(|d| d.value as f32)
    }

    /// Retrieves and trims a property as a string slice without quotes.
    pub fn get_str(&self, key: &str) -> Option<&'a str> {
        self.get(key)
            .and_then(|f| f.value())
            .map(str::trim)
            .map(|s| s.trim_matches('"'))
    }

    /// Retrieves and parses a property as a 64-bit signed integer.
    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(|f| f.as_i64())
    }

    /// Retrieves and parses a property as a 64-bit float.
    pub fn get_f64(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(|f| f.as_f64())
    }

    /// Retrieves and parses a property as a boolean.
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.get(key).and_then(|f| f.as_bool())
    }

    /// Retrieves an action or function binding expression (e.g. `on_click = increment_counter()`).
    pub fn get_binding(&self, key: &str) -> Option<&'a str> {
        self.fields.iter().find_map(|f| match f {
            ParsedField::VarFunc { var_name, signature } if *var_name == key => Some(*signature),
            _ => None,
        })
    }

    /// Retrieves all items from a list field in this scoped block.
    pub fn get_list(&self, key: &str) -> Option<Vec<&'a str>> {
        self.get(key).and_then(|f| match f {
            ParsedField::List { .. } => Some(f.list_items()),
            _ => None,
        })
    }

    /// Retrieves the n-th element (0-indexed) from a list field in this scoped block.
    pub fn get_list_item(&self, key: &str, index: usize) -> Option<&'a str> {
        self.get(key).and_then(|f| f.list_item(index))
    }

    /// Retrieves all variants from an enum field in this scoped block.
    pub fn get_enum(&self, key: &str) -> Option<Vec<&'a str>> {
        self.get(key).and_then(|f| match f {
            ParsedField::Enum { .. } => Some(f.enum_variants()),
            _ => None,
        })
    }

    /// Retrieves the n-th variant (0-indexed) from an enum field in this scoped block.
    pub fn get_enum_variant(&self, key: &str, index: usize) -> Option<&'a str> {
        self.get(key).and_then(|f| f.enum_variant(index))
    }
}

/// Extracts all top-level and nested scoped blocks from a `.rost` document.
pub fn parse_blocks<'a>(data: &'a [u8], fields: &'a [u8]) -> Vec<BlockView<'a>> {
    let mut blocks = Vec::new();
    let iter = FieldIter::new(data, fields);
    let mut stack: Vec<BlockView<'a>> = Vec::new();

    for field in iter {
        match field {
            ParsedField::BlockStart { name, line } => {
                let parent = stack.last().map(|p| p.name);
                stack.push(BlockView {
                    name,
                    parent,
                    line,
                    fields: Vec::new(),
                });
            }
            ParsedField::BlockEnd { .. } => {
                if let Some(b) = stack.pop() {
                    blocks.push(b);
                }
            }
            other => {
                if let Some(top) = stack.last_mut() {
                    top.fields.push(other);
                }
            }
        }
    }

    while let Some(b) = stack.pop() {
        blocks.push(b);
    }

    blocks
}
