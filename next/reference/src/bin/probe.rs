//! The reference side of the two-sided probe (`next/diff/scripts/probe.sh`).
//!
//! Usage: `probe <probes.jsonl> <out.jsonl>`
//!
//! Reads probe inputs (one JSON object per line, `mega_evm2_scenarios::probe`), runs each on the
//! Satin implementation under review exactly as the synthetic gate's reference generator does
//! (`synthetic::run`: plain and inspected), and writes one outcome record per input to
//! `<out.jsonl>`, one JSON line each, in input order. Exit status 2 on unreadable input.

use mega_evm2_reference::synthetic::run;
use mega_evm2_scenarios::probe;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, out] = args.as_slice() else {
        eprintln!("usage: probe <probes.jsonl> <out.jsonl>");
        std::process::exit(2);
    };
    let text = std::fs::read_to_string(input).unwrap_or_else(|e| fail(&format!("{input}: {e}")));
    let probes = probe::parse_file(&text).unwrap_or_else(|e| fail(&format!("{input}: {e}")));
    // Deep call chains need more stack than a main thread may have: run on a thread of its own,
    // as the gates' tests run with `RUST_MIN_STACK`.
    let lines = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            probes
                .iter()
                .map(|s| serde_json::to_string(&run(s)).expect("record") + "\n")
                .collect::<String>()
        })
        .expect("probe thread")
        .join()
        .unwrap_or_else(|_| fail("the reference panicked on a probe"));
    std::fs::write(out, lines).unwrap_or_else(|e| fail(&format!("{out}: {e}")));
}

fn fail(msg: &str) -> ! {
    eprintln!("probe (reference): {msg}");
    std::process::exit(2)
}
