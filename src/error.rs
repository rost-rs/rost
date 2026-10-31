//! # Parse Diagnostics
//!
//! Errors reported by the strict parser. Every [`ParseError`] carries the byte span of
//! the offending input together with its 1-based line and column, and can render a
//! compiler-style snippet pointing at the problem.

use std::fmt;
use std::ops::Range;

/// The category of a [`ParseError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseErrorKind {
    /// A `"` string literal is never closed.
    UnterminatedString,
    /// An enum `key | a, b` is missing its closing `|`.
    UnterminatedEnum,
    /// A list `key [ a, b` is missing its closing `]`.
    UnterminatedList,
    /// A `}` appears without a matching block opening.
    UnexpectedCloseBrace,
    /// A block is still open at the end of the document.
    UnclosedBlock,
    /// A key is not followed by `:`, `=`, `|`, `[` or `{`.
    ExpectedOperator,
    /// The same key is defined twice in one scope.
    DuplicateKey(String),
}

impl fmt::Display for ParseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnterminatedString => f.write_str("unterminated string literal"),
            Self::UnterminatedEnum => f.write_str("unterminated enum: missing closing `|`"),
            Self::UnterminatedList => f.write_str("unterminated list: missing closing `]`"),
            Self::UnexpectedCloseBrace => f.write_str("unexpected `}` without a matching block"),
            Self::UnclosedBlock => f.write_str("block is never closed: missing `}`"),
            Self::ExpectedOperator => {
                f.write_str("expected `:`, `=`, `|`, `[` or `{` after key")
            }
            Self::DuplicateKey(key) => write!(f, "duplicate key `{key}` in the same scope"),
        }
    }
}

/// A diagnostic produced by the strict parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// What went wrong.
    pub kind: ParseErrorKind,
    /// Byte range of the offending input.
    pub span: Range<usize>,
    /// 1-based line of `span.start`.
    pub line: u32,
    /// 1-based column (in characters) of `span.start`.
    pub column: u32,
}

impl ParseError {
    /// Builds an error for `span` inside `src`, computing its line and column.
    pub fn new(kind: ParseErrorKind, src: &[u8], span: Range<usize>) -> Self {
        let start = span.start.min(src.len());
        let before = &src[..start];
        let line = memchr::Memchr::new(b'\n', before).count() as u32 + 1;
        let line_start = memchr::memrchr(b'\n', before).map_or(0, |p| p + 1);
        let column = String::from_utf8_lossy(&src[line_start..start]).chars().count() as u32 + 1;
        Self {
            kind,
            span,
            line,
            column,
        }
    }

    /// Renders a compiler-style snippet of `src` with a caret under the error.
    ///
    /// # Examples
    ///
    /// ```
    /// use rost::{ParseError, ParseErrorKind};
    ///
    /// let src = "name: \"Valkyrie\n";
    /// let err = ParseError::new(ParseErrorKind::UnterminatedString, src.as_bytes(), 6..7);
    /// let report = err.render(src);
    /// assert!(report.contains("1:7: unterminated string literal"));
    /// assert!(report.contains("^"));
    /// ```
    pub fn render(&self, src: &str) -> String {
        let bytes = src.as_bytes();
        let start = self.span.start.min(bytes.len());
        let line_start = memchr::memrchr(b'\n', &bytes[..start]).map_or(0, |p| p + 1);
        let line_end = memchr::memchr(b'\n', &bytes[start..]).map_or(bytes.len(), |p| start + p);
        let text = &src[line_start..line_end];
        let gutter = self.line.to_string();
        let pad = " ".repeat(gutter.len());
        let caret_offset = " ".repeat(self.column as usize - 1);
        let caret_len = src
            .get(start..self.span.end.min(line_end))
            .map_or(1, |s| s.chars().count().max(1));
        format!(
            "error: {self}\n{pad} |\n{gutter} | {text}\n{pad} | {caret_offset}{}\n",
            "^".repeat(caret_len)
        )
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.kind)
    }
}

impl std::error::Error for ParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_line_and_column() {
        let src = "a: 1\nbé: \"x\n";
        let err = ParseError::new(ParseErrorKind::UnterminatedString, src.as_bytes(), 10..11);
        assert_eq!((err.line, err.column), (2, 5));
        assert_eq!(err.to_string(), "2:5: unterminated string literal");
    }

    #[test]
    fn renders_snippet_with_caret() {
        let src = "a: 1\nwidth 120px\n";
        let err = ParseError::new(ParseErrorKind::ExpectedOperator, src.as_bytes(), 5..10);
        assert_eq!(
            err.render(src),
            "error: 2:1: expected `:`, `=`, `|`, `[` or `{` after key\n  |\n2 | width 120px\n  | ^^^^^\n"
        );
    }
}
