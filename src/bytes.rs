//! Storage of a template's document and bytecode: heap memory, or a memory-mapped
//! `.rostc` file (feature `mmap`).

use std::ops::Deref;

/// Read-only bytes that are either owned or borrowed from a shared memory map.
///
/// Writing (`Template::patch_value`) copies a mapped region to the heap first, so a
/// mapped file is never modified.
#[derive(Debug, Clone)]
pub(crate) enum Bytes {
    Owned(Vec<u8>),
    #[cfg(feature = "mmap")]
    Mapped {
        map: std::sync::Arc<memmap2::Mmap>,
        start: usize,
        len: usize,
    },
}

impl Bytes {
    /// The bytes as a mutable vector, copying them out of a map if necessary.
    pub(crate) fn make_mut(&mut self) -> &mut Vec<u8> {
        #[cfg(feature = "mmap")]
        if let Self::Mapped { .. } = self {
            *self = Self::Owned(self.to_vec());
        }
        match self {
            Self::Owned(v) => v,
            #[cfg(feature = "mmap")]
            Self::Mapped { .. } => unreachable!("converted to Owned above"),
        }
    }
}

impl Deref for Bytes {
    type Target = [u8];

    #[inline(always)]
    fn deref(&self) -> &[u8] {
        match self {
            Self::Owned(v) => v,
            #[cfg(feature = "mmap")]
            Self::Mapped { map, start, len } => &map[*start..*start + *len],
        }
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(v: Vec<u8>) -> Self {
        Self::Owned(v)
    }
}
