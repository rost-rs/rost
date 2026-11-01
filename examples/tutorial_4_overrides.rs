use rost::{Overrides, Rost};

#[derive(Rost, Debug)]
struct Window {
    width: u32,
}

#[derive(Rost, Debug)]
struct Config {
    title: String,
    window: Window,
}

const FILE: &str = "// my settings\ntitle: \"My app\"\nwindow {\n    // pixels\n    width: 800\n}\n";

fn main() {
    // `--set window.width=1024` on the command line, or MYAPP_WINDOW__WIDTH=1024 in the environment.
    let overrides = Overrides::new().set("window.width", "1024").set("title", "Other");
    let config: Config = rost::load_with(FILE, &overrides).unwrap();
    assert_eq!((config.title.as_str(), config.window.width), ("Other", 1024));

    // An override patches the text: comments and layout are kept.
    let patched = overrides.patch(FILE).unwrap().unwrap();
    assert!(patched.contains("// my settings") && patched.contains("// pixels"));
    println!("{patched}");

    // In a real program:
    //     let overrides = Overrides::from_env("MYAPP").merge(Overrides::from_args(std::env::args()));
}
