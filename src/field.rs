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
        let hex = if let Some(stripped) = s.strip_prefix('#') {
            stripped
        } else {
            s.strip_prefix("0x")?
        };

        if hex.len() == 3 {
            // #RGB -> #RRGGBB
            let r = u32::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u32::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u32::from_str_radix(&hex[2..3], 16).ok()?;
            return Some(((r * 17) << 16) | ((g * 17) << 8) | (b * 17));
        }

        u32::from_str_radix(hex, 16).ok()
    }

    /// Splits and yields trimmed items if this field is a [`ParsedField::List`].
    pub fn list_items(&self) -> Vec<&'a str> {
        match self {
            ParsedField::List { raw_items, .. } => raw_items
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.trim_matches('"'))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Splits and yields trimmed variants if this field is a [`ParsedField::Enum`].
    pub fn enum_variants(&self) -> Vec<&'a str> {
        match self {
            ParsedField::Enum { raw_variants, .. } => raw_variants
                .split([',', '|', '\n'])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect(),
            _ => Vec::new(),
        }
    }
}
