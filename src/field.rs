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
        let hex = &slice[1..];
        if hex.len() == 6 {
            let h0 = HEX_LUT[hex[0] as usize];
            let h1 = HEX_LUT[hex[1] as usize];
            let h2 = HEX_LUT[hex[2] as usize];
            let h3 = HEX_LUT[hex[3] as usize];
            let h4 = HEX_LUT[hex[4] as usize];
            let h5 = HEX_LUT[hex[5] as usize];
            if (h0 | h1 | h2 | h3 | h4 | h5) != 0xFF {
                return Some(((h0 as u32) << 20) | ((h1 as u32) << 16) | ((h2 as u32) << 12) | ((h3 as u32) << 8) | ((h4 as u32) << 4) | (h5 as u32));
            }
        } else if hex.len() == 8 {
            let h0 = HEX_LUT[hex[0] as usize];
            let h1 = HEX_LUT[hex[1] as usize];
            let h2 = HEX_LUT[hex[2] as usize];
            let h3 = HEX_LUT[hex[3] as usize];
            let h4 = HEX_LUT[hex[4] as usize];
            let h5 = HEX_LUT[hex[5] as usize];
            if (h0 | h1 | h2 | h3 | h4 | h5) != 0xFF {
                return Some(((h0 as u32) << 20) | ((h1 as u32) << 16) | ((h2 as u32) << 12) | ((h3 as u32) << 8) | ((h4 as u32) << 4) | (h5 as u32));
            }
        } else if hex.len() == 3 {
            let h0 = HEX_LUT[hex[0] as usize];
            let h1 = HEX_LUT[hex[1] as usize];
            let h2 = HEX_LUT[hex[2] as usize];
            if (h0 | h1 | h2) != 0xFF {
                return Some(((h0 as u32 * 17) << 16) | ((h1 as u32 * 17) << 8) | (h2 as u32 * 17));
            }
        }
        if let Ok(s) = std::str::from_utf8(hex) {
            return u32::from_str_radix(s, 16).ok();
        }
    } else if slice.len() > 2 && (slice[0] == b'0' && (slice[1] == b'x' || slice[1] == b'X')) {
        let hex = &slice[2..];
        if hex.len() == 6 {
            let h0 = HEX_LUT[hex[0] as usize];
            let h1 = HEX_LUT[hex[1] as usize];
            let h2 = HEX_LUT[hex[2] as usize];
            let h3 = HEX_LUT[hex[3] as usize];
            let h4 = HEX_LUT[hex[4] as usize];
            let h5 = HEX_LUT[hex[5] as usize];
            if (h0 | h1 | h2 | h3 | h4 | h5) != 0xFF {
                return Some(((h0 as u32) << 20) | ((h1 as u32) << 16) | ((h2 as u32) << 12) | ((h3 as u32) << 8) | ((h4 as u32) << 4) | (h5 as u32));
            }
        } else if hex.len() == 8 {
            let h0 = HEX_LUT[hex[0] as usize];
            let h1 = HEX_LUT[hex[1] as usize];
            let h2 = HEX_LUT[hex[2] as usize];
            let h3 = HEX_LUT[hex[3] as usize];
            let h4 = HEX_LUT[hex[4] as usize];
            let h5 = HEX_LUT[hex[5] as usize];
            if (h0 | h1 | h2 | h3 | h4 | h5) != 0xFF {
                return Some(((h0 as u32) << 20) | ((h1 as u32) << 16) | ((h2 as u32) << 12) | ((h3 as u32) << 8) | ((h4 as u32) << 4) | (h5 as u32));
            }
        } else if hex.len() == 3 {
            let h0 = HEX_LUT[hex[0] as usize];
            let h1 = HEX_LUT[hex[1] as usize];
            let h2 = HEX_LUT[hex[2] as usize];
            if (h0 | h1 | h2) != 0xFF {
                return Some(((h0 as u32 * 17) << 16) | ((h1 as u32 * 17) << 8) | (h2 as u32 * 17));
            }
        }
        if let Ok(s) = std::str::from_utf8(hex) {
            return u32::from_str_radix(s, 16).ok();
        }
    } else {
        let mut val = 0u32;
        for &b in slice {
            if b >= b'0' && b <= b'9' {
                val = val.wrapping_mul(10).wrapping_add((b - b'0') as u32);
            } else {
                return std::str::from_utf8(slice).ok()?.parse::<u32>().ok();
            }
        }
        return Some(val);
    }
    None
}

/// Fast float parser extracting the numeric prefix without allocating strings.
#[inline(always)]
pub fn parse_fast_f32(raw: &str) -> Option<f32> {
    let bytes = raw.as_bytes();
    let mut start = 0;
    while start < bytes.len() && (bytes[start] == b' ' || bytes[start] == b'\t') {
        start += 1;
    }
    let mut end = bytes.len();
    while end > start && (bytes[end - 1] == b' ' || bytes[end - 1] == b'\t' || bytes[end - 1] == b';' || bytes[end - 1].is_ascii_alphabetic() || bytes[end - 1] == b'%') {
        end -= 1;
    }
    let slice = &bytes[start..end];
    if slice.is_empty() {
        return None;
    }

    let mut i = 0;
    let mut negative = false;
    if slice[0] == b'-' {
        negative = true;
        i += 1;
    } else if slice[0] == b'+' {
        i += 1;
    }

    let mut int_part: u64 = 0;
    let mut has_digits = false;
    while i < slice.len() && slice[i] >= b'0' && slice[i] <= b'9' {
        int_part = int_part * 10 + (slice[i] - b'0') as u64;
        has_digits = true;
        i += 1;
    }

    let mut val = int_part as f64;
    if i < slice.len() && slice[i] == b'.' {
        i += 1;
        let mut div = 1.0f64;
        while i < slice.len() && slice[i] >= b'0' && slice[i] <= b'9' {
            div *= 10.0;
            val += ((slice[i] - b'0') as f64) / div;
            has_digits = true;
            i += 1;
        }
    }

    if !has_digits {
        return None;
    }

    if i < slice.len() {
        let s = std::str::from_utf8(slice).ok()?;
        return s.parse::<f32>().ok();
    }

    let res = if negative { -val as f32 } else { val as f32 };
    Some(res)
}

/// Decodes backslash escapes in a quoted string value.
///
/// Supported: `\n`, `\t`, `\r`, `\0`, `\"`, `\\`, `\/`, `\u{1F600}` (1–6 hex digits) and
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
        raw.split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.trim_matches('"'))
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
        raw.split([',', '|', '\n'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// Returns the n-th variant (0-indexed) if this field is a [`ParsedField::Enum`].
    pub fn enum_variant(&self, index: usize) -> Option<&'a str> {
        self.iterate_enum().nth(index)
    }
}
