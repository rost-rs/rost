//! Checks run by the attributes of `#[derive(Rost)]`: `range`, `one_of` and `regex`.
//!
//! Each returns the text of the problem (`5000 is out of range: expected 1 to 4096`); the
//! generated loader turns it into an error that carries the line, column and field path.

use std::fmt::Display;

/// Numbers a `range` check applies to.
pub trait AsF64: Copy {
    /// The value as a float (exact up to 2^53 for the 64-bit integers).
    fn as_f64(self) -> f64;
}

macro_rules! as_f64 {
    ($($ty:ty),*) => {$(impl AsF64 for $ty { fn as_f64(self) -> f64 { self as f64 } })*};
}
as_f64!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize, f32, f64);

fn bound(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 { format!("{}", v as i64) } else { format!("{v}") }
}

/// `min <= value <= max`, either bound optional.
pub fn range<T: AsF64 + Display>(value: T, min: Option<f64>, max: Option<f64>) -> Result<(), String> {
    let v = value.as_f64();
    let low = min.is_some_and(|m| v < m);
    let high = max.is_some_and(|m| v > m);
    if !(low || high) && !v.is_nan() {
        return Ok(());
    }
    let expected = match (min, max) {
        (Some(a), Some(b)) => format!("{} to {}", bound(a), bound(b)),
        (Some(a), None) => format!("at least {}", bound(a)),
        (None, Some(b)) => format!("at most {}", bound(b)),
        (None, None) => "a number".to_string(),
    };
    Err(format!("{value} is out of range: expected {expected}"))
}

/// The value, shown as text, is one of `allowed`.
pub fn one_of<T: Display>(value: &T, allowed: &[&str]) -> Result<(), String> {
    let shown = value.to_string();
    if allowed.contains(&shown.as_str()) {
        return Ok(());
    }
    Err(format!("`{shown}` is not allowed: expected one of {}", allowed.join(", ")))
}

/// A regular expression compiled on first use (feature `regex`).
#[cfg(feature = "regex")]
pub struct Pattern {
    source: &'static str,
    compiled: std::sync::OnceLock<Result<regex::Regex, String>>,
}

#[cfg(feature = "regex")]
impl Pattern {
    /// A pattern that compiles `source` the first time it is used.
    pub const fn new(source: &'static str) -> Self {
        Self { source, compiled: std::sync::OnceLock::new() }
    }
}

/// The text matches the pattern.
#[cfg(feature = "regex")]
pub fn matches(value: &str, pattern: &Pattern) -> Result<(), String> {
    match pattern.compiled.get_or_init(|| regex::Regex::new(pattern.source).map_err(|e| e.to_string())) {
        Ok(re) if re.is_match(value) => Ok(()),
        Ok(_) => Err(format!("`{value}` does not match `{}`", pattern.source)),
        Err(e) => Err(format!("invalid pattern `{}`: {e}", pattern.source)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_reports_the_expected_interval() {
        assert!(range(5u32, Some(1.0), Some(10.0)).is_ok());
        assert!(range(1u32, Some(1.0), Some(10.0)).is_ok());
        assert!(range(10u32, Some(1.0), Some(10.0)).is_ok());
        assert_eq!(range(11u32, Some(1.0), Some(10.0)).unwrap_err(), "11 is out of range: expected 1 to 10");
        assert_eq!(range(0i32, Some(1.0), None).unwrap_err(), "0 is out of range: expected at least 1");
        assert_eq!(range(2.5f64, None, Some(2.0)).unwrap_err(), "2.5 is out of range: expected at most 2");
        assert!(range(f64::NAN, Some(0.0), Some(1.0)).is_err());
    }

    #[test]
    fn one_of_compares_the_displayed_value() {
        assert!(one_of(&"dark", &["dark", "light"]).is_ok());
        assert!(one_of(&2u8, &["1", "2"]).is_ok());
        assert!(one_of(&"blue", &["dark", "light"]).unwrap_err().contains("expected one of dark, light"));
    }
}
