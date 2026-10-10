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
    let template = Template::from_bytes(bytes.to_vec());
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

