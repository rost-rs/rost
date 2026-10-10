//! # Serde Deserializer for `.rost` Documents
//!
//! Provides direct zero-boilerplate deserialization from `.rost` templates
//! into any Rust data structure implementing [`serde::Deserialize`].

use serde::de::{
    self, DeserializeSeed, Deserializer, IntoDeserializer, MapAccess, SeqAccess, Visitor,
};
use std::fmt;

use super::field::ParsedField;
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

#[inline]
fn strip_hash(s: &str) -> &str {
    s.strip_prefix('#').unwrap_or(s)
}

/// Deserializes a [`BlockView`] into type `T`.
pub fn from_block<'de, 'a, T: de::Deserialize<'de>>(block: &'a BlockView<'a>) -> Result<T, DeError> {
    T::deserialize(BlockDeserializer {
        block,
        all_blocks: &[],
    })
}

/// Deserializes a [`Template`] into type `T`.
pub fn from_template<'de, 'a, T: de::Deserialize<'de>>(template: &'a Template) -> Result<T, DeError> {
    let blocks = template.blocks();
    T::deserialize(TemplateDeserializer { all_blocks: &blocks })
}

struct TemplateDeserializer<'a> {
    all_blocks: &'a [BlockView<'a>],
}

impl<'de, 'a> Deserializer<'de> for TemplateDeserializer<'a> {
    type Error = DeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let root_blocks: Vec<&'a BlockView<'a>> = self
            .all_blocks
            .iter()
            .filter(|b| b.parent.is_none())
            .collect();

        visitor.visit_map(TemplateMapAccess {
            all_blocks: self.all_blocks,
            root_blocks,
            idx: 0,
            current_value: None,
        })
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

struct TemplateMapAccess<'a> {
    all_blocks: &'a [BlockView<'a>],
    root_blocks: Vec<&'a BlockView<'a>>,
    idx: usize,
    current_value: Option<&'a BlockView<'a>>,
}

impl<'de, 'a> MapAccess<'de> for TemplateMapAccess<'a> {
    type Error = DeError;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error> {
        if self.idx >= self.root_blocks.len() {
            return Ok(None);
        }
        let b = self.root_blocks[self.idx];
        self.idx += 1;
        let key = strip_hash(b.name);
        self.current_value = Some(b);
        seed.deserialize(key.into_deserializer()).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Self::Error> {
        let b = self.current_value.take().ok_or_else(|| DeError("missing block value".into()))?;
        seed.deserialize(BlockDeserializer {
            block: b,
            all_blocks: self.all_blocks,
        })
    }
}

enum BlockEntry<'a> {
    Field(&'a ParsedField<'a>),
    ChildBlock(&'a BlockView<'a>),
}

struct BlockDeserializer<'a> {
    block: &'a BlockView<'a>,
    all_blocks: &'a [BlockView<'a>],
}

impl<'de, 'a> Deserializer<'de> for BlockDeserializer<'a> {
    type Error = DeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let parent_name = strip_hash(self.block.name);
        let child_blocks: Vec<&'a BlockView<'a>> = self
            .all_blocks
            .iter()
            .filter(|b| {
                if let Some(p) = b.parent {
                    strip_hash(p) == parent_name
                } else {
                    false
                }
            })
            .collect();

        visitor.visit_map(BlockMapAccess {
            block: self.block,
            all_blocks: self.all_blocks,
            child_blocks,
            idx: 0,
            current_entry: None,
        })
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

struct BlockMapAccess<'a> {
    block: &'a BlockView<'a>,
    all_blocks: &'a [BlockView<'a>],
    child_blocks: Vec<&'a BlockView<'a>>,
    idx: usize,
    current_entry: Option<BlockEntry<'a>>,
}

impl<'de, 'a> MapAccess<'de> for BlockMapAccess<'a> {
    type Error = DeError;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error> {
        let total = self.block.fields.len() + self.child_blocks.len();
        if self.idx >= total {
            return Ok(None);
        }
        let cur = self.idx;
        self.idx += 1;

        if cur < self.block.fields.len() {
            let field = &self.block.fields[cur];
            self.current_entry = Some(BlockEntry::Field(field));
            seed.deserialize(field.name().into_deserializer()).map(Some)
        } else {
            let child = self.child_blocks[cur - self.block.fields.len()];
            let child_name = strip_hash(child.name);
            self.current_entry = Some(BlockEntry::ChildBlock(child));
            seed.deserialize(child_name.into_deserializer()).map(Some)
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Self::Error> {
        match self.current_entry.take().ok_or_else(|| DeError("missing entry value".into()))? {
            BlockEntry::Field(field) => seed.deserialize(FieldDeserializer { field }),
            BlockEntry::ChildBlock(b) => seed.deserialize(BlockDeserializer {
                block: b,
                all_blocks: self.all_blocks,
            }),
        }
    }
}

struct FieldDeserializer<'a> {
    field: &'a ParsedField<'a>,
}

impl<'de, 'a> Deserializer<'de> for FieldDeserializer<'a> {
    type Error = DeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.field {
            ParsedField::List { .. } => {
                visitor.visit_seq(ListSeqAccess {
                    items: self.field.list_items(),
                    idx: 0,
                })
            }
            ParsedField::Enum { .. } => {
                let first = self.field.enum_variants().first().copied().unwrap_or("");
                visitor.visit_str(first)
            }
            _ => {
                let raw = self.field.as_str().unwrap_or("");
                let trimmed = raw.trim_matches('"');
                if let Some(b) = self.field.as_bool() {
                    visitor.visit_bool(b)
                } else if let Some(h) = self.field.as_hex_color() {
                    visitor.visit_u32(h)
                } else if let Some(i) = self.field.as_i64() {
                    visitor.visit_i64(i)
                } else if let Some(f) = self.field.as_f64() {
                    visitor.visit_f64(f)
                } else {
                    visitor.visit_str(trimmed)
                }
            }
        }
    }

    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        if let Some(h) = self.field.as_hex_color() {
            visitor.visit_u32(h)
        } else if let Some(u) = self.field.as_u64() {
            visitor.visit_u32(u as u32)
        } else {
            Err(DeError(format!("cannot parse '{}' as u32", self.field.name())))
        }
    }

    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        if let Some(d) = self.field.as_dimension() {
            visitor.visit_f32(d.value as f32)
        } else if let Some(f) = self.field.as_f64() {
            visitor.visit_f32(f as f32)
        } else {
            Err(DeError(format!("cannot parse '{}' as f32", self.field.name())))
        }
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_seq(ListSeqAccess {
            items: self.field.list_items(),
            idx: 0,
        })
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_some(self)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 u8 u16 u64 f64 char str string
        bytes byte_buf unit unit_struct newtype_struct tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

struct ListSeqAccess<'a> {
    items: Vec<&'a str>,
    idx: usize,
}

impl<'de, 'a> SeqAccess<'de> for ListSeqAccess<'a> {
    type Error = DeError;

    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error> {
        if self.idx >= self.items.len() {
            return Ok(None);
        }
        let item = self.items[self.idx];
        self.idx += 1;
        seed.deserialize(item.into_deserializer()).map(Some)
    }
}

impl Template {
    /// Deserializes the template or an individual block into any type implementing [`serde::Deserialize`].
    pub fn deserialize<T: de::DeserializeOwned>(&self, selector: &str) -> Result<T, DeError> {
        let block = self
            .find_block(selector)
            .ok_or_else(|| DeError(format!("block '{}' not found", selector)))?;
        let blocks = self.blocks();
        T::deserialize(BlockDeserializer {
            block: &block,
            all_blocks: &blocks,
        })
    }

    /// Deserializes the entire template root into type `T`.
    pub fn deserialize_root<T: de::DeserializeOwned>(&self) -> Result<T, DeError> {
        from_template(self)
    }
}
