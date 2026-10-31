//! # High-Throughput Streaming Serde Deserializer for `.rost` Documents
//!
//! Provides direct zero-copy, zero-allocation deserialization from `.rost`
//! bytecode streams directly into any Rust data structure implementing
//! [`serde::Deserialize`].

use std::fmt;
use serde::de::value::BorrowedStrDeserializer;
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};

use super::parser::*;
use super::query::BlockView;
use super::Template;

/// Error type encountered during `.rost` deserialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeError(pub String);

impl fmt::Display for DeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for DeError {}

impl de::Error for DeError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        DeError(msg.to_string())
    }
}

#[inline(always)]
fn strip_hash(s: &str) -> &str {
    s.strip_prefix('#').unwrap_or(s)
}
use super::field::{
    parse_fast_f32, parse_fast_f64, parse_fast_u32, unescape, unquote, QuoteStyle, TopLevelSplit,
};
use std::borrow::Cow;

/// Parses an integer literal of any width without narrowing through `u32`/`f32`.
///
/// Accepts decimal values with an optional unit suffix (`42`, `-7`, `120px`) and hex
/// literals (`#ff0000`, `0xff`). Returns `None` when the value does not fit `T`.
fn parse_integer<T: TryFrom<i128>>(value: &str) -> Option<T> {
    let raw = value.trim().trim_end_matches(';');
    if raw.starts_with('#') || raw.starts_with("0x") || raw.starts_with("0X") {
        return parse_fast_u32(raw).and_then(|v| T::try_from(v as i128).ok());
    }
    let digits = raw.trim_end_matches(|c: char| c == '%' || c.is_ascii_alphabetic());
    T::try_from(digits.parse::<i128>().ok()?).ok()
}

macro_rules! deserialize_integers {
    ($($method:ident => $visit:ident: $ty:ty),* $(,)?) => {$(
        #[inline]
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
            match parse_integer::<$ty>(self.value) {
                Some(v) => visitor.$visit(v),
                None => Err(DeError(format!(
                    "cannot parse '{}' as {}: {}",
                    self.name,
                    stringify!($ty),
                    self.value
                ))),
            }
        }
    )*};
}

struct FieldDeserializer<'a> {
    name: &'a str,
    value: &'a str,
    quote: QuoteStyle,
}

impl<'a> FieldDeserializer<'a> {
    /// The value as a string: escapes decoded for quoted values, verbatim for raw ones,
    /// trimmed for bare tokens.
    fn text(&self) -> Cow<'a, str> {
        match self.quote {
            QuoteStyle::Quoted => unescape(self.value),
            QuoteStyle::Raw => Cow::Borrowed(self.value),
            QuoteStyle::Bare => Cow::Borrowed(self.value.trim().trim_end_matches(';').trim_matches('"')),
        }
    }

    fn visit_text<V: Visitor<'a>>(&self, visitor: V) -> Result<V::Value, DeError> {
        match self.text() {
            Cow::Borrowed(s) => visitor.visit_borrowed_str(s),
            Cow::Owned(s) => visitor.visit_string(s),
        }
    }
}

impl<'de> Deserializer<'de> for FieldDeserializer<'de> {
    type Error = DeError;

    #[inline]
    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        if self.quote != QuoteStyle::Bare {
            return self.visit_text(visitor);
        }
        let s = self.value.trim().trim_end_matches(';');
        if s == "null" {
            return visitor.visit_unit();
        }
        if let Ok(v) = s.parse::<u64>() {
            return visitor.visit_u64(v);
        }
        if let Ok(v) = s.parse::<i64>() {
            return visitor.visit_i64(v);
        }
        if (s.starts_with('#') || s.starts_with("0x") || s.starts_with("0X"))
            && let Some(u) = parse_fast_u32(s)
        {
            return visitor.visit_u32(u);
        }
        if let Some(f) = parse_fast_f64(s) {
            return visitor.visit_f64(f);
        }
        if s == "true" {
            return visitor.visit_bool(true);
        }
        if s == "false" {
            return visitor.visit_bool(false);
        }
        visitor.visit_borrowed_str(s.trim_matches('"'))
    }

    #[inline]
    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let s = self.value.trim().trim_end_matches(';');
        match s {
            "true" | "1" | "yes" | "on" => visitor.visit_bool(true),
            "false" | "0" | "no" | "off" => visitor.visit_bool(false),
            _ => Err(DeError(format!("cannot parse '{}' as bool: {}", self.name, s))),
        }
    }

    deserialize_integers! {
        deserialize_i8 => visit_i8: i8,
        deserialize_i16 => visit_i16: i16,
        deserialize_i32 => visit_i32: i32,
        deserialize_i64 => visit_i64: i64,
        deserialize_u8 => visit_u8: u8,
        deserialize_u16 => visit_u16: u16,
        deserialize_u32 => visit_u32: u32,
        deserialize_u64 => visit_u64: u64
    }

    #[inline]
    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        if let Some(v) = parse_fast_f32(self.value) {
            return visitor.visit_f32(v);
        }
        Err(DeError(format!("cannot parse '{}' as f32: {}", self.name, self.value)))
    }

    #[inline]
    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        if let Some(v) = parse_fast_f64(self.value) {
            return visitor.visit_f64(v);
        }
        Err(DeError(format!("cannot parse '{}' as f64: {}", self.name, self.value)))
    }

    #[inline]
    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.visit_text(visitor)
    }

    #[inline]
    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_string(self.text().into_owned())
    }

    #[inline]
    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        if self.quote == QuoteStyle::Bare && self.value.trim() == "null" {
            return visitor.visit_none();
        }
        visitor.visit_some(self)
    }

    serde::forward_to_deserialize_any! {
        char bytes byte_buf unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

// ==============================================================================
// 2. LIST AND ARRAY DESERIALIZER
// ==============================================================================

struct RawListDeserializer<'a> {
    raw: &'a str,
}

impl<'de> Deserializer<'de> for RawListDeserializer<'de> {
    type Error = DeError;

    #[inline]
    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_seq(visitor)
    }

    #[inline]
    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let clean = self.raw.trim().trim_start_matches('[').trim_end_matches(']');
        let items: Vec<&'de str> = TopLevelSplit::new(clean, |b| b == b',').collect();
        visitor.visit_seq(RawSeqAccess { items, idx: 0 })
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

struct RawSeqAccess<'a> {
    items: Vec<&'a str>,
    idx: usize,
}

impl<'de> SeqAccess<'de> for RawSeqAccess<'de> {
    type Error = DeError;

    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error> {
        if self.idx >= self.items.len() {
            return Ok(None);
        }
        let item = unquote(self.items[self.idx]);
        self.idx += 1;
        seed.deserialize(BorrowedStrDeserializer::new(item)).map(Some)
    }
}

// ==============================================================================
// 3. ZERO-ALLOCATION BYTECODE STREAM ENGINE
// ==============================================================================

pub struct BytecodeStream<'a> {
    pub data: &'a [u8],
    pub bytes: &'a [u8],
    pub idx: usize,
}

impl<'a> BytecodeStream<'a> {
    #[inline(always)]
    pub fn new(data: &'a [u8], bytes: &'a [u8], idx: usize) -> Self {
        Self { data, bytes, idx }
    }

    #[inline(always)]
    pub fn skip_block(&mut self) {
        let mut depth = 1;
        while self.idx < self.bytes.len() && depth > 0 {
            let tag = self.bytes[self.idx];
            match tag {
                TAG_BLOCK_START => {
                    depth += 1;
                    self.idx += REC_NAMED;
                }
                TAG_BLOCK_END => {
                    depth -= 1;
                    self.idx += REC_BLOCK_END;
                }
                other => self.idx += record_len(other),
            }
        }
    }
}

// ==============================================================================
// 4. ROOT AND BLOCK STREAM DESERIALIZERS
// ==============================================================================

struct TemplateStreamDeserializer<'a, 's> {
    stream: &'s mut BytecodeStream<'a>,
}

impl<'de, 's> Deserializer<'de> for TemplateStreamDeserializer<'de, 's> {
    type Error = DeError;

    #[inline]
    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        // The root is a block without braces: top-level properties, lists, enums and
        // blocks all become keys.
        visitor.visit_map(BlockMapStreamAccess {
            stream: self.stream,
            current_entry: None,
        })
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

struct BlockStreamDeserializer<'a, 's> {
    stream: &'s mut BytecodeStream<'a>,
}

enum StreamEntry<'a> {
    Field { name: &'a str, val: &'a str, quote: QuoteStyle },
    SubBlock,
    List { raw: &'a str },
    Enum { raw: &'a str },
}

struct BlockMapStreamAccess<'a, 's> {
    stream: &'s mut BytecodeStream<'a>,
    current_entry: Option<StreamEntry<'a>>,
}

impl<'de, 's> Deserializer<'de> for BlockStreamDeserializer<'de, 's> {
    type Error = DeError;

    #[inline]
    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_map(BlockMapStreamAccess {
            stream: self.stream,
            current_entry: None,
        })
    }

    #[inline]
    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.stream.skip_block();
        visitor.visit_unit()
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier
    }
}

impl<'de, 's> MapAccess<'de> for BlockMapStreamAccess<'de, 's> {
    type Error = DeError;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error> {
        while self.stream.idx < self.stream.bytes.len() {
            let tag = self.stream.bytes[self.stream.idx];
            match tag {
                TAG_COMMON_TEXT => {
                    let key = resolve_slice(self.stream.data, self.stream.bytes, self.stream.idx + 1);
                    let val = resolve_slice(self.stream.data, self.stream.bytes, self.stream.idx + 9);
                    let quote = QuoteStyle::detect(
                        self.stream.data,
                        read_u32(self.stream.bytes, self.stream.idx + 9) as usize,
                        val.len(),
                    );
                    self.stream.idx += REC_PAIR;
                    self.current_entry = Some(StreamEntry::Field { name: key, val, quote });
                    return seed.deserialize(BorrowedStrDeserializer::new(key)).map(Some);
                }
                TAG_BLOCK_START => {
                    let name = resolve_slice(self.stream.data, self.stream.bytes, self.stream.idx + 1);
                    self.stream.idx += REC_NAMED;
                    let clean = strip_hash(name);
                    self.current_entry = Some(StreamEntry::SubBlock);
                    return seed.deserialize(BorrowedStrDeserializer::new(clean)).map(Some);
                }
                TAG_LIST => {
                    let key = resolve_slice(self.stream.data, self.stream.bytes, self.stream.idx + 1);
                    let raw = resolve_slice(self.stream.data, self.stream.bytes, self.stream.idx + 9);
                    self.stream.idx += REC_RANGE;
                    self.current_entry = Some(StreamEntry::List { raw });
                    return seed.deserialize(BorrowedStrDeserializer::new(key)).map(Some);
                }
                TAG_ENUM => {
                    let key = resolve_slice(self.stream.data, self.stream.bytes, self.stream.idx + 1);
                    let raw = resolve_slice(self.stream.data, self.stream.bytes, self.stream.idx + 9);
                    self.stream.idx += REC_RANGE;
                    self.current_entry = Some(StreamEntry::Enum { raw });
                    return seed.deserialize(BorrowedStrDeserializer::new(key)).map(Some);
                }
                TAG_BLOCK_END => {
                    self.stream.idx += REC_BLOCK_END;
                    return Ok(None);
                }
                TAG_SUBTITLE | TAG_DIRECTIVE => {
                    self.stream.idx += REC_PAIR;
                }
                TAG_VAR_FUNC => {
                    self.stream.idx += REC_BINDING;
                }
                TAG_TITLE => {
                    self.stream.idx += REC_NAMED;
                }
                _ => {
                    self.stream.idx += 1;
                }
            }
        }
        Ok(None)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Self::Error> {
        match self.current_entry.take().ok_or_else(|| DeError("missing stream entry".into()))? {
            StreamEntry::Field { name, val, quote } => {
                seed.deserialize(FieldDeserializer { name, value: val, quote })
            }
            StreamEntry::SubBlock { .. } => {
                seed.deserialize(BlockStreamDeserializer { stream: self.stream })
            }
            StreamEntry::List { raw, .. } => {
                seed.deserialize(RawListDeserializer { raw })
            }
            StreamEntry::Enum { raw, .. } => {
                let first = raw.split(',').next().unwrap_or("").trim();
                seed.deserialize(BorrowedStrDeserializer::new(first))
            }
        }
    }
}

// ==============================================================================
// 5. PUBLIC API & TEMPLATE INTEGRATION
// ==============================================================================

/// Deserializes a [`BlockView`] into type `T`, borrowing strings from the document.
pub fn from_block<'a, T: de::Deserialize<'a>>(block: &'a BlockView<'a>) -> Result<T, DeError> {
    T::deserialize(BlockViewDeserializer { block, idx: 0 })
}

struct BlockViewDeserializer<'a> {
    block: &'a BlockView<'a>,
    idx: usize,
}

impl<'de> Deserializer<'de> for BlockViewDeserializer<'de> {
    type Error = DeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_map(self)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

impl<'de> MapAccess<'de> for BlockViewDeserializer<'de> {
    type Error = DeError;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error> {
        if self.idx >= self.block.fields.len() {
            return Ok(None);
        }
        let f = &self.block.fields[self.idx];
        seed.deserialize(BorrowedStrDeserializer::new(f.name())).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Self::Error> {
        let f = &self.block.fields[self.idx];
        self.idx += 1;
        let val = f.value().unwrap_or("");
        let quote = f.quote_style().unwrap_or_default();
        seed.deserialize(FieldDeserializer { name: f.name(), value: val, quote })
    }
}

/// Deserializes a [`Template`] into type `T`.
///
/// Strings without escapes are borrowed from the document, so `T` may hold `&'a str`.
pub fn from_template<'a, T: de::Deserialize<'a>>(template: &'a Template) -> Result<T, DeError> {
    let mut stream = BytecodeStream::new(&template.data, &template.fields, 0);
    T::deserialize(TemplateStreamDeserializer { stream: &mut stream })
}

impl Template {
    /// Deserializes a specific block directly from the zero-copy bytecode arena into any type `T`.
    pub fn deserialize<T: de::DeserializeOwned>(&self, selector: &str) -> Result<T, DeError> {
        let clean = strip_hash(selector);
        let mut idx = 0;
        let mut found = false;

        while idx < self.fields.len() {
            let tag = self.fields[idx];
            if tag == TAG_BLOCK_START {
                let name = resolve_slice(&self.data, &self.fields, idx + 1);
                idx += REC_NAMED;
                if strip_hash(name) == clean {
                    found = true;
                    break;
                }
            } else if tag == TAG_BLOCK_END {
                idx += REC_BLOCK_END;
            } else if tag == TAG_COMMON_TEXT || tag == TAG_SUBTITLE || tag == TAG_DIRECTIVE {
                idx += REC_PAIR;
            } else if tag == TAG_ENUM || tag == TAG_LIST {
                idx += REC_RANGE;
            } else if tag == TAG_VAR_FUNC {
                idx += REC_BINDING;
            } else if tag == TAG_TITLE {
                idx += REC_NAMED;
            } else {
                idx += 1;
            }
        }

        if !found {
            return Err(DeError(format!("block '{}' not found in template", selector)));
        }

        let mut stream = BytecodeStream::new(&self.data, &self.fields, idx);
        T::deserialize(BlockStreamDeserializer { stream: &mut stream })
    }

    /// Deserializes the entire template root directly from the zero-copy bytecode arena into type `T`.
    pub fn deserialize_root<T: de::DeserializeOwned>(&self) -> Result<T, DeError> {
        let mut stream = BytecodeStream::new(&self.data, &self.fields, 0);
        T::deserialize(TemplateStreamDeserializer { stream: &mut stream })
    }
}
