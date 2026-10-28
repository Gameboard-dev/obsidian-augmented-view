//! One cache file per note (postcard, binary), read only when that note is opened and written only when its results
//! change. It holds the results for the note's current paragraphs, keyed by paragraph hash, with the key of the models
//! (and diversity) that made each layer, so results from other models are ignored rather than shown.
//! Encoding: postcard (<https://docs.rs/postcard>).

use serde::{Deserialize, Serialize};
use xxhash_rust::xxh3::xxh3_64;

use crate::pick::Span;

pub const FORMAT: u16 = 1;

#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct CacheFile {
    pub format: u16,
    /// For each layer (0 terms, quotations, definitions; 1 points, main point, contrast words): the key it was made with, and its entries.
    pub keys: [String; 2],
    pub layers: [Vec<(u64, Vec<Span>)>; 2],
}

impl CacheFile {
    pub fn decode(bytes: &[u8]) -> Option<CacheFile> {
        postcard::from_bytes::<CacheFile>(bytes).ok().filter(|f| f.format == FORMAT)
    }

    pub fn encode(&self) -> Vec<u8> {
        postcard::to_allocvec(self).unwrap_or_default()
    }
}

/// The cache file's name for a note: a hash of its path (xxh3, <https://docs.rs/xxhash-rust>), so names are short
/// and safe on every file system.
pub fn file_name(path: &str) -> String {
    format!("{:016x}.bin", xxh3_64(path.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let f = CacheFile {
            format: FORMAT,
            keys: ["m|0.4".into(), "c|rank2".into()],
            layers: [vec![(7, vec![Span { k: 0, w: 0.5, at: vec![(1, 3)], v: "ab".into() }])], vec![]],
        };
        assert_eq!(CacheFile::decode(&f.encode()), Some(f));
        assert_eq!(CacheFile::decode(b"junk"), None);
    }
}
