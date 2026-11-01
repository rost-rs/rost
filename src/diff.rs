//! Structural diff between two [`Value`] trees, reported as dotted paths.
//!
//! Paths look like `window.width` for block entries and `items[2]` for list elements.

use crate::value::Value;

/// One changed spot between two documents.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    /// Dotted path of the changed value (`""` when the whole root changed).
    pub path: String,
    /// The value before, or `None` if the path was added.
    pub old: Option<Value>,
    /// The value after, or `None` if the path was removed.
    pub new: Option<Value>,
}

/// Lists every leaf that differs between `old` and `new`.
///
/// Blocks and lists are compared entry by entry; a spot whose kind changed (a block that
/// became a number) is reported once at its own path.
pub fn diff(old: &Value, new: &Value) -> Vec<Change> {
    let mut out = Vec::new();
    walk(String::new(), Some(old), Some(new), &mut out);
    out
}

fn child(path: &str, key: &str) -> String {
    if path.is_empty() { key.to_string() } else { format!("{path}.{key}") }
}

fn walk(path: String, old: Option<&Value>, new: Option<&Value>, out: &mut Vec<Change>) {
    match (old, new) {
        (Some(Value::Block(a)), Some(Value::Block(b))) => {
            for (k, v) in a.iter() {
                walk(child(&path, k), Some(v), b.get(k), out);
            }
            for (k, v) in b.iter() {
                if !a.contains_key(k) {
                    walk(child(&path, k), None, Some(v), out);
                }
            }
        }
        (Some(Value::List(a)), Some(Value::List(b))) => {
            for i in 0..a.len().max(b.len()) {
                walk(format!("{path}[{i}]"), a.get(i), b.get(i), out);
            }
        }
        (a, b) if a == b => {}
        (a, b) => out.push(Change { path, old: a.cloned(), new: b.cloned() }),
    }
}

/// Whether a subscription on `watched` is interested in a change at `changed`:
/// the same path, anything below it, or a parent that was replaced wholesale.
pub fn covers(watched: &str, changed: &str) -> bool {
    fn below(parent: &str, path: &str) -> bool {
        parent.is_empty()
            || path == parent
            || path.strip_prefix(parent).is_some_and(|r| r.starts_with('.') || r.starts_with('['))
    }
    below(watched, changed) || below(changed, watched)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::from_str;

    fn v(s: &str) -> Value {
        from_str(s).unwrap()
    }

    #[test]
    fn reports_changed_added_and_removed_paths() {
        let a = v("window { width: 800\n height: 600 }\nname: \"a\"\nitems: [1, 2]");
        let b = v("window { width: 900\n height: 600 }\nitems: [1, 3, 4]\nextra: true");
        let paths: Vec<_> = diff(&a, &b).into_iter().map(|c| c.path).collect();
        assert_eq!(paths, ["window.width", "name", "items[1]", "items[2]", "extra"]);
    }

    #[test]
    fn equal_documents_have_no_changes() {
        let a = v("a: 1\nb { c: 2 }");
        assert!(diff(&a, &a.clone()).is_empty());
    }

    #[test]
    fn covers_matches_prefixes_on_boundaries_only() {
        assert!(covers("window", "window.width"));
        assert!(covers("window.width", "window.width"));
        assert!(covers("window.width", "window"));
        assert!(covers("items", "items[2]"));
        assert!(!covers("win", "window.width"));
        assert!(!covers("window.w", "window.width"));
        assert!(covers("", "anything"));
    }
}
