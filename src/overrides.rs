//! Overrides on top of a document: environment variables and command-line settings replace
//! values of the file without editing it.
//!
//! ```text
//! APP_WINDOW__WIDTH=800 myapp --set window.title="Hi" --set tags.0=x
//! ```
//!
//! `Overrides::from_env("APP")` reads `APP_WINDOW__WIDTH` as the path `window.width`
//! (`__` separates blocks, names are lowercased); `Overrides::from_args(args)` reads
//! `--set path=value` and `--set=path=value`. Values read as `true`/`false`, integers and
//! floats where they look like one, and as text otherwise (use quotes to force text).
//!
//! [`load_with`] loads a typed config with the overrides applied. A mistake in the file is
//! reported at its own line; a value an override made invalid is reported as the override.

use crate::value::{Map, Value};
use crate::{DeError, Load};

/// A set of `path = value` replacements.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overrides {
    entries: Vec<(Vec<String>, String)>,
}

fn infer(raw: &str) -> Value {
    let raw = raw.trim();
    if let Some(quoted) = raw.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        return Value::String(quoted.to_string());
    }
    match raw {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => raw
            .parse::<i64>()
            .map(Value::Int)
            .or_else(|_| raw.parse::<f64>().ok().filter(|f| f.is_finite() && raw.chars().any(|c| c.is_ascii_digit())).map(Value::Float).ok_or(()))
            .unwrap_or_else(|_| Value::String(raw.to_string())),
    }
}

impl Overrides {
    /// No overrides.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether there is nothing to apply.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Adds `path` (dotted: `window.width`) with a value read from `raw`.
    pub fn set(mut self, path: &str, raw: &str) -> Self {
        let keys: Vec<String> = path.split('.').filter(|k| !k.is_empty()).map(str::to_string).collect();
        if !keys.is_empty() {
            self.entries.push((keys, raw.trim().to_string()));
        }
        self
    }

    /// Adds the entries of `other` after this set's own (later entries win).
    pub fn merge(mut self, other: Overrides) -> Self {
        self.entries.extend(other.entries);
        self
    }

    /// Reads every `PREFIX_A__B=value` variable of the process environment.
    pub fn from_env(prefix: &str) -> Self {
        Self::from_pairs(prefix, std::env::vars())
    }

    /// Like [`from_env`](Self::from_env) over any `(name, value)` pairs.
    pub fn from_pairs(prefix: &str, pairs: impl IntoIterator<Item = (String, String)>) -> Self {
        let prefix = format!("{prefix}_");
        let mut found: Vec<(String, String)> = pairs.into_iter().filter(|(k, _)| k.starts_with(&prefix)).collect();
        found.sort();
        let mut out = Self::new();
        for (name, value) in found {
            let path = name[prefix.len()..].to_lowercase().replace("__", ".");
            out = out.set(&path, &value);
        }
        out
    }

    /// Reads `--set path=value` and `--set=path=value` from command-line arguments.
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Self {
        let mut out = Self::new();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let setting = match arg.strip_prefix("--set=") {
                Some(s) => Some(s.to_string()),
                None if arg == "--set" => args.next(),
                None => None,
            };
            if let Some((path, value)) = setting.as_deref().and_then(|s| s.split_once('=')) {
                out = out.set(path, value);
            }
        }
        out
    }

    /// Applies the overrides to a document, creating the blocks on the way.
    pub fn apply(&self, doc: &mut Value) {
        for (path, value) in &self.entries {
            let mut at = &mut *doc;
            for key in &path[..path.len() - 1] {
                if !matches!(at, Value::Block(_)) {
                    *at = Value::Block(Map::new());
                }
                let Value::Block(map) = at else { unreachable!() };
                if !matches!(map.get(key), Some(Value::Block(_))) {
                    map.insert(key.clone(), Value::Block(Map::new()));
                }
                at = map.get_mut(key).expect("just inserted");
            }
            if !matches!(at, Value::Block(_)) {
                *at = Value::Block(Map::new());
            }
            if let Value::Block(map) = at {
                map.insert(path[path.len() - 1].clone(), infer(value));
            }
        }
    }

    fn describe(&self) -> String {
        self.entries.iter().map(|(p, _)| p.join(".")).collect::<Vec<_>>().join(", ")
    }
}

/// The text of a value as it goes into a document: quoted unless it is safe bare.
fn render(raw: &str) -> String {
    let quoted = raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"');
    let plain = |c: char| c.is_alphanumeric() || "_.+-%/@".contains(c);
    let bare = match raw.strip_prefix('#') {
        Some(hex) => !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()),
        None => !raw.is_empty() && raw.chars().all(plain),
    };
    if quoted || bare {
        raw.to_string()
    } else {
        format!("\"{}\"", raw.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

impl Overrides {
    /// Writes the overrides into `text`: an existing value is replaced where it stands, a
    /// missing one is added to its block (or the end of the document). Everything else,
    /// comments and layout included, stays as it was. `None` for a JSON-style document.
    pub fn patch(&self, text: &str) -> Result<Option<String>, DeError> {
        let Some(found) = crate::text::scan_entries(text)? else { return Ok(None) };
        // (offset, order, replace_len, text): edits applied from the end of the file backwards.
        let mut edits: Vec<(usize, usize, usize, String)> = Vec::new();
        for (order, (path, raw)) in self.entries.iter().enumerate() {
            // The last override of a path wins; the earlier ones must not edit the same span.
            if self.entries[order + 1..].iter().any(|(p, _)| p == path) {
                continue;
            }
            let shown = || path.join(".");
            if let Some((_, span)) = found.iter().find(|(p, _)| p == path) {
                match span {
                    crate::text::Span::Scalar { start, end } => edits.push((*start, order, end - start, render(raw))),
                    crate::text::Span::Block { .. } => {
                        return Err(DeError::new(format!("override `{}` targets a block, not a value", shown())));
                    }
                }
                continue;
            }
            // The deepest existing block on the way to the path.
            let mut depth = 0;
            let mut close = text.len();
            for d in 1..path.len() {
                match found.iter().find(|(p, _)| p[..] == path[..d]) {
                    Some((_, crate::text::Span::Block { close: c })) => {
                        depth = d;
                        close = *c;
                    }
                    Some((_, crate::text::Span::Scalar { .. })) => {
                        return Err(DeError::new(format!("override `{}`: `{}` is a value, not a block", shown(), path[..d].join("."))));
                    }
                    None => break,
                }
            }
            let mut entry = format!("{}: {}", path[path.len() - 1], render(raw));
            for key in path[depth..path.len() - 1].iter().rev() {
                entry = format!("{key} {{ {entry} }}");
            }
            edits.push((close, order, 0, format!("\n{entry}\n")));
        }
        edits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        let mut out = text.to_string();
        for (at, _, len, new) in edits {
            out.replace_range(at..at + len, &new);
        }
        Ok(Some(out))
    }
}

/// Loads `text` into a `T` with `overrides` applied on top.
///
/// Without overrides this is [`load`](crate::load). With them, the overrides are written
/// into the text ([`Overrides::patch`]) and the result is loaded. An error in the file
/// itself is reported by loading the file alone first, so its line numbers are the file's;
/// a value an override made invalid is reported as the override.
pub fn load_with<T: for<'a> Load<'a>>(text: &str, overrides: &Overrides) -> Result<T, DeError> {
    if overrides.is_empty() {
        return crate::load(text);
    }
    let merged = match overrides.patch(text)? {
        Some(patched) => patched,
        None => {
            // JSON-style document: patch the generic tree and write it back.
            let mut doc: Value = crate::from_str(text)?;
            overrides.apply(&mut doc);
            crate::to_string(&doc)?
        }
    };
    match crate::load::<T>(&merged) {
        Ok(value) => Ok(value),
        Err(err) => match crate::load::<T>(text) {
            Err(file_err) => Err(file_err),
            Ok(_) => Err(DeError::new(format!("invalid override ({}): {}", overrides.describe(), err.message()))),
        },
    }
}

/// [`load_with`] on a file; errors are prefixed with the path.
pub fn load_file_with<T: for<'a> Load<'a>>(path: impl AsRef<std::path::Path>, overrides: &Overrides) -> Result<T, DeError> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path).map_err(|e| DeError::new(format!("{}: {e}", path.display())))?;
    load_with(&text, overrides).map_err(|e| DeError::new(format!("{}: {}", path.display(), e.message())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_inferred() {
        assert_eq!(infer("800"), Value::Int(800));
        assert_eq!(infer("1.5"), Value::Float(1.5));
        assert_eq!(infer("true"), Value::Bool(true));
        assert_eq!(infer("hello"), Value::String("hello".into()));
        assert_eq!(infer("\"42\""), Value::String("42".into()));
        assert_eq!(infer("#ff8800"), Value::String("#ff8800".into()));
        assert_eq!(infer("inf"), Value::String("inf".into()));
    }

    #[test]
    fn patch_replaces_in_place_and_keeps_everything_else() {
        let text = "// settings\n// http port\nport: 80\nwindow { width: 1\n  title: \"a b\" }\naccent: #ff8800\n";
        let o = Overrides::new().set("port", "8080").set("window.title", "x y").set("accent", "#00ff00");
        let out = o.patch(text).unwrap().unwrap();
        assert_eq!(out, "// settings\n// http port\nport: 8080\nwindow { width: 1\n  title: \"x y\" }\naccent: #00ff00\n");
    }

    #[test]
    fn patch_adds_missing_keys_to_their_block_or_the_end() {
        let text = "window { width: 1 }\nname: a\n";
        let o = Overrides::new().set("window.height", "2").set("debug", "true").set("net.proxy.url", "http://x");
        let out = o.patch(text).unwrap().unwrap();
        assert!(out.contains("window { width: 1 \nheight: 2\n}"), "{out}");
        assert!(out.contains("\ndebug: true\n") && out.contains("\nnet { proxy { url: \"http://x\" } }\n"), "{out}");
        // The result is a valid document with the new values.
        let v: Value = crate::from_str(&out).unwrap();
        let Value::Block(root) = &v else { panic!() };
        assert_eq!(root.get("debug"), Some(&Value::Bool(true)));
    }

    #[test]
    fn patch_refuses_to_confuse_blocks_and_values() {
        let text = "window { width: 1 }\nname: a\n";
        assert!(Overrides::new().set("window", "3").patch(text).unwrap_err().message().contains("targets a block"));
        assert!(Overrides::new().set("name.first", "3").patch(text).unwrap_err().message().contains("is a value, not a block"));
    }

    #[test]
    fn patch_declines_json_style_documents() {
        assert_eq!(Overrides::new().set("a", "1").patch("{\"a\": 0}").unwrap(), None);
    }

    #[test]
    fn render_quotes_only_what_needs_it() {
        assert_eq!(render("800"), "800");
        assert_eq!(render("#ff8800"), "#ff8800");
        assert_eq!(render("dark-theme"), "dark-theme");
        assert_eq!(render("a b"), "\"a b\"");
        assert_eq!(render("\"x\""), "\"x\"");
        assert_eq!(render("say \"hi\""), "\"say \\\"hi\\\"\"");
    }

    #[test]
    fn environment_names_become_paths() {
        let pairs = [
            ("APP_WINDOW__WIDTH".to_string(), "800".to_string()),
            ("APP_DEBUG".to_string(), "true".to_string()),
            ("OTHER_X".to_string(), "1".to_string()),
        ];
        let o = Overrides::from_pairs("APP", pairs);
        assert_eq!(o, Overrides::new().set("debug", "true").set("window.width", "800"));
    }

    #[test]
    fn merged_sets_apply_in_order() {
        let merged = Overrides::new().set("a", "1").merge(Overrides::new().set("a", "2"));
        assert_eq!(merged.patch("a: 0\n").unwrap().unwrap(), "a: 2\n");
        // Also for a key that is not in the file yet: it is added once, with the last value.
        assert_eq!(merged.patch("b: 0\n").unwrap().unwrap(), "b: 0\n\na: 2\n");
    }

    #[test]
    fn arguments_accept_both_spellings() {
        let args = ["bin", "--set", "a.b=1", "--set=c=x", "--other", "--set", "nonsense"].map(String::from);
        assert_eq!(Overrides::from_args(args), Overrides::new().set("a.b", "1").set("c", "x"));
    }
}
