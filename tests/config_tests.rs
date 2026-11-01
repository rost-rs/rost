#![cfg(feature = "derive")]
//! Sprint 8: typed configuration. Derive for enums and newtypes, validation attributes with
//! positions, typed newtypes, flatten, schema generation, overrides and `load_file`.

use rost::types::{Color, Dimension, Duration, Path, Percentage};
use rost::{Describe, Rost};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Rost, Describe, Debug, PartialEq, Clone, Copy)]
#[rost(rename_all = "kebab-case")]
enum Mode {
    DarkTheme,
    LightTheme,
    #[rost(rename = "auto")]
    FollowSystem,
}

#[derive(Rost, Describe, Debug, PartialEq)]
struct Port(u16);

#[derive(Rost, Describe, Debug, PartialEq)]
struct Window {
    /// Width in pixels.
    #[rost(range(min = 100, max = 4096))]
    width: u32,
    #[rost(range(max = 2160), optional)]
    height: Option<u32>,
    #[rost(one_of("main", "tool", "popup"), default)]
    kind: String,
}

#[derive(Rost, Describe, Debug, PartialEq)]
struct Config {
    /// Color scheme.
    mode: Mode,
    port: Port,
    window: Window,
    accent: Color,
    opacity: Percentage,
    timeout: Duration,
    gap: Dimension,
    cache: Path,
    #[rost(default)]
    debug: bool,
}

const DOC: &str = "mode: dark-theme\nport: 8080\nwindow { width: 1280\n height: 720 }\naccent: #ff8800\nopacity: 80%\ntimeout: 1.5s\ngap: 12px\ncache: \"/tmp/cache\"\n";

#[test]
fn a_whole_config_loads_with_every_new_type() {
    let c: Config = rost::load(DOC).unwrap();
    assert_eq!(c.mode, Mode::DarkTheme);
    assert_eq!(c.port, Port(8080));
    assert_eq!(c.window, Window { width: 1280, height: Some(720), kind: String::new() });
    assert_eq!(c.accent, Color::rgb(255, 136, 0));
    assert_eq!(c.opacity.fraction(), 0.8);
    assert_eq!(c.timeout.get().as_millis(), 1500);
    assert_eq!(c.gap.to_string(), "12px");
    assert_eq!(c.cache.to_str(), Some("/tmp/cache"));
    assert!(!c.debug);
}

#[test]
fn enum_names_follow_rename_all_and_rename() {
    #[derive(Rost, Debug, PartialEq)]
    struct S {
        a: Mode,
        b: Mode,
    }
    let s: S = rost::load("a: light-theme\nb: auto\n").unwrap();
    assert_eq!((s.a, s.b), (Mode::LightTheme, Mode::FollowSystem));
    let err = rost::load::<S>("a: dark\nb: auto\n").unwrap_err();
    assert!(err.message().starts_with("1:1: field `a`: unknown variant `dark`: expected one of dark-theme, light-theme, auto"), "{}", err.message());
}

#[test]
fn data_enums_and_tuples_go_through_serde() {
    #[derive(Rost, Deserialize, Debug, PartialEq)]
    enum Shape {
        Circle { radius: f64 },
        Unit,
    }
    #[derive(Rost, Debug, PartialEq)]
    struct S {
        #[rost(serde)]
        pair: (i32, i32),
        shape: Shape,
    }
    let s: S = rost::load("pair [1, 2]\nshape { Circle { radius: 2.5 } }\n").unwrap();
    assert_eq!(s.pair, (1, 2));
    assert_eq!(s.shape, Shape::Circle { radius: 2.5 });
}

#[test]
fn out_of_range_values_point_at_their_key_with_the_field_path() {
    let src = DOC.replace("width: 1280", "width: 5000");
    let err = rost::load::<Config>(&src).unwrap_err();
    assert_eq!(err.message(), "3:10: field `window.width`: 5000 is out of range: expected 100 to 4096");
}

#[test]
fn optional_values_are_checked_only_when_present() {
    let src = DOC.replace("height: 720", "height: 9999");
    let err = rost::load::<Config>(&src).unwrap_err();
    assert!(err.message().contains("field `window.height`: 9999 is out of range: expected at most 2160"), "{}", err.message());
    assert!(rost::load::<Config>(&DOC.replace("\n height: 720", "")).is_ok());
}

#[test]
fn one_of_rejects_other_values() {
    let src = DOC.replace("window { width: 1280", "window { kind: dialog\n width: 1280");
    let err = rost::load::<Config>(&src).unwrap_err();
    assert!(err.message().contains("field `window.kind`: `dialog` is not allowed: expected one of main, tool, popup"), "{}", err.message());
    assert!(rost::load::<Config>(&DOC.replace("window { width: 1280", "window { kind: tool\n width: 1280")).is_ok());
}

#[test]
fn typed_newtypes_report_what_was_expected() {
    let err = rost::load::<Config>(&DOC.replace("#ff8800", "orange")).unwrap_err();
    assert!(err.message().contains("field `accent`") && err.message().contains("is not a color"), "{}", err.message());
    let err = rost::load::<Config>(&DOC.replace("1.5s", "1.5")).unwrap_err();
    assert!(err.message().contains("missing unit"), "{}", err.message());
    let err = rost::load::<Config>(&DOC.replace("12px", "12furlongs")).unwrap_err();
    assert!(err.message().contains("unknown unit"), "{}", err.message());
}

#[test]
fn missing_nested_field_has_a_full_path() {
    let err = rost::load::<Config>(&DOC.replace("width: 1280\n", "")).unwrap_err();
    assert!(err.message().contains("field `window.width`: missing"), "{}", err.message());
}

#[derive(Rost, Debug, PartialEq)]
struct Common {
    name: String,
    #[rost(default)]
    retries: u32,
}

#[derive(Rost, Describe, Debug, PartialEq)]
struct Service {
    port: u16,
    #[rost(flatten)]
    common: Common2,
}

#[derive(Rost, Describe, Debug, PartialEq)]
struct Common2 {
    name: String,
    #[rost(default)]
    retries: u32,
}

#[test]
fn flatten_merges_the_entries_of_a_nested_struct() {
    let s: Service = rost::load("name: api\nport: 80\nretries: 3\n").unwrap();
    assert_eq!(s, Service { port: 80, common: Common2 { name: "api".into(), retries: 3 } });
    let s: Service = rost::load("port: 80\nname: api\n").unwrap();
    assert_eq!(s.common.retries, 0);
    let s: Service = rost::load("svc { port: 80\n name: x }\n").map(|h: HashMap<String, Service>| h.into_iter().next().unwrap().1).unwrap();
    assert_eq!(s.common.name, "x");
    let err = rost::load::<Service>("port: 80\n").unwrap_err();
    assert!(err.message().contains("name"), "{}", err.message());
    let _ = Common { name: String::new(), retries: 0 };
}

#[test]
fn schema_lists_fields_ranges_docs_and_requirements() {
    let schema = Config::describe();
    let json = schema.to_json_schema();
    assert!(json.contains("\"$schema\": \"https://json-schema.org/draft/2020-12/schema\""), "{json}");
    assert!(json.contains("\"minimum\": 100") && json.contains("\"maximum\": 4096"), "{json}");
    assert!(json.contains("\"description\": \"Width in pixels.\""), "{json}");
    assert!(json.contains("\"enum\": [\"dark-theme\", \"light-theme\", \"auto\"]"), "{json}");
    assert!(json.contains("\"enum\": [\"main\", \"tool\", \"popup\"]"), "{json}");
    assert!(json.contains("\"required\": [\"mode\", \"port\", \"window\""), "{json}");
    assert!(!json.contains("\"debug\"\n") || json.contains("\"debug\": {"), "{json}");

    let rost_schema = schema.to_rost_schema();
    assert!(rost_schema.contains("# Color scheme.\nmode: enum(dark-theme | light-theme | auto)"), "{rost_schema}");
    assert!(rost_schema.contains("window {\n  # Width in pixels.\n  width: int(100..4096)\n  height?: int(0..2160)\n  kind?: string(main | tool | popup)\n}"), "{rost_schema}");
    assert!(rost_schema.contains("accent: color") && rost_schema.contains("timeout: duration"), "{rost_schema}");
}

#[test]
fn schema_of_flattened_fields_sits_in_the_parent() {
    let TypeDescAlias::Object(fields) = Service::describe() else { panic!() };
    let names: Vec<_> = fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["port", "name", "retries"]);
}

use rost::TypeDesc as TypeDescAlias;

#[test]
fn load_file_prefixes_errors_with_the_path() {
    let dir = std::env::temp_dir().join(format!("rost-config-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ok = dir.join("ok.rost");
    std::fs::write(&ok, DOC).unwrap();
    let c: Config = rost::load_file(&ok).unwrap();
    assert_eq!(c.port, Port(8080));

    let bad = dir.join("bad.rost");
    std::fs::write(&bad, DOC.replace("width: 1280", "width: 1")).unwrap();
    let err = rost::load_file::<Config>(&bad).unwrap_err();
    assert!(err.message().starts_with(&format!("{}:3:10: field `window.width`", bad.display())), "{}", err.message());

    let err = rost::load_file::<Config>(dir.join("missing.rost")).unwrap_err();
    assert!(err.message().contains("missing.rost"), "{}", err.message());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn path_and_color_roundtrip_through_serde_too() {
    #[derive(Deserialize, Debug)]
    struct S {
        c: Color,
        d: Duration,
    }
    let s: S = rost::from_str("c: #0f0\nd: 2m\n").unwrap();
    assert_eq!(s.c, Color::rgb(0, 255, 0));
    assert_eq!(s.d.get().as_secs(), 120);
}
