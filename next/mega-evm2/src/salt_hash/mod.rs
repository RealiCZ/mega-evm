//! The production SALT bucket hasher, copied from Satin
//! (`crates/mega-evm/src/external/hasher/` @ 5e3f30ad: `AHashBucketHasher`, the vendored AHash
//! v0.8.12 fallback with the fixed seeds; see `NOTICE.md`). It is the engine's mapping
//! (`salt::bucket_id`, which reduces the hash to a KV bucket): the bucket an account or a slot
//! lands in is SALT's own definition. The FNV-1a test hasher of Satin's `TestExternalEnvs`, which
//! the differential scenarios name buckets with, replaces it only under feature
//! `salt-test-hasher`.

mod convert;
mod fallback;

use self::fallback::RandomState;
use core::hash::{BuildHasher, Hasher};

const HASHER_SEEDS: [u64; 4] = [0x921321f4, 0x2ccb667e, 0x60d68842, 0x077ada9d];

/// Satin `AHashBucketHasher`'s hash of `bytes`, before its reduction to a KV bucket.
#[inline(always)]
pub fn hash(bytes: &[u8]) -> u64 {
    static HASH_BUILDER: RandomState =
        RandomState::with_seeds(HASHER_SEEDS[0], HASHER_SEEDS[1], HASHER_SEEDS[2], HASHER_SEEDS[3]);
    let mut hasher = HASH_BUILDER.build_hasher();
    hasher.write(bytes);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    #[test]
    fn stable() {
        assert_eq!(super::hash(b"hello"), 1027176506268606463);
        assert_eq!(super::hash(b"world"), 2337896903564117184);
    }
}
