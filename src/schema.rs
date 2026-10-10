//! # Schema Mapping and Anti-Boilerplate Macros for `.rost`
//!
//! Provides declarative macros to hydrate typed Rust structs from `.rost` templates
//! with zero boilerplate, full compile-time typing, and automatic fallbacks.

#[macro_export]
#[doc(hidden)]
macro_rules! __rost_type {
    (hex) => { u32 };
    (dim) => { f32 };
    (str) => { String };
    (i64) => { i64 };
    (f64) => { f64 };
    (bool) => { bool };
}

#[macro_export]
#[doc(hidden)]
macro_rules! __rost_extract {
    ($t:expr, $path:expr, hex, $default:expr) => {
        $t.query_hex($path).unwrap_or($default)
    };
    ($t:expr, $path:expr, dim, $default:expr) => {
        $t.query_dim($path).unwrap_or($default)
    };
    ($t:expr, $path:expr, str, $default:expr) => {
        $t.query_str($path)
            .map(|s| s.to_string())
            .unwrap_or_else(|| ($default).to_string())
    };
    ($t:expr, $path:expr, i64, $default:expr) => {
        $t.query_i64($path).unwrap_or($default)
    };
    ($t:expr, $path:expr, f64, $default:expr) => {
        $t.query_f64($path).unwrap_or($default)
    };
    ($t:expr, $path:expr, bool, $default:expr) => {
        $t.query_bool($path).unwrap_or($default)
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! __rost_extract_block {
    ($b:expr, $name:ident, hex, $default:expr) => {
        $b.get_hex(stringify!($name)).unwrap_or($default)
    };
    ($b:expr, $name:ident, dim, $default:expr) => {
        $b.get_dim(stringify!($name)).unwrap_or($default)
    };
    ($b:expr, $name:ident, str, $default:expr) => {
        $b.get_str(stringify!($name))
            .map(|s| s.to_string())
            .unwrap_or_else(|| ($default).to_string())
    };
    ($b:expr, $name:ident, i64, $default:expr) => {
        $b.get_i64(stringify!($name)).unwrap_or($default)
    };
    ($b:expr, $name:ident, f64, $default:expr) => {
        $b.get_f64(stringify!($name)).unwrap_or($default)
    };
    ($b:expr, $name:ident, bool, $default:expr) => {
        $b.get_bool(stringify!($name)).unwrap_or($default)
    };
}

/// Generates a typed schema struct mapped across multiple blocks or properties in a `.rost` document.
///
/// # Example
///
/// ```rust
/// use rost::{rost_schema, Template};
///
/// rost_schema! {
///     #[derive(Debug, Clone, PartialEq)]
///     pub struct AppTheme {
///         "palette.bg_window"     => bg_window: hex = 0x1e1e2e,
///         "card.border_radius"    => card_radius: dim = 12.0,
///         "header.title"          => title: str = "Default Title",
///     }
/// }
///
/// let doc = "#palette { bg_window: #11111b }\n#card { border_radius: 16px }";
/// let template = Template::from_str(doc);
/// let theme = AppTheme::from_template(&template);
/// assert_eq!(theme.bg_window, 0x11111b);
/// assert_eq!(theme.card_radius, 16.0);
/// assert_eq!(theme.title, "Default Title");
/// ```
#[macro_export]
macro_rules! rost_schema {
    (
        $(#[$meta:meta])*
        $vis:vis struct $struct_name:ident {
            $(
                $path:literal => $field_name:ident : $type:ident = $default:expr
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis struct $struct_name {
            $(
                pub $field_name: $crate::__rost_type!($type),
            )*
        }

        impl $struct_name {
            /// Instantiates the schema directly from a parsed [Template], resolving all paths
            /// and falling back safely to defaults for missing fields.
            pub fn from_template(template: &$crate::Template) -> Self {
                Self {
                    $(
                        $field_name: $crate::__rost_extract!(template, $path, $type, $default),
                    )*
                }
            }
        }
    };
}

/// Generates a typed schema struct scoped to a single block (e.g. `#palette { ... }`).
///
/// Field names inside the struct automatically correspond to property names inside the block.
///
/// # Example
///
/// ```rust
/// use rost::{rost_block_schema, Template};
///
/// rost_block_schema! {
///     #[derive(Debug, Clone, PartialEq)]
///     pub struct Palette {
///         bg_window: hex = 0x1e1e2e,
///         accent: hex = 0xa6e3a1,
///     }
/// }
///
/// let doc = "#palette { bg_window: #282a36, accent: #50fa7b }";
/// let template = Template::from_str(doc);
/// let palette = Palette::from_template(&template, "palette");
/// assert_eq!(palette.bg_window, 0x282a36);
/// assert_eq!(palette.accent, 0x50fa7b);
/// ```
#[macro_export]
macro_rules! rost_block_schema {
    (
        $(#[$meta:meta])*
        $vis:vis struct $struct_name:ident {
            $(
                $field_name:ident : $type:ident = $default:expr
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis struct $struct_name {
            $(
                pub $field_name: $crate::__rost_type!($type),
            )*
        }

        impl $struct_name {
            /// Hydrates the struct from an individual [BlockView].
            pub fn from_block(block: &$crate::BlockView<'_>) -> Self {
                Self {
                    $(
                        $field_name: $crate::__rost_extract_block!(block, $field_name, $type, $default),
                    )*
                }
            }

            /// Hydrates the struct from a [Template] by looking up the specified block selector.
            pub fn from_template(template: &$crate::Template, selector: &str) -> Self {
                if let Some(block) = template.find_block(selector) {
                    Self::from_block(&block)
                } else {
                    Self {
                        $(
                            $field_name: $default,
                        )*
                    }
                }
            }
        }
    };
}
