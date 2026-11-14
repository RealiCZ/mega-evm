//! Replays the exact Keccak inputs the engine hashed (KECCAK256 opcode inputs or receipt-Bloom
//! inputs, captured by `kec-trace`) through bounded exact-input caches, separately from the EVM.
//!
//! Trace format: little-endian u32 length then the bytes; `u32::MAX` marks a transaction
//! boundary, `u32::MAX - 1` a block boundary. Every mode's digests are checked against uncached
//! hashing before any timing. Caches cover 64-byte inputs (and, for a Bloom trace, 20 and 32
//! bytes), are keyed by the complete input, clear at their scope's boundary and on exhaustion.

use std::{collections::HashMap, hint::black_box, time::Instant};

enum Event<'a> {
    Tx,
    Block,
    Hash(&'a [u8]),
}

fn decode(b: &[u8]) -> Vec<Event<'_>> {
    let mut p = 0;
    let mut out = Vec::new();
    while p < b.len() {
        let n = u32::from_le_bytes(b[p..p + 4].try_into().unwrap());
        p += 4;
        match n {
            u32::MAX => out.push(Event::Tx),
            n if n == u32::MAX - 1 => out.push(Event::Block),
            n => {
                out.push(Event::Hash(&b[p..p + n as usize]));
                p += n as usize;
            }
        }
    }
    out
}

fn hash(b: &[u8], asm: bool) -> [u8; 32] {
    if b.is_empty() {
        return alloy_primitives::KECCAK256_EMPTY.0;
    }
    if asm { keccak_asm::Keccak256::digest(b).into() } else { alloy_primitives::keccak256(b).0 }
}

/// Scope 0: no cache; 1: cleared at every transaction; 2: cleared at every block.
fn run(
    events: &[Event<'_>],
    scope: u8,
    cap: usize,
    asm: bool,
    bloom: bool,
    expected: Option<&[[u8; 32]]>,
) -> (u64, u64, [u8; 32]) {
    let mut map = HashMap::<(u8, [u8; 64]), [u8; 32]>::with_capacity(cap);
    let mut hits = 0;
    let mut misses = 0;
    let mut check = [0u8; 32];
    let mut i = 0;
    for event in events {
        match event {
            Event::Tx => {
                if scope == 1 {
                    map.clear();
                }
            }
            Event::Block => map.clear(),
            Event::Hash(b) => {
                let cacheable = b.len() == 64 || bloom && (b.len() == 20 || b.len() == 32);
                let h = if scope == 0 || !cacheable {
                    hash(black_box(b), asm)
                } else {
                    let mut k = [0u8; 64];
                    k[..b.len()].copy_from_slice(b);
                    let key = (b.len() as u8, k);
                    if let Some(h) = map.get(&key) {
                        hits += 1;
                        *h
                    } else {
                        misses += 1;
                        let h = hash(black_box(b), asm);
                        if map.len() >= cap {
                            map.clear();
                        }
                        map.insert(key, h);
                        h
                    }
                };
                if let Some(ex) = expected {
                    assert_eq!(h, ex[i], "event {i}");
                }
                i += 1;
                for j in 0..32 {
                    check[j] ^= h[j];
                }
                black_box(h);
            }
        }
    }
    (hits, misses, check)
}

fn main() {
    let path = std::env::args().nth(1).expect("trace path");
    let bloom = path.contains("bloom");
    let variant = std::env::var("KEC_VARIANT").unwrap_or_else(|_| "unlabelled".into());
    let reps: usize = std::env::var("KEC_REPS").map(|v| v.parse().expect("KEC_REPS")).unwrap_or(7);
    let data = std::fs::read(&path).expect("trace file");
    let events = decode(&data);
    let expected: Vec<_> = events
        .iter()
        .filter_map(|e| if let Event::Hash(b) = e { Some(hash(b, false)) } else { None })
        .collect();
    let modes = [
        ("baseline", 0, 0, false),
        ("asm", 0, 0, true),
        ("tx1024", 1, 1024, false),
        ("block1024", 2, 1024, false),
        ("block4096", 2, 4096, false),
        ("asm_block4096", 2, 4096, true),
    ];
    for (_, scope, cap, asm) in modes {
        run(&events, scope, cap, asm, bloom, Some(&expected));
    }
    eprintln!("trace={path} hashes={} validated=all-modes", expected.len());
    let trace = if bloom { "bloom" } else { "opcode" };
    println!("variant,trace,rep,mode,ms,hits,misses");
    for rep in 0..reps {
        for k in 0..modes.len() {
            let (name, scope, cap, asm) = modes[(k + rep) % modes.len()];
            let t = Instant::now();
            let (hits, misses, check) = run(&events, scope, cap, asm, bloom, None);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            black_box(check);
            println!("{variant},{trace},{rep},{name},{ms:.6},{hits},{misses}");
        }
    }
}
