//! The examples of the `serde_json` README, ported by renaming `serde_json` to `rost`
//! (and `json!` to `rost!`). The data is written in `.rost` instead of JSON; the Rust
//! code is otherwise unchanged.
#![cfg(feature = "serde")]

use rost::{rost, Result, Value};
use serde::{Deserialize, Serialize};

fn untyped_example() -> Result<String> {
    // Some .rost input data as a &str. Maybe this comes from the user.
    let data = r#"
        name: "John Doe"
        age: 43
        phones [
            "+44 1234567",
            "+44 2345678"
        ]
    "#;

    // Parse the string of data into rost::Value.
    let v: Value = rost::from_str(data)?;

    // Access parts of the data by indexing with square brackets.
    Ok(format!("Please call {} at the number {}", v["name"], v["phones"][0]))
}

#[derive(Serialize, Deserialize)]
struct Person {
    name: String,
    age: u8,
    phones: Vec<String>,
}

fn typed_example() -> Result<String> {
    let data = r#"
        name: "John Doe"
        age: 43
        phones [
            "+44 1234567",
            "+44 2345678"
        ]
    "#;

    // Parse the string of data into a Person object. This is exactly the
    // same function as the one that produced rost::Value above, but
    // now we are asking it for a Person as output.
    let p: Person = rost::from_str(data)?;

    // Do things just like with any other Rust data structure.
    Ok(format!("Please call {} at the number {}", p.name, p.phones[0]))
}

fn macro_example() -> (String, String) {
    let full_name = "John Doe";
    let age_last_year = 42;

    // The type of `john` is `rost::Value`
    let john = rost!({
        "name": full_name,
        "age": age_last_year + 1,
        "phones": [
            format!("+44 {}", 1234567)
        ]
    });

    (format!("first phone number: {}", john["phones"][0]), john.to_string())
}

#[derive(Serialize, Deserialize)]
struct Address {
    street: String,
    city: String,
}

fn print_an_address() -> Result<String> {
    // Some data structure.
    let address = Address {
        street: "10 Downing Street".to_owned(),
        city: "London".to_owned(),
    };

    // Serialize it to a .rost string.
    let j = rost::to_string(&address)?;
    Ok(j)
}

#[test]
fn readme_examples_port_by_renaming() {
    assert_eq!(untyped_example().unwrap(), "Please call \"John Doe\" at the number \"+44 1234567\"");
    assert_eq!(typed_example().unwrap(), "Please call John Doe at the number +44 1234567");
    let (first, whole) = macro_example();
    assert_eq!(first, "first phone number: \"+44 1234567\"");
    assert_eq!(whole, "name: \"John Doe\", age: 43, phones [\"+44 1234567\"]");
    assert_eq!(print_an_address().unwrap(), "street: \"10 Downing Street\", city: \"London\"");
}
