use rost::{Dimension, RostBuilder, Template};
use std::time::Instant;

#[test]
fn test_css_ui_styling_scope() {
    let raw = r#"
    @theme "dark-nebula"

    #button.primary {
        width: 120px,
        height: 40px,
        bg: #1e1e2e,
        opacity: 0.95,
        state | idle, hover, active |,
        on_click = emit_event(id: 1)
    }
    "#;

    let template = Template::from_str(raw);

    // Verify directive
    assert_eq!(template.get_directive("theme"), Some("dark-nebula"));

    // Verify block extraction
    let blocks = template.blocks();
    assert_eq!(blocks.len(), 1);
    let block = &blocks[0];
    assert_eq!(block.name, "button.primary");

    // Scoped property querying
    let width_field = block.get("width").expect("width property");
    assert_eq!(width_field.as_dimension(), Some(Dimension { value: 120.0, unit: "px" }));

    let bg_field = block.get("bg").expect("bg property");
    assert_eq!(bg_field.as_hex_color(), Some(0x1e1e2e));

    let opacity_field = block.get("opacity").expect("opacity property");
    assert_eq!(opacity_field.as_f64(), Some(0.95));

    let state_field = block.get("state").expect("state property");
    assert_eq!(state_field.enum_variants(), vec!["idle", "hover", "active"]);

    let click_field = block.get("on_click").expect("on_click binding");
    assert_eq!(click_field.value(), Some("emit_event(id: 1)"));
}

#[test]
fn test_game_engine_config_scope() {
    let raw = r#"
    #PlayerEntity
    name: "Valkyrie",
    level: 42,
    speed: 350.5,
    is_hero: true,
    inventory [ "spear", "shield", "elixir" ],
    difficulty | easy, normal, nightmare |,
    tick_fn = ecs_system_update()
    "#;

    let template = Template::from_str(raw);

    assert_eq!(template.get_value("name"), Some("Valkyrie"));
    assert_eq!(template.get("level").unwrap().as_i64(), Some(42));
    assert_eq!(template.get("speed").unwrap().as_f64(), Some(350.5));
    assert_eq!(template.get("is_hero").unwrap().as_bool(), Some(true));

    let inv = template.get("inventory").unwrap().list_items();
    assert_eq!(inv, vec!["spear", "shield", "elixir"]);

    let diff = template.get("difficulty").unwrap().enum_variants();
    assert_eq!(diff, vec!["easy", "normal", "nightmare"]);
}

#[test]
fn test_linters_and_tooling_scope() {
    let raw = r#"
    // Linter AST pattern rule
    @rule "no-console-log"
    pattern: "console.log($$$args)",
    severity | warning, error |,
    message: "Avoid stray console output",
    fix = remove_ast_node()
    "#;

    let template = Template::from_str(raw);

    assert_eq!(template.get_directive("rule"), Some("no-console-log"));
    assert_eq!(template.get_value("pattern"), Some("console.log($$$args)"));
    assert_eq!(template.get_value("message"), Some("Avoid stray console output"));

    let sev = template.get("severity").unwrap().enum_variants();
    assert_eq!(sev, vec!["warning", "error"]);
}

#[test]
fn test_malleable_builder_and_patching() {
    // 1. Programmatic building
    let mut builder = RostBuilder::new();
    builder
        .directive("app", "game-client")
        .property("resolution", "1920x1080")
        .property("volume", "0.80")
        .enumeration("modes", &["fullscreen", "windowed"]);
    let template = builder.build();

    assert_eq!(template.get_directive("app"), Some("game-client"));
    assert_eq!(template.get_value("resolution"), Some("1920x1080"));
    assert_eq!(template.get_value("volume"), Some("0.80"));

    // 2. Same-length zero-copy patch
    let mut template_mut = template.clone();
    let patched_same_len = template_mut.patch_value("volume", "0.95");
    assert!(patched_same_len);
    assert_eq!(template_mut.get_value("volume"), Some("0.95"));

    // 3. Different length patch (re-indexing)
    let patched_diff_len = template_mut.patch_value("volume", "1.0");
    assert!(patched_diff_len);
    assert_eq!(template_mut.get_value("volume"), Some("1.0"));
}

#[test]
fn test_high_throughput_speed() {
    // Generate a payload of ~70,000 tokens / repetitions
    let mut payload = String::with_capacity(1_000_000);
    payload.push_str("@theme \"dark-nebula\"\n");
    for i in 0..5_000 {
        payload.push_str(&format!(
            "#component_{i} {{\n    width: 120px,\n    height: 40px,\n    color: #1e1e2e,\n    state | idle, hover |,\n    action = dispatch({i})\n}}\n"
        ));
    }

    let bytes = payload.as_bytes();
    let size_mb = bytes.len() as f64 / (1024.0 * 1024.0);

    let start = Instant::now();
    let template = Template::from_bytes(bytes.to_vec()).expect("valid utf-8");
    let elapsed = start.elapsed();

    let fields_count = template.iter().count();
    let throughput_mb_s = size_mb / elapsed.as_secs_f64();

    println!(
        "\n[BENCHMARK] Parsed {:.2} MB ({} fields) in {:.2?}. Throughput: {:.2} MB/s",
        size_mb, fields_count, elapsed, throughput_mb_s
    );

    assert!(fields_count > 10_000);
    let min_expected = if cfg!(debug_assertions) { 10.0 } else { 150.0 };
    assert!(throughput_mb_s >= min_expected);
}

#[test]
fn test_schema_and_queries() {
    use rost::{rost_block_schema, rost_schema};

    rost_schema! {
        #[derive(Debug, PartialEq)]
        pub struct MyFullTheme {
            "palette.bg_window" => bg_window: hex = 0x000000,
            "palette.accent"    => accent: hex = 0xffffff,
            "card.border_radius" => card_radius: dim = 8.0,
            "header.title"      => title: str = "Fallback Title",
            "missing.prop"      => missing: i64 = 999,
        }
    }

    rost_block_schema! {
        #[derive(Debug, PartialEq)]
        pub struct PaletteBlock {
            bg_window: hex = 0x111111,
            accent: hex = 0x222222,
        }
    }

    let doc = r#"
    #palette {
        bg_window: #1e1e2e,
        accent: #a6e3a1
    }
    #card {
        border_radius: 14.5px
    }
    #header {
        title: "Hello Rost"
    }
    "#;

    let template = Template::from_str(doc);

    // Test direct query methods
    assert_eq!(template.query_hex("palette.bg_window"), Some(0x1e1e2e));
    assert_eq!(template.query_hex("palette.accent"), Some(0xa6e3a1));
    assert_eq!(template.query_dim("card.border_radius"), Some(14.5));
    assert_eq!(template.query_str("header.title"), Some("Hello Rost"));
    assert_eq!(template.query_str("nonexistent"), None);

    // Test rost_schema! hydration
    let theme = MyFullTheme::from_template(&template);
    assert_eq!(theme.bg_window, 0x1e1e2e);
    assert_eq!(theme.accent, 0xa6e3a1);
    assert_eq!(theme.card_radius, 14.5);
    assert_eq!(theme.title, "Hello Rost");
    assert_eq!(theme.missing, 999);

    // Test rost_block_schema! hydration
    let pal = PaletteBlock::from_template(&template, "palette");
    assert_eq!(pal.bg_window, 0x1e1e2e);
    assert_eq!(pal.accent, 0xa6e3a1);
}

#[test]
fn test_hot_reloading_watcher() {
    use std::fs;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join(format!("rost_test_hot_reload_{}.rost", std::process::id()));

    fs::write(&temp_file, "#window { width: 500px }").unwrap();

    let mut template = Template::load_watched(&temp_file).expect("load with hot reload");
    assert!(template.is_hot_reload_enabled());
    assert_eq!(template.query_dim("window.width"), Some(500.0));

    let reloaded_flag = Arc::new(AtomicBool::new(false));
    let flag_clone = reloaded_flag.clone();
    template.on_reload(move |new_tmpl| {
        if new_tmpl.query_dim("window.width") == Some(950.0) {
            flag_clone.store(true, Ordering::Relaxed);
        }
    });

    let rx = template.subscribe().expect("channel subscriber");

    // Give watcher thread a tiny moment to initiate
    thread::sleep(Duration::from_millis(50));

    // Overwrite file
    fs::write(&temp_file, "#window { width: 950px }").unwrap();

    // Check if channel received within 500ms
    let received = rx.recv_timeout(Duration::from_millis(800));
    assert!(received.is_ok(), "Should have received hot reload event");
    let updated = received.unwrap();
    assert_eq!(updated.query_dim("window.width"), Some(950.0));

    // Check check_reload polling
    assert!(template.check_reload());
    assert_eq!(template.query_dim("window.width"), Some(950.0));
    assert!(reloaded_flag.load(Ordering::Relaxed));

    // Clean up
    let _ = fs::remove_file(temp_file);
}

#[test]
fn test_declarative_action_bindings() {
    let doc = r#"
    #player_controls {
        on_jump = jump_action(velocity: 15.0),
        on_boost = activate_nitro(force: 1000.0)
    }
    on_global_pause = pause_game()
    "#;

    let template = Template::from_str(doc);

    // Query binding from scoped block
    assert_eq!(
        template.query_binding("player_controls.on_jump"),
        Some("jump_action(velocity: 15.0)")
    );
    assert_eq!(
        template.query_binding("player_controls.on_boost"),
        Some("activate_nitro(force: 1000.0)")
    );

    // Query top-level binding
    assert_eq!(
        template.query_binding("on_global_pause"),
        Some("pause_game()")
    );

    // Scoped block getter
    let block = template.find_block("player_controls").unwrap();
    assert_eq!(
        block.get_binding("on_jump"),
        Some("jump_action(velocity: 15.0)")
    );
    assert_eq!(block.get_binding("nonexistent"), None);
}

#[test]
fn test_multi_file_scaling_central_watcher() {
    use std::fs;
    use std::time::Duration;
    use rost::CentralWatcher;

    let temp_dir = std::env::temp_dir();
    let file1 = temp_dir.join(format!("rost_multi_1_{}.rost", std::process::id()));
    let file2 = temp_dir.join(format!("rost_multi_2_{}.rost", std::process::id()));

    fs::write(&file1, "#car { mass: 1200kg }").unwrap();
    fs::write(&file2, "#arena { gravity: 9.8 }").unwrap();

    let t1 = Template::load_watched(&file1).expect("load file1");
    let t2 = Template::load_watched(&file2).expect("load file2");

    let rx1 = t1.subscribe().expect("rx1");
    let rx2 = t2.subscribe().expect("rx2");

    assert!(CentralWatcher::global().active_files_count() >= 2);

    std::thread::sleep(Duration::from_millis(50));

    // Update only file2
    fs::write(&file2, "#arena { gravity: 15.0 }").unwrap();

    let res2 = rx2.recv_timeout(Duration::from_millis(800));
    assert!(res2.is_ok());
    assert_eq!(res2.unwrap().query_f64("arena.gravity"), Some(15.0));

    // file1 should not have received anything
    assert!(rx1.try_recv().is_err());

    let _ = fs::remove_file(file1);
    let _ = fs::remove_file(file2);
}

#[test]
fn test_nested_blocks_and_list_enum_queries() {
    let doc = r#"
    #profile {
        name: "Alex 'Shadow'",
        role: "Assasin",
        level: 42,
        accent_color: #cba6f7,
        status | online, in_combat, idle |
    }

    #stats {
        hp: 85,
        max_hp: 200,
        energy: 60,
        shield: 75
    }

    #inventory {
        items [ "Katana Neón", "Nano-Botiquín", "Bomba de Humo", "Tarjeta Cifrada" ]
    }

    #background {
        color: #181825,
    }

    #card {
        color: #1e1e2e,
        #avatar {
            text_color: #11111b,
            text: "AV",
        }
    }
    "#;

    let template = Template::from_str(doc);

    // Test List queries
    let items = template.query_list("inventory.items").expect("inventory.items");
    assert_eq!(items.len(), 4);
    assert_eq!(items[0], "Katana Neón");
    assert_eq!(items[1], "Nano-Botiquín");
    assert_eq!(template.query_list_item("inventory.items", 0), Some("Katana Neón"));
    assert_eq!(template.query_list_item("inventory.items", 3), Some("Tarjeta Cifrada"));
    assert_eq!(template.query_list_item("inventory.items", 4), None);

    // Test Enum queries
    let statuses = template.query_enum("profile.status").expect("profile.status");
    assert_eq!(statuses.len(), 3);
    assert_eq!(statuses[0], "online");
    assert_eq!(statuses[1], "in_combat");
    assert_eq!(statuses[2], "idle");
    assert_eq!(template.query_enum_variant("profile.status", 0), Some("online"));
    assert_eq!(template.query_enum_variant("profile.status", 1), Some("in_combat"));
    assert_eq!(template.query_enum_variant("profile.status", 5), None);

    // Test Nested Blocks
    assert_eq!(template.query_hex("background.color"), Some(0x181825));
    assert_eq!(template.query_hex("card.color"), Some(0x1e1e2e));
    assert_eq!(template.query_hex("card.avatar.text_color"), Some(0x11111b));
    assert_eq!(template.query_str("card.avatar.text"), Some("AV"));
    // Direct block lookups
    let avatar_block = template.find_block("avatar").expect("find avatar");
    assert_eq!(avatar_block.get_str("text"), Some("AV"));
    assert_eq!(avatar_block.parent, Some("card"));

    // BlockView list & enum methods
    let inv_block = template.find_block("inventory").expect("find inventory");
    assert_eq!(inv_block.get_list_item("items", 1), Some("Nano-Botiquín"));
    let prof_block = template.find_block("profile").expect("find profile");
    assert_eq!(prof_block.get_enum_variant("status", 0), Some("online"));
}

#[test]
fn test_dynamic_rost_objects_mapping() {
    let doc = r#"
    #car {
        color: #ff0000,
        speed: 320kmh,
        active: true
    }
    #wheel {
        size: 18px,
        rim: #ffffff
    }
    "#;

    let tmpl = Template::from_str(doc);
    let objs = tmpl.map_objs();
    assert_eq!(objs.len(), 2);
    assert_eq!(objs[0].name, "car");
    assert_eq!(objs[0].hex("color"), Some(0xff0000));
    assert_eq!(objs[0].dim("speed"), Some(320.0));
    assert_eq!(objs[0].bool("active"), Some(true));

    let wheel = tmpl.to_obj("wheel").expect("wheel");
    assert_eq!(wheel.dim("size"), Some(18.0));
    assert_eq!(wheel.hex("rim"), Some(0xffffff));
}

#[test]
fn test_rost_schema_auto_field_naming_and_tail() {
    use rost::rost_schema;

    let doc = r#"
    primary_color: #785d96
    speed: 120px
    "#;

    rost_schema! {
        struct AutoConfig {
            "primary_color" => clr: hex = 0x000000,
            speed: dim = 0.0,
            ..template
        }
    }

    let tmpl = Template::from_str(doc);
    let cfg = AutoConfig::from_template(&tmpl);
    assert_eq!(cfg.clr, 0x785d96);
    assert_eq!(cfg.speed, 120.0);
}

#[cfg(feature = "serde")]
#[test]
fn test_serde_deserialization() {
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct CarPart {
        color: u32,
        speed: f32,
    }

    let doc = "#car { color: #ff0000, speed: 200px }";
    let tmpl = Template::from_str(doc);
    let part: CarPart = tmpl.deserialize("car").expect("deserialize car");
    assert_eq!(part.color, 0xff0000);
    assert_eq!(part.speed, 200.0);
}



#[test]
fn test_from_bytes_rejects_invalid_utf8() {
    assert!(Template::from_bytes(vec![b'k', b':', b' ', 0xff, 0xfe, b'\n']).is_err());
    let t = Template::from_bytes(b"k: v\n".to_vec()).expect("valid utf-8");
    assert_eq!(t.get_value("k"), Some("v"));
}

#[test]
fn test_load_rejects_invalid_utf8() {
    let path = std::env::temp_dir().join(format!("rost_invalid_utf8_{}.rost", std::process::id()));
    std::fs::write(&path, [b'k', b':', b' ', 0xff, b'\n']).unwrap();
    let err = Template::load_with_options(&path, false).expect_err("invalid utf-8 must fail");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_u32_overflow_is_rejected() {
    assert_eq!(rost::field::parse_fast_u32("4294967295"), Some(u32::MAX));
    assert_eq!(rost::field::parse_fast_u32("4294967296"), None);
    assert_eq!(rost::field::parse_fast_u32("4294967297"), None);
    let t = Template::from_str("n: 4294967297\n");
    assert_eq!(t.query_hex("n"), None);
}

#[test]
fn test_hex_colors_keep_alpha_channel() {
    let t = Template::from_str("#c {\n rgba: #11223344\n short: #f00a\n rgb: #1e1e2e\n hex: 0xAABBCCDD\n bad: #1122334g\n}\n");
    assert_eq!(t.query_hex("c.rgba"), Some(0x11223344));
    assert_eq!(t.query_hex("c.short"), Some(0xff0000aa));
    assert_eq!(t.query_hex("c.rgb"), Some(0x1e1e2e));
    assert_eq!(t.query_hex("c.hex"), Some(0xaabbccdd));
    assert_eq!(t.query_hex("c.bad"), None);
    let obj = t.to_obj("c").expect("block c");
    assert_eq!(obj.hex("rgba"), Some(0x11223344));
    assert_eq!(obj.hex("bad"), None);
}

#[test]
fn test_float_parsing_is_correctly_rounded() {
    use rost::field::{parse_fast_f32, parse_fast_f64};
    assert_eq!(parse_fast_f64("0.1"), Some(0.1));
    assert_eq!(parse_fast_f64("0.3"), Some(0.3));
    assert_eq!(parse_fast_f64("2.2250738585072014e-308"), Some(2.2250738585072014e-308));
    assert_eq!(parse_fast_f64("-16777217"), Some(-16777217.0));
    assert_eq!(parse_fast_f32("0.1"), Some(0.1f32));
    assert_eq!(parse_fast_f32("1.5rem"), Some(1.5));
    assert_eq!(parse_fast_f32("75%"), Some(75.0));
    assert_eq!(parse_fast_f32("abc"), None);
    assert_eq!(parse_fast_f32(""), None);
}

#[cfg(feature = "serde")]
#[test]
fn test_serde_numbers_are_not_narrowed() {
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Numbers {
        big: u64,
        neg: i64,
        pi: f64,
        small: i8,
        color: u32,
    }

    let tmpl = Template::from_str(
        "#n {\n big: 5000000000\n neg: -16777217\n pi: 3.141592653589793\n small: -12\n color: #ff0000\n}\n",
    );
    let n: Numbers = tmpl.deserialize("n").expect("deserialize numbers");
    assert_eq!(
        n,
        Numbers { big: 5_000_000_000, neg: -16_777_217, pi: std::f64::consts::PI, small: -12, color: 0xff0000 }
    );

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Tiny {
        v: u8,
    }
    let tmpl = Template::from_str("#t { v: 300 }");
    assert!(tmpl.deserialize::<Tiny>("t").is_err());
    let tmpl = Template::from_str("#t { v: 1.5 }");
    assert!(tmpl.deserialize::<Tiny>("t").is_err());
    let tmpl = Template::from_str("#t { v: 0xff }");
    assert_eq!(tmpl.deserialize::<Tiny>("t").expect("hex u8").v, 0xff);
}

#[test]
fn test_unquoted_values_keep_nested_commas() {
    let t = Template::from_str(
        "color: rgba(1, 2, 3)\nshadow: drop(0 1px, [a, b]), next: 1\nfont: url(\"a,b\")\n#btn { bg: hsl(10, 20%, 30%), w: 4px }\n",
    );
    assert_eq!(t.get_value("color"), Some("rgba(1, 2, 3)"));
    assert_eq!(t.get_value("shadow"), Some("drop(0 1px, [a, b])"));
    assert_eq!(t.get_value("next"), Some("1"));
    assert_eq!(t.get_value("font"), Some("url(\"a,b\")"));
    assert_eq!(t.query_str("btn.bg"), Some("hsl(10, 20%, 30%)"));
    assert_eq!(t.query_str("btn.w"), Some("4px"));
    // An unbalanced parenthesis must not swallow the following lines.
    let t = Template::from_str("a: f(1, 2\nb: 2\n");
    assert_eq!(t.get_value("b"), Some("2"));
}

#[test]
fn test_list_and_enum_splitting_respects_quotes_and_nesting() {
    let t = Template::from_str(
        "tags [\"a, b\", \"c\", rgba(1, 2, 3), [x, y]]\nmode | \"a, b\", idle |\n",
    );
    assert_eq!(t.query_list("tags"), Some(vec!["a, b", "c", "rgba(1, 2, 3)", "[x, y]"]));
    assert_eq!(t.query_list_item("tags", 2), Some("rgba(1, 2, 3)"));
    assert_eq!(t.query_enum("mode"), Some(vec!["\"a, b\"", "idle"]));
}

#[test]
fn test_line_numbers_survive_multiline_strings() {
    let t = Template::from_str(
        "a: \"one\ntwo\"\n@doc \"x\ny\"\n##sub \"p\nq\"\nb: 1\n",
    );
    assert_eq!(t.get("a").unwrap().line(), 1);
    assert_eq!(t.get("doc").unwrap().line(), 3);
    assert_eq!(t.get("sub").unwrap().line(), 5);
    assert_eq!(t.get("b").unwrap().line(), 7);

    let t = Template::from_str("x: 1\ny: 2\n");
    assert_eq!(t.get("x").unwrap().line(), 1);
    assert_eq!(t.get("y").unwrap().line(), 2);
}

#[test]
fn test_patch_value_cannot_inject_structure() {
    let mut t = Template::from_str("name: x\nsafe: 1\n");
    assert!(t.patch_value("name", "a\nevil: 1"));
    assert_eq!(t.get_value("evil"), None);
    assert_eq!(t.get_value("name"), Some("a\nevil: 1"));
    assert_eq!(t.get_value("safe"), Some("1"));

    assert!(t.patch_value("safe", "1, hacked: true }"));
    assert_eq!(t.get_value("hacked"), None);
    assert_eq!(t.get_value("safe"), Some("1, hacked: true }"));

    let mut t = Template::from_str("title: \"old\"\nnext: 2\n");
    assert!(t.patch_value("title", "say \"hi\""));
    assert_eq!(t.get_value("next"), Some("2"));
    assert_eq!(t.as_str(), "title: \"say \\\"hi\\\"\"\nnext: 2\n");

    let mut t = Template::from_str("bg: red\n");
    assert!(t.patch_value("bg", "rgba(1, 2, 3)"));
    assert_eq!(t.as_str(), "bg: rgba(1, 2, 3)\n");
}

#[test]
fn test_patch_value_honours_scope() {
    let src = "volume: 1\n#audio {\n volume: 2\n #music { volume: 3 }\n}\n#video { volume: 4 }\n";
    let mut t = Template::from_str(src);
    assert!(t.patch_value("audio.music.volume", "9"));
    assert!(t.patch_value("video.volume", "0.5"));
    assert!(t.patch_value("volume", "7"));
    assert_eq!(t.query_str("volume"), Some("7"));
    assert_eq!(t.query_str("audio.volume"), Some("2"));
    assert_eq!(t.query_str("audio.music.volume"), Some("9"));
    assert_eq!(t.query_str("video.volume"), Some("0.5"));
    assert!(!t.patch_value("audio.missing", "1"));
    assert!(!t.patch_value("music.volume", "1"));
}

/// Bumps a file's mtime into the future so the watcher reliably sees a change.
fn touch_future(path: &std::path::Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(2))
        .unwrap();
}

#[test]
fn test_reload_callback_can_reenter_watcher() {
    use std::sync::mpsc::channel;
    use std::time::Duration;

    let path = std::env::temp_dir().join(format!("rost_reentrant_cb_{}.rost", std::process::id()));
    std::fs::write(&path, "v: 1\n").unwrap();
    let template = Template::load_with_options(&path, true).expect("load watched");

    let (tx, rx) = channel();
    let cb_path = path.clone();
    template.on_reload(move |t| {
        // Both calls take the global watcher lock; this deadlocked when callbacks ran under it.
        let count = rost::CentralWatcher::global().active_files_count();
        let extra = rost::HotReloadWatcher::new(&cb_path).expect("re-register");
        let _ = extra.subscribe();
        let _ = tx.send((t.query_i64("v"), count));
    });

    touch_future(&path, "v: 2\n");
    let (v, count) = rx.recv_timeout(Duration::from_secs(3)).expect("callback ran without deadlock");
    assert_eq!(v, Some(2));
    assert!(count >= 1);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_watcher_handles_do_not_steal_reloads() {
    use std::thread;
    use std::time::{Duration, Instant};

    let path = std::env::temp_dir().join(format!("rost_shared_handles_{}.rost", std::process::id()));
    std::fs::write(&path, "v: 1\n").unwrap();
    let mut a = Template::load_with_options(&path, true).expect("load a");
    let mut b = Template::load_with_options(&path, true).expect("load b");
    let c = Template::load_with_options(&path, true).expect("load c");
    // Dropping one handle must not unregister the file for the others.
    drop(c);

    touch_future(&path, "v: 2\n");
    let deadline = Instant::now() + Duration::from_secs(3);
    let (mut got_a, mut got_b) = (false, false);
    while !(got_a && got_b) && Instant::now() < deadline {
        got_a |= a.check_reload();
        got_b |= b.check_reload();
        thread::sleep(Duration::from_millis(20));
    }
    assert!(got_a && got_b, "both handles must observe the reload");
    assert_eq!(a.query_i64("v"), Some(2));
    assert_eq!(b.query_i64("v"), Some(2));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_hot_reload_is_opt_in() {
    let path = std::env::temp_dir().join(format!("rost_opt_in_{}.rost", std::process::id()));
    std::fs::write(&path, "v: 1\n").unwrap();
    assert!(!Template::load(&path).unwrap().is_hot_reload_enabled());
    assert!(!Template::from_file(&path).unwrap().is_hot_reload_enabled());
    assert!(Template::load_watched(&path).unwrap().is_hot_reload_enabled());
    let _ = std::fs::remove_file(&path);
}

fn diagnostics(src: &str) -> Vec<String> {
    rost::parser::parse_with_diagnostics(src.as_bytes())
        .1
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn test_diagnostics_unterminated_strings() {
    assert_eq!(diagnostics("a: 1\nname: \"Valkyrie\n"), ["2:7: unterminated string literal"]);
    assert_eq!(diagnostics("@theme \"dark\n"), ["1:8: unterminated string literal"]);
    assert_eq!(diagnostics("##state \"hover\n"), ["1:9: unterminated string literal"]);
    assert!(diagnostics("a: \"ok\"\nb: \"multi\nline\"\n").is_empty());
    // Lenient bytecode is unchanged by diagnostics.
    let src = b"a: 1\nname: \"broken\nb: 2\n";
    assert_eq!(rost::parser::parse_with_diagnostics(src).0, rost::parser::parse_into_bytecode(src));
}

#[test]
fn test_diagnostics_unterminated_enums_and_lists() {
    assert_eq!(diagnostics("state | idle, hover\n"), ["1:7: unterminated enum: missing closing `|`"]);
    assert_eq!(diagnostics("a: 1\ninv [ \"spear\", [x]\n"), ["2:5: unterminated list: missing closing `]`"]);
    assert!(diagnostics("inv [ a, [b, c] ]\nstate | a,\n b |\n").is_empty());
}

#[test]
fn test_diagnostics_block_balance() {
    assert_eq!(diagnostics("a: 1\n}\n"), ["2:1: unexpected `}` without a matching block"]);
    assert_eq!(
        diagnostics("#outer {\n  inner {\n    x: 1\n  }\n"),
        ["1:2: block is never closed: missing `}`"]
    );
    assert_eq!(diagnostics("@rule {\n"), ["1:2: block is never closed: missing `}`"]);
    assert!(diagnostics("#a { b { c: 1 } }\n@r { x: 2 }\n").is_empty());
}

#[test]
fn test_diagnostics_missing_operator() {
    assert_eq!(
        diagnostics("a: 1\nwidth 120px\n"),
        ["2:1: expected `:`, `=`, `|`, `[` or `{` after key"]
    );
    assert_eq!(diagnostics("dangling"), ["1:1: expected `:`, `=`, `|`, `[` or `{` after key"]);
    assert!(diagnostics("a: 1, b = f()\nc | x |\nd [ y ]\ne { }\n#T\n##s \"v\"\n@d v\n").is_empty());
}

#[test]
fn test_diagnostics_stray_delimiter() {
    assert_eq!(diagnostics(": 1\n"), ["1:1: expected a key"]);
    assert_eq!(diagnostics("a: 1\n  = f()\n"), ["2:3: expected a key"]);
    // Lenient parsing still skips it and keeps the rest.
    assert_eq!(Template::parse_lenient(": x\nb: 2\n").query_i64("b"), Some(2));
}

#[test]
fn test_diagnostics_duplicate_keys() {
    assert_eq!(diagnostics("a: 1\nb: 2\na: 3\n"), ["3:1: duplicate key `a` in the same scope"]);
    assert_eq!(
        diagnostics("#btn {\n w: 1\n state | a |\n state [ b ]\n}\n"),
        ["4:2: duplicate key `state` in the same scope"]
    );
    // Same key in different scopes, or after a scope closes, is fine.
    assert!(diagnostics("w: 1\n#a { w: 2 }\n#b { w: 3, inner { w: 4 } }\n").is_empty());
}

#[test]
fn test_diagnostics_duplicate_keys_in_large_scopes() {
    // Scopes past the linear-scan threshold switch to a hash set; detection must not change.
    let keys: String = (0..40).map(|i| format!(" k{i}: {i}\n")).collect();
    let src = format!("#big {{\n{keys} k3: dup\n k39: dup\n}}\nk3: top\n#next {{\n{keys}}}\n");
    assert_eq!(
        diagnostics(&src),
        ["42:2: duplicate key `k3` in the same scope", "43:2: duplicate key `k39` in the same scope"]
    );
}

#[test]
fn test_strict_parse_api() {
    let t = Template::parse("@theme \"dark\"\n#btn { w: 10px }\n").expect("well-formed");
    assert_eq!(t.get_directive("theme"), Some("dark"));
    assert_eq!(t.query_dim("btn.w"), Some(10.0));

    // The first error by position is returned, even if reported later (unclosed block).
    let err = Template::parse("#a {\n x: 1\n x: 2\n").unwrap_err();
    assert_eq!(err.kind, rost::ParseErrorKind::UnclosedBlock);
    assert_eq!((err.line, err.column), (1, 2));

    let all = Template::check("#a {\n x: 1\n x: 2\n");
    assert_eq!(all.len(), 2);
    assert_eq!(all[1].kind, rost::ParseErrorKind::DuplicateKey("x".into()));

    // Lenient parsing still accepts the same input.
    assert_eq!(Template::parse_lenient("#a {\n x: 1\n").query_i64("a.x"), Some(1));
}

#[test]
fn test_quote_style_is_recorded() {
    use rost::QuoteStyle;
    let t = Template::parse("a: 120px\nb: \"120px\"\nc: \"\"\n").unwrap();
    assert_eq!(t.get("a").unwrap().quote_style(), Some(QuoteStyle::Bare));
    assert_eq!(t.get("b").unwrap().quote_style(), Some(QuoteStyle::Quoted));
    assert_eq!(t.get("c").unwrap().quote_style(), Some(QuoteStyle::Quoted));
    assert_eq!(t.get("c").unwrap().value(), Some(""));
}

#[test]
fn test_raw_multiline_strings() {
    use rost::QuoteStyle;
    let src = "#doc {\n  body: \"\"\"\n  Hello, \"world\" \\n {not a block}\n  \"\"\"\n  empty: \"\"\"\"\"\"\n  after: 1\n}\n";
    let t = Template::parse(src).expect("raw strings parse strictly");
    let body = t.query_field("doc.body").unwrap();
    assert_eq!(body.quote_style(), Some(QuoteStyle::Raw));
    assert_eq!(body.value(), Some("\n  Hello, \"world\" \\n {not a block}\n  "));
    assert_eq!(t.query_field("doc.empty").unwrap().quote_style(), Some(QuoteStyle::Raw));
    assert_eq!(t.query_field("doc.empty").unwrap().value(), Some(""));
    assert_eq!(t.query_i64("doc.after"), Some(1));
    assert_eq!(t.query_field("doc.empty").unwrap().line(), 5);

    let err = Template::parse("a: \"\"\"never closed\n").unwrap_err();
    assert_eq!(err.kind, rost::ParseErrorKind::UnterminatedString);
}

#[test]
fn test_string_escapes_are_decoded() {
    let src = r#"
a: "tab\there \"quoted\" back\\slash \u{1F980} é bad\q"
b: raw\nbare
c: """raw\n kept"""
"#;
    let t = Template::parse(src).unwrap();
    assert_eq!(t.query_string("a").unwrap(), "tab\there \"quoted\" back\\slash 🦀 é bad\\q");
    assert_eq!(t.query_string("b").unwrap(), "raw\\nbare");
    assert_eq!(t.query_string("c").unwrap(), "raw\\n kept");
    let blocks = Template::parse("#m { s: \"x\\ny\" }").unwrap();
    assert_eq!(blocks.find_block("m").unwrap().get_string("s").unwrap(), "x\ny");
    assert_eq!(rost::field::unescape(r"\u{110000} \uD800 end\"), r"\u{110000} \uD800 end\");
}

#[test]
fn test_scalar_classification() {
    use rost::{Dimension, Scalar};
    let src = "n: null\nt: true\ni: -42\nf: 1e-3\ng: .5\nc: #1e1e2e\ns: #abc\nd: 1.5rem\np: 75%\nw: easy\nq: \"true\"\nh: #zzz\nbig: 99999999999999999999\nneg: -inf\n";
    let t = Template::parse(src).unwrap();
    let s = |k| t.query_scalar(k).unwrap();
    assert_eq!(s("n"), Scalar::Null);
    assert_eq!(s("t"), Scalar::Bool(true));
    assert_eq!(s("i"), Scalar::Int(-42));
    assert_eq!(s("f"), Scalar::Float(0.001));
    assert_eq!(s("g"), Scalar::Float(0.5));
    assert_eq!(s("c"), Scalar::Color(0x1e1e2e));
    assert_eq!(s("s"), Scalar::Color(0xaabbcc));
    assert_eq!(s("d"), Scalar::Dimension(Dimension { value: 1.5, unit: "rem" }));
    assert_eq!(s("p"), Scalar::Dimension(Dimension { value: 75.0, unit: "%" }));
    assert_eq!(s("w"), Scalar::Str("easy".into()));
    assert_eq!(s("q"), Scalar::Str("true".into()));
    assert_eq!(s("h"), Scalar::Str("#zzz".into()));
    assert_eq!(s("big"), Scalar::Float(99999999999999999999.0));
    assert_eq!(s("neg"), Scalar::Str("-inf".into()));
    assert_eq!(s("w").type_name(), "string");
    assert_eq!(t.query_scalar("missing"), None);
}

#[test]
fn test_top_level_get_does_not_see_nested_fields() {
    let t = Template::parse("#a { x: 1 }\n#b { x: 2 }\ny: 3\n").unwrap();
    assert_eq!(t.get_value("x"), None);
    assert_eq!(t.get_value("y"), Some("3"));
    assert_eq!(t.query_i64("a.x"), Some(1));
    assert_eq!(t.query_i64("b.x"), Some(2));
    let t = Template::parse("#a { x: 1 }\nx: 9\n").unwrap();
    assert_eq!(t.get_value("x"), Some("9"));
}

#[cfg(feature = "serde")]
#[test]
fn test_from_template_reads_top_level_properties() {
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Window {
        width: u32,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct App {
        name: String,
        volume: f32,
        tags: Vec<String>,
        window: Window,
    }

    let t = Template::parse("name: \"rost\"\nvolume: 0.5\ntags [a, b]\n#window { width: 800 }\n").unwrap();
    let app: App = rost::from_template(&t).expect("deserialize root");
    assert_eq!(
        app,
        App { name: "rost".into(), volume: 0.5, tags: vec!["a".into(), "b".into()], window: Window { width: 800 } }
    );
}

#[cfg(feature = "serde")]
#[test]
fn test_serde_honours_quoting_escapes_and_null() {
    use serde::Deserialize;
    use std::collections::HashMap;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Msg {
        text: String,
        raw: String,
        nick: Option<String>,
        code: String,
    }

    let src = "#m {\n text: \"a\\tb \\\"c\\\"\"\n raw: \"\"\"x\\ny\"\"\"\n nick: null\n code: \"007\"\n}\n";
    let t = Template::parse(src).unwrap();
    let m: Msg = t.deserialize("m").expect("deserialize");
    assert_eq!(m, Msg { text: "a\tb \"c\"".into(), raw: "x\\ny".into(), nick: None, code: "007".into() });

    // Untyped maps keep quoted numbers as strings.
    let any: HashMap<String, serde_value_like::Value> = t.deserialize("m").unwrap();
    assert_eq!(any["code"], serde_value_like::Value::Str("007".into()));

    let blocks = t.blocks();
    let via_block: Msg = rost::from_block(&blocks[0]).unwrap();
    assert_eq!(via_block.text, "a\tb \"c\"");
}

/// Minimal self-describing value used to observe what `deserialize_any` produces.
#[cfg(feature = "serde")]
mod serde_value_like {
    use serde::de::{Deserialize, Deserializer, Visitor};

    #[derive(Debug, PartialEq)]
    pub enum Value {
        Num,
        Str(String),
        Other,
    }

    impl<'de> Deserialize<'de> for Value {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            struct V;
            impl<'de> Visitor<'de> for V {
                type Value = Value;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str("any value")
                }
                fn visit_u32<E>(self, _: u32) -> Result<Value, E> { Ok(Value::Num) }
                fn visit_u64<E>(self, _: u64) -> Result<Value, E> { Ok(Value::Num) }
                fn visit_f32<E>(self, _: f32) -> Result<Value, E> { Ok(Value::Num) }
                fn visit_f64<E>(self, _: f64) -> Result<Value, E> { Ok(Value::Num) }
                fn visit_str<E>(self, s: &str) -> Result<Value, E> { Ok(Value::Str(s.into())) }
                fn visit_string<E>(self, s: String) -> Result<Value, E> { Ok(Value::Str(s)) }
                fn visit_unit<E>(self) -> Result<Value, E> { Ok(Value::Other) }
                fn visit_bool<E>(self, _: bool) -> Result<Value, E> { Ok(Value::Other) }
            }
            d.deserialize_any(V)
        }
    }
}

#[test]
fn test_unterminated_list_at_eof_does_not_underflow() {
    for src in ["k[", "k [", "k [a", "#b { k [ x"] {
        let t = Template::from_str(src);
        let _ = t.read_fields();
        assert!(Template::parse(src).is_err());
    }
    assert_eq!(Template::from_str("k [a").get("k").unwrap().list_items(), vec!["a"]);
}

#[test]
fn test_path_index_tracks_patches() {
    let mut t = Template::from_str("#audio {\n volume: 1\n name: \"a\"\n}\nafter: x\n");
    assert_eq!(t.query_raw_value("audio.volume"), Some("1"));
    // Same-length rewrite keeps offsets; a longer one re-indexes the whole document.
    assert!(t.patch_value("audio.volume", "2"));
    assert_eq!(t.query_raw_value("audio.volume"), Some("2"));
    assert!(t.patch_value("audio.name", "a much longer name"));
    assert_eq!(t.query_string("audio.name").as_deref(), Some("a much longer name"));
    assert_eq!(t.query_raw_value("after"), Some("x"));
}

#[cfg(feature = "serde")]
#[test]
fn test_serde_borrows_strings_from_the_template() {
    use serde::Deserialize;
    use std::borrow::Cow;

    #[derive(Debug, Deserialize)]
    struct Theme<'a> {
        name: &'a str,
        accent: &'a str,
        #[serde(borrow)]
        note: Cow<'a, str>,
        #[serde(borrow)]
        escaped: Cow<'a, str>,
    }

    let t = Template::parse("name: \"dark\"\naccent: #1e1e2e\nnote: \"plain\"\nescaped: \"a\\tb\"\n").unwrap();
    let theme: Theme = rost::from_template(&t).unwrap();
    assert_eq!((theme.name, theme.accent), ("dark", "#1e1e2e"));
    // Strings without escapes point into the document; escaped ones are decoded copies.
    assert!(t.as_bytes().as_ptr_range().contains(&theme.name.as_ptr()));
    assert!(matches!(theme.note, Cow::Borrowed("plain")));
    assert_eq!(theme.escaped, "a\tb");
}

#[cfg(feature = "serde")]
#[test]
fn test_serde_list_items_are_typed() {
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Data {
        ids: Vec<u32>,
        weights: Vec<f64>,
        flags: Vec<bool>,
        names: Vec<String>,
        maybe: Vec<Option<i64>>,
    }

    let src = "ids [1, 2, 3]\nweights [0.5, 1e3]\nflags [true, false]\nnames [\"a, b\", c]\nmaybe [null, -4]\n";
    let data: Data = rost::from_template(&Template::parse(src).unwrap()).unwrap();
    assert_eq!(
        data,
        Data {
            ids: vec![1, 2, 3],
            weights: vec![0.5, 1000.0],
            flags: vec![true, false],
            names: vec!["a, b".into(), "c".into()],
            maybe: vec![None, Some(-4)],
        }
    );
}

#[test]
fn test_subtitle_does_not_swallow_the_next_line() {
    // `##Section` without a value used to take the next quoted string in the document,
    // silently dropping the property that owned it.
    let t = Template::parse("##Section\nname: \"Valkyrie\"\n").unwrap();
    assert_eq!(t.query_str("name"), Some("Valkyrie"));
    assert_eq!(t.get("Section").and_then(|f| f.value()), Some(""));

    let t = Template::parse("##state \"hover\"\n").unwrap();
    assert_eq!(t.get("state").and_then(|f| f.value()), Some("hover"));
    assert!(Template::parse("##s x\"v\"\n").is_err());
}

#[cfg(feature = "serde")]
#[test]
fn test_from_str_single_pass() {
    use serde::Deserialize;
    use std::collections::HashMap;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Item<'a> {
        id: u64,
        name: &'a str,
        price: f64,
        active: bool,
        color: &'a str,
        tags: Vec<&'a str>,
        mode: String,
        nick: Option<String>,
    }

    let src = "@theme \"dark\"\n##meta \"x\"\n#Header\n// comment\n\
               #item_1 {\n    id: 18446744073709551615\n    name: \"first\", price: 9.5\n    active: true\n    color: #1e1e2e\n    tags [a, \"b, c\"]\n    mode | fast, slow |\n    nick: null\n    on_click = buy()\n}\n\
               @item_2 { id: 2, name: \"\", price: -1e3, active: false, color: rgba(1, 2, 3), tags [], mode | x |, nick: \"n\" }\n";
    let items: HashMap<&str, Item> = rost::from_str(src).unwrap();
    assert_eq!(
        items["item_1"],
        Item {
            id: u64::MAX,
            name: "first",
            price: 9.5,
            active: true,
            color: "#1e1e2e",
            tags: vec!["a", "b, c"],
            mode: "fast".into(),
            nick: None,
        }
    );
    assert_eq!(items["item_2"].color, "rgba(1, 2, 3)");
    assert_eq!(items["item_2"].nick.as_deref(), Some("n"));
    assert!(src.as_bytes().as_ptr_range().contains(&items["item_1"].name.as_ptr()));

    // Same result as the bytecode path.
    let via_template: HashMap<String, serde_json::Value> = rost::from_template(&Template::parse(src).unwrap()).unwrap();
    let direct: HashMap<String, serde_json::Value> = rost::from_slice(src.as_bytes()).unwrap();
    assert_eq!(direct, via_template);
}

#[cfg(feature = "serde")]
#[test]
fn test_from_str_reports_positions() {
    use std::collections::HashMap;
    type Doc = HashMap<String, serde_json::Value>;

    let err = |src: &str| rost::from_str::<Doc>(src).unwrap_err().to_string();
    assert_eq!(err("a: 1\nname: \"open\n"), "2:7: unterminated string literal");
    assert_eq!(err("#a {\n x: 1\n"), "3:1: unclosed block: missing `}`");
    assert_eq!(err("a: 1\n}\n"), "2:1: unexpected `}` without a matching block");
    assert_eq!(err("a: 1\nwidth 120px\n"), "2:1: expected `:`, `=`, `|`, `[` or `{` after key");
    assert_eq!(err("list [1, 2\n"), "1:7: unterminated list: missing closing `]`");
    assert!(rost::from_slice::<Doc>(b"a: \xff\n").unwrap_err().to_string().starts_with("invalid UTF-8"));

    #[derive(Debug, serde::Deserialize)]
    #[allow(dead_code)]
    struct Strict {
        port: u16,
    }
    assert_eq!(
        rost::from_str::<Strict>("port: 70000\n").unwrap_err().to_string(),
        "cannot parse 'port' as u16: 70000"
    );
}

#[test]
fn test_list_scanning_skips_brackets_inside_strings() {
    let src = "tags [\"a]b\", \"[\", c]\nafter: 1\n";
    let t = Template::parse(src).unwrap();
    assert_eq!(t.query_list("tags"), Some(vec!["a]b", "[", "c"]));
    assert_eq!(t.get_value("after"), Some("1"));
    assert_eq!(t.get("after").unwrap().line(), 2);

    // An unpaired quote is a plain character, as before.
    let t = Template::from_str("tags [a\"b]\nafter: 1\n");
    assert_eq!(t.query_list("tags"), Some(vec!["a\"b"]));
    assert_eq!(t.get_value("after"), Some("1"));
}

#[cfg(feature = "serde")]
#[test]
fn test_from_str_list_skips_brackets_inside_strings() {
    #[derive(Debug, serde::Deserialize, PartialEq)]
    struct Doc {
        tags: Vec<String>,
        after: u32,
    }
    let doc: Doc = rost::from_str("tags [\"a]b\", \"[\", c]\nafter: 1\n").unwrap();
    assert_eq!(doc, Doc { tags: vec!["a]b".into(), "[".into(), "c".into()], after: 1 });
}

#[cfg(feature = "serde")]
#[test]
fn test_serde_lists_of_blocks_and_nested_lists() {
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Item<'a> {
        id: u32,
        name: &'a str,
        #[serde(default)]
        tags: Vec<String>,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Doc<'a> {
        #[serde(borrow)]
        items: Vec<Item<'a>>,
        grid: Vec<Vec<u8>>,
        pair: (u32, String),
        maybe: Option<Vec<f64>>,
    }

    let src = "items [\n    { id: 1, name: \"a]b\" },\n    {\n        id: 2\n        name: \"c\"\n        tags [x, \"y, z\"]\n    },\n]\ngrid [[1, 2], [], [3]]\npair [7, \"seven\"]\nmaybe [0.5]\n";
    let expected = Doc {
        items: vec![
            Item { id: 1, name: "a]b", tags: vec![] },
            Item { id: 2, name: "c", tags: vec!["x".into(), "y, z".into()] },
        ],
        grid: vec![vec![1, 2], vec![], vec![3]],
        pair: (7, "seven".into()),
        maybe: Some(vec![0.5]),
    };
    assert_eq!(rost::from_str::<Doc>(src).unwrap(), expected);
    let template = Template::parse(src).unwrap();
    assert_eq!(rost::from_template::<Doc>(&template).unwrap(), expected);

    // A fixed-size target must not silently drop extra items.
    let err = rost::from_str::<Doc>(&src.replace("pair [7, \"seven\"]", "pair [7, \"seven\", 8]")).unwrap_err();
    assert!(err.to_string().ends_with("list has more items than expected"), "{err}");
    let err = rost::from_str::<Doc>(&src.replace("pair [7, \"seven\"]", "pair [7, \"seven\" x]")).unwrap_err();
    assert_eq!(err.to_string(), "10:18: expected `,` or `]` after a list item");
}

#[cfg(feature = "serde")]
#[test]
fn test_serde_enums_of_every_shape() {
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    enum Shape {
        Empty,
        Circle(f64),
        Rect(u32, u32),
        Poly { points: Vec<(i32, i32)>, closed: bool },
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Doc {
        a: Shape,
        b: Shape,
        c: Shape,
        d: Shape,
        e: Shape,
        all: Vec<Shape>,
        mode: Shape,
    }

    let src = "a: Empty\nb { Circle: 1.5 }\nc { Rect [3, 4] }\nd {\n    Poly {\n        points [[0, 0], [1, -1]]\n        closed: true\n    }\n}\ne: \"Empty\"\nall [Empty, { Circle: 2.0 }, { Rect [1, 2] }]\nmode | Empty, Circle |\n";
    let expected = Doc {
        a: Shape::Empty,
        b: Shape::Circle(1.5),
        c: Shape::Rect(3, 4),
        d: Shape::Poly { points: vec![(0, 0), (1, -1)], closed: true },
        e: Shape::Empty,
        all: vec![Shape::Empty, Shape::Circle(2.0), Shape::Rect(1, 2)],
        mode: Shape::Empty,
    };
    assert_eq!(rost::from_str::<Doc>(src).unwrap(), expected);
    assert_eq!(rost::from_template::<Doc>(&Template::parse(src).unwrap()).unwrap(), expected);

    // An enum at the root is a document with a single entry.
    assert_eq!(rost::from_str::<Shape>("Rect [5, 6]\n").unwrap(), Shape::Rect(5, 6));
    assert_eq!(rost::from_template::<Shape>(&Template::parse("Circle: 3\n").unwrap()).unwrap(), Shape::Circle(3.0));

    let err = |src: &str| rost::from_str::<Doc>(src).unwrap_err().to_string();
    assert!(err(&src.replace("b { Circle: 1.5 }", "b { Circle: 1.5, Empty: null }")).contains("single enum variant"));
    assert!(err(&src.replace("b { Circle: 1.5 }", "b {}")).contains("empty block"));
    assert!(err(&src.replace("a: Empty", "a: Hexagon")).contains("unknown variant `Hexagon`"));
}

#[cfg(feature = "serde")]
#[test]
fn test_serde_map_keys_beyond_strings() {
    use serde::Deserialize;
    use std::collections::BTreeMap;

    #[derive(Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
    enum Level {
        Low,
        High,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Doc {
        ports: BTreeMap<u16, String>,
        offsets: BTreeMap<i64, bool>,
        levels: BTreeMap<Level, u8>,
    }

    let src = "ports {\n    80: \"http\"\n    443: \"https\"\n}\noffsets { -1: true, 2: false }\nlevels { Low: 1, High: 9 }\n";
    let expected = Doc {
        ports: BTreeMap::from([(80, "http".into()), (443, "https".into())]),
        offsets: BTreeMap::from([(-1, true), (2, false)]),
        levels: BTreeMap::from([(Level::Low, 1), (Level::High, 9)]),
    };
    assert_eq!(rost::from_str::<Doc>(src).unwrap(), expected);
    assert_eq!(rost::from_template::<Doc>(&Template::parse(src).unwrap()).unwrap(), expected);
    assert!(rost::from_str::<Doc>(&src.replace("80:", "web:")).unwrap_err().to_string().contains("invalid u16 map key: web"));
}
