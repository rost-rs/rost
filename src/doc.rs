//! # `Doc`: a borrowed document tree on a flat tape
//!
//! [`Doc`] holds a whole JSON or `.rost` document as one `Vec` of 16-byte nodes written in
//! document order. Containers record how many entries they have and how many nodes their
//! subtree spans, so reading a value never chases pointers and skipping a subtree is one
//! addition. Keys and strings are not copied: a node stores their offset in the source
//! text (only strings with escapes are decoded into owned storage).
//!
//! Building a [`Doc`] makes one allocation that grows, instead of one per string, list and
//! block like [`Value`](crate::Value), which is what makes it several times faster to
//! build. Navigate it with [`ValueRef`], a copyable handle.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::error::ParseError;
use crate::index::{finish, mix_segment};

/// (key hash, value node) of every entry of an object, sorted.
type ObjectIndex = Vec<(u64, u32)>;

/// Objects with more entries than this get a hash index on their first lookup.
const LINEAR_LOOKUP_MAX: u32 = 16;
const KEY_SEED: u64 = 0x1F83_D9AB_FB41_BD6B;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Node {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    /// A string that is a slice of the source.
    Str { off: u32, len: u32 },
    /// A decoded string (it had escapes), in [`Tape::owned`].
    Owned(u32),
    /// `len` elements; the subtree spans `skip` nodes, itself included.
    Array { len: u32, skip: u32 },
    /// `len` entries, stored as key, value, key, value…
    Object { len: u32, skip: u32 },
}

/// The nodes under construction.
#[derive(Debug)]
pub(crate) struct Tape {
    base: usize,
    src_len: usize,
    pub(crate) nodes: Vec<Node>,
    pub(crate) owned: Vec<String>,
}

impl Tape {
    pub(crate) fn new(src: &str) -> Self {
        Self {
            base: src.as_ptr() as usize,
            src_len: src.len(),
            // Roughly one node per eight bytes of typical JSON.
            nodes: Vec::with_capacity(src.len() / 8 + 1),
            owned: Vec::new(),
        }
    }

    #[inline]
    pub(crate) fn push(&mut self, node: Node) -> u32 {
        self.nodes.push(node);
        (self.nodes.len() - 1) as u32
    }

    /// Adds a string, by reference when it is a slice of the source.
    #[inline]
    pub(crate) fn string(&mut self, s: &str) -> u32 {
        let off = (s.as_ptr() as usize).wrapping_sub(self.base);
        if off <= self.src_len && off + s.len() <= self.src_len {
            self.push(Node::Str { off: off as u32, len: s.len() as u32 })
        } else {
            self.owned.push(s.to_owned());
            self.push(Node::Owned((self.owned.len() - 1) as u32))
        }
    }

    pub(crate) fn owned_string(&mut self, s: String) -> u32 {
        self.owned.push(s);
        self.push(Node::Owned((self.owned.len() - 1) as u32))
    }

    pub(crate) fn open(&mut self, object: bool) -> u32 {
        self.push(if object { Node::Object { len: 0, skip: 0 } } else { Node::Array { len: 0, skip: 0 } })
    }

    pub(crate) fn close(&mut self, at: u32, len: u32) {
        let skip = self.nodes.len() as u32 - at;
        self.nodes[at as usize] = match self.nodes[at as usize] {
            Node::Object { .. } => Node::Object { len, skip },
            _ => Node::Array { len, skip },
        };
    }
}

/// A parsed document that borrows its text.
///
/// # Examples
///
/// ```
/// use rost::doc::Doc;
///
/// let doc = Doc::from_json(r#"{"name": "rost", "tags": ["fast", "small"], "stars": 3}"#).unwrap();
/// let root = doc.root();
/// assert_eq!(root.get("name").and_then(|v| v.as_str()), Some("rost"));
/// assert_eq!(root.get("stars").and_then(|v| v.as_i64()), Some(3));
/// let tags: Vec<&str> = root.get("tags").unwrap().iter().filter_map(|v| v.as_str()).collect();
/// assert_eq!(tags, ["fast", "small"]);
/// assert!(root.get("missing").is_none());
/// ```
#[derive(Debug)]
pub struct Doc<'a> {
    src: &'a str,
    nodes: Vec<Node>,
    owned: Vec<String>,
    /// Hash indexes of the large objects, built on their first lookup.
    indexes: Mutex<HashMap<u32, Arc<ObjectIndex>>>,
}

impl<'a> Doc<'a> {
    pub(crate) fn from_tape(src: &'a str, tape: Tape) -> Self {
        Self { src, nodes: tape.nodes, owned: tape.owned, indexes: Mutex::new(HashMap::new()) }
    }

    /// Parses strict RFC 8259 JSON (see [`crate::json`]).
    pub fn from_json(src: &'a str) -> Result<Self, ParseError> {
        crate::json::to_doc(src)
    }

    /// Reads a `.rost` document, or any JSON object, without a schema.
    #[cfg(feature = "serde")]
    pub fn from_rost(src: &'a str) -> Result<Self, crate::de::DeError> {
        crate::doc_rost::build(src)
    }

    /// The document's top-level value.
    pub fn root(&self) -> ValueRef<'_, 'a> {
        ValueRef { doc: self, idx: 0 }
    }

    fn string(&self, node: Node) -> &str {
        match node {
            Node::Str { off, len } => &self.src[off as usize..(off + len) as usize],
            Node::Owned(i) => &self.owned[i as usize],
            _ => "",
        }
    }

    fn skip(&self, idx: u32) -> u32 {
        match self.nodes[idx as usize] {
            Node::Array { skip, .. } | Node::Object { skip, .. } => skip,
            _ => 1,
        }
    }

    /// Value node of the entry whose key is `key`, the last one if it is repeated.
    fn lookup(&self, object: u32, len: u32, key: &str) -> Option<u32> {
        if len <= LINEAR_LOOKUP_MAX {
            let mut found = None;
            let mut at = object + 1;
            for _ in 0..len {
                let value = at + 1;
                if self.string(self.nodes[at as usize]) == key {
                    found = Some(value);
                }
                at = value + self.skip(value);
            }
            return found;
        }
        let index = {
            let mut indexes = self.indexes.lock().unwrap_or_else(|e| e.into_inner());
            indexes.entry(object).or_insert_with(|| Arc::new(self.build_index(object, len))).clone()
        };
        let hash = finish(mix_segment(KEY_SEED, key.as_bytes()));
        let first = index.partition_point(|&(h, _)| h < hash);
        index[first..]
            .iter()
            .take_while(|&&(h, _)| h == hash)
            .filter(|&&(_, value)| self.string(self.nodes[value as usize - 1]) == key)
            .map(|&(_, value)| value)
            .last()
    }

    /// (key hash, value node) of every entry, sorted by hash then document order.
    fn build_index(&self, object: u32, len: u32) -> ObjectIndex {
        let mut entries = Vec::with_capacity(len as usize);
        let mut at = object + 1;
        for _ in 0..len {
            let value = at + 1;
            let key = self.string(self.nodes[at as usize]);
            entries.push((finish(mix_segment(KEY_SEED, key.as_bytes())), value));
            at = value + self.skip(value);
        }
        entries.sort_unstable();
        entries
    }
}

/// A value of a [`Doc`]. Cheap to copy; reading never allocates.
#[derive(Debug, Clone, Copy)]
pub struct ValueRef<'d, 'a> {
    doc: &'d Doc<'a>,
    idx: u32,
}

impl<'d, 'a> ValueRef<'d, 'a> {
    fn node(&self) -> Node {
        self.doc.nodes[self.idx as usize]
    }

    /// `true` for `null`.
    pub fn is_null(&self) -> bool {
        self.node() == Node::Null
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self.node() {
            Node::Bool(b) => Some(b),
            _ => None,
        }
    }

    /// Integers that fit an `i64`.
    pub fn as_i64(&self) -> Option<i64> {
        match self.node() {
            Node::Int(i) => Some(i),
            _ => None,
        }
    }

    /// Any number, as a float.
    pub fn as_f64(&self) -> Option<f64> {
        match self.node() {
            Node::Int(i) => Some(i as f64),
            Node::Float(f) => Some(f),
            _ => None,
        }
    }

    /// The text of a string value, borrowed from the document.
    pub fn as_str(&self) -> Option<&'d str> {
        match self.node() {
            n @ (Node::Str { .. } | Node::Owned(_)) => Some(self.doc.string(n)),
            _ => None,
        }
    }

    pub fn is_array(&self) -> bool {
        matches!(self.node(), Node::Array { .. })
    }

    pub fn is_object(&self) -> bool {
        matches!(self.node(), Node::Object { .. })
    }

    /// Number of elements of an array or entries of an object; `0` for other values.
    pub fn len(&self) -> usize {
        match self.node() {
            Node::Array { len, .. } | Node::Object { len, .. } => len as usize,
            _ => 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The value of `key` in an object. A repeated key yields its last value, like
    /// [`json::from_str`](crate::json::from_str). Objects with more than 16 entries build a
    /// hash index on the first lookup.
    pub fn get(&self, key: &str) -> Option<ValueRef<'d, 'a>> {
        match self.node() {
            Node::Object { len, .. } => {
                self.doc.lookup(self.idx, len, key).map(|idx| ValueRef { doc: self.doc, idx })
            }
            _ => None,
        }
    }

    /// The `n`-th element of an array.
    pub fn at(&self, n: usize) -> Option<ValueRef<'d, 'a>> {
        self.iter().nth(n)
    }

    /// The elements of an array (empty for other values).
    pub fn iter(&self) -> impl Iterator<Item = ValueRef<'d, 'a>> + use<'d, 'a> {
        let (doc, len) = match self.node() {
            Node::Array { len, .. } => (self.doc, len),
            _ => (self.doc, 0),
        };
        let mut at = self.idx + 1;
        (0..len).map(move |_| {
            let item = ValueRef { doc, idx: at };
            at += doc.skip(at);
            item
        })
    }

    /// The `(key, value)` entries of an object, in document order, repeated keys included.
    pub fn entries(&self) -> impl Iterator<Item = (&'d str, ValueRef<'d, 'a>)> + use<'d, 'a> {
        let (doc, len) = match self.node() {
            Node::Object { len, .. } => (self.doc, len),
            _ => (self.doc, 0),
        };
        let mut at = self.idx + 1;
        (0..len).map(move |_| {
            let key = doc.string(doc.nodes[at as usize]);
            let value = ValueRef { doc, idx: at + 1 };
            at += 1 + doc.skip(at + 1);
            (key, value)
        })
    }

    /// Copies this value into an owned [`Value`](crate::Value).
    #[cfg(feature = "serde")]
    pub fn to_value(&self) -> crate::Value {
        use crate::Value;
        match self.node() {
            Node::Null => Value::Null,
            Node::Bool(b) => Value::Bool(b),
            Node::Int(i) => Value::Int(i),
            Node::Float(f) => Value::Float(f),
            n @ (Node::Str { .. } | Node::Owned(_)) => Value::String(self.doc.string(n).to_owned()),
            Node::Array { .. } => Value::List(self.iter().map(|v| v.to_value()).collect()),
            Node::Object { .. } => {
                let mut map = crate::Map::new();
                for (k, v) in self.entries() {
                    map.insert(k.to_owned(), v.to_value());
                }
                Value::Block(map)
            }
        }
    }
}
