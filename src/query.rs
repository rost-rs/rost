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
                self.idx += 12;
                Some(ParsedField::Title { name, line })
            }
            TAG_SUBTITLE => {
                let name = resolve_slice(self.data, self.bytes, self.idx);
                let value = resolve_slice(self.data, self.bytes, self.idx + 8);
                let line = read_u32(self.bytes, self.idx + 16);
                self.idx += 20;
                Some(ParsedField::Subtitle { name, value, line })
            }
            TAG_COMMON_TEXT => {
                let name = resolve_slice(self.data, self.bytes, self.idx);
                let value = resolve_slice(self.data, self.bytes, self.idx + 8);
                let line = read_u32(self.bytes, self.idx + 16);
                self.idx += 20;
                Some(ParsedField::CommonText { name, value, line })
            }
            TAG_ENUM => {
                let name = resolve_slice(self.data, self.bytes, self.idx);
                let raw_variants = resolve_slice(self.data, self.bytes, self.idx + 8);
                let start_line = read_u32(self.bytes, self.idx + 16);
                let end_line = read_u32(self.bytes, self.idx + 20);
                self.idx += 24;
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
                self.idx += 24;
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
                self.idx += 16;
                Some(ParsedField::VarFunc {
                    var_name,
                    signature,
                })
            }
            TAG_DIRECTIVE => {
                let name = resolve_slice(self.data, self.bytes, self.idx);
                let value = resolve_slice(self.data, self.bytes, self.idx + 8);
                let line = read_u32(self.bytes, self.idx + 16);
                self.idx += 20;
                Some(ParsedField::Directive { name, value, line })
            }
            TAG_BLOCK_START => {
                let name = resolve_slice(self.data, self.bytes, self.idx);
                let line = read_u32(self.bytes, self.idx + 8);
                self.idx += 12;
                Some(ParsedField::BlockStart { name, line })
            }
            TAG_BLOCK_END => {
                let line = read_u32(self.bytes, self.idx);
                self.idx += 4;
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
}

/// Extracts all top-level and nested scoped blocks from a `.rost` document.
pub fn parse_blocks<'a>(data: &'a [u8], fields: &'a [u8]) -> Vec<BlockView<'a>> {
    let mut blocks = Vec::new();
    let iter = FieldIter::new(data, fields);
    let mut current_block: Option<BlockView<'a>> = None;

    for field in iter {
        match field {
            ParsedField::BlockStart { name, line } => {
                if let Some(prev) = current_block.take() {
                    blocks.push(prev);
                }
                current_block = Some(BlockView {
                    name,
                    line,
                    fields: Vec::new(),
                });
            }
            ParsedField::BlockEnd { .. } => {
                if let Some(b) = current_block.take() {
                    blocks.push(b);
                }
            }
            other => {
                if let Some(ref mut b) = current_block {
                    b.fields.push(other);
                }
            }
        }
    }

    if let Some(b) = current_block {
        blocks.push(b);
    }

    blocks
}
