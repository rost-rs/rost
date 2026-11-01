#![cfg(feature = "serde")]

//! A `Doc` is built with a handful of allocations however many keys and strings the
//! document repeats: names are slices of the source, not copies (or shared copies).

use rost::doc::Doc;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counting;

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

fn count() {
    ALLOCATIONS.with(|n| n.set(n.get() + 1));
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count();
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static A: Counting = Counting;

#[test]
fn building_a_doc_does_not_allocate_per_key_or_string() {
    let json: String = {
        let records: Vec<String> = (0..5_000)
            .map(|i| format!("\"item_{i}\": {{\"id\": {i}, \"name\": \"item {i}\", \"tags\": [\"a\", \"b\"], \"on\": true}}"))
            .collect();
        format!("{{{}}}", records.join(", "))
    };
    let rost = json.replace('"', "").replace(": {", " {");

    for (label, build) in [
        ("json", Box::new(|| drop(Doc::from_json(&json).unwrap())) as Box<dyn Fn()>),
        ("rost", Box::new(|| drop(Doc::from_rost(&rost).unwrap()))),
    ] {
        let before = ALLOCATIONS.with(Cell::get);
        build();
        let allocations = ALLOCATIONS.with(Cell::get) - before;
        // 5000 records x (6 keys + 3 strings) would be 45,000 allocations if each were copied.
        assert!(allocations < 60, "{label}: {allocations} allocations");
    }
}
