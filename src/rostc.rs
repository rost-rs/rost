//! # `.rostc` — precompiled documents
//!
//! A `.rostc` file stores a document together with its bytecode index, so loading it skips
//! parsing altogether: the file is checksummed, validated and handed to [`Template`].
//!
//! ```text
//! offset  size  field
//!      0     5  magic `ROSTC`
//!      5     1  format version (currently 1)
//!      6     2  reserved, zero
//!      8     8  document length  (u64, little endian)
//!     16     8  bytecode length  (u64, little endian)
//!     24     8  checksum of bytes 0..24 and the payload (u64, little endian)
//!     32     …  document bytes, then bytecode bytes
//! ```
//!
//! The checksum catches truncation and bit rot; it is **not** a signature. A file from an
//! untrusted source is therefore also validated structurally before use: the document must
//! be UTF-8 and every offset in the bytecode must stay inside it, on a character boundary,
//! because the zero-copy queries hand out `&str` slices without re-checking them.
//!
//! A reader rejects any version it does not know, so the layout can evolve.

use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[cfg(feature = "mmap")]
use crate::bytes::Bytes;
use crate::Template;
use crate::parser::{self, read_u32};

/// First five bytes of every `.rostc` file.
pub const MAGIC: [u8; 5] = *b"ROSTC";
/// Version written by this crate and the only one it reads.
pub const FORMAT_VERSION: u8 = 1;
const HEADER_LEN: usize = 32;

/// Why a `.rostc` file could not be loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RostcError {
    /// Shorter than its header or than the lengths the header announces.
    Truncated,
    /// Does not start with the `ROSTC` magic.
    BadMagic,
    /// Written by a newer (or unknown) format version.
    UnsupportedVersion(u8),
    /// The stored checksum does not match the contents.
    BadChecksum,
    /// The document part is not valid UTF-8.
    InvalidUtf8,
    /// The bytecode is malformed or points outside the document.
    InvalidBytecode,
}

impl fmt::Display for RostcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("truncated .rostc file"),
            Self::BadMagic => f.write_str("not a .rostc file (bad magic)"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported .rostc format version {v}"),
            Self::BadChecksum => f.write_str(".rostc checksum mismatch: the file is corrupted"),
            Self::InvalidUtf8 => f.write_str(".rostc document is not valid UTF-8"),
            Self::InvalidBytecode => f.write_str(".rostc bytecode is malformed"),
        }
    }
}

impl std::error::Error for RostcError {}

/// A fast 64-bit checksum: one multiply per eight bytes, about 5 GB/s.
fn checksum(parts: &[&[u8]]) -> u64 {
    const K: u64 = 0xff51_afd7_ed55_8ccd;
    let mut h = 0x9e37_79b9_7f4a_7c15_u64;
    for part in parts {
        let (words, rest) = part.as_chunks::<8>();
        for w in words {
            h = (h ^ u64::from_le_bytes(*w)).wrapping_mul(K);
            h ^= h >> 32;
        }
        let mut tail = [0u8; 8];
        tail[..rest.len()].copy_from_slice(rest);
        h = (h ^ u64::from_le_bytes(tail) ^ rest.len() as u64).wrapping_mul(K);
        h ^= h >> 32;
    }
    h
}

/// Checks that walking `fields` is safe for a document `text`: whole records, known tags,
/// every (offset, length) pair inside the text and on character boundaries, and block tree
/// links that point forward or backward to the right kind of record (so navigating them
/// always terminates).
fn validate_bytecode(text: &str, fields: &[u8]) -> bool {
    let tag_at = |at: u32| fields.get(at as usize).copied();
    let mut idx = 0;
    while idx < fields.len() {
        let tag = fields[idx];
        let len = parser::record_len(tag);
        if !(parser::TAG_TITLE..=parser::TAG_BLOCK_END).contains(&tag) || idx + len > fields.len() {
            return false;
        }
        let pairs = match tag {
            parser::TAG_BLOCK_END => 0,
            parser::TAG_TITLE | parser::TAG_BLOCK_START => 1,
            _ => 2,
        };
        for p in 0..pairs {
            let off = read_u32(fields, idx + 1 + 8 * p) as usize;
            let end = off + read_u32(fields, idx + 5 + 8 * p) as usize;
            if end > text.len() || !text.is_char_boundary(off) || !text.is_char_boundary(end) {
                return false;
            }
        }
        if tag == parser::TAG_BLOCK_START {
            let rec = idx as u32;
            let link = |at: usize| read_u32(fields, idx + at);
            let (parent, end, first, next) = (
                link(parser::BLOCK_PARENT),
                link(parser::BLOCK_END),
                link(parser::BLOCK_FIRST_CHILD),
                link(parser::BLOCK_NEXT_SIBLING),
            );
            let none = parser::NO_RECORD;
            let ok = (parent == none || (parent < rec && tag_at(parent) == Some(parser::TAG_BLOCK_START)))
                && (end == none || (end > rec && tag_at(end) == Some(parser::TAG_BLOCK_END)))
                && (first == none || (first > rec && tag_at(first) == Some(parser::TAG_BLOCK_START)))
                && (next == none || (next > rec && tag_at(next) == Some(parser::TAG_BLOCK_START)));
            if !ok {
                return false;
            }
        }
        idx += len;
    }
    true
}

/// Verifies a `.rostc` file and returns where its document and bytecode lie in `bytes`.
fn decode(bytes: &[u8]) -> Result<(Range<usize>, Range<usize>), RostcError> {
    let header = bytes.get(..HEADER_LEN).ok_or(RostcError::Truncated)?;
    if header[..5] != MAGIC {
        return Err(RostcError::BadMagic);
    }
    if header[5] != FORMAT_VERSION {
        return Err(RostcError::UnsupportedVersion(header[5]));
    }
    let data_len = u64::from_le_bytes(header[8..16].try_into().unwrap());
    let fields_len = u64::from_le_bytes(header[16..24].try_into().unwrap());
    let stored = u64::from_le_bytes(header[24..32].try_into().unwrap());
    let total = data_len
        .checked_add(fields_len)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or(RostcError::Truncated)?;
    if bytes.len() - HEADER_LEN != total {
        return Err(RostcError::Truncated);
    }
    let data = HEADER_LEN..HEADER_LEN + data_len as usize;
    let fields = data.end..bytes.len();
    if checksum(&[&header[..24], &bytes[data.clone()], &bytes[fields.clone()]]) != stored {
        return Err(RostcError::BadChecksum);
    }
    let text = std::str::from_utf8(&bytes[data.clone()]).map_err(|_| RostcError::InvalidUtf8)?;
    if !validate_bytecode(text, &bytes[fields.clone()]) {
        return Err(RostcError::InvalidBytecode);
    }
    Ok((data, fields))
}

impl Template {
    /// Serializes this template, bytecode included, as a `.rostc` file.
    ///
    /// # Examples
    ///
    /// ```
    /// use rost::Template;
    ///
    /// let doc = Template::parse("#window { width: 800 }").unwrap();
    /// let bytes = doc.to_rostc();
    /// let loaded = Template::from_rostc(&bytes).unwrap();
    /// assert_eq!(loaded.query_i64("window.width"), Some(800));
    /// ```
    pub fn to_rostc(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + self.data.len() + self.fields.len());
        out.extend_from_slice(&MAGIC);
        out.push(FORMAT_VERSION);
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&(self.data.len() as u64).to_le_bytes());
        out.extend_from_slice(&(self.fields.len() as u64).to_le_bytes());
        let sum = checksum(&[&out, &self.data, &self.fields]);
        out.extend_from_slice(&sum.to_le_bytes());
        out.extend_from_slice(&self.data);
        out.extend_from_slice(&self.fields);
        out
    }

    /// Loads a template from the bytes of a `.rostc` file, without parsing the document.
    ///
    /// # Errors
    /// See [`RostcError`]: wrong magic or version, truncation, a checksum mismatch, or a
    /// structurally invalid document or bytecode.
    pub fn from_rostc(bytes: &[u8]) -> Result<Self, RostcError> {
        let (data, fields) = decode(bytes)?;
        Ok(Self {
            path: PathBuf::new(),
            data: bytes[data].to_vec().into(),
            fields: bytes[fields].to_vec().into(),
            watcher: None,
            index: OnceLock::new(),
        })
    }

    /// Maps a `.rostc` file into memory and uses it in place: nothing is copied or parsed,
    /// and the pages are shared with every other process that maps the same file.
    ///
    /// The file is verified exactly like [`Template::from_rostc`] does. Patching the
    /// template copies it to the heap first; the file is never written.
    ///
    /// # Safety
    /// The file must not be modified or truncated while the template (or a clone of it) is
    /// alive. That would change bytes that were validated as UTF-8 under the template's
    /// feet, which is undefined behaviour.
    ///
    /// # Errors
    /// I/O errors, and [`std::io::ErrorKind::InvalidData`] for a malformed file.
    #[cfg(feature = "mmap")]
    pub unsafe fn mmap_rostc<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let path = path.as_ref();
        let file = std::fs::File::open(path)?;
        let map = std::sync::Arc::new(unsafe { memmap2::Mmap::map(&file)? });
        let (data, fields) =
            decode(&map).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(Self {
            path: path.to_path_buf(),
            data: Bytes::Mapped { map: map.clone(), start: data.start, len: data.len() },
            fields: Bytes::Mapped { map, start: fields.start, len: fields.len() },
            watcher: None,
            index: OnceLock::new(),
        })
    }

    /// Writes this template to `path` as a `.rostc` file.
    pub fn save_rostc<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        std::fs::write(path, self.to_rostc())
    }

    /// Reads a `.rostc` file; a malformed one is an [`std::io::ErrorKind::InvalidData`] error.
    pub fn load_rostc<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let path = path.as_ref();
        let mut doc = Self::from_rostc(&std::fs::read(path)?)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        doc.path = path.to_path_buf();
        Ok(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Re-encodes `fields` for `data` with a valid checksum, as a hostile writer would.
    fn forge(data: &[u8], fields: &[u8]) -> Vec<u8> {
        let mut t = Template::parse_lenient("a: 1");
        t.data = data.to_vec().into();
        t.fields = fields.to_vec().into();
        t.to_rostc()
    }

    #[test]
    fn offsets_inside_a_character_are_rejected_despite_a_valid_checksum() {
        // "é" is two bytes; a property whose key slice ends in the middle of it.
        let doc = Template::parse_lenient("é: 1");
        let mut fields = doc.fields.to_vec();
        fields[5..9].copy_from_slice(&1u32.to_le_bytes());
        let forged = forge(&doc.data, &fields);
        assert_eq!(Template::from_rostc(&forged).unwrap_err(), RostcError::InvalidBytecode);
    }

    #[test]
    fn offsets_past_the_document_and_cut_records_are_rejected() {
        let doc = Template::parse_lenient("a: 1");
        let mut fields = doc.fields.to_vec();
        fields[1..5].copy_from_slice(&100u32.to_le_bytes());
        assert_eq!(
            Template::from_rostc(&forge(&doc.data, &fields)).unwrap_err(),
            RostcError::InvalidBytecode
        );
        let cut = &doc.fields[..doc.fields.len() - 1];
        assert_eq!(
            Template::from_rostc(&forge(&doc.data, cut)).unwrap_err(),
            RostcError::InvalidBytecode
        );
        assert_eq!(
            Template::from_rostc(&forge(&doc.data, &[0xEE])).unwrap_err(),
            RostcError::InvalidBytecode
        );
    }

    #[test]
    fn invalid_utf8_is_rejected_despite_a_valid_checksum() {
        assert_eq!(Template::from_rostc(&forge(&[0xff, 0xfe], &[])).unwrap_err(), RostcError::InvalidUtf8);
    }

    #[test]
    fn forged_block_links_that_could_loop_are_rejected() {
        let doc = Template::parse_lenient("#a { #b { } }");
        for (link, target) in [
            (parser::BLOCK_NEXT_SIBLING, 0u32),   // a sibling that is the block itself
            (parser::BLOCK_FIRST_CHILD, 0),       // a child that is the block itself
            (parser::BLOCK_PARENT, 0),            // a parent that is the block itself
            (parser::BLOCK_END, 1),               // an end that is not an end record
            (parser::BLOCK_NEXT_SIBLING, 9999),   // outside the bytecode
        ] {
            let mut fields = doc.fields.to_vec();
            fields[link..link + 4].copy_from_slice(&target.to_le_bytes());
            assert_eq!(
                Template::from_rostc(&forge(&doc.data, &fields)).unwrap_err(),
                RostcError::InvalidBytecode,
                "link at {link} -> {target}"
            );
        }
        assert!(Template::from_rostc(&forge(&doc.data, &doc.fields)).is_ok());
    }
}
