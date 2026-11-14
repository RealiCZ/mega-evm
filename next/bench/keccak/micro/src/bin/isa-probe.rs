//! Prints which ISA variants of the benchmarks this CPU can run, as `name=yes|no` lines, then the
//! CPU's features as JSON. Built for the generic target, so it runs everywhere.

#[path = "../features.rs"]
mod features;

fn main() {
    let d = features::detected();
    let all = |fs: &[&str]| fs.iter().all(|f| d.contains(f));
    let v2 = ["sse2", "ssse3", "sse4.1", "sse4.2", "popcnt"];
    let v3 = ["avx", "avx2", "bmi1", "bmi2", "f16c", "fma", "lzcnt", "movbe", "xsave"];
    let v4 = ["avx512f", "avx512vl", "avx512bw", "avx512dq", "avx512cd"];
    let yes = |b: bool| if b { "yes" } else { "no" };
    if cfg!(target_arch = "x86_64") {
        println!("x86-64-v3={}", yes(all(&v2) && all(&v3)));
        println!("x86-64-v4={}", yes(all(&v2) && all(&v3) && all(&v4)));
    }
    if cfg!(target_arch = "aarch64") {
        println!("sha3={}", yes(all(&["neon", "sha3"])));
    }
    eprintln!("{}", features::describe("probe"));
}
