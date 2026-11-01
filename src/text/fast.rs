//! # Generated loaders
//!
//! [`Load`] is the trait `#[derive(Rost)]` implements: the derive writes, for each struct,
//! the loop that reads its entries and the conversion of each field, so a document is read
//! straight into the type without going through serde's visitor protocol.
//!
//! Every fast path has the serde reader as its fallback: a value the fast path does not
//! recognise (an integer written `1_000`, a quoted number, a unit, an escaped key…) is
//! read by exactly the code behind [`from_str`](crate::from_str), so [`load`] and
//! `from_str` agree on every document.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;

use super::*;

/// What [`Fast::field`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// The entry is the `n`-th of the names given.
    Known(usize),
    /// An entry that is none of the names: skip it with [`Fast::skip`].
    Unknown,
    /// The end of the block (or of the document).
    End,
}

/// A cursor over the entries of a block or of the document root.
pub struct Fast<'a, 'de>(Entries<'a, 'de>);

/// A type that reads itself from a `.rost` document. Derive it with `#[derive(Rost)]`.
pub trait Load<'de>: Sized {
    /// Reads the value of the entry whose key the cursor just consumed.
    fn load(f: &mut Fast<'_, 'de>) -> Result<Self, DeError>;

    /// Reads the entries of a block, up to its closing `}` (or to the end of the document),
    /// as this type. Only block-shaped types (structs, maps) provide it.
    fn load_body(f: &mut Fast<'_, 'de>) -> Result<Self, DeError> {
        f.skip()?;
        Err(DeError::new("expected a scalar value, found a block"))
    }
}

/// Loads a `T` from `.rost` text, or from a JSON object, like [`from_str`](crate::from_str)
/// but through the code `#[derive(Rost)]` generated.
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "derive")] {
/// use rost::Rost;
///
/// #[derive(Rost, Debug, PartialEq)]
/// struct Window {
///     width: u32,
///     title: String,
/// }
///
/// #[derive(Rost, Debug, PartialEq)]
/// struct Config {
///     debug: bool,
///     window: Window,
/// }
///
/// let config = rost::load::<Config>("debug: true\n#window { width: 1280, title: \"Hi\" }").unwrap();
/// assert_eq!(config, Config { debug: true, window: Window { width: 1280, title: "Hi".into() } });
/// # }
/// ```
pub fn load<'de, T: Load<'de>>(src: &'de str) -> Result<T, DeError> {
    let mut parser = Parser { src, bytes: src.as_bytes(), pos: 0, locate: true };
    parser.skip_trivia();
    let value = match parser.peek() {
        Some(b'[') => return Err(parser.error("a top-level list cannot be loaded as this type")),
        Some(b'{') => {
            // A document wrapped in `{ … }` reads as its contents.
            let Some((start, end)) = root_braces(&parser.bytes[parser.pos..]).map(|(s, e)| (parser.pos + s, parser.pos + e))
            else {
                return T::load_body(&mut Fast(Entries { p: &mut parser, root: true, key: "", pending: None }));
            };
            let mut body = Parser { src: &src[..end], bytes: &src.as_bytes()[..end], pos: start, locate: true };
            let value = T::load_body(&mut Fast(Entries { p: &mut body, root: true, key: "", pending: None }))?;
            parser.pos = end + 1;
            value
        }
        _ => T::load_body(&mut Fast(Entries { p: &mut parser, root: true, key: "", pending: None }))?,
    };
    parser.skip_trivia();
    if parser.pos < parser.bytes.len() {
        return Err(parser.error("trailing characters after the document"));
    }
    Ok(value)
}

impl<'a, 'de> Fast<'a, 'de> {
    /// Reads the next entry's key and matches it against `names`. `hint` is the index of the
    /// name expected next (entries usually come in declaration order), tried first.
    #[inline(always)]
    pub fn field(&mut self, names: &[&str], hint: usize) -> Result<Field, DeError> {
        match self.0.next_entry() {
            Ok(Some(key)) => {
                self.0.key = key;
                Ok(match_key(names, hint, key))
            }
            Ok(None) => Ok(Field::End),
            Err(Detour::EscapedKey(key)) => {
                self.0.key = key;
                Ok(match_key(names, hint, &unescape(key)))
            }
            Err(Detour::Error(e)) => Err(e),
        }
    }

    /// Reads the next entry's key, for maps. `None` at the end of the block.
    #[inline]
    pub fn key(&mut self) -> Result<Option<Cow<'de, str>>, DeError> {
        match self.0.next_entry() {
            Ok(key) => Ok(key.map(Cow::Borrowed)),
            Err(Detour::EscapedKey(key)) => Ok(Some(unescape(key))),
            Err(Detour::Error(e)) => Err(e),
        }
    }

    /// Prefixes `err` with the `line:column` of the entry just read, unless it already
    /// carries a position (an error from a nested block does).
    #[cold]
    pub fn locate(&self, err: DeError) -> DeError {
        let p = &*self.0.p;
        let msg = err.message();
        let mut parts = msg.splitn(3, ':');
        let positioned = matches!(
            (parts.next(), parts.next(), parts.next()),
            (Some(l), Some(c), Some(_)) if l.parse::<u32>().is_ok() && c.trim().parse::<u32>().is_ok()
        );
        if positioned || !p.locate {
            return err;
        }
        let base = p.src.as_ptr() as usize;
        let key = self.0.key.as_ptr() as usize;
        let at = if key >= base && key <= base + p.src.len() { key - base } else { p.pos };
        let at = at.min(p.bytes.len());
        let before = &p.bytes[..at];
        let line = memchr::Memchr::new(b'\n', before).count() + 1;
        let line_start = memchr::memrchr(b'\n', before).map_or(0, |i| i + 1);
        let column = p.src[line_start..at].chars().count() + 1;
        DeError::new(format!("{line}:{column}: {msg}"))
    }

    /// Skips the value of the entry just read, validating it.
    pub fn skip(&mut self) -> Result<(), DeError> {
        self.0.next_value::<IgnoredAny>().map(drop)
    }

    /// Reads the value of the entry just read with its serde `Deserialize` implementation.
    /// The fallback of every fast path, and the way to load types without a [`Load`].
    pub fn value<T: de::Deserialize<'de>>(&mut self) -> Result<T, DeError> {
        self.0.next_value()
    }

    /// Reads the value of the entry as a block, running `body` over its entries.
    #[inline]
    pub fn block<T>(&mut self, body: impl FnOnce(&mut Fast<'_, 'de>) -> Result<T, DeError>) -> Result<T, DeError> {
        let p = &mut *self.0.p;
        let opens = match self.0.pending {
            Some(Pending::Block) => Some(p.pos),
            Some(Pending::Scalar) => {
                let at = skip_inline_ws(p.bytes, p.pos);
                (p.bytes.get(at) == Some(&b'{')).then_some(at + 1)
            }
            _ => None,
        };
        match opens {
            Some(pos) => {
                self.0.pending = None;
                p.pos = pos;
                body(&mut Fast(Entries { p, root: false, key: "", pending: None }))
            }
            None => {
                // Not a block: let the serde reader read, and report, whatever it is.
                self.0.next_value::<IgnoredAny>()?;
                Err(DeError::new("expected a block"))
            }
        }
    }

    /// Reads an integer, falling back to the serde reader for anything but a plain one.
    #[inline(always)]
    fn plain_int(&mut self) -> Option<i64> {
        if !matches!(self.0.pending, Some(Pending::Scalar)) {
            return None;
        }
        let (value, _) = self.0.p.plain_int()?;
        self.0.pending = None;
        Some(value)
    }

    #[inline(always)]
    fn plain_decimal(&mut self) -> Option<f64> {
        if !matches!(self.0.pending, Some(Pending::Scalar)) {
            return None;
        }
        let value = self.0.p.plain_decimal()?;
        self.0.pending = None;
        Some(value)
    }

    #[inline(always)]
    fn plain_word(&mut self, word: &[u8]) -> bool {
        if !matches!(self.0.pending, Some(Pending::Scalar)) || !self.0.p.plain_word(word) {
            return false;
        }
        self.0.pending = None;
        true
    }
}

#[inline(always)]
fn match_key(names: &[&str], hint: usize, key: &str) -> Field {
    if names.get(hint).is_some_and(|n| *n == key) {
        return Field::Known(hint);
    }
    names.iter().position(|n| *n == key).map_or(Field::Unknown, Field::Known)
}

macro_rules! load_integers {
    ($($ty:ty),*) => {$(
        impl<'de> Load<'de> for $ty {
            #[inline]
            fn load(f: &mut Fast<'_, 'de>) -> Result<Self, DeError> {
                let saved = f.0.p.pos;
                if let Some(v) = f.plain_int() {
                    match <$ty>::try_from(v) {
                        Ok(v) => return Ok(v),
                        // Out of range: re-read it so the serde path reports the usual error.
                        Err(_) => {
                            f.0.p.pos = saved;
                            f.0.pending = Some(Pending::Scalar);
                        }
                    }
                }
                f.value()
            }
        }
    )*};
}

load_integers!(u8, u16, u32, u64, i8, i16, i32, i64);

impl<'de> Load<'de> for f64 {
    #[inline]
    fn load(f: &mut Fast<'_, 'de>) -> Result<Self, DeError> {
        match f.plain_decimal() {
            Some(v) => Ok(v),
            None => f.value(),
        }
    }
}

impl<'de> Load<'de> for f32 {
    fn load(f: &mut Fast<'_, 'de>) -> Result<Self, DeError> {
        f.value()
    }
}

impl<'de> Load<'de> for bool {
    #[inline]
    fn load(f: &mut Fast<'_, 'de>) -> Result<Self, DeError> {
        if f.plain_word(b"true") {
            Ok(true)
        } else if f.plain_word(b"false") {
            Ok(false)
        } else {
            f.value()
        }
    }
}

impl<'a, 'de> Fast<'a, 'de> {
    /// Reads a scalar the way the serde reader does and gives its text: escapes decoded
    /// for quoted values, verbatim for raw ones, trimmed for bare tokens. `None` if the
    /// value is not a scalar (the cursor is left where it was, for the serde fallback).
    #[inline(always)]
    fn text(&mut self) -> Option<Result<Cow<'de, str>, DeError>> {
        if !matches!(self.0.pending, Some(Pending::Scalar)) {
            return None;
        }
        let key = self.0.key;
        match self.0.p.read_scalar() {
            Ok((value, quote)) => {
                self.0.pending = None;
                Some(Ok(FieldDeserializer { name: key, value, quote }.text()))
            }
            Err(e) => Some(Err(e)),
        }
    }
}

impl<'de> Load<'de> for String {
    #[inline]
    fn load(f: &mut Fast<'_, 'de>) -> Result<Self, DeError> {
        match f.text() {
            Some(text) => text.map(Cow::into_owned),
            None => f.value(),
        }
    }
}

impl<'de: 'a, 'a> Load<'de> for &'a str {
    #[inline]
    fn load(f: &mut Fast<'_, 'de>) -> Result<Self, DeError> {
        let saved = f.0.p.pos;
        match f.text() {
            Some(Ok(Cow::Borrowed(text))) => Ok(text),
            Some(Ok(Cow::Owned(_))) => {
                // Escapes need an owned string: let the serde reader report that.
                f.0.p.pos = saved;
                f.0.pending = Some(Pending::Scalar);
                f.value()
            }
            Some(Err(e)) => Err(e),
            None => f.value(),
        }
    }
}

impl<'de, T: Load<'de>> Load<'de> for Option<T> {
    #[inline]
    fn load(f: &mut Fast<'_, 'de>) -> Result<Self, DeError> {
        if f.plain_word(b"null") {
            return Ok(None);
        }
        T::load(f).map(Some)
    }
}

impl<'de, T: Load<'de>> Load<'de> for Box<T> {
    fn load(f: &mut Fast<'_, 'de>) -> Result<Self, DeError> {
        T::load(f).map(Box::new)
    }

    fn load_body(f: &mut Fast<'_, 'de>) -> Result<Self, DeError> {
        T::load_body(f).map(Box::new)
    }
}

macro_rules! load_map {
    ($map:ident, $($bound:tt)*) => {
        impl<'de, V: Load<'de>> Load<'de> for $map<String, V> where String: $($bound)* {
            fn load(f: &mut Fast<'_, 'de>) -> Result<Self, DeError> {
                f.block(Self::load_body)
            }

            fn load_body(f: &mut Fast<'_, 'de>) -> Result<Self, DeError> {
                let mut map = $map::new();
                while let Some(key) = f.key()? {
                    let key = key.into_owned();
                    map.insert(key, V::load(f)?);
                }
                Ok(map)
            }
        }
    };
}

load_map!(HashMap, Hash + Eq);
load_map!(BTreeMap, Ord);
