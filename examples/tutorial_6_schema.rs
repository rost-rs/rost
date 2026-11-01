#![allow(dead_code)]
use rost::{Describe, Rost};

#[derive(Rost, Describe)]
struct Window {
    /// Width in pixels.
    #[rost(range(min = 100, max = 4096))]
    width: u32,
}

#[derive(Rost, Describe)]
struct Config {
    /// Shown in the title bar.
    title: String,
    window: Window,
}

fn main() {
    // A JSON Schema, so editors can autocomplete and check your .rost files...
    let schema = Config::describe().to_json_schema();
    assert!(schema.contains("Width in pixels."));
    println!("{schema}");
    // ...and a readable one.
    println!("{}", Config::describe().to_rost_schema());
}
