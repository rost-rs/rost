//! # Block tree
//!
//! Every `{ … }` block of a document is a record of the bytecode that knows its parent,
//! its matching end, its first child and its next sibling (see
//! [`parser::REC_BLOCK_START`]). [`Block`] is a copyable handle over such a record:
//! navigating the tree and reading a block's properties never allocates and never scans
//! more than the block itself.

use crate::field::ParsedField;
use crate::parser::{self, read_u32, resolve_slice, NO_RECORD};
use crate::query::{BlockView, FieldIter};

/// A block of a [`Template`](crate::Template), borrowed from its document.
///
/// Obtained from [`Template::block`](crate::Template::block) or
/// [`Template::root_blocks`](crate::Template::root_blocks).
///
/// # Examples
///
/// ```
/// use rost::Template;
///
/// let doc = Template::parse("#window { width: 800\n  #title { text: \"Hi\" }\n  #body { }\n}").unwrap();
/// let window = doc.block("window").unwrap();
/// assert_eq!(window.name(), "window");
/// assert_eq!(window.get("width").map(|f| f.value().unwrap().to_owned()), Some("800".into()));
/// let names: Vec<&str> = window.children().map(|b| b.name()).collect();
/// assert_eq!(names, ["title", "body"]);
/// assert_eq!(window.children().next().unwrap().parent().unwrap().name(), "window");
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Block<'a> {
    data: &'a [u8],
    fields: &'a [u8],
    rec: usize,
}

impl<'a> Block<'a> {
    pub(crate) fn new(data: &'a [u8], fields: &'a [u8], rec: usize) -> Self {
        debug_assert_eq!(fields[rec], parser::TAG_BLOCK_START);
        Self { data, fields, rec }
    }

    /// The block's name; a leading `#` selector mark is not part of it.
    pub fn name(&self) -> &'a str {
        resolve_slice(self.data, self.fields, self.rec + 1)
    }

    /// 1-based line of the opening brace's selector.
    pub fn line(&self) -> u32 {
        read_u32(self.fields, self.rec + 9)
    }

    fn link(&self, at: usize) -> Option<Block<'a>> {
        match read_u32(self.fields, self.rec + at) {
            NO_RECORD => None,
            rec => Some(Block::new(self.data, self.fields, rec as usize)),
        }
    }

    /// The enclosing block, or `None` at the top level.
    pub fn parent(&self) -> Option<Block<'a>> {
        self.link(parser::BLOCK_PARENT)
    }

    /// The first block nested directly inside this one.
    pub fn first_child(&self) -> Option<Block<'a>> {
        self.link(parser::BLOCK_FIRST_CHILD)
    }

    /// The next block with the same parent.
    pub fn next_sibling(&self) -> Option<Block<'a>> {
        self.link(parser::BLOCK_NEXT_SIBLING)
    }

    /// The blocks nested directly inside this one, in document order.
    pub fn children(&self) -> impl Iterator<Item = Block<'a>> + use<'a> {
        std::iter::successors(self.first_child(), Block::next_sibling)
    }

    /// The first block directly inside this one named `name`.
    pub fn child(&self, name: &str) -> Option<Block<'a>> {
        self.children().find(|c| c.name() == name)
    }

    /// Follows `a.b.c` down from this block, one child per segment. Walks the tree links
    /// written while parsing, so it needs no index: cost is the number of siblings
    /// compared at each level.
    pub fn descend(&self, path: &str) -> Option<Block<'a>> {
        path.split('.').try_fold(*self, |block, name| block.child(name))
    }

    /// Index of the record after this block's end record (the end of the bytecode if the
    /// block is never closed).
    pub(crate) fn after(&self) -> usize {
        match read_u32(self.fields, self.rec + parser::BLOCK_END) {
            NO_RECORD => self.fields.len(),
            end => end as usize + parser::REC_BLOCK_END,
        }
    }

    /// The properties, lists, enums and bindings written directly in this block, in
    /// document order. Nested blocks are skipped as a whole.
    pub fn fields(&self) -> BlockFields<'a> {
        BlockFields {
            data: self.data,
            fields: self.fields,
            idx: self.rec + parser::REC_BLOCK_START,
            end: self.after().saturating_sub(parser::REC_BLOCK_END),
        }
    }

    /// Copies this block into an owned-fields [`BlockView`] (allocates its field list).
    pub fn to_view(&self) -> BlockView<'a> {
        BlockView {
            name: self.name(),
            parent: self.parent().map(|p| p.name()),
            line: self.line(),
            fields: self.fields().collect(),
        }
    }

    /// The first field named `key` in this block (not in nested blocks).
    pub fn get(&self, key: &str) -> Option<ParsedField<'a>> {
        self.fields().find(|f| f.name() == key)
    }
}

/// Iterator over the fields of one block, returned by [`Block::fields`].
#[derive(Debug, Clone)]
pub struct BlockFields<'a> {
    data: &'a [u8],
    fields: &'a [u8],
    idx: usize,
    end: usize,
}

impl<'a> Iterator for BlockFields<'a> {
    type Item = ParsedField<'a>;

    fn next(&mut self) -> Option<ParsedField<'a>> {
        while self.idx < self.end {
            let tag = self.fields[self.idx];
            if tag == parser::TAG_BLOCK_START {
                self.idx = Block::new(self.data, self.fields, self.idx).after();
                continue;
            }
            let len = parser::record_len(tag);
            let field = FieldIter::new(self.data, &self.fields[self.idx..self.idx + len]).next();
            self.idx += len;
            return field;
        }
        None
    }
}

/// Iterator over the top-level blocks of a document, returned by
/// [`Template::root_blocks`](crate::Template::root_blocks).
#[derive(Debug, Clone)]
pub struct RootBlocks<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) fields: &'a [u8],
    pub(crate) idx: usize,
}

impl<'a> Iterator for RootBlocks<'a> {
    type Item = Block<'a>;

    fn next(&mut self) -> Option<Block<'a>> {
        while self.idx < self.fields.len() {
            let tag = self.fields[self.idx];
            if tag == parser::TAG_BLOCK_START {
                let block = Block::new(self.data, self.fields, self.idx);
                self.idx = block.after();
                return Some(block);
            }
            self.idx += parser::record_len(tag);
        }
        None
    }
}
