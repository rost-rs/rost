//! Lists, enums and fields resolved through the path index must agree with the block-view
//! lookup they replaced, and iterating them must not allocate.

use rost::Template;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counting;

thread_local! {
    // Per thread, so tests running in parallel do not count each other's allocations.
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

fn allocations() -> usize {
    ALLOCATIONS.with(Cell::get)
}

fn count_allocation() {
    ALLOCATIONS.with(|n| n.set(n.get() + 1));
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count_allocation();
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static A: Counting = Counting;

const DOC: &str = "\
tags [top, level]\nmode | a, b |\nplain: 1\n\
#inventory {\n    items [sword, \"magic shield\", potion]\n    status | open, closed |\n    count: 3\n    #nested {\n        items [deep]\n    }\n}\n\
#inventory {\n    extra [again]\n}\n\
#button.primary {\n    classes [big, bold]\n    width: 10\n}\n";

#[test]
fn collection_queries_resolve_through_the_index() {
    let doc = Template::parse(DOC).unwrap();
    let list = |p: &str| doc.query_list(p);
    assert_eq!(list("tags").unwrap(), ["top", "level"]);
    assert_eq!(list("inventory.items").unwrap(), ["sword", "magic shield", "potion"]);
    assert_eq!(list("inventory.nested.items").unwrap(), ["deep"]);
    // The second `#inventory` block is reached through the chain of same-path blocks.
    assert_eq!(list("inventory.extra").unwrap(), ["again"]);
    // A block whose own name contains a dot is not addressable by the index: the lookup
    // falls back to the block views and still answers.
    assert_eq!(list("button.primary.classes").unwrap(), ["big", "bold"]);
    assert_eq!(doc.query_list_item("inventory.items", 1), Some("magic shield"));
    assert_eq!(doc.query_list_item("inventory.items", 3), None);

    assert_eq!(doc.query_enum("mode").unwrap(), ["a", "b"]);
    assert_eq!(doc.query_enum("inventory.status").unwrap(), ["open", "closed"]);
    assert_eq!(doc.query_enum_variant("inventory.status", 1), Some("closed"));

    // A key of another kind, or a missing one, is `None`, never the wrong thing.
    assert_eq!(list("plain"), None);
    assert_eq!(list("inventory.count"), None);
    assert_eq!(doc.query_enum("inventory.items"), None);
    assert_eq!(list("inventory.nope"), None);
    assert_eq!(list("nope.items"), None);
    // A scalar query on a list key stays a miss, as before.
    assert_eq!(doc.query_i64("inventory.items"), None);
}

#[test]
fn query_field_returns_the_record_at_the_path() {
    let doc = Template::parse(DOC).unwrap();
    for path in ["plain", "inventory.count", "inventory.items", "inventory.status", "inventory.nested.items", "button.primary.width"] {
        let by_index = doc.query_field(path).unwrap_or_else(|| panic!("{path}"));
        let (block, key) = path.rsplit_once('.').map_or((None, path), |(b, k)| (doc.find_block(b), k));
        let reference = match block {
            Some(b) => b.get(key).cloned(),
            None => doc.get(key),
        };
        assert_eq!(Some(by_index), reference, "{path}");
    }
    assert!(doc.query_field("inventory.nope").is_none());
}

#[test]
fn iterating_lists_and_enums_does_not_allocate() {
    let doc = Template::parse(DOC).unwrap();
    // The first query builds the path index; measure the ones after it.
    assert_eq!(doc.query_list_iter("inventory.items").unwrap().count(), 3);

    let before = allocations();
    let mut total = 0;
    for _ in 0..100 {
        total += doc.query_list_iter("inventory.items").unwrap().count();
        total += doc.query_list_iter("tags").unwrap().count();
        total += doc.query_enum_iter("inventory.status").unwrap().count();
        total += usize::from(doc.query_list_item("inventory.nested.items", 0).is_some());
    }
    let allocated = allocations() - before;
    assert_eq!(total, 100 * (3 + 2 + 2 + 1));
    assert_eq!(allocated, 0, "iterating allocated {allocated} times");
}
