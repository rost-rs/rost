//! # Field Definitions and Zero-Copy Types for the `.rost` Format
//!
//! This module defines the core data representations emitted by the `.rost` parser.
//! All string slices (`&'a str`) are zero-copy references pointing directly into
//! the underlying raw buffer.

/// Represents a parsed unit of measurement or CSS-like dimension.
///
/// Commonly used in UI styling and game engine layout systems (e.g., `120px`, `1.5rem`, `80%`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dimension<'a> {
    /// The numeric magnitude of the dimension.
    pub value: f64,
    /// The unit suffix (e.g., `"px"`, `"rem"`, `"em"`, `"%"`, `"vh"`, `"vw"`).
    pub unit: &'a str,
}

impl<'a> Dimension<'a> {
    /// Attempts to parse a raw string slice into a [`Dimension`].
    ///
    /// # Examples
    ///
    /// ```
    /// use rost::field::Dimension;
    ///
    /// let dim = Dimension::parse("120px").unwrap();
    /// assert_eq!(dim.value, 120.0);
    /// assert_eq!(dim.unit, "px");
    ///
    /// let pct = Dimension::parse("75.5%").unwrap();
    /// assert_eq!(pct.value, 75.5);
    /// assert_eq!(pct.unit, "%");
    /// ```
    pub fn parse(raw: &'a str) -> Option<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }

        // Find where the numeric part ends and the unit starts
        let split_idx = trimmed
            .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
            .unwrap_or(trimmed.len());

        let (num_part, unit_part) = trimmed.split_at(split_idx);
        let value = num_part.parse::<f64>().ok()?;

        Some(Self {
            value,
            unit: unit_part.trim(),
        })
    }
}

/// Fast 256-entry lookup table for hexadecimal character decoding in O(1).
pub const HEX_LUT: [u8; 256] = {
    let mut lut = [0xFFu8; 256];
    let mut i = b'0';
    while i <= b'9' {
        lut[i as usize] = i - b'0';
        i += 1;
    }
    let mut i = b'a';
    while i <= b'f' {
        lut[i as usize] = i - b'a' + 10;
        i += 1;
    }
    let mut i = b'A';
    while i <= b'F' {
        lut[i as usize] = i - b'A' + 10;
        i += 1;
    }
    lut
};

/// Decodes hex digits (without the `#` / `0x` prefix) into a `u32`.
///
/// Color shorthands `RGB` / `RGBA` expand each digit (`f` → `ff`), and `RRGGBBAA` keeps
/// the alpha channel in the low byte (`0xRRGGBBAA`). Other lengths are read as a plain
/// hexadecimal number.
///
/// # Examples
///
/// ```
/// use rost::field::parse_hex_digits;
///
/// assert_eq!(parse_hex_digits(b"1e1e2e"), Some(0x1e1e2e));
/// assert_eq!(parse_hex_digits(b"11223344"), Some(0x11223344));
/// assert_eq!(parse_hex_digits(b"fff"), Some(0xffffff));
/// assert_eq!(parse_hex_digits(b"f00a"), Some(0xff0000aa));
/// ```
pub fn parse_hex_digits(hex: &[u8]) -> Option<u32> {
    let len = hex.len();
    if !matches!(len, 3 | 4 | 6 | 8) {
        return u32::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok();
    }
    let mut acc = 0u32;
    for &b in hex {
        let nibble = HEX_LUT[b as usize];
        if nibble == 0xFF {
            return None;
        }
        acc = if len <= 4 {
            (acc << 8) | (nibble as u32 * 17)
        } else {
            (acc << 4) | nibble as u32
        };
    }
    Some(acc)
}

/// High-throughput scalar parser for hexadecimal colors or unsigned integers without heap allocations.
#[inline(always)]
pub fn parse_fast_u32(raw: &str) -> Option<u32> {
    let bytes = raw.as_bytes();
    let mut start = 0;
    while start < bytes.len() && (bytes[start] == b' ' || bytes[start] == b'\t') {
        start += 1;
    }
    let mut end = bytes.len();
    while end > start && (bytes[end - 1] == b' ' || bytes[end - 1] == b'\t' || bytes[end - 1] == b';') {
        end -= 1;
    }
    let slice = &bytes[start..end];
    if slice.is_empty() {
        return None;
    }

    if slice[0] == b'#' {
        return parse_hex_digits(&slice[1..]);
    }
    if slice.len() > 2 && slice[0] == b'0' && (slice[1] == b'x' || slice[1] == b'X') {
        return parse_hex_digits(&slice[2..]);
    }
    let mut val = 0u32;
    for &b in slice {
        if b.is_ascii_digit() {
            val = val.checked_mul(10)?.checked_add((b - b'0') as u32)?;
        } else {
            return std::str::from_utf8(slice).ok()?.parse::<u32>().ok();
        }
    }
    Some(val)
}

/// Strips surrounding whitespace, trailing `;`, and unit suffixes (`px`, `%`, …) from a
/// numeric literal.
#[inline(always)]
fn numeric_slice(raw: &str) -> &str {
    raw.trim_matches(|c: char| c == ' ' || c == '\t')
        .trim_end_matches(|c: char| c == ' ' || c == '\t' || c == ';' || c == '%' || c.is_ascii_alphabetic())
}

/// Parses a short, unsigned-or-signed integer literal exactly, without float rounding.
///
/// Returns `None` for anything that is not a plain integer of at most `max_digits` digits.
#[inline(always)]
fn parse_small_int(s: &str, max_digits: usize) -> Option<i64> {
    let (negative, digits) = match s.as_bytes().first()? {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };
    if digits.is_empty() || digits.len() > max_digits || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let val = digits.bytes().fold(0i64, |acc, b| acc * 10 + (b - b'0') as i64);
    Some(if negative { -val } else { val })
}

/// Exact powers of ten representable in an `f64`.
const POW10: [f64; 16] = [1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15];

/// Parses a plain decimal `[-]digits[.digits]` with at most 15 significant digits.
///
/// Clinger's fast path: the digits and the power of ten are both exact in an `f64`, and a
/// single IEEE division is correctly rounded, so the result equals the standard parser's.
/// Anything else (exponents, units, surrounding whitespace, long mantissas) returns `None`.
#[inline(always)]
fn parse_plain_decimal(bytes: &[u8]) -> Option<f64> {
    let (negative, digits) = match bytes.first()? {
        b'-' => (true, &bytes[1..]),
        _ => (false, bytes),
    };
    let mut mantissa: u64 = 0;
    let mut count = 0usize;
    let mut fraction: Option<usize> = None;
    for &b in digits {
        match b {
            b'0'..=b'9' => {
                mantissa = mantissa * 10 + u64::from(b - b'0');
                count += 1;
                if count > 15 {
                    return None;
                }
            }
            b'.' if fraction.is_none() => fraction = Some(count),
            _ => return None,
        }
    }
    if count == 0 {
        return None;
    }
    let value = mantissa as f64 / POW10[count - fraction.unwrap_or(count)];
    Some(if negative { -value } else { value })
}

/// Parses a plain decimal integer `[-]digits` of at most 18 digits (always fits `i64`).
#[cfg_attr(not(feature = "serde"), allow(dead_code))] // used by the serde deserializers
#[inline(always)]
pub(crate) fn parse_plain_int(bytes: &[u8]) -> Option<i64> {
    let (negative, digits) = match bytes.first()? {
        b'-' => (true, &bytes[1..]),
        _ => (false, bytes),
    };
    if digits.is_empty() || digits.len() > 18 {
        return None;
    }
    let mut value: i64 = 0;
    for &b in digits {
        let d = b.wrapping_sub(b'0');
        if d > 9 {
            return None;
        }
        value = value * 10 + i64::from(d);
    }
    Some(if negative { -value } else { value })
}

/// Fast float parser extracting the numeric prefix without allocating strings.
///
/// Integers that fit the `f32` mantissa take an exact fast path; everything else is
/// delegated to the correctly rounded standard library parser.
#[inline(always)]
pub fn parse_fast_f32(raw: &str) -> Option<f32> {
    let s = numeric_slice(raw);
    if let Some(i) = parse_small_int(s, 7) {
        return Some(i as f32);
    }
    s.parse::<f32>().ok()
}

/// Fast `f64` parser extracting the numeric prefix without allocating strings.
///
/// Integers that fit the `f64` mantissa take an exact fast path; everything else is
/// delegated to the correctly rounded standard library parser.
///
/// # Examples
///
/// ```
/// use rost::field::parse_fast_f64;
///
/// assert_eq!(parse_fast_f64("3.141592653589793"), Some(std::f64::consts::PI));
/// assert_eq!(parse_fast_f64("120px"), Some(120.0));
/// assert_eq!(parse_fast_f64("-0.5;"), Some(-0.5));
/// ```
#[inline(always)]
pub fn parse_fast_f64(raw: &str) -> Option<f64> {
    if let Some(v) = parse_plain_decimal(raw.as_bytes()) {
        return Some(v);
    }
    let s = numeric_slice(raw);
    if let Some(i) = parse_small_int(s, 15) {
        return Some(i as f64);
    }
    s.parse::<f64>().ok()
}

/// Decodes backslash escapes in a quoted string value.
///
/// Supported: `\n`, `\t`, `\r`, `\0`, `\b`, `\f`, `\"`, `\\`, `\/`, `\u{1F600}` (1–6 hex digits) and
/// JSON-style `\uXXXX` (including surrogate pairs). Unknown or malformed escapes are kept
/// verbatim. Borrows when there is nothing to decode.
///
/// # Examples
///
/// ```
/// use rost::field::unescape;
///
/// assert_eq!(unescape(r#"say \"hi\"\n"#), "say \"hi\"\n");
/// assert_eq!(unescape(r"\u{48}\u0069 \uD83D\uDE00"), "Hi 😀");
/// assert!(matches!(unescape("plain"), std::borrow::Cow::Borrowed("plain")));
/// ```
pub fn unescape(s: &str) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;

    let Some(first) = memchr::memchr(b'\\', s.as_bytes()) else {
        return Cow::Borrowed(s);
    };
    let mut out = String::with_capacity(s.len());
    out.push_str(&s[..first]);
    let mut rest = &s[first..];
    while let Some(pos) = rest.find('\\') {
        out.push_str(&rest[..pos]);
        let esc = &rest[pos + 1..];
        let (decoded, consumed) = decode_escape(esc);
        match decoded {
            Some(c) => out.push(c),
            None => out.push('\\'),
        }
        rest = &esc[if decoded.is_some() { consumed } else { 0 }..];
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// Decodes one escape (the text right after a backslash): returns the char and the
/// number of bytes consumed, or `None` when the escape is unknown or malformed.
fn decode_escape(esc: &str) -> (Option<char>, usize) {
    let simple = match esc.as_bytes().first() {
        Some(b'n') => Some('\n'),
        Some(b't') => Some('\t'),
        Some(b'r') => Some('\r'),
        Some(b'0') => Some('\0'),
        Some(b'b') => Some('\u{8}'),
        Some(b'f') => Some('\u{c}'),
        Some(b'"') => Some('"'),
        Some(b'\\') => Some('\\'),
        Some(b'/') => Some('/'),
        _ => None,
    };
    if simple.is_some() {
        return (simple, 1);
    }
    if let Some(braced) = esc.strip_prefix("u{") {
        if let Some(close) = braced.find('}')
            && (1..=6).contains(&close)
            && let Ok(code) = u32::from_str_radix(&braced[..close], 16)
            && let Some(c) = char::from_u32(code)
        {
            return (Some(c), close + 3);
        }
        return (None, 0);
    }
    let hex4 = |s: &str| s.get(..4).and_then(|h| u16::from_str_radix(h, 16).ok());
    if let Some(digits) = esc.strip_prefix('u')
        && let Some(hi) = hex4(digits)
    {
        if let Some(c) = char::from_u32(hi as u32) {
            return (Some(c), 5);
        }
        // High surrogate: must be followed by `\uXXXX` low surrogate.
        if let Some(lo) = digits[4..].strip_prefix("\\u").and_then(hex4)
            && let Some(Ok(c)) = char::decode_utf16([hi, lo]).next()
        {
            return (Some(c), 11);
        }
    }
    (None, 0)
}

/// A property value classified into its type.
///
/// Bare tokens are typed by their shape; quoted and raw strings are always [`Scalar::Str`],
/// so `"42"` stays a string while `42` is an integer.
#[derive(Debug, Clone, PartialEq)]
pub enum Scalar<'a> {
    /// `null`
    Null,
    /// `true` / `false`
    Bool(bool),
    /// Integer literal that fits `i64` (`42`, `-7`).
    Int(i64),
    /// Floating point literal (`3.14`, `-0.5`, `1e-3`).
    Float(f64),
    /// Hex color (`#fff`, `#1e1e2e`), packed as returned by [`parse_fast_u32`].
    Color(u32),
    /// Number with a unit (`120px`, `1.5rem`, `75%`).
    Dimension(Dimension<'a>),
    /// Any other bare token (`easy`, `rgba(1, 2, 3)`) or a quoted / raw string.
    Str(std::borrow::Cow<'a, str>),
}

impl<'a> Scalar<'a> {
    /// Classifies a value slice according to how it was quoted.
    ///
    /// # Examples
    ///
    /// ```
    /// use rost::field::{Scalar, QuoteStyle};
    ///
    /// assert_eq!(Scalar::classify("42", QuoteStyle::Bare), Scalar::Int(42));
    /// assert_eq!(Scalar::classify("42", QuoteStyle::Quoted), Scalar::Str("42".into()));
    /// assert_eq!(Scalar::classify("#ff0000", QuoteStyle::Bare), Scalar::Color(0xff0000));
    /// assert!(matches!(Scalar::classify("12px", QuoteStyle::Bare), Scalar::Dimension(_)));
    /// ```
    pub fn classify(value: &'a str, quote: QuoteStyle) -> Self {
        match quote {
            QuoteStyle::Quoted => return Scalar::Str(unescape(value)),
            QuoteStyle::Raw => return Scalar::Str(std::borrow::Cow::Borrowed(value)),
            QuoteStyle::Bare => {}
        }
        let s = value.trim().trim_end_matches(';').trim_end();
        match s {
            "null" => return Scalar::Null,
            "true" => return Scalar::Bool(true),
            "false" => return Scalar::Bool(false),
            _ => {}
        }
        let bytes = s.as_bytes();
        if let Some(hex) = s.strip_prefix('#')
            && matches!(hex.len(), 3 | 4 | 6 | 8)
            && hex.bytes().all(|b| b.is_ascii_hexdigit())
            && let Some(c) = parse_fast_u32(s)
        {
            return Scalar::Color(c);
        }
        let starts_numeric = matches!(
            bytes,
            [b'-' | b'+', b'0'..=b'9' | b'.', ..] | [b'0'..=b'9', ..] | [b'.', b'0'..=b'9', ..]
        );
        if starts_numeric {
            if let Ok(i) = s.parse::<i64>() {
                return Scalar::Int(i);
            }
            if let Ok(f) = s.parse::<f64>() {
                return Scalar::Float(f);
            }
            if let Some(dim) = Dimension::parse(s)
                && !dim.unit.is_empty()
                && dim.unit.bytes().all(|b| b.is_ascii_alphabetic() || b == b'%')
            {
                return Scalar::Dimension(dim);
            }
        }
        Scalar::Str(std::borrow::Cow::Borrowed(s))
    }

    /// Returns a short name of the value's type (`"int"`, `"color"`, …), for messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Scalar::Null => "null",
            Scalar::Bool(_) => "bool",
            Scalar::Int(_) => "int",
            Scalar::Float(_) => "float",
            Scalar::Color(_) => "color",
            Scalar::Dimension(_) => "dimension",
            Scalar::Str(_) => "string",
        }
    }
}

/// How a property value was written in the source document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum QuoteStyle {
    /// Unquoted token (`width: 120px`); typed as number, color, dimension, bool, … on demand.
    #[default]
    Bare,
    /// Double-quoted string (`title: "Hello"`); always a string, escapes apply.
    Quoted,
    /// Raw multi-line string (`body: """..."""`); always a string, taken verbatim.
    Raw,
}

impl QuoteStyle {
    /// Classifies the value slice `data[off..off + len]` by inspecting its delimiters.
    #[inline]
    pub(crate) fn detect(data: &[u8], off: usize, len: usize) -> Self {
        let raw = crate::parser::RAW_QUOTE;
        if off >= raw.len() && data[off - raw.len()..off] == *raw && data[off + len..].starts_with(raw) {
            return Self::Raw;
        }
        if off >= 1 && data.get(off - 1) == Some(&b'"') && data.get(off + len) == Some(&b'"') {
            Self::Quoted
        } else {
            Self::Bare
        }
    }
}

/// Represents an individual field or syntactic node extracted from a `.rost` document.
///
/// Designed with zero allocations in mind: each field borrows its textual content
/// directly from the source buffer.
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedField<'a> {
    /// Top-level section or identifier header defined with `#` (e.g., `#button.primary`).
    Title {
        name: &'a str,
        line: u32,
    },
    /// Secondary section or qualified key-value defined with `##` (e.g., `##state "hover"`).
    Subtitle {
        name: &'a str,
        value: &'a str,
        line: u32,
    },
    /// Common property or key-value pair defined with `:` (e.g., `width: 120px` or `color: "#ffffff"`).
    CommonText {
        name: &'a str,
        /// Value without its surrounding quotes, if any.
        value: &'a str,
        line: u32,
        /// How the value was written in the source.
        quote: QuoteStyle,
    },
    /// Enumerated variants or state sets enclosed in pipes `| ... |` (e.g., `state | idle, hover, active |`).
    Enum {
        name: &'a str,
        raw_variants: &'a str,
        start_line: u32,
        end_line: u32,
    },
    /// Array or list of elements enclosed in brackets `[ ... ]` (e.g., `palette [ #1e1e2e, #cdd6f4 ]`).
    List {
        name: &'a str,
        raw_items: &'a str,
        start_line: u32,
        end_line: u32,
    },
    /// Variable definition or functional/reactive binding defined with `=` (e.g., `on_click = emit_event()`).
    VarFunc {
        var_name: &'a str,
        signature: &'a str,
    },
    /// Top-level or scoped metadata directive defined with `@` (e.g., `@theme "dark-nebula"` or `@rule "no-console"`).
    Directive {
        name: &'a str,
        value: &'a str,
        line: u32,
    },
    /// Opening of a hierarchical or scoped block `{`.
    BlockStart {
        name: &'a str,
        line: u32,
    },
    /// Closing of a hierarchical or scoped block `}`.
    BlockEnd {
        line: u32,
    },
}

impl<'a> ParsedField<'a> {
    /// Returns how the value of a [`ParsedField::CommonText`] was written, if this is one.
    #[inline]
    pub fn quote_style(&self) -> Option<QuoteStyle> {
        match self {
            ParsedField::CommonText { quote, .. } => Some(*quote),
            _ => None,
        }
    }

    /// Classifies the value of a [`ParsedField::CommonText`] into a typed [`Scalar`].
    pub fn scalar(&self) -> Option<Scalar<'a>> {
        match self {
            ParsedField::CommonText { value, quote, .. } => Some(Scalar::classify(value, *quote)),
            _ => None,
        }
    }

    /// Returns the value as a string with escapes decoded for quoted values.
    ///
    /// Bare and raw values are returned verbatim (borrowed); quoted values go through
    /// [`unescape`], which only allocates when an escape is present.
    pub fn string_value(&self) -> Option<std::borrow::Cow<'a, str>> {
        match self {
            ParsedField::CommonText { value, quote: QuoteStyle::Quoted, .. } => Some(unescape(value)),
            _ => self.value().map(std::borrow::Cow::Borrowed),
        }
    }

    /// Returns the primary identifier or key name of the field.
    #[inline]
    pub fn name(&self) -> &'a str {
        match self {
            ParsedField::Title { name, .. } => name,
            ParsedField::Subtitle { name, .. } => name,
            ParsedField::CommonText { name, .. } => name,
            ParsedField::Enum { name, .. } => name,
            ParsedField::List { name, .. } => name,
            ParsedField::VarFunc { var_name, .. } => var_name,
            ParsedField::Directive { name, .. } => name,
            ParsedField::BlockStart { name, .. } => name,
            ParsedField::BlockEnd { .. } => "}",
        }
    }

    /// Returns the value string slice if this field contains an scalar or literal value.
    #[inline]
    pub fn value(&self) -> Option<&'a str> {
        match self {
            ParsedField::Subtitle { value, .. } => Some(value),
            ParsedField::CommonText { value, .. } => Some(value),
            ParsedField::Directive { value, .. } => Some(value),
            ParsedField::VarFunc { signature, .. } => Some(signature),
            _ => None,
        }
    }

    /// Returns the starting line number where this field occurs.
    #[inline]
    pub fn line(&self) -> u32 {
        match self {
            ParsedField::Title { line, .. } => *line,
            ParsedField::Subtitle { line, .. } => *line,
            ParsedField::CommonText { line, .. } => *line,
            ParsedField::Enum { start_line, .. } => *start_line,
            ParsedField::List { start_line, .. } => *start_line,
            ParsedField::VarFunc { .. } => 0,
            ParsedField::Directive { line, .. } => *line,
            ParsedField::BlockStart { line, .. } => *line,
            ParsedField::BlockEnd { line } => *line,
        }
    }

    /// Returns the value as a trimmed string slice.
    #[inline]
    pub fn as_str(&self) -> Option<&'a str> {
        self.value().map(str::trim)
    }

    /// Attempts to parse the field's value as a 64-bit signed integer.
    pub fn as_i64(&self) -> Option<i64> {
        self.as_str()?.parse::<i64>().ok()
    }

    /// Attempts to parse the field's value as a 64-bit unsigned integer.
    pub fn as_u64(&self) -> Option<u64> {
        self.as_str()?.parse::<u64>().ok()
    }

    /// Attempts to parse the field's value as a 64-bit floating point number.
    pub fn as_f64(&self) -> Option<f64> {
        self.as_str()?.parse::<f64>().ok()
    }

    /// Attempts to parse the field's value as a boolean (`true`/`false`).
    pub fn as_bool(&self) -> Option<bool> {
        match self.as_str()? {
            "true" | "1" | "yes" | "on" => Some(true),
            "false" | "0" | "no" | "off" => Some(false),
            _ => None,
        }
    }

    /// Attempts to parse the field's value as a CSS or layout dimension (e.g., `120px`, `1.5rem`).
    pub fn as_dimension(&self) -> Option<Dimension<'a>> {
        Dimension::parse(self.as_str()?)
    }

    /// Attempts to parse the field's value as a hexadecimal color code (e.g., `#1e1e2e` or `0x1e1e2e`).
    ///
    /// Returns the color packed into a `u32` (RGB or RGBA).
    pub fn as_hex_color(&self) -> Option<u32> {
        let s = self.as_str()?;
        parse_fast_u32(s)
    }

    /// Splits and yields trimmed items if this field is a [`ParsedField::List`].
    pub fn list_items(&self) -> Vec<&'a str> {
        self.iterate_list().collect()
    }

    /// Returns an iterator over trimmed items if this field is a [`ParsedField::List`].
    pub fn iterate_list(&self) -> impl Iterator<Item = &'a str> {
        let raw = match self {
            ParsedField::List { raw_items, .. } => raw_items,
            _ => "",
        };
        TopLevelSplit::new(raw, |b| b == b',').map(unquote)
    }

    /// Returns the n-th item (0-indexed) if this field is a [`ParsedField::List`].
    pub fn list_item(&self, index: usize) -> Option<&'a str> {
        self.iterate_list().nth(index)
    }

    /// Splits and yields trimmed variants if this field is a [`ParsedField::Enum`].
    pub fn enum_variants(&self) -> Vec<&'a str> {
        self.iterate_enum().collect()
    }

    /// Returns an iterator over trimmed variants if this field is a [`ParsedField::Enum`].
    pub fn iterate_enum(&self) -> impl Iterator<Item = &'a str> {
        let raw = match self {
            ParsedField::Enum { raw_variants, .. } => raw_variants,
            _ => "",
        };
        TopLevelSplit::new(raw, |b| matches!(b, b',' | b'|' | b'\n'))
    }

    /// Returns the n-th variant (0-indexed) if this field is a [`ParsedField::Enum`].
    pub fn enum_variant(&self, index: usize) -> Option<&'a str> {
        self.iterate_enum().nth(index)
    }
}

/// Removes one pair of surrounding double quotes, if present.
#[inline]
pub(crate) fn unquote(s: &str) -> &str {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Zero-allocation iterator splitting list/enum bodies on separator bytes that are not
/// nested inside quotes, `(...)`, `[...]` or `{...}`. Items are trimmed and empty items
/// are skipped.
#[derive(Debug, Clone)]
pub(crate) struct TopLevelSplit<'a> {
    rest: &'a str,
    is_sep: fn(u8) -> bool,
}

impl<'a> TopLevelSplit<'a> {
    pub(crate) fn new(raw: &'a str, is_sep: fn(u8) -> bool) -> Self {
        Self { rest: raw, is_sep }
    }
}

impl<'a> Iterator for TopLevelSplit<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        while !self.rest.is_empty() {
            let bytes = self.rest.as_bytes();
            let mut depth: u32 = 0;
            let mut in_quote = false;
            let mut end = bytes.len();
            let mut i = 0;
            while i < bytes.len() {
                let b = bytes[i];
                if in_quote {
                    match b {
                        b'\\' => i += 1,
                        b'"' => in_quote = false,
                        _ => {}
                    }
                } else {
                    match b {
                        b'"' => in_quote = true,
                        b'(' | b'[' | b'{' => depth += 1,
                        b')' | b']' | b'}' => depth = depth.saturating_sub(1),
                        _ if depth == 0 && (self.is_sep)(b) => {
                            end = i;
                            break;
                        }
                        _ => {}
                    }
                }
                i += 1;
            }
            // Separators are ASCII, so `end` and `end + 1` are char boundaries.
            let item = self.rest[..end].trim();
            self.rest = if end < bytes.len() { &self.rest[end + 1..] } else { "" };
            if !item.is_empty() {
                return Some(item);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_decimal_fast_path_matches_std() {
        let fixed = [
            "0", "-0", "5.", ".5", "0.1", "0.3", "-25000.5", "123456789012345", "1.23456789012345",
            "9007199254740.993", "0.000000000000001", "999999999999999",
        ];
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let generated = (0..20_000).map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let digits = format!("{}", state % 10u64.pow(1 + (state % 15) as u32));
            let dot = (state >> 40) as usize % (digits.len() + 1);
            let sign = if state & 1 == 0 { "" } else { "-" };
            format!("{sign}{}.{}", &digits[..dot], &digits[dot..])
        });
        for s in fixed.iter().map(|s| s.to_string()).chain(generated) {
            let expected = s.parse::<f64>().ok();
            if let Some(fast) = parse_plain_decimal(s.as_bytes()) {
                assert_eq!(Some(fast.to_bits()), expected.map(f64::to_bits), "{s}");
            }
            assert_eq!(parse_fast_f64(&s).map(f64::to_bits), expected.map(f64::to_bits), "{s}");
        }
        assert_eq!(parse_plain_decimal(b"1e3"), None);
        assert_eq!(parse_plain_decimal(b"1234567890123456"), None);
        assert_eq!(parse_plain_decimal(b"-"), None);
    }

    #[test]
    fn plain_int_fast_path() {
        assert_eq!(parse_plain_int(b"0"), Some(0));
        assert_eq!(parse_plain_int(b"-42"), Some(-42));
        assert_eq!(parse_plain_int(b"999999999999999999"), Some(999_999_999_999_999_999));
        assert_eq!(parse_plain_int(b"9999999999999999999"), None);
        assert_eq!(parse_plain_int(b"12px"), None);
        assert_eq!(parse_plain_int(b"-"), None);
        assert_eq!(parse_plain_int(b""), None);
    }
}
