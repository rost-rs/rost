//! The block tree stored in the bytecode and `find_block` through the path index must agree
//! with a plain scan of the document.

use rost::{Block, BlockView, ParsedField, Template};

/// The scan `find_block` used before the index: first match in closing order.
fn scan<'a>(doc: &'a Template, selector: &str) -> Option<BlockView<'a>> {
    let clean = selector.strip_prefix('#').unwrap_or(selector);
    doc.blocks().into_iter().find(|b| {
        let b_clean = b.name.strip_prefix('#').unwrap_or(b.name);
        if b.name == selector || b_clean == clean {
            return true;
        }
        b.parent.is_some_and(|parent| {
            let p_clean = parent.strip_prefix('#').unwrap_or(parent);
            clean.starts_with(p_clean)
                && clean[p_clean.len()..].starts_with('.')
                && &clean[p_clean.len() + 1..] == b_clean
        })
    })
}

/// Nested (name, children) built from the flat start/end events, independent of the links.
#[derive(Debug, PartialEq)]
struct Node {
    name: String,
    line: u32,
    children: Vec<Node>,
}

fn reference_tree(doc: &Template) -> Vec<Node> {
    let mut stack: Vec<Node> = Vec::new();
    let mut roots = Vec::new();
    for field in doc.iter() {
        match field {
            ParsedField::BlockStart { name, line } => {
                stack.push(Node { name: name.to_owned(), line, children: Vec::new() })
            }
            ParsedField::BlockEnd { .. } => {
                if let Some(done) = stack.pop() {
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(done),
                        None => roots.push(done),
                    }
                }
            }
            _ => {}
        }
    }
    while let Some(done) = stack.pop() {
        match stack.last_mut() {
            Some(parent) => parent.children.push(done),
            None => roots.push(done),
        }
    }
    roots
}

fn walk(block: Block<'_>) -> Node {
    Node {
        name: block.name().to_owned(),
        line: block.line(),
        children: block
            .children()
            .map(|c| {
                assert_eq!(c.parent().unwrap().name(), block.name());
                walk(c)
            })
            .collect(),
    }
}

/// Tiny deterministic generator so the corpus is the same on every run.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) % n
    }
}

const NAMES: [&str; 6] = ["a", "b", "#c", "d.e", "#a", "f"];

fn random_doc(rng: &mut Lcg) -> String {
    let mut out = String::new();
    let mut open = 0;
    for i in 0..rng.next(40) + 1 {
        match rng.next(10) {
            0..=3 => {
                out.push_str(&format!("{} {{\n", NAMES[rng.next(6) as usize]));
                open += 1;
            }
            4..=5 if open > 0 => {
                out.push_str("}\n");
                open -= 1;
            }
            6 => out.push_str(&format!("l{i} [x, y]\n")),
            7 => out.push_str(&format!("e{i} | p, q |\n")),
            _ => out.push_str(&format!("k{i}: {i}\n")),
        }
    }
    // Most documents close everything; some stay unclosed on purpose.
    if rng.next(4) != 0 {
        out.push_str(&"}\n".repeat(open));
    }
    out
}

fn corpus() -> Vec<String> {
    let mut docs: Vec<String> = [
        "",
        "a: 1\n",
        "#a { x: 1 }\n#a { y: 2 }\n",
        "#a { #a { x: 1 } y: 2 }\n",
        "#button.primary { w: 1 }\n#button { #primary { w: 2 } }\n",
        "#a { #b { #c { v: 1 } } }\n",
        "#a {\n#b {\n",
        "} #a { x: 1 }\n",
        "@theme { dark: 1 }\n#Title\n#a { x: 1 }\n",
    ]
    .map(String::from)
    .to_vec();
    let mut rng = Lcg(0x5eed);
    docs.extend((0..400).map(|_| random_doc(&mut rng)));
    docs
}

#[test]
fn the_links_describe_the_same_tree_as_the_start_and_end_records() {
    for src in corpus() {
        let doc = Template::parse_lenient(&src);
        let linked: Vec<Node> = doc.root_blocks().map(walk).collect();
        assert_eq!(linked, reference_tree(&doc), "{src:?}");
    }
}

#[test]
fn find_block_agrees_with_a_scan_of_every_block() {
    let mut selectors: Vec<String> = Vec::new();
    for a in NAMES {
        let a_clean = a.trim_start_matches('#');
        selectors.push(a.to_owned());
        selectors.push(a_clean.to_owned());
        for b in NAMES {
            let b_clean = b.trim_start_matches('#');
            selectors.push(format!("{a_clean}.{b_clean}"));
            selectors.push(format!("#{a_clean}.{b_clean}"));
        }
    }
    selectors.extend(["nope", "", ".", "a.", ".a", "a.b.c", "button.primary", "button"].map(String::from));
    let mut compared = 0;
    for src in corpus() {
        let doc = Template::parse_lenient(&src);
        for selector in &selectors {
            let fast = doc.find_block(selector);
            let slow = scan(&doc, selector);
            assert_eq!(fast.is_some(), slow.is_some(), "{selector:?} in {src:?}");
            if let (Some(fast), Some(slow)) = (fast, slow) {
                assert_eq!((fast.name, fast.parent, fast.line), (slow.name, slow.parent, slow.line), "{selector:?} in {src:?}");
                assert_eq!(fast.fields, slow.fields, "{selector:?} in {src:?}");
                compared += 1;
            }
        }
    }
    assert!(compared > 2_000, "only {compared} matches were compared");
}

#[test]
fn blocks_are_navigated_and_read_from_the_bytecode() {
    let doc = Template::parse(
        "title: \"x\"\n#window {\n  width: 800\n  tags [a, b]\n  #title { text: \"Hi\" }\n  #body { #inner { k: 1 } }\n}\n#other { z: 1 }\n",
    )
    .unwrap();
    let roots: Vec<&str> = doc.root_blocks().map(|b| b.name()).collect();
    assert_eq!(roots, ["window", "other"]);

    let window = doc.block("window").unwrap();
    assert!(window.parent().is_none());
    assert_eq!(window.fields().map(|f| f.name()).collect::<Vec<_>>(), ["width", "tags"]);
    assert_eq!(window.get("width").unwrap().value(), Some("800"));
    assert!(window.get("text").is_none(), "nested fields are not the block's own");
    let body = window.children().nth(1).unwrap();
    assert_eq!(body.name(), "body");
    assert!(body.next_sibling().is_none());
    assert_eq!(body.first_child().unwrap().get("k").unwrap().value(), Some("1"));
    assert_eq!(doc.block("window.title").unwrap().get("text").unwrap().value(), Some("Hi"));
}
