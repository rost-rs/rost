//! # Path Index
//!
//! A lazily built index that makes every `query_*` after the first one independent of
//! document size.
//!
//! Two hash tables are built in one pass over the bytecode: one from block paths
//! (`"audio"`, `"car.dimensions"`) to blocks, and one from top-level keys to their
//! records. A dot path resolves its block through the table and then scans only that
//! block's direct children, jumping over nested blocks. Indexing blocks rather than every
//! property keeps the table small (and cache-resident) so building it costs about as much
//! as parsing.
//!
//! The tables store 64-bit path hashes, not strings. Every hit is verified against the
//! document by walking parent links, so a hash collision can never return the wrong
//! property.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::{BuildHasherDefault, Hasher};

use crate::parser::{self, resolve_slice};

/// Marks the absence of a block (the root, or the end of a chain).
const NONE: u32 = u32::MAX;

#[derive(Debug, Clone, Copy)]
struct Block {
    /// Bytecode index of the block start record.
    rec: u32,
    /// Bytecode index of the matching block end record (or the end of the bytecode).
    end: u32,
    parent: u32,
    /// Next block with the same path, in document order (`#x { … }` repeated).
    next_same: u32,
}

/// First and last block of a chain of blocks sharing one path.
#[derive(Debug, Clone, Copy)]
struct Chain {
    first: u32,
    last: u32,
}

pub(crate) type PreHashedMap<V> = HashMap<u64, V, BuildHasherDefault<PreHashed>>;

/// Resolves dot paths to property records. See the [module docs](self).
#[derive(Debug, Clone, Default)]
pub(crate) struct PathIndex {
    blocks: Vec<Block>,
    block_paths: PreHashedMap<Chain>,
    root_keys: PreHashedMap<u32>,
    /// Entries whose hash collided with a different path (practically always empty).
    block_overflow: Vec<(u64, Chain)>,
    root_overflow: Vec<(u64, u32)>,
}

impl PathIndex {
    pub(crate) fn build(data: &[u8], fields: &[u8]) -> Self {
        let mut index = Self::default();
        // (block id, path hash) of each open block.
        let mut stack: Vec<(u32, u64)> = Vec::new();
        let mut idx = 0;
        while idx < fields.len() {
            let tag = fields[idx];
            match tag {
                parser::TAG_BLOCK_START => {
                    let (parent, parent_hash) = stack.last().copied().unwrap_or((NONE, SEED));
                    let name = block_name(resolve_slice(data, fields, idx + 1));
                    let hash = mix_segment(parent_hash, name.as_bytes());
                    let id = index.blocks.len() as u32;
                    index.blocks.push(Block {
                        rec: idx as u32,
                        end: fields.len() as u32,
                        parent,
                        next_same: NONE,
                    });
                    index.insert_block(data, fields, finish(hash), id);
                    stack.push((id, hash));
                }
                parser::TAG_BLOCK_END => {
                    if let Some((id, _)) = stack.pop() {
                        index.blocks[id as usize].end = idx as u32;
                    }
                }
                parser::TAG_COMMON_TEXT if stack.is_empty() => {
                    let key = resolve_slice(data, fields, idx + 1);
                    index.insert_root_key(data, fields, finish(mix_segment(SEED, key.as_bytes())), idx as u32);
                }
                _ => {}
            }
            idx += parser::record_len(tag);
        }
        index
    }

    fn insert_block(&mut self, data: &[u8], fields: &[u8], hash: u64, id: u32) {
        let chain = std::iter::once(self.block_paths.get_mut(&hash))
            .flatten()
            .chain(self.block_overflow.iter_mut().filter(|(h, _)| *h == hash).map(|(_, c)| c))
            .find(|c| same_block_path(&self.blocks, data, fields, c.first, id));
        match chain {
            Some(chain) => {
                self.blocks[chain.last as usize].next_same = id;
                chain.last = id;
            }
            None => {
                let chain = Chain { first: id, last: id };
                match self.block_paths.entry(hash) {
                    Entry::Vacant(slot) => {
                        slot.insert(chain);
                    }
                    Entry::Occupied(_) => self.block_overflow.push((hash, chain)),
                }
            }
        }
    }

    fn insert_root_key(&mut self, data: &[u8], fields: &[u8], hash: u64, rec: u32) {
        let key = resolve_slice(data, fields, rec as usize + 1);
        let known = self.root_keys.get(&hash).into_iter().chain(
            self.root_overflow.iter().filter(|(h, _)| *h == hash).map(|(_, r)| r),
        );
        // The first definition wins, matching the linear lookup.
        if known.into_iter().any(|&r| resolve_slice(data, fields, r as usize + 1) == key) {
            return;
        }
        match self.root_keys.entry(hash) {
            Entry::Vacant(slot) => {
                slot.insert(rec);
            }
            Entry::Occupied(_) => self.root_overflow.push((hash, rec)),
        }
    }

    /// Returns the bytecode index of the property addressed by `path` (without a leading `#`).
    pub(crate) fn get(&self, data: &[u8], fields: &[u8], path: &str) -> Option<usize> {
        let Some((block_path, key)) = path.rsplit_once('.') else {
            let hash = finish(mix_segment(SEED, path.as_bytes()));
            return self
                .root_keys
                .get(&hash)
                .into_iter()
                .chain(self.root_overflow.iter().filter(|(h, _)| *h == hash).map(|(_, r)| r))
                .find(|&&r| resolve_slice(data, fields, r as usize + 1) == path)
                .map(|&r| r as usize);
        };

        let hash = finish(block_path.split('.').fold(SEED, |h, s| mix_segment(h, s.as_bytes())));
        let chain = self
            .block_paths
            .get(&hash)
            .into_iter()
            .chain(self.block_overflow.iter().filter(|(h, _)| *h == hash).map(|(_, c)| c))
            .find(|c| self.block_matches(data, fields, c.first, block_path))?;

        let mut block = chain.first;
        while block != NONE {
            if let Some(rec) = self.find_child(data, fields, block, key) {
                return Some(rec);
            }
            block = self.blocks[block as usize].next_same;
        }
        None
    }

    /// Scans the direct property children of `block` for `key`, skipping nested blocks.
    fn find_child(&self, data: &[u8], fields: &[u8], block: u32, key: &str) -> Option<usize> {
        let b = self.blocks[block as usize];
        let mut idx = b.rec as usize + parser::REC_NAMED;
        let end = b.end as usize;
        while idx < end {
            let tag = fields[idx];
            match tag {
                parser::TAG_COMMON_TEXT => {
                    if resolve_slice(data, fields, idx + 1) == key {
                        return Some(idx);
                    }
                }
                parser::TAG_BLOCK_START => {
                    // Blocks are stored in document order, so the nested block is found by
                    // binary search and skipped as a whole.
                    let nested = self.blocks.partition_point(|n| (n.rec as usize) < idx);
                    idx = self.blocks[nested].end as usize;
                }
                _ => {}
            }
            idx += parser::record_len(fields.get(idx).copied().unwrap_or(0));
        }
        None
    }

    /// Checks that the chain of block names ending at `block` spells `path`.
    fn block_matches(&self, data: &[u8], fields: &[u8], mut block: u32, path: &str) -> bool {
        for segment in path.rsplit('.') {
            if block == NONE || block_name_of(&self.blocks, data, fields, block) != segment {
                return false;
            }
            block = self.blocks[block as usize].parent;
        }
        block == NONE
    }
}

/// Checks whether two blocks have the same chain of names up to the root.
fn same_block_path(blocks: &[Block], data: &[u8], fields: &[u8], mut a: u32, mut b: u32) -> bool {
    loop {
        match (a == NONE, b == NONE) {
            (true, true) => return true,
            (false, false)
                if block_name_of(blocks, data, fields, a) == block_name_of(blocks, data, fields, b) =>
            {
                a = blocks[a as usize].parent;
                b = blocks[b as usize].parent;
            }
            _ => return false,
        }
    }
}

fn block_name_of<'a>(blocks: &[Block], data: &'a [u8], fields: &[u8], block: u32) -> &'a str {
    block_name(resolve_slice(data, fields, blocks[block as usize].rec as usize + 1))
}

/// Block names are addressed without their `#` prefix.
#[inline]
fn block_name(name: &str) -> &str {
    name.strip_prefix('#').unwrap_or(name)
}

const SEED: u64 = 0x243F_6A88_85A3_08D3;
const K: u64 = 0x9E37_79B9_7F4A_7C15;

/// Folds one path segment into a running path hash, 8 bytes at a time.
#[inline]
pub(crate) fn mix_segment(mut h: u64, bytes: &[u8]) -> u64 {
    let (chunks, rest) = bytes.as_chunks::<8>();
    for chunk in chunks {
        h = folded_multiply(h ^ u64::from_le_bytes(*chunk), K);
    }
    let mut tail = [0u8; 8];
    tail[..rest.len()].copy_from_slice(rest);
    h = folded_multiply(h ^ u64::from_le_bytes(tail), K);
    // The length acts as the segment separator: "ab.c" and "a.bc" hash differently.
    folded_multiply(h ^ bytes.len() as u64, K ^ SEED)
}

/// Full 64×64→128-bit multiply folded back to 64 bits. Unlike a truncating multiply it
/// spreads differences in the high bits of the input (e.g. trailing digits of `item_123`)
/// into the low bits too.
#[inline(always)]
fn folded_multiply(a: u64, b: u64) -> u64 {
    let full = u128::from(a) * u128::from(b);
    (full as u64) ^ ((full >> 64) as u64)
}

/// Final avalanche so every bit of the table hash depends on the whole path.
#[inline]
pub(crate) fn finish(h: u64) -> u64 {
    folded_multiply(h, 0xFF51_AFD7_ED55_8CCD)
}

/// Hasher for keys that are already well-mixed `u64` hashes.
#[derive(Default)]
pub(crate) struct PreHashed(u64);

impl Hasher for PreHashed {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(8) ^ u64::from(b)).wrapping_mul(K);
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = n;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(src: &str, paths: &[&str]) -> Vec<Option<String>> {
        let fields = parser::parse_into_bytecode(src.as_bytes());
        let index = PathIndex::build(src.as_bytes(), &fields);
        paths
            .iter()
            .map(|p| {
                index
                    .get(src.as_bytes(), &fields, p)
                    .map(|r| resolve_slice(src.as_bytes(), &fields, r + 9).to_owned())
            })
            .collect()
    }

    #[test]
    fn resolves_nested_and_repeated_blocks() {
        let src = "a: 1\n#x { a: 2\n y { a: 3 }\n z: 0 }\n#x { b: 4 }\na: 5\n";
        let got = lookup(src, &["a", "x.a", "x.y.a", "x.z", "x.b", "y.a", "x", "b"]);
        let want = [Some("1"), Some("2"), Some("3"), Some("0"), Some("4"), None, None, None];
        assert_eq!(got, want.map(|v| v.map(str::to_owned)));
    }

    #[test]
    fn colliding_hashes_are_resolved_exactly() {
        let src = "a: 1\nb: 2\n#p { k: 1 }\n#q { k: 2 }\n";
        let fields = parser::parse_into_bytecode(src.as_bytes());
        let data = src.as_bytes();
        let mut index = PathIndex::build(data, &fields);
        // Re-insert everything under one hash to force the overflow paths.
        let (a, b) = (0, parser::REC_PAIR as u32);
        index.root_keys.clear();
        index.insert_root_key(data, &fields, 7, a);
        index.insert_root_key(data, &fields, 7, b);
        index.insert_root_key(data, &fields, 7, a);
        assert_eq!(index.root_overflow.len(), 1);
        index.block_paths.clear();
        index.insert_block(data, &fields, 9, 0);
        index.insert_block(data, &fields, 9, 1);
        assert_eq!(index.block_overflow.len(), 1);

        let root = |key: &str| {
            [index.root_keys[&7], index.root_overflow[0].1]
                .into_iter()
                .find(|&r| resolve_slice(data, &fields, r as usize + 1) == key)
        };
        assert_eq!((root("a"), root("b")), (Some(a), Some(b)));
        let block = |path: &str| {
            [index.block_paths[&9], index.block_overflow[0].1]
                .into_iter()
                .find(|c| index.block_matches(data, &fields, c.first, path))
                .map(|c| c.first)
        };
        assert_eq!((block("p"), block("q")), (Some(0), Some(1)));
    }
}
