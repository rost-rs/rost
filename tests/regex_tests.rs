#![cfg(all(feature = "derive", feature = "regex"))]

use rost::{Describe, Rost};

#[derive(Rost, Describe, Debug, PartialEq)]
struct User {
    #[rost(regex = "^[a-z][a-z0-9_]{2,15}$")]
    name: String,
    #[rost(regex = "^v[0-9]+$", optional)]
    api: Option<String>,
}

#[test]
fn a_matching_value_loads() {
    let u: User = rost::load("name: rodrigo_1\napi: v2\n").unwrap();
    assert_eq!(u, User { name: "rodrigo_1".into(), api: Some("v2".into()) });
}

#[test]
fn a_non_matching_value_names_the_pattern_and_the_line() {
    let err = rost::load::<User>("api: v1\nname: \"Bad Name\"\n").unwrap_err();
    assert_eq!(err.message(), "2:1: field `name`: `Bad Name` does not match `^[a-z][a-z0-9_]{2,15}$`");
    let err = rost::load::<User>("name: abc\napi: x1\n").unwrap_err();
    assert!(err.message().starts_with("2:1: field `api`: `x1` does not match"), "{}", err.message());
}

#[test]
fn the_schema_carries_the_pattern() {
    let json = User::describe().to_json_schema();
    assert!(json.contains("\"pattern\": \"^[a-z][a-z0-9_]{2,15}$\""), "{json}");
}
