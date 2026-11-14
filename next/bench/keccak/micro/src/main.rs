//! Keccak-256 backends, one-shot, at the input sizes the engine hashes.
//!
//! Every run first checks each backend against RustCrypto on 287 lengths, unaligned input,
//! segmented streaming and the empty-input constant; `VALIDATE_ONLY=1` stops there.
//! Then it prints one CSV row per (repetition, implementation, length) sample to stdout. The
//! implementation order rotates every repetition, so no backend always runs first or last.
//!
//! Environment: `KEC_VARIANT` labels the rows (the build's ISA flags), `KEC_REPS` (default 7)
//! and `KEC_SAMPLE_MS` (default 15) set the number of repetitions and the length of one sample.

mod features;

use std::{hint::black_box, time::Instant};

type HashFn = fn(&[u8]) -> [u8; 32];

fn rustcrypto(b: &[u8]) -> [u8; 32] {
    use sha3::Digest;
    sha3::Keccak256::digest(b).into()
}

fn tiny(b: &[u8]) -> [u8; 32] {
    use tiny_keccak::Hasher;
    let mut h = tiny_keccak::Keccak::v256();
    h.update(b);
    let mut o = [0; 32];
    h.finalize(&mut o);
    o
}

fn asm(b: &[u8]) -> [u8; 32] {
    keccak_asm::Keccak256::digest(b).into()
}

fn alloy(b: &[u8]) -> [u8; 32] {
    alloy_primitives::keccak256(b).0
}

fn run(f: HashFn, b: &[u8], n: usize) -> f64 {
    let now = Instant::now();
    for _ in 0..n {
        black_box(f(black_box(b)));
    }
    now.elapsed().as_secs_f64() * 1e9 / n as f64
}

fn env_or<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().map(|v| v.parse().ok().expect(name)).unwrap_or(default)
}

fn validate(fs: &[(&str, HashFn)]) {
    for n in (0..=280).chain([512, 1024, 4096, 16384, 24576, 65536]) {
        let b: Vec<u8> = (0..n).map(|i| (i * 71 + 17) as u8).collect();
        let expected = rustcrypto(&b);
        for (name, f) in fs {
            assert_eq!(f(&b), expected, "{name} size {n}");
        }
        let mut shifted = vec![0u8; 3];
        shifted.extend_from_slice(&b);
        for (name, f) in fs {
            assert_eq!(f(&shifted[3..]), expected, "unaligned {name} size {n}");
        }
        for chunk in [1, 7, 32, 136] {
            use keccak_asm::Digest as _;
            use sha3::Digest;
            let mut rc = sha3::Keccak256::new();
            let mut ka = keccak_asm::Keccak256::new();
            let mut al = alloy_primitives::Keccak256::new();
            let mut tk = tiny_keccak::Keccak::v256();
            for part in b.chunks(chunk) {
                rc.update(part);
                ka.update(part);
                al.update(part);
                tiny_keccak::Hasher::update(&mut tk, part);
            }
            let mut t = [0u8; 32];
            tiny_keccak::Hasher::finalize(tk, &mut t);
            assert_eq!(<[u8; 32]>::from(rc.finalize()), expected);
            assert_eq!(<[u8; 32]>::from(ka.finalize()), expected);
            assert_eq!(al.finalize().0, expected);
            assert_eq!(t, expected);
        }
    }
    // Keccak-256 of the empty input, a known answer independent of every backend here.
    let empty =
        alloy_primitives::hex!("c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470");
    for (name, f) in fs {
        assert_eq!(f(&[]), empty, "{name} empty-input constant");
    }
}

fn main() {
    let fs: [(&str, HashFn); 4] =
        [("rustcrypto", rustcrypto), ("tiny", tiny), ("asm", asm), ("alloy", alloy)];
    validate(&fs);
    let variant = std::env::var("KEC_VARIANT").unwrap_or_else(|_| "unlabelled".into());
    eprintln!("{}", features::describe(&variant));
    eprintln!(
        "validation=ok variant={variant} 287-lengths 4-backends unaligned segmented-1-7-32-136 empty-KAT"
    );
    if std::env::var_os("VALIDATE_ONLY").is_some() {
        return;
    }
    // Warm-up, so the first implementation of the first repetition does not pay for the CPU
    // leaving idle or for first-touch page faults.
    let warm: Vec<u8> = (0..64u32).map(|i| i as u8).collect();
    for (_, f) in fs {
        run(f, &warm, 200_000);
    }
    let reps: usize = env_or("KEC_REPS", 7);
    let sample_ns = env_or("KEC_SAMPLE_MS", 15.0) * 1e6;
    println!("variant,rep,implementation,len,iterations,ns_per_hash");
    let sizes = [0, 20, 32, 64, 85, 96, 135, 136, 137, 272, 1024, 4096, 16384, 24576, 65536];
    for rep in 0..reps {
        for len in sizes {
            let b: Vec<u8> = (0..len).map(|i| (i * 71 + 17) as u8).collect();
            for k in 0..fs.len() {
                let (name, f) = fs[(k + rep) % fs.len()];
                let trial = run(f, &b, 300);
                let n = (sample_ns / trial).round().clamp(100.0, 500_000.0) as usize;
                let ns = run(f, &b, n);
                println!("{variant},{rep},{name},{len},{n},{ns:.4}");
            }
        }
    }
}
