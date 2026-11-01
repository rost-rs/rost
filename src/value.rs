//! # Dynamic Values
//!
//! [`Value`] holds any `.rost` document without a Rust type for it, like
//! `serde_json::Value`: deserialize into it with [`crate::from_str`], index it with
//! `value["key"][0]`, build it with [`crate::rost!`] and write it back with
//! [`crate::to_string`].
//!
//! Blocks are [`Map`]s that keep their entries in document order, so a file read into a
//! `Value` and written back keeps its layout.

use std::hash::BuildHasher;
use std::sync::OnceLock;
use std::fmt;
use std::ops::{Index, IndexMut};

use serde::de::{self, Deserializer, IntoDeserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};

use crate::de::{DeError, MapEnum};
use crate::index::{finish, mix_segment, PreHashedMap};

/// Any `.rost` value.
///
/// # Examples
///
/// ```
/// use rost::Value;
///
/// let v: Value = rost::from_str("name: \"Ada\"\nlangs [\"en\", \"fr\"]\n#window { width: 1280 }\n").unwrap();
/// assert_eq!(v["name"], "Ada");
/// assert_eq!(v["langs"][1], "fr");
/// assert_eq!(v["window"]["width"].as_i64(), Some(1280));
/// assert!(v["missing"]["deeper"].is_null());
/// ```
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Value {
    /// `null`, `()` and `None`.
    #[default]
    Null,
    Bool(bool),
    /// Integers; unsigned values above `i64::MAX` become [`Value::Float`].
    Int(i64),
    Float(f64),
    String(String),
    /// A list `[ … ]`.
    List(Vec<Value>),
    /// A block `{ … }`, or the document root.
    Block(Map),
}

static NULL: Value = Value::Null;

impl Value {
    /// Looks up a key in a block; `None` for other values.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_block()?.get(key)
    }

    /// Mutable [`Value::get`].
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.as_block_mut()?.get_mut(key)
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        self.as_i64().and_then(|i| u64::try_from(i).ok())
    }

    /// The value as a float; integers convert.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Int(i) => Some(*i as f64),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&Vec<Value>> {
        match self {
            Value::List(l) => Some(l),
            _ => None,
        }
    }

    pub fn as_list_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Value::List(l) => Some(l),
            _ => None,
        }
    }

    pub fn as_block(&self) -> Option<&Map> {
        match self {
            Value::Block(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_block_mut(&mut self) -> Option<&mut Map> {
        match self {
            Value::Block(m) => Some(m),
            _ => None,
        }
    }

    /// Replaces the value with `Null` and returns it.
    pub fn take(&mut self) -> Value {
        std::mem::take(self)
    }
}

/// `{}` writes a block as a single-line document and any other value as a list item
/// would appear; `{:#}` writes one entry per line.
///
/// ```
/// let v: rost::Value = rost::from_str("name: \"Ada\"\nlangs [\"en\", \"fr\"]\n").unwrap();
/// assert_eq!(v.to_string(), "name: \"Ada\", langs [\"en\", \"fr\"]");
/// assert_eq!(format!("{v:#}"), "name: \"Ada\"\nlangs [\"en\", \"fr\"]\n");
/// assert_eq!(v["langs"].to_string(), "[\"en\", \"fr\"]");
/// ```
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pretty = f.alternate();
        let text = match self {
            Value::Block(_) if pretty => crate::to_string_pretty(self),
            Value::Block(_) => crate::to_string(self),
            _ => crate::ser::to_fragment(self, pretty),
        };
        f.write_str(&text.map_err(|_| fmt::Error)?)
    }
}

impl Index<&str> for Value {
    type Output = Value;

    /// Missing keys and non-blocks yield `Null`, so lookups chain without panicking.
    fn index(&self, key: &str) -> &Value {
        self.get(key).unwrap_or(&NULL)
    }
}

impl Index<usize> for Value {
    type Output = Value;

    /// Out-of-range indices and non-lists yield `Null`.
    fn index(&self, index: usize) -> &Value {
        self.as_list().and_then(|l| l.get(index)).unwrap_or(&NULL)
    }
}

impl IndexMut<&str> for Value {
    /// Inserts `Null` for a missing key; a `Null` value becomes an empty block first.
    ///
    /// # Panics
    ///
    /// If the value is neither a block nor `Null`.
    fn index_mut(&mut self, key: &str) -> &mut Value {
        if self.is_null() {
            *self = Value::Block(Map::new());
        }
        match self {
            Value::Block(map) => map.entry(key),
            other => panic!("cannot index {} with a key", other.kind()),
        }
    }
}

impl IndexMut<usize> for Value {
    /// # Panics
    ///
    /// If the value is not a list or the index is out of range.
    fn index_mut(&mut self, index: usize) -> &mut Value {
        match self {
            Value::List(list) => {
                let len = list.len();
                list.get_mut(index).unwrap_or_else(|| panic!("index {index} out of range for a list of {len}"))
            }
            other => panic!("cannot index {} with a number", other.kind()),
        }
    }
}

impl Value {
    fn kind(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "a bool",
            Value::Int(_) => "an integer",
            Value::Float(_) => "a float",
            Value::String(_) => "a string",
            Value::List(_) => "a list",
            Value::Block(_) => "a block",
        }
    }
}

// ------------------------------------------------------------------------------
// Conversions and comparisons
// ------------------------------------------------------------------------------

macro_rules! from_int {
    ($($ty:ty),*) => {$(
        impl From<$ty> for Value {
            fn from(v: $ty) -> Self {
                Value::Int(v.into())
            }
        }
    )*};
}
from_int!(i8, i16, i32, i64, u8, u16, u32);

macro_rules! from_wide_int {
    ($($ty:ty),*) => {$(
        impl From<$ty> for Value {
            fn from(v: $ty) -> Self {
                i64::try_from(v).map_or(Value::Float(v as f64), Value::Int)
            }
        }
    )*};
}
from_wide_int!(u64, usize, isize, i128, u128);

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}

impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::Float(v.into())
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::String(v.to_string())
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::String(v)
    }
}

impl From<char> for Value {
    fn from(v: char) -> Self {
        Value::String(v.to_string())
    }
}

impl From<()> for Value {
    fn from((): ()) -> Self {
        Value::Null
    }
}

impl From<Map> for Value {
    fn from(v: Map) -> Self {
        Value::Block(v)
    }
}

impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(v: Vec<T>) -> Self {
        Value::List(v.into_iter().map(Into::into).collect())
    }
}

impl<T: Clone + Into<Value>> From<&[T]> for Value {
    fn from(v: &[T]) -> Self {
        Value::List(v.iter().cloned().map(Into::into).collect())
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Self {
        v.map_or(Value::Null, Into::into)
    }
}

impl<T: Into<Value>> FromIterator<T> for Value {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Value::List(iter.into_iter().map(Into::into).collect())
    }
}

impl PartialEq<str> for Value {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == Some(other)
    }
}

impl PartialEq<&str> for Value {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == Some(*other)
    }
}

impl PartialEq<String> for Value {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == Some(other.as_str())
    }
}

impl PartialEq<bool> for Value {
    fn eq(&self, other: &bool) -> bool {
        self.as_bool() == Some(*other)
    }
}

impl PartialEq<f64> for Value {
    fn eq(&self, other: &f64) -> bool {
        self.as_f64() == Some(*other)
    }
}

macro_rules! eq_int {
    ($($ty:ty),*) => {$(
        impl PartialEq<$ty> for Value {
            fn eq(&self, other: &$ty) -> bool {
                self.as_i64().is_some_and(|i| i128::from(i) == *other as i128)
            }
        }
    )*};
}
eq_int!(i8, i16, i32, i64, u8, u16, u32, u64, usize, isize);

// ------------------------------------------------------------------------------
// Serde
// ------------------------------------------------------------------------------

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Null => serializer.serialize_unit(),
            Value::Bool(b) => serializer.serialize_bool(*b),
            Value::Int(i) => serializer.serialize_i64(*i),
            Value::Float(f) => serializer.serialize_f64(*f),
            Value::String(s) => serializer.serialize_str(s),
            Value::List(list) => {
                let mut seq = serializer.serialize_seq(Some(list.len()))?;
                for item in list {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Value::Block(map) => {
                let mut out = serializer.serialize_map(Some(map.len()))?;
                for (k, v) in map {
                    out.serialize_entry(k, v)?;
                }
                out.end()
            }
        }
    }
}

impl<'de> de::Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(ValueVisitor)
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any .rost value")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Int(v))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(v.into())
    }

    fn visit_i128<E>(self, v: i128) -> Result<Value, E> {
        Ok(v.into())
    }

    fn visit_u128<E>(self, v: u128) -> Result<Value, E> {
        Ok(v.into())
    }

    fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
        Ok(Value::Float(v))
    }

    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_string()))
    }

    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut list = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(4096));
        while let Some(item) = seq.next_element()? {
            list.push(item);
        }
        Ok(Value::List(list))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Value, A::Error> {
        let mut map = Map::new();
        while let Some((k, v)) = access.next_entry::<String, Value>()? {
            map.insert(k, v);
        }
        Ok(Value::Block(map))
    }
}

/// Deserializes a `T` from a [`Value`].
///
/// # Examples
///
/// ```
/// #[derive(serde::Deserialize)]
/// struct Window {
///     width: u32,
///     scale: f64,
/// }
///
/// let doc: rost::Value = rost::from_str("window { width: 1280, scale: 1.25 }").unwrap();
/// let window: Window = rost::from_value(doc["window"].clone()).unwrap();
/// assert_eq!((window.width, window.scale), (1280, 1.25));
/// ```
pub fn from_value<T: de::DeserializeOwned>(value: Value) -> Result<T, DeError> {
    T::deserialize(value)
}

impl<'de> IntoDeserializer<'de, DeError> for Value {
    type Deserializer = Value;

    fn into_deserializer(self) -> Value {
        self
    }
}

impl<'de> Deserializer<'de> for Value {
    type Error = DeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match self {
            Value::Null => visitor.visit_unit(),
            Value::Bool(b) => visitor.visit_bool(b),
            Value::Int(i) => visitor.visit_i64(i),
            Value::Float(f) => visitor.visit_f64(f),
            Value::String(s) => visitor.visit_string(s),
            Value::List(list) => {
                let mut seq = de::value::SeqDeserializer::new(list.into_iter());
                let value = visitor.visit_seq(&mut seq)?;
                seq.end()?;
                Ok(value)
            }
            Value::Block(map) => {
                let mut access = de::value::MapDeserializer::new(map.into_iter());
                let value = visitor.visit_map(&mut access)?;
                access.end()?;
                Ok(value)
            }
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match self {
            Value::Null => visitor.visit_none(),
            other => visitor.visit_some(other),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(self, _name: &'static str, visitor: V) -> Result<V::Value, DeError> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, DeError> {
        match self {
            Value::String(s) => visitor.visit_enum(s.into_deserializer()),
            Value::Block(map) => visitor.visit_enum(MapEnum(de::value::MapDeserializer::new(map.into_iter()))),
            other => Err(DeError::new(format!("expected an enum, found {}", other.kind()))),
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct seq tuple tuple_struct map struct identifier ignored_any
    }
}

// ------------------------------------------------------------------------------
// Map
// ------------------------------------------------------------------------------

/// Above this many entries a [`Map`] keeps a hash index next to its entries.
const INDEX_THRESHOLD: usize = 16;

/// The entries of a block, in insertion order.
///
/// Small maps search their entries directly; larger ones keep a hash index, so building
/// a map from a large document stays linear.
#[derive(Clone, Default)]
pub struct Map {
    entries: Vec<(String, Value)>,
    index: Option<Box<KeyIndex>>,
}

/// Positions of the entries by 64-bit key hash. The keys themselves stay in `entries`:
/// a hit is confirmed by comparing the key, and the rare entries whose hash is already
/// taken by another key are kept in `overflow`.
#[derive(Clone, Default)]
struct KeyIndex {
    slots: PreHashedMap<u32>,
    overflow: Vec<u32>,
}

impl KeyIndex {
    fn add(&mut self, hash: u64, position: usize) {
        match self.slots.entry(hash) {
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(position as u32);
            }
            std::collections::hash_map::Entry::Occupied(_) => self.overflow.push(position as u32),
        }
    }
}

/// Hashes a key with a per-process random seed, so colliding keys cannot be prepared in
/// advance to slow a map down.
fn key_hash(key: &str) -> u64 {
    static SEED: OnceLock<u64> = OnceLock::new();
    let seed = *SEED.get_or_init(|| std::collections::hash_map::RandomState::new().hash_one(0x5EED_u64));
    finish(mix_segment(seed, key.as_bytes()))
}

impl Map {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn position(&self, key: &str) -> Option<usize> {
        let Some(index) = &self.index else {
            return self.entries.iter().position(|(k, _)| k == key);
        };
        let first = *index.slots.get(&key_hash(key))? as usize;
        if self.entries[first].0 == key {
            return Some(first);
        }
        index.overflow.iter().map(|&i| i as usize).find(|&i| self.entries[i].0 == key)
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.position(key).map(|i| &self.entries[i].1)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.position(key).map(|i| &mut self.entries[i].1)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.position(key).is_some()
    }

    /// Inserts or replaces an entry; a replaced entry keeps its position. Returns the old value.
    pub fn insert(&mut self, key: impl Into<String>, value: Value) -> Option<Value> {
        let key = key.into();
        if let Some(i) = self.position(&key) {
            return Some(std::mem::replace(&mut self.entries[i].1, value));
        }
        if let Some(index) = &mut self.index {
            index.add(key_hash(&key), self.entries.len());
        }
        self.entries.push((key, value));
        if self.index.is_none() && self.entries.len() > INDEX_THRESHOLD {
            self.reindex();
        }
        None
    }

    /// Removes an entry, keeping the order of the others.
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        let i = self.position(key)?;
        let (_, value) = self.entries.remove(i);
        if self.index.is_some() {
            self.reindex();
        }
        Some(value)
    }

    /// The value for `key`, inserting `Null` if it is missing.
    fn entry(&mut self, key: &str) -> &mut Value {
        let i = match self.position(key) {
            Some(i) => i,
            None => {
                self.insert(key, Value::Null);
                self.entries.len() - 1
            }
        };
        &mut self.entries[i].1
    }

    fn reindex(&mut self) {
        let mut index = KeyIndex::default();
        index.slots.reserve(self.entries.len());
        for (i, (k, _)) in self.entries.iter().enumerate() {
            index.add(key_hash(k), i);
        }
        self.index = Some(Box::new(index));
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&String, &mut Value)> {
        self.entries.iter_mut().map(|(k, v)| (&*k, v))
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.entries.iter().map(|(k, _)| k)
    }

    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.entries.iter().map(|(_, v)| v)
    }
}

impl fmt::Debug for Map {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

/// Maps are equal when they hold the same entries, in any order.
impl PartialEq for Map {
    fn eq(&self, other: &Map) -> bool {
        self.len() == other.len() && self.iter().all(|(k, v)| other.get(k) == Some(v))
    }
}

impl Index<&str> for Map {
    type Output = Value;

    fn index(&self, key: &str) -> &Value {
        self.get(key).unwrap_or(&NULL)
    }
}

impl<K: Into<String>, V: Into<Value>> FromIterator<(K, V)> for Map {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut map = Map::new();
        map.extend(iter);
        map
    }
}

impl<K: Into<String>, V: Into<Value>> Extend<(K, V)> for Map {
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) {
        for (k, v) in iter {
            self.insert(k, v.into());
        }
    }
}

impl IntoIterator for Map {
    type Item = (String, Value);
    type IntoIter = std::vec::IntoIter<(String, Value)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

impl<'a> IntoIterator for &'a Map {
    type Item = (&'a String, &'a Value);
    type IntoIter = std::iter::Map<std::slice::Iter<'a, (String, Value)>, fn(&'a (String, Value)) -> (&'a String, &'a Value)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter().map(|(k, v)| (k, v))
    }
}

// ------------------------------------------------------------------------------
// Literal macro
// ------------------------------------------------------------------------------

/// Builds a [`Value`] from a literal, like `serde_json::json!`.
///
/// Blocks are `{ key: value, … }` with identifier or string keys; lists are `[ … ]`;
/// `null` is [`Value::Null`]; anything else is an expression converted with
/// [`Value::from`].
///
/// # Examples
///
/// ```
/// use rost::rost;
///
/// let phone = 2345678;
/// let john = rost!({
///     name: "John Doe",
///     age: 43,
///     "favourite-color": null,
///     phones: ["+44 1234567", format!("+44 {phone}")],
///     address: { city: "London", zip: None::<String> },
/// });
/// assert_eq!(john["phones"][1], "+44 2345678");
/// assert_eq!(
///     john.to_string(),
///     "name: \"John Doe\", age: 43, favourite-color: null, phones [\"+44 1234567\", \"+44 2345678\"], address { city: \"London\", zip: null }"
/// );
/// ```
#[macro_export]
macro_rules! rost {
    (null) => {
        $crate::Value::Null
    };
    ([]) => {
        $crate::Value::List(::std::vec::Vec::new())
    };
    ([ $($tt:tt)+ ]) => {{
        let mut list = ::std::vec::Vec::new();
        $crate::__rost_list!(list () $($tt)+);
        $crate::Value::List(list)
    }};
    ({}) => {
        $crate::Value::Block($crate::Map::new())
    };
    ({ $($tt:tt)+ }) => {{
        let mut map = $crate::Map::new();
        $crate::__rost_map!(map $($tt)+);
        $crate::Value::Block(map)
    }};
    ($other:expr) => {
        $crate::Value::from($other)
    };
}

/// Collects the tokens of one list item up to a top-level comma.
#[macro_export]
#[doc(hidden)]
macro_rules! __rost_list {
    ($list:ident ($($item:tt)+) , $($rest:tt)*) => {
        $list.push($crate::rost!($($item)+));
        $crate::__rost_list!($list () $($rest)*);
    };
    ($list:ident ($($item:tt)*) $next:tt $($rest:tt)*) => {
        $crate::__rost_list!($list ($($item)* $next) $($rest)*);
    };
    ($list:ident ($($item:tt)+)) => {
        $list.push($crate::rost!($($item)+));
    };
    ($list:ident ()) => {};
}

/// Reads one `key:` of a block literal.
#[macro_export]
#[doc(hidden)]
macro_rules! __rost_map {
    ($map:ident) => {};
    ($map:ident $key:ident : $($rest:tt)+) => {
        $crate::__rost_entry!($map (stringify!($key)) () $($rest)+);
    };
    ($map:ident $key:literal : $($rest:tt)+) => {
        $crate::__rost_entry!($map ($key) () $($rest)+);
    };
}

/// Collects the tokens of one block value up to a top-level comma.
#[macro_export]
#[doc(hidden)]
macro_rules! __rost_entry {
    ($map:ident ($key:expr) ($($value:tt)+) , $($rest:tt)*) => {
        $map.insert($key, $crate::rost!($($value)+));
        $crate::__rost_map!($map $($rest)*);
    };
    ($map:ident ($key:expr) ($($value:tt)*) $next:tt $($rest:tt)*) => {
        $crate::__rost_entry!($map ($key) ($($value)* $next) $($rest)*);
    };
    ($map:ident ($key:expr) ($($value:tt)+)) => {
        $map.insert($key, $crate::rost!($($value)+));
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colliding_hashes_fall_back_to_overflow() {
        let mut map: Map = (0..20).map(|i| (format!("k{i}"), i)).collect();
        // Simulate a collision: "k7" now shares its slot with "k3", so it lives in overflow.
        let index = map.index.as_mut().unwrap();
        index.slots.insert(key_hash("k7"), 3);
        index.overflow.push(7);
        assert_eq!(map.get("k7"), Some(&Value::Int(7)));
        assert_eq!(map.get("k3"), Some(&Value::Int(3)));
        assert_eq!(map.insert("k7", Value::Null), Some(Value::Int(7)));
        assert_eq!(map.len(), 20);
        assert!(map.get("k20").is_none());
    }
}
