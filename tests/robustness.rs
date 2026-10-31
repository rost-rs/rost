//! Deterministic stress test: feeds mutated and random documents through every public
//! entry point and checks that nothing panics and that strict and lenient parsing agree.
//!
//! Runs on stable in a few hundred milliseconds; `fuzz/` holds the coverage-guided
//! equivalents for `cargo +nightly fuzz`.

use rost::Template;

const SEEDS: &[&str] = &[
    "@theme \"cyber-dark\"\n#button.primary {\n    width: 140px,\n    bg: #1e1e2e,\n    state | idle, hover, active |\n    on_click = emit(\"submit\")\n}\n",
    "#PlayerEntity\nname: \"Valkyrie\",\nspeed: 350.0,\ninventory [ \"spear\", [\"nested\"] ],\ndifficulty | easy, nightmare |\n",
    "##state \"hover\"\nbody: \"\"\"raw\n\"text\" {}\"\"\"\nesc: \"a\\\"b\\u{1F980}\"\nnil: null\n",
    "palette {\n  bg: #fff\n  inner { x: -1e3, y: .5 }\n}\n// comment\n@rule { fix = f() }\n",
];

/// xorshift64*: tiny deterministic PRNG so failures are reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const ALPHABET: &[u8] = b"#@{}[]|:=,;\"\\/\n\t abcxyz019.-%";
const UNICODE: &[&str] = &["é", "🦀", "\u{2028}", "ß"];

fn mutate(rng: &mut Rng, seed: &str) -> String {
    let mut bytes = seed.as_bytes().to_vec();
    for _ in 0..1 + rng.below(8) {
        let at = rng.below(bytes.len() + 1);
        match rng.below(4) {
            0 if !bytes.is_empty() => {
                bytes.remove(at.min(bytes.len() - 1));
            }
            1 => bytes.insert(at, ALPHABET[rng.below(ALPHABET.len())]),
            2 => {
                let s = UNICODE[rng.below(UNICODE.len())].as_bytes();
                bytes.splice(at..at, s.iter().copied());
            }
            _ => {
                let len = rng.below(16).min(bytes.len().saturating_sub(at));
                let chunk: Vec<u8> = bytes[at..at + len].to_vec();
                bytes.splice(at..at, chunk);
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn exercise(src: &str) {
    let lenient = Template::from_str(src);
    let errors = Template::check(src);
    let strict = Template::parse(src);

    // Strict and lenient parsing agree on the bytecode and on validity.
    let (bytecode, _) = rost::parser::parse_with_diagnostics(src.as_bytes());
    assert_eq!(bytecode, rost::parser::parse_into_bytecode(src.as_bytes()));
    assert_eq!(strict.is_ok(), errors.is_empty(), "parse/check disagree on {src:?}");
    for e in &errors {
        assert!(e.span.start <= src.len() && e.line >= 1 && e.column >= 1);
        let _ = e.render(src);
    }

    // Every query path is total.
    for field in lenient.iter() {
        let name = field.name();
        let _ = (field.scalar(), field.string_value(), field.list_items(), field.enum_variants());
        let _ = (lenient.query_scalar(name), lenient.query_str(name), lenient.query_hex(name));
    }
    for block in lenient.blocks() {
        let _ = lenient.find_block(block.name);
        for field in &block.fields {
            let path = format!("{}.{}", block.name, field.name());
            let _ = (lenient.query_raw_value(&path), lenient.query_string(&path));
        }
    }

    #[cfg(feature = "serde")]
    {
        let _ = rost::from_template::<std::collections::HashMap<String, String>>(&lenient);
    }
}

#[test]
fn mutated_documents_never_panic() {
    let mut rng = Rng(0x5EED_1234_ABCD_0001);
    for i in 0..20_000 {
        let seed = SEEDS[i % SEEDS.len()];
        let src = mutate(&mut rng, seed);
        exercise(&src);
    }
}

#[test]
fn random_documents_never_panic() {
    let mut rng = Rng(0xC0FF_EE00_DEAD_BEEF);
    for _ in 0..5_000 {
        let len = rng.below(96);
        let src: String = (0..len)
            .map(|_| ALPHABET[rng.below(ALPHABET.len())] as char)
            .collect();
        exercise(&src);
    }
}
