#![allow(dead_code)]
use rost::Rost;

#[derive(Rost, Debug)]
struct Window {
    #[rost(range(min = 100, max = 4096))]
    width: u32,
    #[rost(one_of("main", "tool", "popup"), default)]
    kind: String,
}

fn main() {
    let ok: Window = rost::load("width: 800\nkind: tool\n").unwrap();
    assert_eq!(ok.width, 800);

    // Errors carry line, column and the field.
    let err = rost::load::<Window>("width: 5000\n").unwrap_err();
    assert_eq!(err.message(), "1:1: field `width`: 5000 is out of range: expected 100 to 4096");
    println!("{err}");

    let err = rost::load::<Window>("width: 800\nkind: dialog\n").unwrap_err();
    println!("{err}");
}
