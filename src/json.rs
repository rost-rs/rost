//! # Strict JSON
//!
//! Every JSON object is already a rost document: [`Template::parse`](crate::Template::parse)
//! and [`from_str`](crate::from_str) read it as one. The rost grammar is lenient about
//! JSON, though: it also accepts comments, trailing commas, bare words and unquoted keys.
//! This module is the strict side, for input that must be RFC 8259 JSON and nothing else:
//!
//! - [`validate`] checks a document without building anything;
//! - [`Template::from_json`](crate::Template::from_json) validates, then indexes the
//!   object with the regular zero-copy rost parser;
//! - [`from_str`] and [`from_slice`] build a [`Value`](crate::Value) (`serde` feature);
//! - [`to_string`] and [`to_string_pretty`] write a [`Value`](crate::Value) as JSON.
//!
//! Errors are [`ParseError`]s with the line and column of the offending byte. Nesting is
//! limited to [`MAX_DEPTH`] arrays and objects, so hostile input cannot exhaust the stack.
//!
//! # Converting
//!
//! ```
//! # #[cfg(feature = "serde")] {
//! // JSON -> rost
//! let value = rost::json::from_str(r#"{"window": {"width": 800, "tags": ["a", "b"]}}"#).unwrap();
//! let doc = rost::to_string(&value).unwrap();
//! assert_eq!(rost::from_str::<rost::Value>(&doc).unwrap(), value);
//!
//! // rost -> JSON
//! let value: rost::Value = rost::from_str("window { width: 800 }").unwrap();
//! assert_eq!(rost::json::to_string(&value), r#"{"window":{"width":800}}"#);
//! # }
//! ```

use std::borrow::Cow;

use crate::error::{ParseError, ParseErrorKind};

/// The deepest nesting of arrays and objects the strict parser accepts.
pub const MAX_DEPTH: usize = 128;

/// Checks that `src` is a single RFC 8259 JSON value, building nothing.
///
/// # Examples
///
/// ```
/// assert!(rost::json::validate(r#"{"a": [1, 2.5e3, "x", null]}"#).is_ok());
///
/// let err = rost::json::validate("{\"a\": [1, 2,]}").unwrap_err();
/// assert_eq!(err.to_string(), "1:13: invalid JSON: expected a value");
/// ```
pub fn validate(src: &str) -> Result<(), ParseError> {
    Parser::new(src, Validate).document()
}

/// Parses `src` as strict JSON into a [`Value`](crate::Value).
///
/// Objects keep their key order; a repeated key keeps its last value. Integers that fit
/// an `i64` become [`Value::Int`](crate::Value::Int), every other number (and `-0`) a
/// [`Value::Float`](crate::Value::Float).
///
/// # Examples
///
/// ```
/// let v = rost::json::from_str(r#"{"name": "app", "size": [800, 600]}"#).unwrap();
/// assert_eq!(v["size"][1], 600);
/// assert!(rost::json::from_str("{name: 'app'}").is_err());
/// ```
#[cfg(feature = "serde")]
pub fn from_str(src: &str) -> Result<crate::Value, ParseError> {
    Parser::new(src, Build).document()
}

/// Like [`from_str`], for bytes that must also be valid UTF-8.
#[cfg(feature = "serde")]
pub fn from_slice(src: &[u8]) -> Result<crate::Value, ParseError> {
    match std::str::from_utf8(src) {
        Ok(text) => from_str(text),
        Err(e) => {
            let at = e.valid_up_to();
            Err(json_error(src, at, at + 1, "invalid UTF-8"))
        }
    }
}

/// Writes `value` as compact JSON.
///
/// Non-finite floats have no JSON form and are written as `null`.
///
/// # Examples
///
/// ```
/// let v = rost::rost!({ "name": "app", "size": [800, 600], "ratio": 1.0 });
/// assert_eq!(rost::json::to_string(&v), r#"{"name":"app","size":[800,600],"ratio":1.0}"#);
/// ```
#[cfg(feature = "serde")]
pub fn to_string(value: &crate::Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, None, 0);
    out
}

/// Writes `value` as JSON indented by two spaces.
#[cfg(feature = "serde")]
pub fn to_string_pretty(value: &crate::Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, Some("  "), 0);
    out
}

fn json_error(src: &[u8], start: usize, end: usize, msg: &'static str) -> ParseError {
    ParseError::new(ParseErrorKind::InvalidJson(msg), src, start..end.min(src.len()).max(start))
}

/// What the parser does with each value it reads.
trait Sink {
    type Value;
    type Array;
    type Object;

    fn null(&mut self) -> Self::Value;
    fn bool(&mut self, b: bool) -> Self::Value;
    /// `text` is a valid JSON number; `integer` is true when it has no fraction or exponent.
    fn number(&mut self, text: &str, integer: bool) -> Option<Self::Value>;
    fn string(&mut self, s: Cow<'_, str>) -> Self::Value;
    fn array(&mut self) -> Self::Array;
    fn push(&mut self, array: &mut Self::Array, value: Self::Value);
    fn end_array(&mut self, array: Self::Array) -> Self::Value;
    fn object(&mut self) -> Self::Object;
    fn insert(&mut self, object: &mut Self::Object, key: Cow<'_, str>, value: Self::Value);
    fn end_object(&mut self, object: Self::Object) -> Self::Value;
}

struct Validate;

impl Sink for Validate {
    type Value = ();
    type Array = ();
    type Object = ();

    fn null(&mut self) {}
    fn bool(&mut self, _: bool) {}
    fn number(&mut self, text: &str, _: bool) -> Option<()> {
        finite(text).map(drop)
    }
    fn string(&mut self, _: Cow<'_, str>) {}
    fn array(&mut self) {}
    fn push(&mut self, _: &mut (), _: ()) {}
    fn end_array(&mut self, _: ()) {}
    fn object(&mut self) {}
    fn insert(&mut self, _: &mut (), _: Cow<'_, str>, _: ()) {}
    fn end_object(&mut self, _: ()) {}
}

/// Parses a number that must stay finite: `1e400` has no `f64` and is rejected.
fn finite(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|f| f.is_finite())
}

#[cfg(feature = "serde")]
struct Build;

#[cfg(feature = "serde")]
impl Sink for Build {
    type Value = crate::Value;
    type Array = Vec<crate::Value>;
    type Object = crate::Map;

    fn null(&mut self) -> crate::Value {
        crate::Value::Null
    }
    fn bool(&mut self, b: bool) -> crate::Value {
        crate::Value::Bool(b)
    }
    fn number(&mut self, text: &str, integer: bool) -> Option<crate::Value> {
        // `-0` has no i64 form; as a float it keeps its sign, like the rost readers.
        if integer
            && let Ok(i) = text.parse::<i64>()
            && (i != 0 || !text.starts_with('-'))
        {
            return Some(crate::Value::Int(i));
        }
        finite(text).map(crate::Value::Float)
    }
    fn string(&mut self, s: Cow<'_, str>) -> crate::Value {
        crate::Value::String(s.into_owned())
    }
    fn array(&mut self) -> Vec<crate::Value> {
        Vec::new()
    }
    fn push(&mut self, array: &mut Vec<crate::Value>, value: crate::Value) {
        array.push(value);
    }
    fn end_array(&mut self, array: Vec<crate::Value>) -> crate::Value {
        crate::Value::List(array)
    }
    fn object(&mut self) -> crate::Map {
        crate::Map::new()
    }
    fn insert(&mut self, object: &mut crate::Map, key: Cow<'_, str>, value: crate::Value) {
        object.insert(key.into_owned(), value);
    }
    fn end_object(&mut self, object: crate::Map) -> crate::Value {
        crate::Value::Block(object)
    }
}

struct Parser<'a, S> {
    src: &'a [u8],
    pos: usize,
    depth: usize,
    sink: S,
}

impl<'a, S: Sink> Parser<'a, S> {
    fn new(src: &'a str, sink: S) -> Self {
        Self {
            src: src.as_bytes(),
            pos: 0,
            depth: 0,
            sink,
        }
    }

    fn error(&self, msg: &'static str) -> ParseError {
        json_error(self.src, self.pos, self.pos + 1, msg)
    }

    fn document(mut self) -> Result<S::Value, ParseError> {
        self.skip_ws();
        let value = self.value()?;
        self.skip_ws();
        if self.pos < self.src.len() {
            return Err(self.error("trailing characters after the value"));
        }
        Ok(value)
    }

    fn skip_ws(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.src.get(self.pos) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn literal(&mut self, word: &[u8]) -> Result<(), ParseError> {
        if self.src[self.pos..].starts_with(word) {
            self.pos += word.len();
            Ok(())
        } else {
            Err(self.error("expected a value"))
        }
    }

    fn value(&mut self) -> Result<S::Value, ParseError> {
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => {
                let s = self.string()?;
                Ok(self.sink.string(s))
            }
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(b't') => self.literal(b"true").map(|()| self.sink.bool(true)),
            Some(b'f') => self.literal(b"false").map(|()| self.sink.bool(false)),
            Some(b'n') => self.literal(b"null").map(|()| self.sink.null()),
            _ => Err(self.error("expected a value")),
        }
    }

    fn enter(&mut self) -> Result<(), ParseError> {
        if self.depth == MAX_DEPTH {
            return Err(self.error("nested too deeply"));
        }
        self.depth += 1;
        self.pos += 1;
        self.skip_ws();
        Ok(())
    }

    fn array(&mut self) -> Result<S::Value, ParseError> {
        self.enter()?;
        let mut array = self.sink.array();
        if self.peek() != Some(b']') {
            loop {
                let value = self.value()?;
                self.sink.push(&mut array, value);
                self.skip_ws();
                match self.peek() {
                    Some(b',') => {
                        self.pos += 1;
                        self.skip_ws();
                    }
                    Some(b']') => break,
                    _ => return Err(self.error("expected `,` or `]`")),
                }
            }
        }
        self.pos += 1;
        self.depth -= 1;
        Ok(self.sink.end_array(array))
    }

    fn object(&mut self) -> Result<S::Value, ParseError> {
        self.enter()?;
        let mut object = self.sink.object();
        if self.peek() != Some(b'}') {
            loop {
                if self.peek() != Some(b'"') {
                    return Err(self.error("expected a string key"));
                }
                let key = self.string()?;
                self.skip_ws();
                if self.peek() != Some(b':') {
                    return Err(self.error("expected `:`"));
                }
                self.pos += 1;
                self.skip_ws();
                let value = self.value()?;
                self.sink.insert(&mut object, key, value);
                self.skip_ws();
                match self.peek() {
                    Some(b',') => {
                        self.pos += 1;
                        self.skip_ws();
                    }
                    Some(b'}') => break,
                    _ => return Err(self.error("expected `,` or `}`")),
                }
            }
        }
        self.pos += 1;
        self.depth -= 1;
        Ok(self.sink.end_object(object))
    }

    fn digits(&mut self) -> usize {
        let start = self.pos;
        while let Some(b'0'..=b'9') = self.peek() {
            self.pos += 1;
        }
        self.pos - start
    }

    fn number(&mut self) -> Result<S::Value, ParseError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(self.error("expected a digit")),
        }
        let mut integer = true;
        if self.peek() == Some(b'.') {
            integer = false;
            self.pos += 1;
            if self.digits() == 0 {
                return Err(self.error("expected a digit after `.`"));
            }
        }
        if let Some(b'e' | b'E') = self.peek() {
            integer = false;
            self.pos += 1;
            if let Some(b'+' | b'-') = self.peek() {
                self.pos += 1;
            }
            if self.digits() == 0 {
                return Err(self.error("expected a digit in the exponent"));
            }
        }
        // Only ASCII digits, signs, `.` and `e` were consumed.
        let text = std::str::from_utf8(&self.src[start..self.pos]).unwrap_or_default();
        self.sink
            .number(text, integer)
            .ok_or_else(|| json_error(self.src, start, self.pos, "number out of range"))
    }

    /// Reads a string starting at its opening quote; borrows it when it has no escapes.
    fn string(&mut self) -> Result<Cow<'a, str>, ParseError> {
        self.pos += 1;
        let start = self.pos;
        loop {
            match self.peek() {
                None => return Err(self.error("unterminated string")),
                Some(b'"') => {
                    // Quotes and backslashes are ASCII, so `start..pos` is on char boundaries.
                    let s = std::str::from_utf8(&self.src[start..self.pos]).unwrap_or_default();
                    self.pos += 1;
                    return Ok(Cow::Borrowed(s));
                }
                Some(b'\\') => break,
                Some(0..=0x1f) => return Err(self.error("control character in string")),
                Some(_) => self.pos += 1,
            }
        }
        self.escaped_string(start).map(Cow::Owned)
    }

    #[cold]
    fn escaped_string(&mut self, start: usize) -> Result<String, ParseError> {
        let mut out = String::from_utf8_lossy(&self.src[start..self.pos]).into_owned();
        loop {
            let run = self.pos;
            while let Some(b) = self.peek() {
                if b == b'"' || b == b'\\' || b < 0x20 {
                    break;
                }
                self.pos += 1;
            }
            out.push_str(std::str::from_utf8(&self.src[run..self.pos]).unwrap_or_default());
            match self.peek() {
                None => return Err(self.error("unterminated string")),
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(b'\\') => {}
                Some(_) => return Err(self.error("control character in string")),
            }
            self.pos += 1;
            let c = match self.peek() {
                Some(b'"') => '"',
                Some(b'\\') => '\\',
                Some(b'/') => '/',
                Some(b'b') => '\u{8}',
                Some(b'f') => '\u{c}',
                Some(b'n') => '\n',
                Some(b'r') => '\r',
                Some(b't') => '\t',
                Some(b'u') => {
                    self.pos += 1;
                    let c = self.unicode_escape()?;
                    out.push(c);
                    continue;
                }
                _ => return Err(self.error("invalid escape")),
            };
            self.pos += 1;
            out.push(c);
        }
    }

    fn hex4(&mut self) -> Result<u32, ParseError> {
        let mut v = 0;
        for _ in 0..4 {
            let d = match self.peek() {
                Some(b @ b'0'..=b'9') => b - b'0',
                Some(b @ b'a'..=b'f') => b - b'a' + 10,
                Some(b @ b'A'..=b'F') => b - b'A' + 10,
                _ => return Err(self.error("expected four hex digits")),
            };
            v = v * 16 + u32::from(d);
            self.pos += 1;
        }
        Ok(v)
    }

    /// Decodes the digits after `\u`, joining a surrogate pair into one char.
    fn unicode_escape(&mut self) -> Result<char, ParseError> {
        let at = self.pos;
        let hi = self.hex4()?;
        let code = match hi {
            0xD800..=0xDBFF => {
                if !self.src[self.pos..].starts_with(b"\\u") {
                    return Err(json_error(self.src, at, self.pos, "unpaired surrogate"));
                }
                self.pos += 2;
                let lo = self.hex4()?;
                if !(0xDC00..=0xDFFF).contains(&lo) {
                    return Err(json_error(self.src, at, self.pos, "unpaired surrogate"));
                }
                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
            }
            0xDC00..=0xDFFF => {
                return Err(json_error(self.src, at, self.pos, "unpaired surrogate"));
            }
            _ => hi,
        };
        Ok(char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER))
    }
}

#[cfg(feature = "serde")]
fn write_value(out: &mut String, value: &crate::Value, indent: Option<&str>, depth: usize) {
    use crate::Value;
    use std::fmt::Write;

    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => {
            let _ = write!(out, "{i}");
        }
        // `{:?}` is the shortest text that reads back to the same float, and keeps a
        // `.0` or an exponent so the value stays a float.
        Value::Float(f) if f.is_finite() => {
            let _ = write!(out, "{f:?}");
        }
        Value::Float(_) => out.push_str("null"),
        Value::String(s) => write_string(out, s),
        Value::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, indent, depth + 1);
                write_value(out, item, indent, depth + 1);
            }
            if !items.is_empty() {
                newline(out, indent, depth);
            }
            out.push(']');
        }
        Value::Block(map) => {
            out.push('{');
            for (i, (key, item)) in map.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, indent, depth + 1);
                write_string(out, key);
                out.push(':');
                if indent.is_some() {
                    out.push(' ');
                }
                write_value(out, item, indent, depth + 1);
            }
            if !map.is_empty() {
                newline(out, indent, depth);
            }
            out.push('}');
        }
    }
}

#[cfg(feature = "serde")]
fn newline(out: &mut String, indent: Option<&str>, depth: usize) {
    if let Some(unit) = indent {
        out.push('\n');
        for _ in 0..depth {
            out.push_str(unit);
        }
    }
}

#[cfg(feature = "serde")]
fn write_string(out: &mut String, s: &str) {
    out.push('"');
    let mut run = 0;
    for (i, b) in s.bytes().enumerate() {
        let escape = match b {
            b'"' => "\\\"",
            b'\\' => "\\\\",
            b'\n' => "\\n",
            b'\r' => "\\r",
            b'\t' => "\\t",
            0..=0x1f => "",
            _ => continue,
        };
        out.push_str(&s[run..i]);
        run = i + 1;
        if escape.is_empty() {
            use std::fmt::Write;
            let _ = write!(out, "\\u{b:04x}");
        } else {
            out.push_str(escape);
        }
    }
    out.push_str(&s[run..]);
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rejects(src: &str) -> String {
        validate(src).unwrap_err().to_string()
    }

    #[test]
    fn accepts_rfc_values() {
        for src in [
            "0", "-0", "-0.5e+3", "1E2", "true", "null", "\"\"", "[]", "{}", " [1, [2, {\"a\": []}]] ",
            r#""é😀\/\b\f\n\r\t\"\\""#,
        ] {
            assert!(validate(src).is_ok(), "{src}");
        }
    }

    #[test]
    fn rejects_the_lenient_extensions() {
        assert_eq!(rejects("{a: 1}"), "1:2: invalid JSON: expected a string key");
        assert_eq!(rejects("[1,]"), "1:4: invalid JSON: expected a value");
        assert_eq!(rejects("{\"a\": 1,}"), "1:9: invalid JSON: expected a string key");
        assert_eq!(rejects("[1] // c"), "1:5: invalid JSON: trailing characters after the value");
        assert_eq!(rejects("['a']"), "1:2: invalid JSON: expected a value");
        assert_eq!(rejects("[01]"), "1:3: invalid JSON: expected `,` or `]`");
        assert_eq!(rejects("[1.]"), "1:4: invalid JSON: expected a digit after `.`");
        assert_eq!(rejects("[.5]"), "1:2: invalid JSON: expected a value");
        assert_eq!(rejects("[+1]"), "1:2: invalid JSON: expected a value");
        assert_eq!(rejects("[1e400]"), "1:2: invalid JSON: number out of range");
        assert_eq!(rejects("\"a\tb\""), "1:3: invalid JSON: control character in string");
        assert_eq!(rejects(r#""\x""#), "1:3: invalid JSON: invalid escape");
        assert_eq!(rejects(r#""\ud800""#), "1:4: invalid JSON: unpaired surrogate");
        assert_eq!(rejects("\u{feff}{}"), "1:1: invalid JSON: expected a value");
        assert_eq!(rejects(""), "1:1: invalid JSON: expected a value");
        assert_eq!(rejects("{\"a\" 1}"), "1:6: invalid JSON: expected `:`");
    }

    #[test]
    fn nesting_is_limited_without_overflowing_the_stack() {
        let ok = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
        assert!(validate(&ok).is_ok());
        let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
        assert!(rejects(&deep).ends_with("nested too deeply"));
        assert!(rejects(&"[".repeat(100_000)).ends_with("nested too deeply"));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn builds_and_writes_values() {
        use crate::Value;

        let src = r#"{"s":"a\"b\\c\n\u0001é","i":-7,"big":18446744073709551615,"f":1.5,"e":1e-7,"z":2.0,"l":[true,false,null],"o":{},"d":1,"d":2}"#;
        let v = from_str(src).unwrap();
        assert_eq!(v["s"], "a\"b\\c\n\u{1}é");
        assert_eq!(v["i"], Value::Int(-7));
        assert_eq!(v["big"], Value::Float(18446744073709551615.0));
        assert_eq!(v["d"], Value::Int(2));
        assert_eq!(v.as_block().unwrap().len(), 9);
        let compact = to_string(&v);
        assert_eq!(
            compact,
            r#"{"s":"a\"b\\c\n\u0001é","i":-7,"big":1.8446744073709552e19,"f":1.5,"e":1e-7,"z":2.0,"l":[true,false,null],"o":{},"d":2}"#
        );
        assert_eq!(from_str(&compact).unwrap(), v);
        assert_eq!(from_str(&to_string_pretty(&v)).unwrap(), v);
        assert_eq!(to_string(&Value::Float(f64::NAN)), "null");
        let zero = from_str("-0").unwrap().as_f64().unwrap();
        assert!(zero == 0.0 && zero.is_sign_negative());
        assert_eq!(to_string(&Value::Float(zero)), "-0.0");
        assert_eq!(
            to_string_pretty(&from_str(r#"{"a":[1,{}],"b":[]}"#).unwrap()),
            "{\n  \"a\": [\n    1,\n    {}\n  ],\n  \"b\": []\n}"
        );
        assert!(from_slice(b"[\"\xff\"]").unwrap_err().to_string().ends_with("invalid UTF-8"));
    }
}
