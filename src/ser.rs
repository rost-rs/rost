//! # Serde Serializer
//!
//! [`to_string`], [`to_string_pretty`], [`to_vec`] and [`to_writer`] turn any
//! [`serde::Serialize`] value into `.rost` text that [`crate::from_str`] reads back.
//!
//! | Rust | `.rost` |
//! |------|---------|
//! | struct, map | block `key { … }` (the root has no braces) |
//! | `Vec`, slice, tuple | list `key [ … ]` |
//! | string, `char` | quoted string, escaped |
//! | integer, float, `bool` | bare token |
//! | `None`, `()` | `null` |
//! | unit variant | bare name: `mode: Dark` |
//! | variant with data | block with one entry: `shape { Circle: 1.5 }` |
//!
//! `to_string` writes a document on one line; `to_string_pretty` writes one entry per line
//! with four-space indentation. Non-finite floats are an error rather than a silent `null`,
//! and keys must be valid `.rost` keys (no whitespace or `: = | [ ] { } , ; "`).

use std::fmt::Write as _;
use std::io;

use serde::ser::{self, Serialize};

use crate::de::DeError;
use crate::parser::IS_DELIM_OR_WS;

impl ser::Error for DeError {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        DeError(msg.to_string())
    }
}

/// Serializes `value` as a single-line `.rost` document.
///
/// # Examples
///
/// ```
/// #[derive(serde::Serialize)]
/// struct Window {
///     title: &'static str,
///     size: (u32, u32),
/// }
///
/// let text = rost::to_string(&Window { title: "Main", size: (1280, 720) }).unwrap();
/// assert_eq!(text, "title: \"Main\", size [1280, 720]");
/// ```
pub fn to_string<T: ?Sized + Serialize>(value: &T) -> Result<String, DeError> {
    write_document(value, false)
}

/// Serializes `value` as a `.rost` document with one entry per line.
///
/// # Examples
///
/// ```
/// #[derive(serde::Serialize)]
/// struct Config {
///     name: String,
///     window: Window,
/// }
///
/// #[derive(serde::Serialize)]
/// struct Window {
///     width: u32,
///     scale: f64,
/// }
///
/// let config = Config { name: "demo".into(), window: Window { width: 1280, scale: 1.25 } };
/// assert_eq!(
///     rost::to_string_pretty(&config).unwrap(),
///     "name: \"demo\"\nwindow {\n    width: 1280\n    scale: 1.25\n}\n"
/// );
/// ```
pub fn to_string_pretty<T: ?Sized + Serialize>(value: &T) -> Result<String, DeError> {
    write_document(value, true)
}

/// Serializes `value` as single-line `.rost` bytes.
pub fn to_vec<T: ?Sized + Serialize>(value: &T) -> Result<Vec<u8>, DeError> {
    to_string(value).map(String::into_bytes)
}

/// Serializes `value` as `.rost` bytes with one entry per line.
pub fn to_vec_pretty<T: ?Sized + Serialize>(value: &T) -> Result<Vec<u8>, DeError> {
    to_string_pretty(value).map(String::into_bytes)
}

/// Serializes `value` as a single-line `.rost` document into `writer`.
///
/// The document is built in memory and written with one `write_all`.
pub fn to_writer<W: io::Write, T: ?Sized + Serialize>(mut writer: W, value: &T) -> Result<(), DeError> {
    writer.write_all(to_string(value)?.as_bytes()).map_err(io_error)
}

/// Serializes `value` as a `.rost` document with one entry per line into `writer`.
pub fn to_writer_pretty<W: io::Write, T: ?Sized + Serialize>(mut writer: W, value: &T) -> Result<(), DeError> {
    writer.write_all(to_string_pretty(value)?.as_bytes()).map_err(io_error)
}

fn io_error(e: io::Error) -> DeError {
    DeError(format!("I/O error: {e}"))
}

fn write_document<T: ?Sized + Serialize>(value: &T, pretty: bool) -> Result<String, DeError> {
    let mut w = Writer { out: String::with_capacity(128), pretty, depth: 0 };
    value.serialize(ValueSerializer { w: &mut w, ctx: Ctx::Root })?;
    Ok(w.out)
}

/// Output buffer and layout state.
struct Writer {
    out: String,
    pretty: bool,
    /// Number of open blocks and lists (the root is not counted).
    depth: usize,
}

/// Separator state of an open block or list.
#[derive(Default)]
struct Layout {
    first: bool,
    /// The list has an item that spans lines, so every item goes on its own line.
    multiline: bool,
}

/// Where a value is being written.
enum Ctx<'s> {
    /// The document itself: only blocks (and variants with data) are allowed.
    Root,
    /// After a key that is already written: scalars add `: value`, containers ` {` / ` [`.
    Entry,
    /// Inside a list.
    Item(&'s mut Layout),
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Block,
    List,
}

impl Writer {
    fn newline(&mut self) {
        self.out.push('\n');
        for _ in 0..self.depth {
            self.out.push_str("    ");
        }
    }

    /// Writes what goes before a list item; `spans_lines` items force a multi-line list.
    fn item_prefix(&mut self, layout: &mut Layout, spans_lines: bool) {
        if self.pretty && (spans_lines || layout.multiline) {
            if !layout.first {
                self.out.push(',');
            }
            layout.multiline = true;
            self.newline();
        } else if !layout.first {
            self.out.push_str(", ");
        }
        layout.first = false;
    }

    /// Writes what goes before a scalar.
    fn scalar_prefix(&mut self, ctx: Ctx<'_>) -> Result<(), DeError> {
        match ctx {
            Ctx::Root => Err(DeError(
                "the document root must be a struct, a map or an enum variant with data".into(),
            )),
            Ctx::Entry => {
                self.out.push_str(": ");
                Ok(())
            }
            Ctx::Item(layout) => {
                self.item_prefix(layout, false);
                Ok(())
            }
        }
    }

    /// Opens a block or a list. Returns whether it is the (brace-less) root block.
    fn open(&mut self, ctx: Ctx<'_>, kind: Kind) -> Result<bool, DeError> {
        let brace = if kind == Kind::Block { '{' } else { '[' };
        match ctx {
            Ctx::Root if kind == Kind::List => Err(DeError(
                "the document root must be a struct, a map or an enum variant with data, not a list".into(),
            )),
            Ctx::Root => Ok(true),
            Ctx::Entry => {
                self.out.push(' ');
                self.out.push(brace);
                self.depth += 1;
                Ok(false)
            }
            Ctx::Item(layout) => {
                self.item_prefix(layout, self.pretty);
                self.out.push(brace);
                self.depth += 1;
                Ok(false)
            }
        }
    }

    /// Writes what goes before an entry of a block.
    fn entry_prefix(&mut self, root: bool, layout: &mut Layout) {
        if self.pretty {
            if !(root && layout.first) {
                self.newline();
            }
        } else if !layout.first {
            self.out.push_str(", ");
        } else if !root {
            self.out.push(' ');
        }
        layout.first = false;
    }

    fn close_block(&mut self, root: bool, layout: &Layout) {
        if root {
            if self.pretty && !layout.first {
                self.out.push('\n');
            }
            return;
        }
        self.depth -= 1;
        if !layout.first {
            if self.pretty {
                self.newline();
            } else {
                self.out.push(' ');
            }
        }
        self.out.push('}');
    }

    fn close_list(&mut self, layout: &Layout) {
        self.depth -= 1;
        if layout.multiline {
            self.newline();
        }
        self.out.push(']');
    }

    /// Appends a key, rejecting text the parser would not read back as the same key.
    fn key(&mut self, key: &str) -> Result<(), DeError> {
        check_key(key)?;
        self.out.push_str(key);
        Ok(())
    }

    fn string(&mut self, s: &str) {
        self.out.reserve(s.len() + 2);
        self.out.push('"');
        let mut start = 0;
        for (i, b) in s.bytes().enumerate() {
            let escape = match b {
                b'"' => "\\\"",
                b'\\' => "\\\\",
                b'\n' => "\\n",
                b'\r' => "\\r",
                b'\t' => "\\t",
                0..=0x1f | 0x7f => "",
                _ => continue,
            };
            self.out.push_str(&s[start..i]);
            if escape.is_empty() {
                let _ = write!(self.out, "\\u{{{b:x}}}");
            } else {
                self.out.push_str(escape);
            }
            start = i + 1;
        }
        self.out.push_str(&s[start..]);
        self.out.push('"');
    }
}

fn check_key(key: &str) -> Result<(), DeError> {
    let bytes = key.as_bytes();
    let valid = !bytes.is_empty()
        && !matches!(bytes[0], b'#' | b'@' | b'/')
        && bytes.iter().all(|&b| !IS_DELIM_OR_WS[b as usize] && b != b']' && b != b'"');
    if valid {
        Ok(())
    } else {
        Err(DeError(format!("`{key}` cannot be written as a .rost key")))
    }
}

/// Unit variant names are written bare when they read back as the same text.
fn is_bare_word(s: &str) -> bool {
    let mut bytes = s.bytes();
    matches!(bytes.next(), Some(b) if b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        && !matches!(s, "true" | "false" | "null")
}

/// Serializes one value in a given position.
struct ValueSerializer<'a, 's> {
    w: &'a mut Writer,
    ctx: Ctx<'s>,
}

impl<'a> ValueSerializer<'a, '_> {
    fn bare(self, args: std::fmt::Arguments<'_>) -> Result<(), DeError> {
        self.w.scalar_prefix(self.ctx)?;
        let _ = self.w.out.write_fmt(args);
        Ok(())
    }

    fn float(self, v: f64, text: std::fmt::Arguments<'_>) -> Result<(), DeError> {
        if !v.is_finite() {
            return Err(DeError(format!("cannot serialize the non-finite float {v}")));
        }
        self.bare(text)
    }

    /// Opens the single-entry block of a variant with data and writes its name.
    fn open_variant(self, variant: &'static str) -> Result<(&'a mut Writer, bool), DeError> {
        let w = self.w;
        let root = w.open(self.ctx, Kind::Block)?;
        w.entry_prefix(root, &mut Layout { first: true, multiline: false });
        w.key(variant)?;
        Ok((w, root))
    }
}

impl<'a, 's> ser::Serializer for ValueSerializer<'a, 's> {
    type Ok = ();
    type Error = DeError;
    type SerializeSeq = Compound<'a>;
    type SerializeTuple = Compound<'a>;
    type SerializeTupleStruct = Compound<'a>;
    type SerializeTupleVariant = Compound<'a>;
    type SerializeMap = Compound<'a>;
    type SerializeStruct = Compound<'a>;
    type SerializeStructVariant = Compound<'a>;

    fn serialize_bool(self, v: bool) -> Result<(), DeError> {
        self.bare(format_args!("{v}"))
    }

    fn serialize_i8(self, v: i8) -> Result<(), DeError> {
        self.bare(format_args!("{v}"))
    }

    fn serialize_i16(self, v: i16) -> Result<(), DeError> {
        self.bare(format_args!("{v}"))
    }

    fn serialize_i32(self, v: i32) -> Result<(), DeError> {
        self.bare(format_args!("{v}"))
    }

    fn serialize_i64(self, v: i64) -> Result<(), DeError> {
        self.bare(format_args!("{v}"))
    }

    fn serialize_i128(self, v: i128) -> Result<(), DeError> {
        self.bare(format_args!("{v}"))
    }

    fn serialize_u8(self, v: u8) -> Result<(), DeError> {
        self.bare(format_args!("{v}"))
    }

    fn serialize_u16(self, v: u16) -> Result<(), DeError> {
        self.bare(format_args!("{v}"))
    }

    fn serialize_u32(self, v: u32) -> Result<(), DeError> {
        self.bare(format_args!("{v}"))
    }

    fn serialize_u64(self, v: u64) -> Result<(), DeError> {
        self.bare(format_args!("{v}"))
    }

    fn serialize_u128(self, v: u128) -> Result<(), DeError> {
        self.bare(format_args!("{v}"))
    }

    // `Debug` prints the shortest text that reads back to the same float, and always
    // keeps a `.` or an exponent, so `1.0` stays a float.
    fn serialize_f32(self, v: f32) -> Result<(), DeError> {
        self.float(f64::from(v), format_args!("{v:?}"))
    }

    fn serialize_f64(self, v: f64) -> Result<(), DeError> {
        self.float(v, format_args!("{v:?}"))
    }

    fn serialize_char(self, v: char) -> Result<(), DeError> {
        self.serialize_str(v.encode_utf8(&mut [0; 4]))
    }

    fn serialize_str(self, v: &str) -> Result<(), DeError> {
        self.w.scalar_prefix(self.ctx)?;
        self.w.string(v);
        Ok(())
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<(), DeError> {
        use ser::SerializeSeq;
        let mut seq = self.serialize_seq(Some(v.len()))?;
        for b in v {
            seq.serialize_element(b)?;
        }
        seq.end()
    }

    fn serialize_none(self) -> Result<(), DeError> {
        self.serialize_unit()
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<(), DeError> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<(), DeError> {
        self.bare(format_args!("null"))
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<(), DeError> {
        self.serialize_unit()
    }

    fn serialize_unit_variant(self, _name: &'static str, _index: u32, variant: &'static str) -> Result<(), DeError> {
        if is_bare_word(variant) {
            self.bare(format_args!("{variant}"))
        } else {
            self.serialize_str(variant)
        }
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(self, _name: &'static str, value: &T) -> Result<(), DeError> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<(), DeError> {
        let (w, root) = self.open_variant(variant)?;
        value.serialize(ValueSerializer { w: &mut *w, ctx: Ctx::Entry })?;
        w.close_block(root, &Layout { first: false, multiline: false });
        Ok(())
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Compound<'a>, DeError> {
        let w = self.w;
        w.open(self.ctx, Kind::List)?;
        Ok(Compound::new(w, Kind::List, false))
    }

    fn serialize_tuple(self, len: usize) -> Result<Compound<'a>, DeError> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(self, _name: &'static str, len: usize) -> Result<Compound<'a>, DeError> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Compound<'a>, DeError> {
        let (w, root) = self.open_variant(variant)?;
        w.open(Ctx::Entry, Kind::List)?;
        let mut compound = Compound::new(w, Kind::List, false);
        compound.variant = Some(root);
        Ok(compound)
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Compound<'a>, DeError> {
        let w = self.w;
        let root = w.open(self.ctx, Kind::Block)?;
        Ok(Compound::new(w, Kind::Block, root))
    }

    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<Compound<'a>, DeError> {
        self.serialize_map(Some(len))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Compound<'a>, DeError> {
        let (w, root) = self.open_variant(variant)?;
        w.open(Ctx::Entry, Kind::Block)?;
        let mut compound = Compound::new(w, Kind::Block, false);
        compound.variant = Some(root);
        Ok(compound)
    }
}

/// An open block or list.
struct Compound<'a> {
    w: &'a mut Writer,
    kind: Kind,
    root: bool,
    layout: Layout,
    /// Set for tuple and struct variants: the enclosing `{ Variant … }` block to close,
    /// and whether that block is the root.
    variant: Option<bool>,
}

impl<'a> Compound<'a> {
    fn new(w: &'a mut Writer, kind: Kind, root: bool) -> Self {
        Compound { w, kind, root, layout: Layout { first: true, multiline: false }, variant: None }
    }

    fn element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), DeError> {
        value.serialize(ValueSerializer { w: &mut *self.w, ctx: Ctx::Item(&mut self.layout) })
    }

    fn field<T: ?Sized + Serialize>(&mut self, key: &str, value: &T) -> Result<(), DeError> {
        self.w.entry_prefix(self.root, &mut self.layout);
        self.w.key(key)?;
        value.serialize(ValueSerializer { w: &mut *self.w, ctx: Ctx::Entry })
    }

    fn finish(self) -> Result<(), DeError> {
        match self.kind {
            Kind::Block => self.w.close_block(self.root, &self.layout),
            Kind::List => self.w.close_list(&self.layout),
        }
        if let Some(root) = self.variant {
            self.w.close_block(root, &Layout { first: false, multiline: false });
        }
        Ok(())
    }
}

impl ser::SerializeSeq for Compound<'_> {
    type Ok = ();
    type Error = DeError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), DeError> {
        self.element(value)
    }

    fn end(self) -> Result<(), DeError> {
        self.finish()
    }
}

impl ser::SerializeTuple for Compound<'_> {
    type Ok = ();
    type Error = DeError;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), DeError> {
        self.element(value)
    }

    fn end(self) -> Result<(), DeError> {
        self.finish()
    }
}

impl ser::SerializeTupleStruct for Compound<'_> {
    type Ok = ();
    type Error = DeError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), DeError> {
        self.element(value)
    }

    fn end(self) -> Result<(), DeError> {
        self.finish()
    }
}

impl ser::SerializeTupleVariant for Compound<'_> {
    type Ok = ();
    type Error = DeError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), DeError> {
        self.element(value)
    }

    fn end(self) -> Result<(), DeError> {
        self.finish()
    }
}

impl ser::SerializeMap for Compound<'_> {
    type Ok = ();
    type Error = DeError;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), DeError> {
        self.w.entry_prefix(self.root, &mut self.layout);
        let start = self.w.out.len();
        key.serialize(KeySerializer { out: &mut self.w.out })?;
        check_key(&self.w.out[start..])
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), DeError> {
        value.serialize(ValueSerializer { w: &mut *self.w, ctx: Ctx::Entry })
    }

    fn end(self) -> Result<(), DeError> {
        self.finish()
    }
}

impl ser::SerializeStruct for Compound<'_> {
    type Ok = ();
    type Error = DeError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> Result<(), DeError> {
        self.field(key, value)
    }

    fn end(self) -> Result<(), DeError> {
        self.finish()
    }
}

impl ser::SerializeStructVariant for Compound<'_> {
    type Ok = ();
    type Error = DeError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> Result<(), DeError> {
        self.field(key, value)
    }

    fn end(self) -> Result<(), DeError> {
        self.finish()
    }
}

/// Writes a map key: strings, integers, `bool`, `char` and unit variants.
struct KeySerializer<'a> {
    out: &'a mut String,
}

impl KeySerializer<'_> {
    fn text(self, args: std::fmt::Arguments<'_>) -> Result<(), DeError> {
        let _ = self.out.write_fmt(args);
        Ok(())
    }
}

fn key_error() -> DeError {
    DeError("map keys must be strings, integers, booleans or unit variants".into())
}

impl ser::Serializer for KeySerializer<'_> {
    type Ok = ();
    type Error = DeError;
    type SerializeSeq = ser::Impossible<(), DeError>;
    type SerializeTuple = ser::Impossible<(), DeError>;
    type SerializeTupleStruct = ser::Impossible<(), DeError>;
    type SerializeTupleVariant = ser::Impossible<(), DeError>;
    type SerializeMap = ser::Impossible<(), DeError>;
    type SerializeStruct = ser::Impossible<(), DeError>;
    type SerializeStructVariant = ser::Impossible<(), DeError>;

    fn serialize_bool(self, v: bool) -> Result<(), DeError> {
        self.text(format_args!("{v}"))
    }

    fn serialize_i8(self, v: i8) -> Result<(), DeError> {
        self.text(format_args!("{v}"))
    }

    fn serialize_i16(self, v: i16) -> Result<(), DeError> {
        self.text(format_args!("{v}"))
    }

    fn serialize_i32(self, v: i32) -> Result<(), DeError> {
        self.text(format_args!("{v}"))
    }

    fn serialize_i64(self, v: i64) -> Result<(), DeError> {
        self.text(format_args!("{v}"))
    }

    fn serialize_i128(self, v: i128) -> Result<(), DeError> {
        self.text(format_args!("{v}"))
    }

    fn serialize_u8(self, v: u8) -> Result<(), DeError> {
        self.text(format_args!("{v}"))
    }

    fn serialize_u16(self, v: u16) -> Result<(), DeError> {
        self.text(format_args!("{v}"))
    }

    fn serialize_u32(self, v: u32) -> Result<(), DeError> {
        self.text(format_args!("{v}"))
    }

    fn serialize_u64(self, v: u64) -> Result<(), DeError> {
        self.text(format_args!("{v}"))
    }

    fn serialize_u128(self, v: u128) -> Result<(), DeError> {
        self.text(format_args!("{v}"))
    }

    fn serialize_f32(self, _v: f32) -> Result<(), DeError> {
        Err(key_error())
    }

    fn serialize_f64(self, _v: f64) -> Result<(), DeError> {
        Err(key_error())
    }

    fn serialize_char(self, v: char) -> Result<(), DeError> {
        self.out.push(v);
        Ok(())
    }

    fn serialize_str(self, v: &str) -> Result<(), DeError> {
        self.out.push_str(v);
        Ok(())
    }

    fn serialize_bytes(self, _v: &[u8]) -> Result<(), DeError> {
        Err(key_error())
    }

    fn serialize_none(self) -> Result<(), DeError> {
        Err(key_error())
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<(), DeError> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<(), DeError> {
        Err(key_error())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<(), DeError> {
        Err(key_error())
    }

    fn serialize_unit_variant(self, _name: &'static str, _index: u32, variant: &'static str) -> Result<(), DeError> {
        self.serialize_str(variant)
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(self, _name: &'static str, value: &T) -> Result<(), DeError> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<(), DeError> {
        Err(key_error())
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, DeError> {
        Err(key_error())
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, DeError> {
        Err(key_error())
    }

    fn serialize_tuple_struct(self, _name: &'static str, _len: usize) -> Result<Self::SerializeTupleStruct, DeError> {
        Err(key_error())
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, DeError> {
        Err(key_error())
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, DeError> {
        Err(key_error())
    }

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self::SerializeStruct, DeError> {
        Err(key_error())
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, DeError> {
        Err(key_error())
    }
}
