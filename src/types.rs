//! Typed values for configuration: [`Color`], [`Dimension`], [`Duration`], [`Percentage`]
//! and [`Path`].
//!
//! Every type parses from its text (`FromStr`), prints back in the same notation
//! (`Display`), loads from a `.rost` field with `rost::load` / `#[derive(Rost)]` and with
//! serde, and describes itself for schema generation.

use std::fmt;
use std::str::FromStr;

use crate::describe::{Describe, TypeDesc};
use crate::{DeError, Fast, Load};

macro_rules! text_type {
    ($ty:ident, $desc:expr) => {
        impl<'de> Load<'de> for $ty {
            fn load(f: &mut Fast<'_, 'de>) -> Result<Self, DeError> {
                f.text_value()?.parse().map_err(DeError::new)
            }
        }

        impl<'de> serde::Deserialize<'de> for $ty {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let text = <std::borrow::Cow<'de, str> as serde::Deserialize>::deserialize(d)?;
                text.parse().map_err(serde::de::Error::custom)
            }
        }

        impl Describe for $ty {
            fn describe() -> TypeDesc {
                $desc
            }
        }
    };
}

/// An RGBA color written `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// An opaque color.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// `0xRRGGBBAA`.
    pub const fn to_u32(self) -> u32 {
        u32::from_be_bytes([self.r, self.g, self.b, self.a])
    }
}

impl FromStr for Color {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let hex = s.trim().strip_prefix('#').ok_or_else(|| format!("`{s}` is not a color: expected `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`"))?;
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("`{s}` is not a color: `#` must be followed by hexadecimal digits"));
        }
        let digit = |i: usize| u8::from_str_radix(&hex[i..=i], 16).unwrap() * 17;
        let pair = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap();
        match hex.len() {
            3 => Ok(Self::rgb(digit(0), digit(1), digit(2))),
            4 => Ok(Self { r: digit(0), g: digit(1), b: digit(2), a: digit(3) }),
            6 => Ok(Self::rgb(pair(0), pair(2), pair(4))),
            8 => Ok(Self { r: pair(0), g: pair(2), b: pair(4), a: pair(6) }),
            n => Err(format!("`{s}` is not a color: {n} hexadecimal digits, expected 3, 4, 6 or 8")),
        }
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)?;
        if self.a != 255 {
            write!(f, "{:02x}", self.a)?;
        }
        Ok(())
    }
}

text_type!(Color, TypeDesc::Color);

/// A percentage written `50%` (`fraction()` gives `0.5`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Percentage(pub f64);

impl Percentage {
    /// The value as a fraction: `50%` is `0.5`.
    pub fn fraction(self) -> f64 {
        self.0 / 100.0
    }
}

impl FromStr for Percentage {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let number = s.trim().strip_suffix('%').ok_or_else(|| format!("`{s}` is not a percentage: expected a number followed by `%`"))?;
        number.trim().parse::<f64>().map(Self).map_err(|_| format!("`{s}` is not a percentage: `{number}` is not a number"))
    }
}

impl fmt::Display for Percentage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}%", self.0)
    }
}

text_type!(Percentage, TypeDesc::Percentage);

/// A length of time written `250ms`, `1.5s`, `2m`, `1h`, `3d`, `10us` or `5ns`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct Duration(pub std::time::Duration);

impl Duration {
    /// The standard library duration.
    pub fn get(self) -> std::time::Duration {
        self.0
    }
}

impl FromStr for Duration {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let text = s.trim();
        let split = text.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(text.len());
        let (number, unit) = text.split_at(split);
        let value: f64 = number.parse().map_err(|_| format!("`{s}` is not a duration: expected a number and a unit, like `250ms`"))?;
        let seconds = match unit.trim() {
            "ns" => 1e-9,
            "us" | "µs" => 1e-6,
            "ms" => 1e-3,
            "s" => 1.0,
            "m" => 60.0,
            "h" => 3600.0,
            "d" => 86400.0,
            "" => return Err(format!("`{s}` is not a duration: missing unit (ns, us, ms, s, m, h, d)")),
            other => return Err(format!("`{s}` is not a duration: unknown unit `{other}` (ns, us, ms, s, m, h, d)")),
        };
        std::time::Duration::try_from_secs_f64(value * seconds)
            .map(Self)
            .map_err(|_| format!("`{s}` is not a duration: out of range"))
    }
}

impl fmt::Display for Duration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = self.0;
        if d.subsec_nanos() == 0 {
            write!(f, "{}s", d.as_secs())
        } else if d.subsec_nanos().is_multiple_of(1_000_000) {
            write!(f, "{}ms", d.as_millis())
        } else if d.subsec_nanos().is_multiple_of(1_000) {
            write!(f, "{}us", d.as_micros())
        } else {
            write!(f, "{}ns", d.as_nanos())
        }
    }
}

text_type!(Duration, TypeDesc::Duration);

/// The unit of a [`Dimension`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Unit {
    Px,
    Em,
    Rem,
    Percent,
    Vw,
    Vh,
    Pt,
    Cm,
    Mm,
    In,
}

impl Unit {
    const ALL: [(&'static str, Unit); 10] = [
        ("px", Unit::Px),
        ("em", Unit::Em),
        ("rem", Unit::Rem),
        ("%", Unit::Percent),
        ("vw", Unit::Vw),
        ("vh", Unit::Vh),
        ("pt", Unit::Pt),
        ("cm", Unit::Cm),
        ("mm", Unit::Mm),
        ("in", Unit::In),
    ];

    /// The suffix as written.
    pub fn as_str(self) -> &'static str {
        Self::ALL.iter().find(|(_, u)| *u == self).map_or("", |(s, _)| s)
    }
}

/// A number with a unit: `120px`, `1.5rem`, `80%`. An owned, validated counterpart of
/// [`crate::Dimension`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dimension {
    pub value: f64,
    pub unit: Unit,
}

impl FromStr for Dimension {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let text = s.trim();
        let split = text
            .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+')))
            .ok_or_else(|| format!("`{s}` is not a dimension: missing unit (px, em, rem, %, vw, vh, pt, cm, mm, in)"))?;
        let (number, unit) = text.split_at(split);
        let value: f64 = number.parse().map_err(|_| format!("`{s}` is not a dimension: `{number}` is not a number"))?;
        let unit = Unit::ALL
            .iter()
            .find(|(name, _)| *name == unit)
            .map(|(_, u)| *u)
            .ok_or_else(|| format!("`{s}` is not a dimension: unknown unit `{unit}` (px, em, rem, %, vw, vh, pt, cm, mm, in)"))?;
        Ok(Self { value, unit })
    }
}

impl fmt::Display for Dimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.value, self.unit.as_str())
    }
}

text_type!(Dimension, TypeDesc::Dimension);

/// A filesystem path. Unlike a bare `PathBuf` it describes itself in schemas.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Path(pub std::path::PathBuf);

impl FromStr for Path {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        Ok(Self(std::path::PathBuf::from(s)))
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.display().fmt(f)
    }
}

impl std::ops::Deref for Path {
    type Target = std::path::Path;
    fn deref(&self) -> &std::path::Path {
        &self.0
    }
}

text_type!(Path, TypeDesc::Path);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_parse_in_every_notation_and_print_back() {
        assert_eq!("#f80".parse::<Color>().unwrap(), Color { r: 255, g: 136, b: 0, a: 255 });
        assert_eq!("#f808".parse::<Color>().unwrap().a, 0x88);
        assert_eq!("#ff8800".parse::<Color>().unwrap().to_u32(), 0xff8800ff);
        assert_eq!("#ff880080".parse::<Color>().unwrap().a, 0x80);
        assert_eq!(Color::rgb(255, 136, 0).to_string(), "#ff8800");
        assert_eq!("#ff880080".parse::<Color>().unwrap().to_string(), "#ff880080");
        assert!("ff8800".parse::<Color>().is_err());
        assert!("#ff88".parse::<Color>().is_ok());
        assert!("#ff880".parse::<Color>().unwrap_err().contains("5 hexadecimal digits"));
        assert!("#gg0000".parse::<Color>().is_err());
    }

    #[test]
    fn percentages_have_a_fraction() {
        assert_eq!("50%".parse::<Percentage>().unwrap().fraction(), 0.5);
        assert_eq!("12.5 %".parse::<Percentage>().unwrap(), Percentage(12.5));
        assert!("50".parse::<Percentage>().is_err());
        assert!("x%".parse::<Percentage>().is_err());
    }

    #[test]
    fn durations_use_units() {
        let ms = |s: &str| s.parse::<Duration>().unwrap().get().as_millis();
        assert_eq!(ms("250ms"), 250);
        assert_eq!(ms("1.5s"), 1500);
        assert_eq!(ms("2m"), 120_000);
        assert_eq!(ms("1h"), 3_600_000);
        assert_eq!("10us".parse::<Duration>().unwrap().get().as_micros(), 10);
        assert_eq!("250ms".parse::<Duration>().unwrap().to_string(), "250ms");
        assert_eq!("3s".parse::<Duration>().unwrap().to_string(), "3s");
        assert!("10".parse::<Duration>().unwrap_err().contains("missing unit"));
        assert!("10 parsecs".parse::<Duration>().unwrap_err().contains("unknown unit"));
    }

    #[test]
    fn dimensions_validate_the_unit() {
        assert_eq!("120px".parse::<Dimension>().unwrap(), Dimension { value: 120.0, unit: Unit::Px });
        assert_eq!("1.5rem".parse::<Dimension>().unwrap().unit, Unit::Rem);
        assert_eq!("80%".parse::<Dimension>().unwrap().to_string(), "80%");
        assert_eq!("-4.5pt".parse::<Dimension>().unwrap().value, -4.5);
        assert!("12".parse::<Dimension>().unwrap_err().contains("missing unit"));
        assert!("12furlongs".parse::<Dimension>().unwrap_err().contains("unknown unit"));
    }
}
