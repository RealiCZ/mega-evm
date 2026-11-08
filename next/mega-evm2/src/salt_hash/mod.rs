//! The production SALT bucket hasher, copied from Satin
//! (`crates/mega-evm/src/external/hasher/` @ 5e3f30ad: `AHashBucketHasher`, the vendored AHash
//! v0.8.12 fallback with the fixed seeds; see `NOTICE.md`). It is the engine's mapping
//! (`ledger::bucket_id`): the bucket an account or a slot lands in is SALT's own definition. The
//! FNV-1a test hasher of Satin's `TestExternalEnvs`, which the differential scenarios name buckets
//! with, replaces it only under feature `salt-test-hasher`.

mod convert;
mod fallback;

use self::fallback::RandomState;
use core::hash::{BuildHasher, Hasher};

const NUM_BUCKETS: u64 = 1 << 24;
const NUM_META_BUCKETS: u64 = NUM_BUCKETS / 256;
const NUM_KV_BUCKETS: u64 = NUM_BUCKETS - NUM_META_BUCKETS;
const HASHER_SEEDS: [u64; 4] = [0x921321f4, 0x2ccb667e, 0x60d68842, 0x077ada9d];

#[inline(always)]
fn hash(bytes: &[u8]) -> u64 {
    static HASH_BUILDER: RandomState =
        RandomState::with_seeds(HASHER_SEEDS[0], HASHER_SEEDS[1], HASHER_SEEDS[2], HASHER_SEEDS[3]);
    let mut hasher = HASH_BUILDER.build_hasher();
    hasher.write(bytes);
    hasher.finish()
}

/// Satin `AHashBucketHasher::bucket_id`.
#[inline(always)]
pub fn bucket_id(key: &[u8]) -> u64 {
    hash(key) % NUM_KV_BUCKETS + NUM_META_BUCKETS
}

#[cfg(test)]
mod tests {
    #[test]
    fn stable() {
        assert_eq!(super::hash(b"hello"), 1027176506268606463);
        assert_eq!(super::hash(b"world"), 2337896903564117184);
    }
}
