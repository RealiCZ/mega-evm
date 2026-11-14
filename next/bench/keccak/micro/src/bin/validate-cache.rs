//! Checks Alloy's exact-input Keccak cache against uncached hashing on every captured input, with
//! a 4-entry cache, so replacement and collisions happen constantly. Each input is hashed twice
//! through the cache (a miss, then usually a hit) and once uncached.

fn main() {
    assert!(alloy_primitives::utils::init_keccak_cache(4));
    let mut calls = 0u64;
    for path in std::env::args().skip(1) {
        let b = std::fs::read(&path).expect("trace file");
        let mut p = 0;
        while p < b.len() {
            let n = u32::from_le_bytes(b[p..p + 4].try_into().unwrap());
            p += 4;
            if n >= u32::MAX - 1 {
                continue;
            }
            let input = &b[p..p + n as usize];
            p += n as usize;
            let expected = alloy_primitives::utils::keccak256_uncached(input);
            assert_eq!(alloy_primitives::utils::keccak256_cached(input), expected);
            assert_eq!(alloy_primitives::utils::keccak256_cached(input), expected);
            calls += 1;
        }
    }
    assert!(calls > 0, "no inputs");
    println!("cache-validation=ok inputs={calls} each-twice 4-entry-cache against uncached Alloy");
}
