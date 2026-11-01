#![cfg(all(feature = "derive", feature = "watch"))]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rost::{Reloader, Rost};

#[derive(Rost, Debug, PartialEq)]
struct Cfg {
    width: u32,
    #[rost(default)]
    height: u32,
    #[rost(default)]
    name: String,
}

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("rost-reload-{}-{}-{tag}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }
    fn file(&self, name: &str, text: &str) -> PathBuf {
        let p = self.0.join(name);
        fs::write(&p, text).unwrap();
        p
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const WAIT: Duration = Duration::from_secs(5);

fn until(mut cond: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + WAIT;
    while Instant::now() < end {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    cond()
}

fn write(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
}

#[test]
fn initial_load_and_invalid_initial_load() {
    let d = Dir::new("initial");
    let ok = d.file("a.rost", "width: 5\n");
    let r = Reloader::<Cfg>::watch_load(&ok).unwrap();
    assert_eq!(r.get().width, 5);
    assert_eq!(r.version(), 1);
    assert!(r.last_error().is_none());

    let bad = d.file("b.rost", "width: 5\nheight: nope\n");
    let err = Reloader::<Cfg>::watch_load(&bad).unwrap_err();
    assert_eq!(err.line, Some(2), "{err}");
    assert!(err.to_string().contains("b.rost"));

    let missing = d.0.join("missing.rost");
    assert!(Reloader::<Cfg>::watch_load(&missing).is_err());
}

#[test]
fn an_edit_reaches_the_value() {
    let d = Dir::new("edit");
    let p = d.file("a.rost", "width: 1\n");
    let r = Reloader::<Cfg>::watch_load(&p).unwrap();
    write(&p, "width: 2\n");
    assert!(r.wait_for_version(2, WAIT));
    assert_eq!(r.get().width, 2);
}

#[test]
fn a_broken_edit_keeps_the_last_good_value_and_reports_the_line() {
    let d = Dir::new("broken");
    let p = d.file("a.rost", "width: 1\n");
    let r = Reloader::<Cfg>::watch_load(&p).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    r.on_error(move |e| s.lock().unwrap().push(e.clone()));

    write(&p, "width: 2\nheight: oops\n");
    assert!(r.wait_for_errors(1, WAIT));
    assert_eq!(r.get().width, 1, "the running value is untouched");
    assert_eq!(r.version(), 1);
    let err = r.last_error().unwrap();
    assert_eq!(err.line, Some(2), "{err}");
    assert_eq!(seen.lock().unwrap().len(), 1);

    write(&p, "width: 3\nheight: 4\n");
    assert!(r.wait_for_version(2, WAIT));
    assert_eq!(*r.get(), Cfg { width: 3, height: 4, name: String::new() });
    assert!(r.last_error().is_none(), "a good edit clears the error");
}

#[test]
fn rename_saves_are_picked_up() {
    let d = Dir::new("rename");
    let p = d.file("a.rost", "width: 1\n");
    let r = Reloader::<Cfg>::watch_load(&p).unwrap();
    for w in 2..5 {
        let tmp = d.0.join(".a.rost.swp");
        fs::write(&tmp, format!("width: {w}\n")).unwrap();
        fs::rename(&tmp, &p).unwrap();
        assert!(until(|| r.get().width == w), "rename save {w} was missed");
    }
    assert!(r.last_error().is_none());
}

#[test]
fn rewriting_identical_content_does_not_reload() {
    let d = Dir::new("same");
    let p = d.file("a.rost", "width: 1\n");
    let r = Reloader::<Cfg>::watch_load(&p).unwrap();
    write(&p, "width: 1\n");
    r.reload_now();
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(r.version(), 1);
    write(&p, "width: 9\n");
    assert!(r.wait_for_version(2, WAIT));
    assert_eq!(r.version(), 2);
}

#[test]
fn change_callbacks_see_the_new_value() {
    let d = Dir::new("cb");
    let p = d.file("a.rost", "width: 1\n");
    let r = Reloader::<Cfg>::watch_load(&p).unwrap();
    let widths = Arc::new(Mutex::new(Vec::new()));
    let w = widths.clone();
    r.on_change(move |c| w.lock().unwrap().push(c.width));
    write(&p, "width: 7\n");
    assert!(until(|| widths.lock().unwrap().as_slice() == [7]));
}

#[test]
fn path_callbacks_fire_only_for_the_paths_that_changed() {
    let d = Dir::new("paths");
    let p = d.file("a.rost", "width: 1\nheight: 2\nname: \"x\"\n");
    let r = Reloader::<Cfg>::watch_load(&p).unwrap();
    let width = Arc::new(Mutex::new(Vec::new()));
    let name = Arc::new(AtomicUsize::new(0));
    let (w, n) = (width.clone(), name.clone());
    r.on_change_path("width", move |c| w.lock().unwrap().push((c.old.clone(), c.new.clone())));
    r.on_change_path("name", move |_| {
        n.fetch_add(1, Ordering::SeqCst);
    });

    write(&p, "width: 10\nheight: 2\nname: \"x\"\n");
    assert!(r.wait_for_version(2, WAIT));
    assert!(until(|| width.lock().unwrap().len() == 1));
    assert_eq!(name.load(Ordering::SeqCst), 0, "name did not change");
    let (old, new) = width.lock().unwrap()[0].clone();
    assert_eq!(old, Some(rost::Value::Int(1)));
    assert_eq!(new, Some(rost::Value::Int(10)));

    write(&p, "width: 10\nheight: 3\nname: \"y\"\n");
    assert!(r.wait_for_version(3, WAIT));
    assert!(until(|| name.load(Ordering::SeqCst) == 1));
    assert_eq!(width.lock().unwrap().len(), 1, "width did not change the second time");
}

#[test]
fn a_dependency_reloads_the_main_value() {
    let d = Dir::new("deps");
    let main = d.file("main.rost", "width: 1\n");
    let palette = d.file("palette.rost", "x: 1\n");
    let pal = palette.clone();
    let r = Reloader::<Cfg>::builder(&main, move |text| {
        let mut cfg: Cfg = rost::load(text).map_err(rost::ReloadError::from_display)?;
        // The loader reads the dependency itself; the builder only watches it.
        let extra = fs::read_to_string(&pal).unwrap();
        cfg.height = extra.trim().trim_start_matches("x:").trim().parse().unwrap_or(0);
        Ok(cfg)
    })
    .depends_on(&palette)
    .build()
    .unwrap();
    assert_eq!(r.get().height, 1);
    write(&palette, "x: 42\n");
    assert!(r.wait_for_version(2, WAIT));
    assert_eq!(r.get().height, 42);
}

#[test]
fn a_panicking_loader_is_an_error_not_a_dead_reloader() {
    let d = Dir::new("panic");
    let p = d.file("a.rost", "width: 1\n");
    let r = Reloader::<u32>::watch(&p, |text| {
        if text.contains("boom") {
            panic!("loader bug");
        }
        Ok(text.len() as u32)
    })
    .unwrap();
    write(&p, "boom\n");
    assert!(r.wait_for_errors(1, WAIT));
    assert!(r.last_error().unwrap().message.contains("panicked"));
    write(&p, "width: 123456\n");
    assert!(r.wait_for_version(2, WAIT), "the dispatcher survived the panic");
    assert_eq!(*r.get(), 14);
}

#[test]
fn a_panicking_callback_does_not_stop_the_others() {
    let d = Dir::new("cbpanic");
    let p = d.file("a.rost", "width: 1\n");
    let r = Reloader::<Cfg>::watch_load(&p).unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    r.on_change(|_| panic!("bad callback"));
    let h = hits.clone();
    r.on_change(move |_| {
        h.fetch_add(1, Ordering::SeqCst);
    });
    write(&p, "width: 2\n");
    assert!(until(|| hits.load(Ordering::SeqCst) == 1));
    write(&p, "width: 3\n");
    assert!(until(|| hits.load(Ordering::SeqCst) == 2));
}

#[test]
fn ten_saves_in_a_second_with_three_broken_end_on_the_last_valid_one() {
    let d = Dir::new("burst");
    let p = d.file("a.rost", "width: 1\nheight: 1\n");
    let r = Reloader::<Cfg>::watch_load(&p).unwrap();

    // Readers hammer the value; width and height always move together, so a torn read
    // would show as width != height.
    let stop = Arc::new(AtomicBool::new(false));
    let torn = Arc::new(AtomicUsize::new(0));
    let readers: Vec<_> = (0..4)
        .map(|_| {
            let (r, stop, torn) = (r.clone(), stop.clone(), torn.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let c = r.get();
                    if c.width != c.height {
                        torn.fetch_add(1, Ordering::Relaxed);
                    }
                }
            })
        })
        .collect();

    for i in 2..=11u32 {
        let text = match i {
            4 | 7 | 9 => format!("width: {i}\nheight: ???\n"),
            _ => format!("width: {i}\nheight: {i}\n"),
        };
        write(&p, &text);
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(until(|| r.get().width == 11), "ended on width {}", r.get().width);
    stop.store(true, Ordering::Relaxed);
    for t in readers {
        t.join().unwrap();
    }
    assert_eq!(torn.load(Ordering::Relaxed), 0);
    assert!(r.last_error().is_none());
    assert_eq!(r.get().height, 11);
}

#[test]
fn rapid_writes_collapse_but_the_final_state_wins() {
    let d = Dir::new("collapse");
    let p = d.file("a.rost", "width: 0\n");
    let r = Reloader::<Cfg>::watch_load(&p).unwrap();
    for i in 1..=200u32 {
        write(&p, &format!("width: {i}\n"));
    }
    assert!(until(|| r.get().width == 200));
    assert!(r.version() < 201, "the burst was debounced (version {})", r.version());
}

#[test]
fn a_thousand_files_with_rapid_saves_all_end_on_their_last_write() {
    let d = Dir::new("many");
    let files: Vec<_> = (0..1000).map(|i| d.file(&format!("f{i}.rost"), "width: 0\n")).collect();
    let reloaders: Vec<_> = files.iter().map(|p| Reloader::<Cfg>::watch_load(p).unwrap()).collect();
    for round in 1..=3u32 {
        for (i, p) in files.iter().enumerate() {
            write(p, &format!("width: {}\n", round * 1000 + i as u32));
        }
    }
    let ok = until(|| reloaders.iter().enumerate().all(|(i, r)| r.get().width == 3000 + i as u32));
    let stale = reloaders.iter().enumerate().filter(|(i, r)| r.get().width != 3000 + *i as u32).count();
    assert!(ok, "{stale} of 1000 reloaders missed their final state");
}

#[test]
fn reload_latency_is_small() {
    let d = Dir::new("latency");
    let p = d.file("a.rost", "width: 0\n");
    let r = Reloader::<Cfg>::watch_load(&p).unwrap();
    let mut samples = Vec::new();
    for i in 1..=20u32 {
        let start = Instant::now();
        write(&p, &format!("width: {i}\n"));
        assert!(until(|| r.get().width == i));
        samples.push(start.elapsed());
        std::thread::sleep(Duration::from_millis(30));
    }
    samples.sort();
    let median = samples[samples.len() / 2];
    eprintln!("reload latency: median {median:?}, worst {:?}", samples.last().unwrap());
    assert!(median < Duration::from_millis(150), "median {median:?}");
}

#[test]
fn dropping_a_reloader_does_not_disturb_the_others() {
    let d = Dir::new("drop");
    let a = d.file("a.rost", "width: 1\n");
    let b = d.file("b.rost", "width: 1\n");
    let ra = Reloader::<Cfg>::watch_load(&a).unwrap();
    let rb = Reloader::<Cfg>::watch_load(&b).unwrap();
    drop(ra);
    write(&a, "width: 2\n");
    write(&b, "width: 2\n");
    assert!(rb.wait_for_version(2, WAIT));
}

#[test]
fn serde_entry_point_works_too() {
    let d = Dir::new("serde");
    let p = d.file("a.rost", "width: 1\n");
    #[derive(serde::Deserialize, Debug)]
    struct S {
        width: u32,
    }
    let r = Reloader::<S>::watch_serde(&p).unwrap();
    write(&p, "width: 8\n");
    assert!(r.wait_for_version(2, WAIT));
    assert_eq!(r.get().width, 8);
}

#[derive(Rost, Debug, PartialEq)]
struct Checked {
    #[rost(range(min = 1, max = 10))]
    level: u32,
}

#[test]
fn a_value_outside_its_range_is_a_rejected_edit() {
    let d = Dir::new("range");
    let p = d.file("a.rost", "level: 3\n");
    let r = Reloader::<Checked>::watch_load(&p).unwrap();
    write(&p, "level: 99\n");
    assert!(r.wait_for_errors(1, WAIT));
    assert_eq!(r.get().level, 3, "the running value is untouched");
    let err = r.last_error().unwrap();
    assert_eq!(err.line, Some(1));
    assert!(err.message.contains("field `level`: 99 is out of range: expected 1 to 10"), "{err}");
    write(&p, "level: 7\n");
    assert!(r.wait_for_version(2, WAIT));
    assert_eq!(r.get().level, 7);
}

#[test]
fn a_cross_field_check_rejects_an_edit_like_any_other_error() {
    #[derive(Rost, Debug)]
    struct Pair {
        low: u32,
        high: u32,
    }
    let d = Dir::new("validate");
    let p = d.file("a.rost", "low: 1\nhigh: 5\n");
    let r = Reloader::<Pair>::builder(&p, |t| rost::load(t).map_err(rost::ReloadError::from_display))
        .validate(|v| if v.low <= v.high { Ok(()) } else { Err(format!("low ({}) is above high ({})", v.low, v.high)) })
        .build()
        .unwrap();
    write(&p, "low: 9\nhigh: 5\n");
    assert!(r.wait_for_errors(1, WAIT));
    assert_eq!(r.get().low, 1);
    assert!(r.last_error().unwrap().message.contains("low (9) is above high (5)"));
    write(&p, "low: 2\nhigh: 5\n");
    assert!(r.wait_for_version(2, WAIT));
    assert_eq!(r.get().low, 2);
}

#[test]
fn overrides_survive_reloads() {
    let d = Dir::new("overrides");
    let p = d.file("a.rost", "level: 3\n");
    let r = Reloader::<Checked>::watch_load_with(&p, rost::Overrides::new().set("level", "8")).unwrap();
    assert_eq!(r.get().level, 8);
    write(&p, "level: 2\nextra: 1\n");
    r.reload_now();
    assert_eq!(r.get().level, 8, "the override still wins after the file changed");
}
