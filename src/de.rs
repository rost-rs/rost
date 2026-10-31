//! # High-Throughput Streaming Serde Deserializer for `.rost` Documents
//!
//! Provides direct zero-copy, zero-allocation deserialization from `.rost`
//! bytecode streams directly into any Rust data structure implementing
//! [`serde::Deserialize`].

use std::fmt;
use serde::de::{
    self, DeserializeSeed, Deserializer, IntoDeserializer, MapAccess, SeqAccess, Visitor,
};

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
use super::field::{parse_fast_f32, parse_fast_u32};

struct FieldDeserializer<'a> {
    name: &'a str,
    value: &'a str,
}

impl<'de, 'a> Deserializer<'de> for FieldDeserializer<'a> {
    type Error = DeError;

    #[inline]
    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        if let Some(u) = parse_fast_u32(self.value) {
            return visitor.visit_u32(u);
        }
        if let Some(f) = parse_fast_f32(self.value) {
            return visitor.visit_f32(f);
        }
        let s = self.value.trim().trim_end_matches(';');
        if s == "true" {
            return visitor.visit_bool(true);
        }
        if s == "false" {
            return visitor.visit_bool(false);
        }
        visitor.visit_str(s.trim_matches('"'))
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

    #[inline]
    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        if let Some(v) = parse_fast_u32(self.value) {
            return visitor.visit_u32(v);
        }
        Err(DeError(format!("cannot parse '{}' as u32: {}", self.name, self.value)))
    }

    #[inline]
    fn deserialize_u64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        if let Some(v) = parse_fast_u32(self.value) {
            return visitor.visit_u64(v as u64);
        }
        Err(DeError(format!("cannot parse '{}' as u64: {}", self.name, self.value)))
    }

    #[inline]
    fn deserialize_i64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        if let Some(v) = parse_fast_u32(self.value) {
            return visitor.visit_i64(v as i64);
        }
        if let Some(f) = parse_fast_f32(self.value) {
            return visitor.visit_i64(f as i64);
        }
        Err(DeError(format!("cannot parse '{}' as i64: {}", self.name, self.value)))
    }

    #[inline]
    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        if let Some(v) = parse_fast_u32(self.value) {
            return visitor.visit_i32(v as i32);
        }
        if let Some(f) = parse_fast_f32(self.value) {
            return visitor.visit_i32(f as i32);
        }
        Err(DeError(format!("cannot parse '{}' as i32: {}", self.name, self.value)))
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
        if let Some(v) = parse_fast_f32(self.value) {
            return visitor.visit_f64(v as f64);
        }
        Err(DeError(format!("cannot parse '{}' as f64: {}", self.name, self.value)))
    }

    #[inline]
    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let s = self.value.trim().trim_end_matches(';').trim_matches('"');
        visitor.visit_str(s)
    }

    #[inline]
    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let s = self.value.trim().trim_end_matches(';').trim_matches('"');
        visitor.visit_string(s.to_string())
    }

    #[inline]
    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_some(self)
    }

    serde::forward_to_deserialize_any! {
        i8 i16 u8 u16 char bytes byte_buf unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

// ==============================================================================
// 2. LIST AND ARRAY DESERIALIZER
// ==============================================================================

struct RawListDeserializer<'a> {
    raw: &'a str,
}

impl<'de, 'a> Deserializer<'de> for RawListDeserializer<'a> {
    type Error = DeError;

    #[inline]
    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_seq(visitor)
    }

    #[inline]
    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let clean = self.raw.trim().trim_start_matches('[').trim_end_matches(']');
        let items: Vec<&'a str> = clean.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
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

impl<'de, 'a> SeqAccess<'de> for RawSeqAccess<'a> {
    type Error = DeError;

    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error> {
        if self.idx >= self.items.len() {
            return Ok(None);
        }
        let item = self.items[self.idx].trim_matches('"');
        self.idx += 1;
        seed.deserialize(item.into_deserializer()).map(Some)
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

impl<'de, 'a, 's> Deserializer<'de> for TemplateStreamDeserializer<'a, 's> {
    type Error = DeError;

    #[inline]
    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_map(RootMapStreamAccess {
            stream: self.stream,
            current_block: None,
        })
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

struct RootMapStreamAccess<'a, 's> {
    stream: &'s mut BytecodeStream<'a>,
    current_block: Option<&'a str>,
}

impl<'de, 'a, 's> MapAccess<'de> for RootMapStreamAccess<'a, 's> {
    type Error = DeError;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error> {
        while self.stream.idx < self.stream.bytes.len() {
            let tag = self.stream.bytes[self.stream.idx];
            if tag == TAG_BLOCK_START {
                let name = resolve_slice(self.stream.data, self.stream.bytes, self.stream.idx + 1);
                self.stream.idx += REC_NAMED;
                let clean = strip_hash(name);
                self.current_block = Some(clean);
                return seed.deserialize(clean.into_deserializer()).map(Some);
            } else if tag == TAG_BLOCK_END {
                self.stream.idx += REC_BLOCK_END;
            } else if tag == TAG_COMMON_TEXT || tag == TAG_SUBTITLE || tag == TAG_DIRECTIVE {
                self.stream.idx += REC_PAIR;
            } else if tag == TAG_ENUM || tag == TAG_LIST {
                self.stream.idx += REC_RANGE;
            } else if tag == TAG_VAR_FUNC {
                self.stream.idx += REC_BINDING;
            } else if tag == TAG_TITLE {
                self.stream.idx += REC_NAMED;
            } else {
                self.stream.idx += 1;
            }
        }
        Ok(None)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Self::Error> {
        let _ = self.current_block.take();
        seed.deserialize(BlockStreamDeserializer { stream: self.stream })
    }
}

struct BlockStreamDeserializer<'a, 's> {
    stream: &'s mut BytecodeStream<'a>,
}

enum StreamEntry<'a> {
    Field { name: &'a str, val: &'a str },
    SubBlock,
    List { raw: &'a str },
    Enum { raw: &'a str },
}

struct BlockMapStreamAccess<'a, 's> {
    stream: &'s mut BytecodeStream<'a>,
    current_entry: Option<StreamEntry<'a>>,
}

impl<'de, 'a, 's> Deserializer<'de> for BlockStreamDeserializer<'a, 's> {
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

impl<'de, 'a, 's> MapAccess<'de> for BlockMapStreamAccess<'a, 's> {
    type Error = DeError;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error> {
        while self.stream.idx < self.stream.bytes.len() {
            let tag = self.stream.bytes[self.stream.idx];
            match tag {
                TAG_COMMON_TEXT => {
                    let key = resolve_slice(self.stream.data, self.stream.bytes, self.stream.idx + 1);
                    let val = resolve_slice(self.stream.data, self.stream.bytes, self.stream.idx + 9);
                    self.stream.idx += REC_PAIR;
                    self.current_entry = Some(StreamEntry::Field { name: key, val });
                    return seed.deserialize(key.into_deserializer()).map(Some);
                }
                TAG_BLOCK_START => {
                    let name = resolve_slice(self.stream.data, self.stream.bytes, self.stream.idx + 1);
                    self.stream.idx += REC_NAMED;
                    let clean = strip_hash(name);
                    self.current_entry = Some(StreamEntry::SubBlock);
                    return seed.deserialize(clean.into_deserializer()).map(Some);
                }
                TAG_LIST => {
                    let key = resolve_slice(self.stream.data, self.stream.bytes, self.stream.idx + 1);
                    let raw = resolve_slice(self.stream.data, self.stream.bytes, self.stream.idx + 9);
                    self.stream.idx += REC_RANGE;
                    self.current_entry = Some(StreamEntry::List { raw });
                    return seed.deserialize(key.into_deserializer()).map(Some);
                }
                TAG_ENUM => {
                    let key = resolve_slice(self.stream.data, self.stream.bytes, self.stream.idx + 1);
                    let raw = resolve_slice(self.stream.data, self.stream.bytes, self.stream.idx + 9);
                    self.stream.idx += REC_RANGE;
                    self.current_entry = Some(StreamEntry::Enum { raw });
                    return seed.deserialize(key.into_deserializer()).map(Some);
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
            StreamEntry::Field { name, val } => {
                seed.deserialize(FieldDeserializer { name, value: val })
            }
            StreamEntry::SubBlock { .. } => {
                seed.deserialize(BlockStreamDeserializer { stream: self.stream })
            }
            StreamEntry::List { raw, .. } => {
                seed.deserialize(RawListDeserializer { raw })
            }
            StreamEntry::Enum { raw, .. } => {
                let first = raw.split(',').next().unwrap_or("").trim();
                seed.deserialize(first.into_deserializer())
            }
        }
    }
}

// ==============================================================================
// 5. PUBLIC API & TEMPLATE INTEGRATION
// ==============================================================================

/// Deserializes a [`BlockView`] into type `T`.
pub fn from_block<'de, 'a, T: de::Deserialize<'de>>(block: &'a BlockView<'a>) -> Result<T, DeError> {
    T::deserialize(BlockViewDeserializer { block, idx: 0 })
}

struct BlockViewDeserializer<'a> {
    block: &'a BlockView<'a>,
    idx: usize,
}

impl<'de, 'a> Deserializer<'de> for BlockViewDeserializer<'a> {
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

impl<'de, 'a> MapAccess<'de> for BlockViewDeserializer<'a> {
    type Error = DeError;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error> {
        if self.idx >= self.block.fields.len() {
            return Ok(None);
        }
        let f = &self.block.fields[self.idx];
        seed.deserialize(f.name().into_deserializer()).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Self::Error> {
        let f = &self.block.fields[self.idx];
        self.idx += 1;
        let val = f.value().unwrap_or("");
        seed.deserialize(FieldDeserializer { name: f.name(), value: val })
    }
}

/// Deserializes a [`Template`] into type `T`.
pub fn from_template<'de, 'a, T: de::Deserialize<'de>>(template: &'a Template) -> Result<T, DeError> {
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
