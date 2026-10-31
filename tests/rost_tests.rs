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

    let mut template = Template::load(&temp_file).expect("load with hot reload");
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

    let t1 = Template::load(&file1).expect("load file1");
    let t2 = Template::load(&file2).expect("load file2");

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
        "a: \"one\ntwo\"\n@doc \"x\ny\"\n##sub\n\"p\nq\"\nb: 1\n",
    );
    assert_eq!(t.get("a").unwrap().line(), 1);
    assert_eq!(t.get("doc").unwrap().line(), 3);
    assert_eq!(t.get("sub").unwrap().line(), 6);
    assert_eq!(t.get("b").unwrap().line(), 8);

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
