use rost::types::{Color, Dimension, Duration, Percentage};
use rost::Rost;

// A closed set of words becomes an enum. `rename_all` picks how the words are spelled.
#[derive(Rost, Debug, PartialEq)]
#[rost(rename_all = "lowercase")]
enum Mode {
    Dark,
    Light,
}

#[derive(Rost, Debug)]
struct Style {
    mode: Mode,
    accent: Color,       // #rgb or #rrggbb
    timeout: Duration,   // 250ms, 1.5s, 2m
    opacity: Percentage, // 80%
    gap: Dimension,      // 12px, 1.5rem
}

fn main() {
    let style: Style = rost::load("mode: dark\naccent: #ff8800\ntimeout: 1.5s\nopacity: 80%\ngap: 12px\n").unwrap();
    assert_eq!(style.mode, Mode::Dark);
    assert_eq!((style.accent.r, style.accent.g, style.accent.b), (255, 136, 0));
    assert_eq!(style.timeout.get().as_millis(), 1500);
    assert_eq!(style.opacity.fraction(), 0.8);
    assert_eq!(style.gap.to_string(), "12px");

    // Each type explains itself when the value is wrong.
    let err = rost::load::<Style>("mode: dark\naccent: salmon\ntimeout: 1s\nopacity: 80%\ngap: 1px\n").unwrap_err();
    println!("{err}");
}
