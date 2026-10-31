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
    } else if slice.len() > 2 && slice[0] == b'0' && (slice[1] == b'x' || slice[1] == b'X') {
        return parse_hex_digits(&slice[2..]);
    } else {
        let mut val = 0u32;
        for &b in slice {
            if b.is_ascii_digit() {
                val = val.checked_mul(10)?.checked_add((b - b'0') as u32)?;
            } else {
                return std::str::from_utf8(slice).ok()?.parse::<u32>().ok();
            }
        }
        return Some(val);
    }
    None
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
    let s = numeric_slice(raw);
    if let Some(i) = parse_small_int(s, 15) {
        return Some(i as f64);
    }
    s.parse::<f64>().ok()
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
        value: &'a str,
        line: u32,
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
