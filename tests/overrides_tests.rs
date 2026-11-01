#![cfg(feature = "derive")]
//! Overrides (environment, command line) on top of a typed config.

use rost::types::{Color, Dimension, Duration, Path, Percentage};
use rost::{Overrides, Rost};

#[derive(Rost, Debug, PartialEq)]
enum Mode {
    #[rost(rename = "dark-theme")]
    DarkTheme,
    #[rost(rename = "auto")]
    FollowSystem,
}

#[derive(Rost, Debug, PartialEq)]
struct Port(u16);

#[derive(Rost, Debug, PartialEq)]
struct Window {
    #[rost(range(min = 100, max = 4096))]
    width: u32,
    height: Option<u32>,
}

#[derive(Rost, Debug, PartialEq)]
struct Config {
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
fn overrides_replace_file_values_and_create_blocks() {
    let o = Overrides::new().set("window.width", "800").set("debug", "true").set("mode", "auto");
    let c: Config = rost::load_with(DOC, &o).unwrap();
    assert_eq!(c.window.width, 800);
    assert!(c.debug);
    assert_eq!(c.mode, Mode::FollowSystem);
    assert_eq!(c.port, Port(8080), "untouched values stay");
}

#[test]
fn an_invalid_override_is_blamed_on_the_override() {
    let o = Overrides::new().set("window.width", "50");
    let err = rost::load_with::<Config>(DOC, &o).unwrap_err();
    assert!(err.message().starts_with("invalid override (window.width): "), "{}", err.message());
    assert!(err.message().contains("50 is out of range"), "{}", err.message());
}

#[test]
fn a_broken_file_is_blamed_on_the_file_even_with_overrides() {
    let o = Overrides::new().set("debug", "true");
    let src = DOC.replace("width: 1280", "width: 5000");
    let err = rost::load_with::<Config>(&src, &o).unwrap_err();
    assert!(err.message().starts_with("3:10: field `window.width`"), "{}", err.message());
}

#[test]
fn environment_overrides_use_double_underscore_for_blocks() {
    let env = [("APP_WINDOW__WIDTH".to_string(), "640".to_string()), ("APP_PORT".to_string(), "9".to_string())];
    let c: Config = rost::load_with(DOC, &Overrides::from_pairs("APP", env)).unwrap();
    assert_eq!((c.window.width, c.port), (640, Port(9)));
}

#[test]
fn overrides_keep_the_rest_of_the_file_byte_for_byte() {
    let o = Overrides::new().set("port", "9");
    let patched = o.patch(DOC).unwrap().unwrap();
    assert_eq!(patched, DOC.replace("port: 8080", "port: 9"));
}

#[test]
fn command_line_settings_override_the_file() {
    let args = ["app", "--set", "window.width=800", "--set=mode=auto"].map(String::from);
    let c: Config = rost::load_with(DOC, &Overrides::from_args(args)).unwrap();
    assert_eq!((c.window.width, c.mode), (800, Mode::FollowSystem));
}

#[test]
fn json_style_documents_can_be_overridden_too() {
    #[derive(Rost, Debug)]
    struct S {
        a: u32,
        b: u32,
    }
    let s: S = rost::load_with("{\"a\": 1, \"b\": 2}", &Overrides::new().set("b", "7")).unwrap();
    assert_eq!((s.a, s.b), (1, 7));
}
