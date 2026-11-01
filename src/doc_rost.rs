//! Builds a [`Doc`] from `.rost` text through the single-pass deserializer.

use std::fmt;

use serde::de::{DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};

use crate::de::DeError;
use crate::doc::{Doc, Node, Tape};

pub(crate) fn build(src: &str) -> Result<Doc<'_>, DeError> {
    if src.len() > crate::parser::MAX_DOCUMENT_LEN {
        return Err(DeError("document is larger than 4 GiB".into()));
    }
    let mut tape = Tape::new(src);
    crate::text::from_str_seed(src, Value(&mut tape))?;
    Ok(Doc::from_tape(src, tape))
}

/// Appends one value (and, for containers, everything inside it) to the tape.
struct Value<'t>(&'t mut Tape);

impl<'de> DeserializeSeed<'de> for Value<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Value<'_> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any .rost value")
    }

    fn visit_bool<E>(self, v: bool) -> Result<(), E> {
        self.0.push(Node::Bool(v));
        Ok(())
    }

    fn visit_i64<E>(self, v: i64) -> Result<(), E> {
        self.0.push(Node::Int(v));
        Ok(())
    }

    fn visit_u64<E>(self, v: u64) -> Result<(), E> {
        // Like `rost::Value`: unsigned values above i64::MAX become floats.
        self.0.push(i64::try_from(v).map_or(Node::Float(v as f64), Node::Int));
        Ok(())
    }

    fn visit_f64<E>(self, v: f64) -> Result<(), E> {
        self.0.push(Node::Float(v));
        Ok(())
    }

    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<(), E> {
        self.0.string(v);
        Ok(())
    }

    fn visit_str<E>(self, v: &str) -> Result<(), E> {
        self.0.string(v);
        Ok(())
    }

    fn visit_string<E>(self, v: String) -> Result<(), E> {
        self.0.owned_string(v);
        Ok(())
    }

    fn visit_unit<E>(self) -> Result<(), E> {
        self.0.push(Node::Null);
        Ok(())
    }

    fn visit_none<E>(self) -> Result<(), E> {
        self.0.push(Node::Null);
        Ok(())
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        let at = self.0.open(false);
        let mut len = 0;
        while seq.next_element_seed(Value(&mut *self.0))?.is_some() {
            len += 1;
        }
        self.0.close(at, len);
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let at = self.0.open(true);
        let mut len = 0;
        while map.next_key_seed(Key(&mut *self.0))?.is_some() {
            map.next_value_seed(Value(&mut *self.0))?;
            len += 1;
        }
        self.0.close(at, len);
        Ok(())
    }
}

/// Appends a map key, as a string node.
struct Key<'t>(&'t mut Tape);

impl<'de> DeserializeSeed<'de> for Key<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl<'de> Visitor<'de> for Key<'_> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a key")
    }

    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<(), E> {
        self.0.string(v);
        Ok(())
    }

    fn visit_str<E>(self, v: &str) -> Result<(), E> {
        self.0.string(v);
        Ok(())
    }

    fn visit_string<E>(self, v: String) -> Result<(), E> {
        self.0.owned_string(v);
        Ok(())
    }
}
