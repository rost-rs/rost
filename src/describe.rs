//! Describing a type: its fields, kinds, ranges and docs, rendered as a JSON Schema (for
//! editor autocompletion and validation) or as a readable `.rost` schema.
//!
//! `#[derive(Describe)]` implements [`Describe`] for structs and unit-only enums, reading
//! the same `#[rost(...)]` attributes as `#[derive(Rost)]` plus the `///` doc comments.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

/// The shape of a type.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeDesc {
    /// Anything (types the schema cannot see into).
    Any,
    Bool,
    Int { min: Option<f64>, max: Option<f64> },
    Float { min: Option<f64>, max: Option<f64> },
    String { one_of: Vec<String>, regex: Option<String> },
    /// One of these names.
    Enum(Vec<String>),
    Color,
    Percentage,
    Duration,
    Dimension,
    Path,
    Optional(Box<TypeDesc>),
    List(Box<TypeDesc>),
    /// String keys to values.
    Map(Box<TypeDesc>),
    Object(Vec<FieldDesc>),
}

/// A field of an [`TypeDesc::Object`].
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDesc {
    pub name: String,
    /// The `///` documentation of the field.
    pub doc: String,
    pub ty: TypeDesc,
    /// False for `Option` fields and fields with a default.
    pub required: bool,
}

/// A type that can describe itself.
pub trait Describe {
    /// The shape of the type.
    fn describe() -> TypeDesc;
}

impl TypeDesc {
    /// Narrows the numeric range (looks through `Option`).
    pub fn with_range(self, lo: Option<f64>, hi: Option<f64>) -> Self {
        match self {
            Self::Int { min, max } => Self::Int { min: lo.or(min), max: hi.or(max) },
            Self::Float { min, max } => Self::Float { min: lo.or(min), max: hi.or(max) },
            Self::Optional(inner) => Self::Optional(Box::new(inner.with_range(lo, hi))),
            other => other,
        }
    }

    /// Restricts a string or number to these values (looks through `Option`).
    pub fn with_one_of(self, allowed: Vec<String>) -> Self {
        match self {
            Self::String { regex, .. } => Self::String { one_of: allowed, regex },
            Self::Optional(inner) => Self::Optional(Box::new(inner.with_one_of(allowed))),
            Self::Int { .. } | Self::Float { .. } => Self::Enum(allowed),
            other => other,
        }
    }

    /// Requires a string to match a regular expression (looks through `Option`).
    pub fn with_regex(self, pattern: &str) -> Self {
        match self {
            Self::String { one_of, .. } => Self::String { one_of, regex: Some(pattern.to_string()) },
            Self::Optional(inner) => Self::Optional(Box::new(inner.with_regex(pattern))),
            other => other,
        }
    }

    /// The schema as JSON Schema (draft 2020-12), as pretty printed text.
    pub fn to_json_schema(&self) -> String {
        let mut out = String::new();
        let mut doc = self.json();
        doc.insert(0, ("$schema".to_string(), Json::Str("https://json-schema.org/draft/2020-12/schema".into())));
        Json::Obj(doc).write(&mut out, 0);
        out.push('\n');
        out
    }

    /// The schema in `.rost` notation.
    pub fn to_rost_schema(&self) -> String {
        let mut out = String::new();
        match self {
            Self::Object(fields) => write_fields(fields, 0, &mut out),
            other => {
                let _ = writeln!(out, "value: {}", other.label());
            }
        }
        out
    }

    fn label(&self) -> String {
        fn bounds(min: &Option<f64>, max: &Option<f64>) -> String {
            let b = |v: f64| if v.fract() == 0.0 && v.abs() < 1e15 { format!("{}", v as i64) } else { format!("{v}") };
            match (min, max) {
                (None, None) => String::new(),
                (a, b2) => format!("({}..{})", a.map(b).unwrap_or_default(), b2.map(b).unwrap_or_default()),
            }
        }
        match self {
            Self::Any => "any".into(),
            Self::Bool => "bool".into(),
            Self::Int { min, max } => format!("int{}", bounds(min, max)),
            Self::Float { min, max } => format!("float{}", bounds(min, max)),
            Self::String { one_of, regex } if !one_of.is_empty() => {
                format!("string({})", one_of.join(" | ")) + &regex.as_ref().map(|r| format!(" /{r}/")).unwrap_or_default()
            }
            Self::String { regex: Some(r), .. } => format!("string /{r}/"),
            Self::String { .. } => "string".into(),
            Self::Enum(names) => format!("enum({})", names.join(" | ")),
            Self::Color => "color".into(),
            Self::Percentage => "percentage".into(),
            Self::Duration => "duration".into(),
            Self::Dimension => "dimension".into(),
            Self::Path => "path".into(),
            Self::Optional(inner) => inner.label(),
            Self::List(inner) => format!("list({})", inner.label()),
            Self::Map(inner) => format!("map({})", inner.label()),
            Self::Object(_) => "block".into(),
        }
    }

    fn json(&self) -> Vec<(String, Json)> {
        let s = |t: &str| ("type".to_string(), Json::Str(t.to_string()));
        let pattern = |p: &str| ("pattern".to_string(), Json::Str(p.to_string()));
        match self {
            Self::Any => vec![],
            Self::Bool => vec![s("boolean")],
            Self::Int { min, max } | Self::Float { min, max } => {
                let mut v = vec![s(if matches!(self, Self::Int { .. }) { "integer" } else { "number" })];
                if let Some(m) = min {
                    v.push(("minimum".into(), Json::Num(*m)));
                }
                if let Some(m) = max {
                    v.push(("maximum".into(), Json::Num(*m)));
                }
                v
            }
            Self::String { one_of, regex } => {
                let mut v = vec![s("string")];
                if !one_of.is_empty() {
                    v.push(("enum".into(), Json::Arr(one_of.iter().cloned().map(Json::Str).collect())));
                }
                if let Some(r) = regex {
                    v.push(pattern(r));
                }
                v
            }
            Self::Enum(names) => vec![s("string"), ("enum".into(), Json::Arr(names.iter().cloned().map(Json::Str).collect()))],
            Self::Color => vec![s("string"), pattern("^#([0-9a-fA-F]{3,4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$")],
            Self::Percentage => vec![s("string"), pattern(r"^\s*-?[0-9]+(\.[0-9]+)?\s*%$")],
            Self::Duration => vec![s("string"), pattern(r"^[0-9]+(\.[0-9]+)?\s*(ns|us|µs|ms|s|m|h|d)$")],
            Self::Dimension => vec![s("string"), pattern(r"^[-+]?[0-9]+(\.[0-9]+)?(px|em|rem|%|vw|vh|pt|cm|mm|in)$")],
            Self::Path => vec![s("string")],
            Self::Optional(inner) => inner.json(),
            Self::List(inner) => vec![s("array"), ("items".into(), Json::Obj(inner.json()))],
            Self::Map(inner) => vec![s("object"), ("additionalProperties".into(), Json::Obj(inner.json()))],
            Self::Object(fields) => {
                let mut props = Vec::new();
                for f in fields {
                    let mut p = f.ty.json();
                    if !f.doc.is_empty() {
                        p.push(("description".into(), Json::Str(f.doc.clone())));
                    }
                    props.push((f.name.clone(), Json::Obj(p)));
                }
                let required: Vec<_> = fields.iter().filter(|f| f.required).map(|f| Json::Str(f.name.clone())).collect();
                let mut v = vec![s("object"), ("properties".into(), Json::Obj(props))];
                if !required.is_empty() {
                    v.push(("required".into(), Json::Arr(required)));
                }
                v
            }
        }
    }
}

fn write_fields(fields: &[FieldDesc], depth: usize, out: &mut String) {
    let pad = "  ".repeat(depth);
    for f in fields {
        for line in f.doc.lines() {
            let _ = writeln!(out, "{pad}# {}", line.trim());
        }
        let mark = if f.required { "" } else { "?" };
        let inner = match &f.ty {
            TypeDesc::Optional(i) => i.as_ref(),
            t => t,
        };
        match inner {
            TypeDesc::Object(sub) => {
                let _ = writeln!(out, "{pad}{}{mark} {{", f.name);
                write_fields(sub, depth + 1, out);
                let _ = writeln!(out, "{pad}}}");
            }
            t => {
                let _ = writeln!(out, "{pad}{}{mark}: {}", f.name, t.label());
            }
        }
    }
}

enum Json {
    Str(String),
    Num(f64),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn write(&self, out: &mut String, depth: usize) {
        match self {
            Json::Str(s) => {
                out.push('"');
                for c in s.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\t' => out.push_str("\\t"),
                        c if (c as u32) < 0x20 => {
                            let _ = write!(out, "\\u{:04x}", c as u32);
                        }
                        c => out.push(c),
                    }
                }
                out.push('"');
            }
            Json::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => {
                let _ = write!(out, "{}", *n as i64);
            }
            Json::Num(n) => {
                let _ = write!(out, "{n}");
            }
            Json::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    item.write(out, depth);
                }
                out.push(']');
            }
            Json::Obj(entries) if entries.is_empty() => out.push_str("{}"),
            Json::Obj(entries) => {
                out.push_str("{\n");
                for (i, (k, v)) in entries.iter().enumerate() {
                    out.push_str(&"  ".repeat(depth + 1));
                    Json::Str(k.clone()).write(out, depth + 1);
                    out.push_str(": ");
                    v.write(out, depth + 1);
                    out.push_str(if i + 1 < entries.len() { ",\n" } else { "\n" });
                }
                out.push_str(&"  ".repeat(depth));
                out.push('}');
            }
        }
    }
}

macro_rules! ints {
    ($($ty:ty),*) => {$(impl Describe for $ty {
        fn describe() -> TypeDesc { TypeDesc::Int { min: Some(<$ty>::MIN as f64), max: Some(<$ty>::MAX as f64) } }
    })*};
}
ints!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

impl Describe for f64 {
    fn describe() -> TypeDesc {
        TypeDesc::Float { min: None, max: None }
    }
}
impl Describe for f32 {
    fn describe() -> TypeDesc {
        TypeDesc::Float { min: None, max: None }
    }
}
impl Describe for bool {
    fn describe() -> TypeDesc {
        TypeDesc::Bool
    }
}
impl Describe for String {
    fn describe() -> TypeDesc {
        TypeDesc::String { one_of: Vec::new(), regex: None }
    }
}
impl Describe for &str {
    fn describe() -> TypeDesc {
        String::describe()
    }
}
impl<T: Describe> Describe for Option<T> {
    fn describe() -> TypeDesc {
        TypeDesc::Optional(Box::new(T::describe()))
    }
}
impl<T: Describe> Describe for Box<T> {
    fn describe() -> TypeDesc {
        T::describe()
    }
}
impl<T: Describe> Describe for Vec<T> {
    fn describe() -> TypeDesc {
        TypeDesc::List(Box::new(T::describe()))
    }
}
impl<T: Describe> Describe for HashMap<String, T> {
    fn describe() -> TypeDesc {
        TypeDesc::Map(Box::new(T::describe()))
    }
}
impl<T: Describe> Describe for BTreeMap<String, T> {
    fn describe() -> TypeDesc {
        TypeDesc::Map(Box::new(T::describe()))
    }
}
