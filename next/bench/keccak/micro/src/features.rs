//! What a binary was compiled for and what the CPU it runs on offers, as one JSON line.

/// The target features this binary was compiled with, among those that select a Keccak backend
/// or change its code generation.
pub fn compiled() -> Vec<&'static str> {
    let mut v = Vec::new();
    macro_rules! check {
        ($($f:tt),*) => {$( if cfg!(target_feature = $f) { v.push($f); } )*};
    }
    check!("sse2", "ssse3", "sse4.1", "sse4.2", "popcnt", "avx", "avx2", "bmi1", "bmi2", "fma");
    check!("lzcnt", "movbe", "avx512f", "avx512vl", "avx512bw", "avx512dq", "avx512cd");
    check!("neon", "aes", "sha2", "sha3");
    v
}

/// The features the running CPU reports, among the same list.
#[allow(unused_mut, clippy::vec_init_then_push)]
pub fn detected() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    #[cfg(target_arch = "x86_64")]
    {
        macro_rules! check {
            ($($f:tt),*) => {$( if std::arch::is_x86_feature_detected!($f) { v.push($f); } )*};
        }
        check!("sse2", "ssse3", "sse4.1", "sse4.2", "popcnt", "avx", "avx2", "bmi1", "bmi2");
        check!("fma", "lzcnt", "movbe", "f16c", "xsave", "avx512f", "avx512vl", "avx512bw");
        check!("avx512dq", "avx512cd");
    }
    #[cfg(target_arch = "aarch64")]
    {
        macro_rules! check {
            ($($f:tt),*) => {$( if std::arch::is_aarch64_feature_detected!($f) { v.push($f); } )*};
        }
        check!("neon", "aes", "sha2", "sha3");
    }
    v
}

fn list(v: &[&str]) -> String {
    let quoted: Vec<String> = v.iter().map(|f| format!("\"{f}\"")).collect();
    format!("[{}]", quoted.join(","))
}

/// One JSON line: the variant label, the architecture, keccak-asm's compiled backend, and both
/// feature lists.
pub fn describe(variant: &str) -> String {
    format!(
        "{{\"variant\":\"{variant}\",\"arch\":\"{}\",\"keccak_asm_impl\":\"{}\",\"compiled\":{},\"detected\":{}}}",
        std::env::consts::ARCH,
        keccak_asm::IMPL,
        list(&compiled()),
        list(&detected()),
    )
}
