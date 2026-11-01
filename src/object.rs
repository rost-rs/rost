//! # Dynamic Object Mapping for `.rost` Documents
//!
//! Provides zero-boilerplate dynamic objects, block maps, and value containers.

use std::collections::HashMap;
use super::query::BlockView;
use super::field::ParsedField;
use super::Template;

/// A dynamic key-value object representing a scoped `.rost` block or entity.
#[derive(Debug, Clone, Default)]
pub struct RostObject {
    /// Identifier or selector name of the block.
    pub name: String,
    /// Parent block name if nested.
    pub parent: Option<String>,
    /// Properties mapped by key name.
    pub properties: HashMap<String, String>,
}

impl RostObject {
    /// Creates a new empty [`RostObject`].
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            parent: None,
            properties: HashMap::new(),
        }
    }

    /// Converts a [`BlockView`] into an owned [`RostObject`].
    pub fn from_block(block: &BlockView<'_>) -> Self {
        let mut obj = Self {
            name: block.name.to_string(),
            parent: block.parent.map(ToString::to_string),
            properties: HashMap::with_capacity(block.fields.len()),
        };

        for field in &block.fields {
            match field {
                ParsedField::CommonText { name, value, .. } => {
                    obj.properties.insert(name.to_string(), value.to_string());
                }
                ParsedField::Subtitle { name, value, .. } => {
                    obj.properties.insert(name.to_string(), value.to_string());
                }
                ParsedField::Directive { name, value, .. } => {
                    obj.properties.insert(name.to_string(), value.to_string());
                }
                ParsedField::VarFunc { var_name, signature } => {
                    obj.properties.insert(var_name.to_string(), signature.to_string());
                }
                ParsedField::Enum { name, raw_variants, .. } => {
                    obj.properties.insert(name.to_string(), raw_variants.to_string());
                }
                ParsedField::List { name, raw_items, .. } => {
                    obj.properties.insert(name.to_string(), raw_items.to_string());
                }
                _ => {}
            }
        }

        obj
    }

    /// Retrieves a raw string property value.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.properties.get(key).map(|s| s.as_str())
    }

    /// Retrieves and parses a property as a hex color (`u32`).
    pub fn hex(&self, key: &str) -> Option<u32> {
        let s = self.get(key)?.trim();
        let hex = s.strip_prefix('#').or_else(|| s.strip_prefix("0x")).unwrap_or(s);
        crate::field::parse_hex_digits(hex.as_bytes())
    }

    /// Retrieves and parses a property as a dimension/float (`f32`).
    pub fn dim(&self, key: &str) -> Option<f32> {
        let s = self.get(key)?.trim();
        let split_idx = s
            .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
            .unwrap_or(s.len());
        s[..split_idx].parse::<f32>().ok()
    }

    /// Retrieves and parses a property as a float (`f64`).
    pub fn f64(&self, key: &str) -> Option<f64> {
        self.get(key)?.trim().parse::<f64>().ok()
    }

    /// Retrieves and parses a property as an integer (`i64`).
    pub fn i64(&self, key: &str) -> Option<i64> {
        self.get(key)?.trim().parse::<i64>().ok()
    }

    /// Retrieves and parses a property as a boolean.
    pub fn bool(&self, key: &str) -> Option<bool> {
        match self.get(key)?.trim() {
            "true" | "1" | "yes" | "on" => Some(true),
            "false" | "0" | "no" | "off" => Some(false),
            _ => None,
        }
    }

    /// Retrieves a string slice with surrounding quotes stripped.
    pub fn str(&self, key: &str) -> Option<&str> {
        self.get(key).map(|s| s.trim().trim_matches('"'))
    }

    /// Returns all property key names.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.properties.keys().map(String::as_str)
    }
}

impl Template {
    /// Extracts all scoped blocks in the template into owned [`RostObject`] entities.
    ///
    /// # Example
    /// ```rust
    /// use rost::Template;
    /// let doc = "#car { color: #ff0000 } #wheel { size: 18px }";
    /// let tmpl = Template::from_str(doc);
    /// let objs = tmpl.map_objs();
    /// assert_eq!(objs.len(), 2);
    /// assert_eq!(objs[0].name, "car");
    /// ```
    pub fn map_objs(&self) -> Vec<RostObject> {
        self.blocks().iter().map(RostObject::from_block).collect()
    }

    /// Extracts blocks matching a specific selector or parent namespace.
    pub fn map_objs_under(&self, parent: &str) -> Vec<RostObject> {
        let clean = parent.strip_prefix('#').unwrap_or(parent);
        self.blocks()
            .iter()
            .filter(|b| b.parent == Some(clean) || b.parent == Some(parent))
            .map(RostObject::from_block)
            .collect()
    }

    /// Converts a specific block into an owned [`RostObject`].
    pub fn to_obj(&self, selector: &str) -> Option<RostObject> {
        let clean = selector.strip_prefix('#').unwrap_or(selector);
        let mut idx = 0;
        let mut parent_name: Option<String> = None;
        let mut target_start: Option<usize> = None;
        let mut matched_name = String::new();

        let mut stack: Vec<String> = Vec::with_capacity(4);

        while idx < self.fields.len() {
            let tag = self.fields[idx];
            match tag {
                crate::parser::TAG_BLOCK_START => {
                    let name = crate::parser::resolve_slice(&self.data, &self.fields, idx + 1);
                    idx += crate::parser::REC_BLOCK_START;
                    let clean_name = name.strip_prefix('#').unwrap_or(name);
                    let full_path = if let Some(p) = stack.last() {
                        format!("{}.{}", p, clean_name)
                    } else {
                        clean_name.to_string()
                    };

                    if clean_name == clean || full_path == clean {
                        parent_name = stack.last().cloned();
                        matched_name = clean_name.to_string();
                        target_start = Some(idx);
                        break;
                    }
                    stack.push(full_path);
                }
                crate::parser::TAG_BLOCK_END => {
                    stack.pop();
                    idx += crate::parser::REC_BLOCK_END;
                }
                other => idx += crate::parser::record_len(other),
            }
        }

        if let Some(start_idx) = target_start {
            let mut obj = RostObject::new(matched_name);
            obj.parent = parent_name;

            let mut cur = start_idx;
            let mut depth = 1;

            while cur < self.fields.len() && depth > 0 {
                let tag = self.fields[cur];
                match tag {
                    crate::parser::TAG_COMMON_TEXT => {
                        if depth == 1 {
                            let key = crate::parser::resolve_slice(&self.data, &self.fields, cur + 1);
                            let val = crate::parser::resolve_slice(&self.data, &self.fields, cur + 9);
                            obj.properties.insert(key.to_string(), val.to_string());
                        }
                        cur += crate::parser::REC_PAIR;
                    }
                    crate::parser::TAG_BLOCK_START => {
                        depth += 1;
                        cur += crate::parser::REC_BLOCK_START;
                    }
                    crate::parser::TAG_BLOCK_END => {
                        depth -= 1;
                        cur += crate::parser::REC_BLOCK_END;
                    }
                    crate::parser::TAG_ENUM | crate::parser::TAG_LIST => {
                        if depth == 1 {
                            let key = crate::parser::resolve_slice(&self.data, &self.fields, cur + 1);
                            let val = crate::parser::resolve_slice(&self.data, &self.fields, cur + 9);
                            obj.properties.insert(key.to_string(), val.to_string());
                        }
                        cur += crate::parser::REC_RANGE;
                    }
                    other => cur += crate::parser::record_len(other),
                }
            }

            Some(obj)
        } else {
            self.find_block(selector).as_ref().map(RostObject::from_block)
        }
    }
}
