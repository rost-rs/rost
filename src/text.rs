//! # Single-Pass Text Deserializer
//!
//! [`from_str`] and [`from_slice`] deserialize `.rost` text straight into Rust types in a
//! single pass, without building the bytecode index first. This is the fastest way to
//! load a document you only need as typed data, and the drop-in counterpart of
//! `serde_json::from_str`.
//!
//! The lexing rules are shared with [`crate::parser`], and scalars are converted by the
//! same code as [`crate::from_template`], so both paths accept the same documents and
//! produce the same values. Unlike the lenient bytecode parser, this deserializer is
//! strict: malformed input is an error that reports its line and column.

use memchr::{memchr, memchr2};
use serde::de::value::BorrowedStrDeserializer;
use serde::de::{self, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};

use crate::de::{DeError, FieldDeserializer, KeyDeserializer, MapEnum};
use std::borrow::Cow;

use crate::field::{unescape, QuoteStyle};
use crate::parser::{find_list_end, find_unescaped_quote, find_value_end, trim_trailing_ws, IS_DELIM_OR_WS, RAW_QUOTE};

/// Deserializes a `.rost` document from a string in a single pass.
///
/// The document root is a map: top-level properties, lists, enums and blocks become
/// fields. Strings without escapes are borrowed from `src`, so `T` may hold `&'de str`.
///
/// # Examples
///
/// ```
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct Config<'a> {
///     title: &'a str,
///     version: u32,
///     window: Window,
/// }
///
/// #[derive(Deserialize)]
/// struct Window {
///     width: u32,
///     scale: f64,
/// }
///
/// let src = "title: \"My App\"\nversion: 3\n#window {\n    width: 1280\n    scale: 1.25\n}\n";
/// let config: Config = rost::from_str(src).unwrap();
/// assert_eq!((config.title, config.version), ("My App", 3));
/// assert_eq!((config.window.width, config.window.scale), (1280, 1.25));
/// ```
pub fn from_str<'de, T: de::Deserialize<'de>>(src: &'de str) -> Result<T, DeError> {
    let mut parser = Parser { src, bytes: src.as_bytes(), pos: 0, locate: true };
    let value = T::deserialize(Root { p: &mut parser })?;
    parser.skip_trivia();
    if parser.pos < parser.bytes.len() {
        return Err(parser.error("trailing characters after the document"));
    }
    Ok(value)
}

/// Deserializes a `.rost` document from bytes in a single pass.
///
/// The bytes must be valid UTF-8; see [`from_str`].
pub fn from_slice<'de, T: de::Deserialize<'de>>(bytes: &'de [u8]) -> Result<T, DeError> {
    let src = std::str::from_utf8(bytes).map_err(|e| DeError(format!("invalid UTF-8: {e}")))?;
    from_str(src)
}

/// Deserializes a `.rost` document from an I/O stream.
///
/// The stream is read to the end first, so `T` must own its data
/// ([`DeserializeOwned`](de::DeserializeOwned)); use [`from_str`] to borrow strings.
///
/// # Examples
///
/// ```
/// #[derive(serde::Deserialize)]
/// struct Config {
///     port: u16,
/// }
///
/// let file = std::io::Cursor::new("port: 8080\n");
/// let config: Config = rost::from_reader(file).unwrap();
/// assert_eq!(config.port, 8080);
/// ```
pub fn from_reader<R: std::io::Read, T: de::DeserializeOwned>(mut reader: R) -> Result<T, DeError> {
    let mut src = String::new();
    reader.read_to_string(&mut src).map_err(|e| DeError(format!("I/O error: {e}")))?;
    from_str(&src)
}

/// Byte classes skipped between entries: whitespace, separators.
const TRIVIA: [bool; 256] = {
    let mut table = [false; 256];
    table[b' ' as usize] = true;
    table[b'\t' as usize] = true;
    table[b'\r' as usize] = true;
    table[b'\n' as usize] = true;
    table[b',' as usize] = true;
    table[b';' as usize] = true;
    table
};

/// Finds the closing quote of a string body and reports whether it contains escapes.
///
/// Typical values are short, so a byte loop handles strings without escapes; the first
/// backslash hands over to the escape-aware SIMD search.
#[inline(always)]
fn find_string_end(body: &[u8]) -> Option<(usize, bool)> {
    for (i, &b) in body.iter().enumerate().take(32) {
        match b {
            b'"' => return Some((i, false)),
            b'\\' => return find_unescaped_quote(body).map(|end| (end, true)),
            _ => {}
        }
    }
    let end = find_unescaped_quote(body)?;
    Some((end, memchr(b'\\', &body[..end]).is_some()))
}

// Scanning primitives are pure `(bytes, pos) -> pos` functions. The hot paths thread
// `pos` through them in a register and store it once: writing `Parser::pos` after every
// step and reading it back in the next one puts a store-to-load round trip on the
// critical path of every field.

/// Skips whitespace, newlines, `,` / `;` separators and `//` comments.
#[inline(always)]
fn skip_trivia(bytes: &[u8], mut pos: usize) -> usize {
    while let Some(&b) = bytes.get(pos) {
        if TRIVIA[b as usize] {
            pos += 1;
        } else if b == b'/' && bytes.get(pos + 1) == Some(&b'/') {
            pos = memchr(b'\n', &bytes[pos..]).map_or(bytes.len(), |p| pos + p);
        } else {
            break;
        }
    }
    pos
}

/// Skips spaces and tabs.
#[inline(always)]
fn skip_inline_ws(bytes: &[u8], mut pos: usize) -> usize {
    while matches!(bytes.get(pos), Some(b' ' | b'\t')) {
        pos += 1;
    }
    pos
}

/// Returns the end of the key starting at `pos`.
#[inline(always)]
fn key_end(bytes: &[u8], mut pos: usize) -> usize {
    while pos < bytes.len() && !IS_DELIM_OR_WS[bytes[pos] as usize] {
        pos += 1;
    }
    pos
}

/// Returns the end of a block or directive name: whitespace, `{` or `:`.
#[inline(always)]
fn name_end(bytes: &[u8], mut pos: usize) -> usize {
    while let Some(&b) = bytes.get(pos) {
        if b.is_ascii_whitespace() || b == b'{' || b == b':' {
            break;
        }
        pos += 1;
    }
    pos
}

/// Completes a bare token ending at `end` if nothing but trailing whitespace or `;`
/// separates it from the end of the value. Returns the position after the value,
/// consuming a following `,` like [`Parser::read_scalar`].
#[inline(always)]
fn finish_bare(bytes: &[u8], mut end: usize) -> Option<usize> {
    while matches!(bytes.get(end), Some(b' ' | b'\t' | b'\r' | b';')) {
        end += 1;
    }
    match bytes.get(end) {
        None | Some(b'\n' | b'}') => Some(end),
        Some(b',') => Some(end + 1),
        _ => None,
    }
}

struct Parser<'de> {
    src: &'de str,
    bytes: &'de [u8],
    pos: usize,
    /// Prefix errors with `line:col`; off when `src` is a fragment of a larger document.
    locate: bool,
}

impl<'de> Parser<'de> {
    /// Builds an error located at the current position.
    #[cold]
    fn error(&self, msg: &str) -> DeError {
        if !self.locate {
            return DeError(msg.to_string());
        }
        let at = self.pos.min(self.bytes.len());
        let before = &self.bytes[..at];
        let line = memchr::Memchr::new(b'\n', before).count() + 1;
        let line_start = memchr::memrchr(b'\n', before).map_or(0, |p| p + 1);
        let column = self.src[line_start..at].chars().count() + 1;
        DeError(format!("{line}:{column}: {msg}"))
    }

    #[inline(always)]
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    #[inline(always)]
    fn skip_trivia(&mut self) {
        self.pos = skip_trivia(self.bytes, self.pos);
    }

    #[inline(always)]
    fn skip_inline_ws(&mut self) {
        self.pos = skip_inline_ws(self.bytes, self.pos);
    }

    /// Takes a block or directive name: everything up to whitespace, `{` or `:`.
    #[inline]
    fn take_name(&mut self) -> &'de str {
        let start = self.pos;
        self.pos = name_end(self.bytes, start);
        &self.src[start..self.pos]
    }

    /// Reads the value after `key:` and reports how it was quoted.
    #[inline]
    fn read_scalar(&mut self) -> Result<(&'de str, QuoteStyle), DeError> {
        let bytes = self.bytes;
        let start = skip_inline_ws(bytes, self.pos);
        if bytes.get(start) != Some(&b'"') {
            let end = find_value_end(bytes, start).unwrap_or(bytes.len());
            let len = trim_trailing_ws(bytes, start, end);
            // A ',' belongs to this value; '\n' and '}' are left for the enclosing map.
            self.pos = if bytes.get(end) == Some(&b',') { end + 1 } else { end };
            return Ok((&self.src[start..start + len], QuoteStyle::Bare));
        }
        self.pos = start;
        if self.bytes[self.pos..].starts_with(RAW_QUOTE) {
            let start = self.pos + RAW_QUOTE.len();
            let Some(len) = memchr::memmem::find(&self.bytes[start..], RAW_QUOTE) else {
                return Err(self.error("unterminated raw string: missing closing `\"\"\"`"));
            };
            self.pos = start + len + RAW_QUOTE.len();
            return Ok((&self.src[start..start + len], QuoteStyle::Raw));
        }
        let start = self.pos + 1;
        let Some((len, escaped)) = find_string_end(&self.bytes[start..]) else {
            return Err(self.error("unterminated string literal"));
        };
        self.pos = start + len + 1;
        // Without escapes the body is used verbatim, which is exactly what `Raw` means.
        let quote = if escaped { QuoteStyle::Quoted } else { QuoteStyle::Raw };
        Ok((&self.src[start..start + len], quote))
    }

    /// Parses a plain integer value in place. Returns the value and its text.
    #[inline(always)]
    fn plain_int(&mut self) -> Option<(i64, &'de str)> {
        let bytes = self.bytes;
        let start = skip_inline_ws(bytes, self.pos);
        let negative = bytes.get(start) == Some(&b'-');
        let digits = start + usize::from(negative);
        let mut i = digits;
        let mut value: i64 = 0;
        while let Some(&b) = bytes.get(i) {
            let d = b.wrapping_sub(b'0');
            if d > 9 || i - digits == 18 {
                break;
            }
            value = value * 10 + i64::from(d);
            i += 1;
        }
        if i == digits || bytes.get(i).is_some_and(u8::is_ascii_digit) {
            return None;
        }
        self.pos = finish_bare(bytes, i)?;
        Some((if negative { -value } else { value }, &self.src[start..i]))
    }

    /// Parses a plain decimal value in place (see `parse_fast_f64` for the exactness
    /// argument of the fast path).
    #[inline(always)]
    fn plain_decimal(&mut self) -> Option<f64> {
        const POW10: [f64; 16] = [1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15];
        let bytes = self.bytes;
        let start = skip_inline_ws(bytes, self.pos);
        let negative = bytes.get(start) == Some(&b'-');
        let mut i = start + usize::from(negative);
        let mut mantissa: u64 = 0;
        let mut count = 0usize;
        let mut fraction: Option<usize> = None;
        while let Some(&b) = bytes.get(i) {
            match b {
                b'0'..=b'9' if count < 15 => {
                    mantissa = mantissa * 10 + u64::from(b - b'0');
                    count += 1;
                }
                b'.' if fraction.is_none() => fraction = Some(count),
                _ => break,
            }
            i += 1;
        }
        if count == 0 || bytes.get(i).is_some_and(|b| b.is_ascii_digit() || *b == b'.') {
            return None;
        }
        self.pos = finish_bare(bytes, i)?;
        let value = mantissa as f64 / POW10[count - fraction.unwrap_or(count)];
        Some(if negative { -value } else { value })
    }

    /// Consumes `word` (`true`, `false`, `null`) if it is the whole bare value.
    #[inline(always)]
    fn plain_word(&mut self, word: &[u8]) -> bool {
        let start = skip_inline_ws(self.bytes, self.pos);
        if !self.bytes[start..].starts_with(word) {
            return false;
        }
        match finish_bare(self.bytes, start + word.len()) {
            Some(pos) => {
                self.pos = pos;
                true
            }
            None => false,
        }
    }

    /// Reads a scalar list item: a quoted string, or a bare token up to the next `,` or
    /// `]` outside parentheses and brackets (so `rgb(1, 2, 3)` is one item).
    fn read_item(&mut self) -> Result<(&'de str, QuoteStyle), DeError> {
        let bytes = self.bytes;
        let start = self.pos;
        if bytes.get(start) == Some(&b'"') {
            let item = self.read_scalar()?;
            let next = skip_inline_ws(bytes, self.pos);
            match bytes.get(next) {
                None | Some(b',' | b']' | b'\n' | b'\r' | b';') => {}
                Some(b'/') if bytes.get(next + 1) == Some(&b'/') => {}
                _ => {
                    self.pos = next;
                    return Err(self.error("expected `,` or `]` after a list item"));
                }
            }
            return Ok(item);
        }
        let mut depth = 0u32;
        let mut i = start;
        while let Some(&b) = bytes.get(i) {
            match b {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b'}' => depth = depth.saturating_sub(1),
                b']' | b',' if depth == 0 => break,
                b']' => depth -= 1,
                b'"' => {
                    if let Some(end) = find_unescaped_quote(&bytes[i + 1..]) {
                        i += end + 1;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        self.pos = i;
        Ok((&self.src[start..start + trim_trailing_ws(bytes, start, i)], QuoteStyle::Bare))
    }

    /// Reads an enum body after `|`, up to the closing `|`.
    fn read_enum(&mut self) -> Result<&'de str, DeError> {
        let start = self.pos;
        let Some(len) = memchr(b'|', &self.bytes[start..]) else {
            return Err(self.error("unterminated enum: missing closing `|`"));
        };
        self.pos = start + len + 1;
        Ok(&self.src[start..start + len])
    }

    /// Skips the value of a `@directive` that does not open a block.
    fn skip_directive_value(&mut self) -> Result<(), DeError> {
        if self.peek() == Some(b'"') {
            let Some(len) = find_unescaped_quote(&self.bytes[self.pos + 1..]) else {
                return Err(self.error("unterminated string literal"));
            };
            self.pos += len + 2;
        } else {
            self.pos = memchr2(b'\n', b';', &self.bytes[self.pos..]).map_or(self.bytes.len(), |p| self.pos + p);
        }
        Ok(())
    }
}

/// What the key just read introduces.
#[derive(Clone, Copy)]
enum Pending {
    Block,
    Scalar,
    List,
    Enum,
}

/// Map over the entries of the root (`root == true`, ends at EOF) or of a block (ends at `}`).
struct Entries<'a, 'de> {
    p: &'a mut Parser<'de>,
    root: bool,
    key: &'de str,
    /// The current key was quoted and contains escapes to decode.
    escaped_key: bool,
    pending: Option<Pending>,
}

impl<'de> Entries<'_, 'de> {
    /// Advances to the next entry, returning its key, or `None` at the end of the map.
    ///
    /// Inlined fast path for `key: …`, `key { … }`, `key [ … ]`, `key | … |` and `}`;
    /// everything else goes through [`Entries::next_entry_slow`].
    #[inline(always)]
    fn next_entry(&mut self) -> Result<Option<&'de str>, DeError> {
        let bytes = self.p.bytes;
        let start = skip_trivia(bytes, self.p.pos);
        match bytes.get(start) {
            Some(&b) if b != b'#' && b != b'@' && b != b'"' && !IS_DELIM_OR_WS[b as usize] => {
                let end = key_end(bytes, start);
                let op = skip_inline_ws(bytes, end);
                let pending = match bytes.get(op) {
                    Some(b':') => Pending::Scalar,
                    Some(b'{') => Pending::Block,
                    Some(b'[') => Pending::List,
                    Some(b'|') => Pending::Enum,
                    _ => {
                        self.p.pos = start;
                        return self.next_entry_slow();
                    }
                };
                self.p.pos = op + 1;
                self.pending = Some(pending);
                Ok(Some(&self.p.src[start..end]))
            }
            Some(b'}') if !self.root => {
                self.p.pos = start + 1;
                Ok(None)
            }
            Some(b'#') if bytes.get(start + 1) != Some(&b'#') => {
                // `#name {` block selector; `#Title` headers take the slow path.
                let end = name_end(bytes, start + 1);
                let brace = skip_inline_ws(bytes, end);
                if bytes.get(brace) != Some(&b'{') {
                    self.p.pos = start;
                    return self.next_entry_slow();
                }
                self.p.pos = brace + 1;
                self.pending = Some(Pending::Block);
                Ok(Some(&self.p.src[start + 1..end]))
            }
            _ => {
                self.p.pos = start;
                self.next_entry_slow()
            }
        }
    }

    /// Handles every entry form, including directives, titles, bindings and errors.
    #[cold]
    #[inline(never)]
    fn next_entry_slow(&mut self) -> Result<Option<&'de str>, DeError> {
        let p = &mut *self.p;
        loop {
            p.skip_trivia();
            let Some(b) = p.peek() else {
                return if self.root { Ok(None) } else { Err(p.error("unclosed block: missing `}`")) };
            };
            match b {
                b'}' => {
                    if self.root {
                        return Err(p.error("unexpected `}` without a matching block"));
                    }
                    p.pos += 1;
                    return Ok(None);
                }
                b'@' => {
                    p.pos += 1;
                    let name = p.take_name();
                    p.skip_inline_ws();
                    if p.peek() == Some(b'{') {
                        p.pos += 1;
                        self.pending = Some(Pending::Block);
                        // Block names are addressed without `#`, as in `from_template`.
                        return Ok(Some(name.strip_prefix('#').unwrap_or(name)));
                    }
                    p.skip_directive_value()?;
                }
                b'#' => {
                    if p.bytes.get(p.pos + 1) == Some(&b'#') {
                        // `##name "value"` subtitle: metadata, not data.
                        p.pos += 2;
                        p.take_name();
                        p.skip_inline_ws();
                        if p.peek() == Some(b'"') {
                            p.skip_directive_value()?;
                        }
                        continue;
                    }
                    p.pos += 1;
                    let name = p.take_name();
                    p.skip_inline_ws();
                    if p.peek() == Some(b'{') {
                        p.pos += 1;
                        self.pending = Some(Pending::Block);
                        return Ok(Some(name));
                    }
                    // `#Title` without a block: a section header, not data.
                }
                b'"' => {
                    // Quoted key: `"any key": value`.
                    let start = p.pos + 1;
                    let Some((len, escaped)) = find_string_end(&p.bytes[start..]) else {
                        return Err(p.error("unterminated string literal"));
                    };
                    p.pos = start + len + 1;
                    let key = &p.src[start..start + len];
                    p.skip_inline_ws();
                    let pending = match p.peek() {
                        Some(b':') => Pending::Scalar,
                        Some(b'{') => Pending::Block,
                        Some(b'[') => Pending::List,
                        Some(b'|') => Pending::Enum,
                        Some(b'=') => {
                            p.pos = memchr2(b'\n', b'}', &p.bytes[p.pos..]).map_or(p.bytes.len(), |e| p.pos + e);
                            continue;
                        }
                        _ => return Err(p.error("expected `:`, `=`, `|`, `[` or `{` after key")),
                    };
                    p.pos += 1;
                    self.pending = Some(pending);
                    self.escaped_key = escaped;
                    return Ok(Some(key));
                }
                _ => {
                    let start = p.pos;
                    p.pos = key_end(p.bytes, start);
                    let key = &p.src[start..p.pos];
                    if key.is_empty() {
                        return Err(p.error("expected a key"));
                    }
                    p.skip_inline_ws();
                    let pending = match p.peek() {
                        Some(b':') => Pending::Scalar,
                        Some(b'{') => Pending::Block,
                        Some(b'[') => Pending::List,
                        Some(b'|') => Pending::Enum,
                        Some(b'=') => {
                            // `key = expr` bindings are behaviour, not data.
                            p.pos = memchr2(b'\n', b'}', &p.bytes[p.pos..]).map_or(p.bytes.len(), |e| p.pos + e);
                            continue;
                        }
                        _ => {
                            p.pos = start;
                            return Err(p.error("expected `:`, `=`, `|`, `[` or `{` after key"));
                        }
                    };
                    p.pos += 1;
                    self.pending = Some(pending);
                    return Ok(Some(key));
                }
            }
        }
    }
}

impl<'de> MapAccess<'de> for Entries<'_, 'de> {
    type Error = DeError;

    #[inline]
    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, DeError> {
        match self.next_entry()? {
            Some(key) => {
                self.key = key;
                let text = if self.escaped_key {
                    self.escaped_key = false;
                    unescape(key)
                } else {
                    Cow::Borrowed(key)
                };
                seed.deserialize(KeyDeserializer(text)).map(Some)
            }
            None => Ok(None),
        }
    }

    #[inline]
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, DeError> {
        match self.pending.take() {
            Some(Pending::Scalar) => seed.deserialize(Scalar { p: &mut *self.p, key: self.key }),
            Some(Pending::Block) => seed.deserialize(Block { p: &mut *self.p, root: false }),
            Some(Pending::List) => seed.deserialize(List { p: &mut *self.p }),
            Some(Pending::Enum) => {
                // Like `from_template`: an enum field deserializes as its first variant.
                let body = self.p.read_enum()?;
                let first = body.split(',').next().unwrap_or("").trim();
                seed.deserialize(BorrowedStrDeserializer::new(first))
            }
            None => Err(DeError("next_value called before next_key".into())),
        }
    }
}

/// The document root: a block without braces.
struct Root<'a, 'de> {
    p: &'a mut Parser<'de>,
}

impl<'de> Deserializer<'de> for Root<'_, 'de> {
    type Error = DeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        Block { p: self.p, root: true }.deserialize_any(visitor)
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        visitor.visit_some(self)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        Block { p: self.p, root: true }.deserialize_ignored_any(visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, DeError> {
        Block { p: self.p, root: true }.deserialize_enum(name, variants, visitor)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct newtype_struct seq tuple
        tuple_struct map struct identifier
    }
}

/// A block body (after `{`), or the root when `root` is set.
struct Block<'a, 'de> {
    p: &'a mut Parser<'de>,
    root: bool,
}

impl<'de> Deserializer<'de> for Block<'_, 'de> {
    type Error = DeError;

    #[inline]
    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        visitor.visit_map(Entries { p: self.p, root: self.root, key: "", escaped_key: false, pending: None })
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        visitor.visit_some(self)
    }

    /// Skips the block while still validating it.
    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        let mut entries = Entries { p: self.p, root: self.root, key: "", escaped_key: false, pending: None };
        while entries.next_key::<IgnoredAny>()?.is_some() {
            entries.next_value::<IgnoredAny>()?;
        }
        visitor.visit_unit()
    }

    /// A block with one entry holds an enum variant with data.
    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, DeError> {
        visitor.visit_enum(MapEnum(Entries { p: self.p, root: self.root, key: "", escaped_key: false, pending: None }))
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct newtype_struct seq tuple
        tuple_struct map struct identifier
    }
}

/// Deserializes a list body (the text between `[` and `]`) with the same item rules as
/// [`from_str`]. Used by the bytecode path, whose list records store the raw body.
pub(crate) fn visit_list_body<'de, V: Visitor<'de>>(raw: &'de str, visitor: V) -> Result<V::Value, DeError> {
    let mut parser = Parser { src: raw, bytes: raw.as_bytes(), pos: 0, locate: false };
    visit_items(&mut parser, visitor)
}

/// Visits the items of a list body that runs to the end of `p`'s input.
fn visit_items<'de, V: Visitor<'de>>(p: &mut Parser<'de>, visitor: V) -> Result<V::Value, DeError> {
    let mut items = Items { p, done: false };
    let value = visitor.visit_seq(&mut items)?;
    // A visitor may stop early (tuples, fixed arrays): the rest must not be left unread.
    if items.next_element::<IgnoredAny>()?.is_some() {
        return Err(items.p.error("list has more items than expected"));
    }
    Ok(value)
}

/// A list value after its `[`.
///
/// The body is delimited first with the bytecode parser's own scanner, then its items
/// are read inside those bounds, so both paths agree on where every list ends.
struct List<'a, 'de> {
    p: &'a mut Parser<'de>,
}

impl<'de> Deserializer<'de> for List<'_, 'de> {
    type Error = DeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        let p = self.p;
        let Some(end) = find_list_end(p.bytes, p.pos) else {
            return Err(p.error("unterminated list: missing closing `]`"));
        };
        // Truncating the input keeps positions absolute for error messages.
        let mut body = Parser { src: &p.src[..end], bytes: &p.bytes[..end], pos: p.pos, locate: p.locate };
        let value = visit_items(&mut body, visitor)?;
        p.pos = end + 1;
        Ok(value)
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        visitor.visit_some(self)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

/// The items of a list body: scalars, inline blocks `{ … }` and nested lists `[ … ]`.
struct Items<'a, 'de> {
    p: &'a mut Parser<'de>,
    done: bool,
}

impl<'de> SeqAccess<'de> for Items<'_, 'de> {
    type Error = DeError;

    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>, DeError> {
        if self.done {
            return Ok(None);
        }
        let p = &mut *self.p;
        p.skip_trivia();
        match p.peek() {
            None => {
                self.done = true;
                Ok(None)
            }
            Some(b']') => Err(p.error("unexpected `]` in list")),
            Some(b'{') => {
                p.pos += 1;
                seed.deserialize(Block { p, root: false }).map(Some)
            }
            Some(b'[') => {
                p.pos += 1;
                seed.deserialize(List { p }).map(Some)
            }
            Some(_) => {
                let (value, quote) = p.read_item()?;
                seed.deserialize(FieldDeserializer { name: "list item", value, quote }).map(Some)
            }
        }
    }
}

/// The value after `key:`, read lazily so the type hint picks the parser.
///
/// Integers, plain decimals, booleans and `null` are converted while scanning; anything
/// else (units, hex colors, escapes, …) is read as text and handed to the same
/// [`FieldDeserializer`] that `from_template` uses, so results never differ.
struct Scalar<'a, 'de> {
    p: &'a mut Parser<'de>,
    key: &'de str,
}

impl<'de> Scalar<'_, 'de> {
    fn text(self) -> Result<FieldDeserializer<'de>, DeError> {
        let (value, quote) = self.p.read_scalar()?;
        Ok(FieldDeserializer { name: self.key, value, quote })
    }
}

macro_rules! fused_integers {
    ($($method:ident => $visit:ident: $ty:ty),* $(,)?) => {$(
        #[inline]
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
            match self.p.plain_int() {
                Some((value, text)) => match <$ty>::try_from(value) {
                    Ok(v) => visitor.$visit(v),
                    // Out of range: let the shared path produce the usual error.
                    Err(_) => FieldDeserializer { name: self.key, value: text, quote: QuoteStyle::Bare }.$method(visitor),
                },
                None => self.text()?.$method(visitor),
            }
        }
    )*};
}

impl<'de> Deserializer<'de> for Scalar<'_, 'de> {
    type Error = DeError;

    #[inline]
    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        self.text()?.deserialize_any(visitor)
    }

    fused_integers! {
        deserialize_i8 => visit_i8: i8,
        deserialize_i16 => visit_i16: i16,
        deserialize_i32 => visit_i32: i32,
        deserialize_i64 => visit_i64: i64,
        deserialize_u8 => visit_u8: u8,
        deserialize_u16 => visit_u16: u16,
        deserialize_u32 => visit_u32: u32,
        deserialize_u64 => visit_u64: u64,
    }

    #[inline]
    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match self.p.plain_decimal() {
            Some(v) => visitor.visit_f64(v),
            None => self.text()?.deserialize_f64(visitor),
        }
    }

    #[inline]
    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        self.text()?.deserialize_f32(visitor)
    }

    #[inline]
    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        if self.p.plain_word(b"true") {
            return visitor.visit_bool(true);
        }
        if self.p.plain_word(b"false") {
            return visitor.visit_bool(false);
        }
        self.text()?.deserialize_bool(visitor)
    }

    #[inline]
    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        self.text()?.deserialize_str(visitor)
    }

    #[inline]
    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        self.text()?.deserialize_string(visitor)
    }

    #[inline]
    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, DeError> {
        self.text()?.deserialize_enum(name, variants, visitor)
    }

    #[inline]
    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        if self.p.plain_word(b"null") {
            return visitor.visit_none();
        }
        if self.p.bytes.get(skip_inline_ws(self.p.bytes, self.p.pos)) == Some(&b'"') {
            // Quoted values are never `null`; keep the lazy fast paths for the inner value.
            return visitor.visit_some(self);
        }
        // Bare values decide `null` with the shared trimming rules.
        self.text()?.deserialize_option(visitor)
    }

    serde::forward_to_deserialize_any! {
        i128 u128 char bytes byte_buf unit unit_struct newtype_struct seq tuple
        tuple_struct map struct identifier ignored_any
    }
}
