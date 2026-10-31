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
