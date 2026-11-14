//! The engine's prefix-extension hash: 64 bytes as two 32-byte streaming updates, against the
//! same 64 bytes in one shot, per backend. Rotating order; one CSV row per sample.

use std::{hint::black_box, time::Instant};

type HashFn = fn(&[u8]) -> [u8; 32];

fn rustcrypto_one(b: &[u8]) -> [u8; 32] {
    use sha3::Digest;
    sha3::Keccak256::digest(b).into()
}
fn rustcrypto_stream(b: &[u8]) -> [u8; 32] {
    use sha3::Digest;
    let mut h = sha3::Keccak256::new();
    h.update(&b[..32]);
    h.update(&b[32..]);
    h.finalize().into()
}
fn tiny_one(b: &[u8]) -> [u8; 32] {
    use tiny_keccak::Hasher;
    let mut h = tiny_keccak::Keccak::v256();
    h.update(b);
    let mut o = [0; 32];
    h.finalize(&mut o);
    o
}
fn tiny_stream(b: &[u8]) -> [u8; 32] {
    use tiny_keccak::Hasher;
    let mut h = tiny_keccak::Keccak::v256();
    h.update(&b[..32]);
    h.update(&b[32..]);
    let mut o = [0; 32];
    h.finalize(&mut o);
    o
}
fn asm_one(b: &[u8]) -> [u8; 32] {
    keccak_asm::Keccak256::digest(b).into()
}
fn asm_stream(b: &[u8]) -> [u8; 32] {
    use keccak_asm::Digest;
    let mut h = keccak_asm::Keccak256::new();
    h.update(&b[..32]);
    h.update(&b[32..]);
    h.finalize().into()
}
fn alloy_one(b: &[u8]) -> [u8; 32] {
    alloy_primitives::keccak256(b).0
}
fn alloy_stream(b: &[u8]) -> [u8; 32] {
    let mut h = alloy_primitives::Keccak256::new();
    h.update(&b[..32]);
    h.update(&b[32..]);
    h.finalize().0
}

fn run(f: HashFn, b: &[u8], n: usize) -> f64 {
    let t = Instant::now();
    for _ in 0..n {
        black_box(f(black_box(b)));
    }
    t.elapsed().as_secs_f64() * 1e9 / n as f64
}

fn main() {
    let variant = std::env::var("KEC_VARIANT").unwrap_or_else(|_| "unlabelled".into());
    let reps: usize = std::env::var("KEC_REPS").map(|v| v.parse().expect("KEC_REPS")).unwrap_or(7);
    let b: [u8; 64] = std::array::from_fn(|i| (i * 71 + 17) as u8);
    let fs: [(&str, HashFn); 8] = [
        ("rustcrypto_one", rustcrypto_one),
        ("rustcrypto_stream", rustcrypto_stream),
        ("tiny_one", tiny_one),
        ("tiny_stream", tiny_stream),
        ("asm_one", asm_one),
        ("asm_stream", asm_stream),
        ("alloy_one", alloy_one),
        ("alloy_stream", alloy_stream),
    ];
    for (name, f) in fs {
        assert_eq!(f(&b), rustcrypto_one(&b), "{name}");
    }
    // Warm-up, as in kec-micro.
    for (_, f) in fs {
        run(f, &b, 200_000);
    }
    println!("variant,rep,mode,iterations,ns");
    for rep in 0..reps {
        for k in 0..fs.len() {
            let (name, f) = fs[(rep + k) % fs.len()];
            let n = 100_000;
            let ns = run(f, &b, n);
            println!("{variant},{rep},{name},{n},{ns:.3}");
        }
    }
}
