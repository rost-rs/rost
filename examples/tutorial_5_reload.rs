use std::time::{Duration, Instant};

use rost::{Reloader, Rost};

#[derive(Rost, Debug)]
struct Config {
    width: u32,
}

fn wait_until(mut done: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !done() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn main() {
    let path = std::env::temp_dir().join(format!("rost-tutorial-{}.rost", std::process::id()));
    std::fs::write(&path, "width: 800\n").unwrap();

    // One call: load, watch the file, and keep the value up to date.
    let live = Reloader::<Config>::watch_load(&path).unwrap();
    assert_eq!(live.get().width, 800);

    // Someone edits the file: the new value appears, no restart.
    std::fs::write(&path, "width: 1024\n").unwrap();
    wait_until(|| live.get().width == 1024);
    assert_eq!(live.get().width, 1024);

    // Someone saves a broken file: it is rejected, the last good value stays.
    std::fs::write(&path, "width: banana\n").unwrap();
    wait_until(|| live.last_error().is_some());
    assert_eq!(live.get().width, 1024);
    let error = live.last_error().unwrap();
    println!("rejected at {}:{}: {}", error.line.unwrap(), error.column.unwrap(), error.message);

    // Fixing the file recovers.
    std::fs::write(&path, "width: 640\n").unwrap();
    wait_until(|| live.get().width == 640);
    assert_eq!(live.get().width, 640);

    // Rules between fields: checked before a new value replaces the old one.
    let strict = Reloader::<Config>::builder(&path, |text| rost::load(text).map_err(rost::ReloadError::from_display))
        .validate(|c| if c.width % 2 == 0 { Ok(()) } else { Err("width must be even".into()) })
        .build()
        .unwrap();
    std::fs::write(&path, "width: 641\n").unwrap();
    wait_until(|| strict.last_error().is_some());
    assert_eq!(strict.get().width, 640);
    println!("rejected: {}", strict.last_error().unwrap().message);

    let _ = std::fs::remove_file(&path);
}
