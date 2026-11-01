//! A tiny "app" whose theme follows its file while it runs.
//!
//! ```text
//! cargo run --example live_theme --features derive,watch
//! ```
//!
//! Edit the printed file in another terminal: valid edits show up within milliseconds, broken
//! ones are reported with their line and the previous theme stays in use.

use std::time::Duration;

use rost::{Reloader, Rost};

#[derive(Rost, Debug)]
struct Theme {
    title: String,
    #[rost(default)]
    accent: String,
    width: u32,
}

fn main() -> Result<(), rost::ReloadError> {
    let path = std::env::temp_dir().join("live_theme.rost");
    std::fs::write(&path, "title: \"Hello\"\naccent: \"#ff8800\"\nwidth: 80\n").unwrap();
    println!("edit {} (Ctrl-C to quit)", path.display());

    let theme = Reloader::<Theme>::watch_load(&path)?;
    theme.on_error(|e| eprintln!("rejected: {e}"));
    theme.on_change(|t| println!("reloaded: {t:?}"));

    let mut shown = 0;
    loop {
        // The render loop reads a lock-free snapshot every frame.
        let t = theme.get();
        if theme.version() != shown {
            shown = theme.version();
            println!("frame uses {} / {} / {}", t.title, t.accent, t.width);
        }
        std::thread::sleep(Duration::from_millis(16));
    }
}
