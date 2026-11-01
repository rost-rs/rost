#![allow(dead_code)]
use rost::Rost;

// Every struct you load into derives `Rost`. Field names are the keys of the file.
#[derive(Rost, Debug)]
struct Window {
    width: u32,
    height: u32,
}

#[derive(Rost, Debug)]
struct Config {
    title: String,
    scale: f64,
    fullscreen: bool,
    #[rost(default)] // may be missing from the file
    nickname: String,
    #[rost(serde)] // lists go through serde: add `serde = "1"` to your Cargo.toml
    tags: Vec<String>,
    window: Window, // a nested block
}

const FILE: &str = r#"
// Comments start with two slashes and go on their own line.
title: "My app"
scale: 1.25
fullscreen: false
tags: ["fast", "small"]

window {
    width: 1280
    height: 720
}
"#;

fn main() {
    let config: Config = rost::load(FILE).unwrap();
    assert_eq!(config.title, "My app");
    assert_eq!(config.window.width, 1280);
    assert_eq!(config.nickname, ""); // the default
    assert_eq!(config.tags, ["fast", "small"]);
    println!("{config:#?}");
}
