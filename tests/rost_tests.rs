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
