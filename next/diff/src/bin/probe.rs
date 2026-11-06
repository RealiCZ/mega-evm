//! The engine side of the two-sided probe (`next/diff/scripts/probe.sh`).
//!
//! Usage: `probe <probes.jsonl> <reference.jsonl>`
//!
//! Reads probe inputs (one JSON object per line, `mega_evm2_scenarios::probe`) and the reference's
//! records of them (`next/reference`'s `probe`, one JSON line each, in input order), runs each
//! input on the Satin engine plain and inspected, and judges it as the synthetic gate does
//! (`gate::scenario_verdict`, accepted differences included). Prints per input:
//!
//! ```text
//! PROBE <name>
//!   reference <its record>
//!   engine    <its plain run's record, with its inspected run's frames>
//! MATCH <name> | KNOWN <name> | CLASS <name> <class> | DIFFER <name> [<differences>]
//!   <field>: reference <value> | engine <value>     (each differing field, normalized)
//! ```
//!
//! and last `PROBE total=<n> match=<n> known=<n> class=<n> differ=<n>`. Exit status 0 when nothing
//! differs, 1 when an input differs, 2 on unreadable input. `MEGA_EVM2_DISABLE` turns engine
//! switches off as in the gates.

use mega_evm2_diff::{
    driver::run,
    gate::{self, Verdict},
};
use mega_evm2_scenarios::probe;
use serde_json::Value;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, reference] = args.as_slice() else {
        eprintln!("usage: probe <probes.jsonl> <reference.jsonl>");
        std::process::exit(2);
    };
    let text = std::fs::read_to_string(input).unwrap_or_else(|e| fail(&format!("{input}: {e}")));
    let probes = probe::parse_file(&text).unwrap_or_else(|e| fail(&format!("{input}: {e}")));
    let records: Vec<Value> = std::fs::read_to_string(reference)
        .unwrap_or_else(|e| fail(&format!("{reference}: {e}")))
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| fail(&format!("{reference}: {e}"))))
        .collect();
    if records.len() != probes.len() {
        fail(&format!("{} probes but {} reference records", probes.len(), records.len()));
    }
    for (s, r) in probes.iter().zip(&records) {
        if r["name"] != s.name {
            fail(&format!("reference record {} for probe {}", r["name"], s.name));
        }
    }
    // Deep call chains need more stack than a main thread may have (see the reference's probe).
    let differ = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || judge(&probes, &records))
        .expect("probe thread")
        .join()
        .unwrap_or_else(|_| fail("the engine panicked on a probe"));
    std::process::exit(if differ { 1 } else { 0 });
}

/// Runs and judges every probe; whether any differs.
fn judge(probes: &[mega_evm2_scenarios::Scenario], records: &[Value]) -> bool {
    let known = gate::known_differences();
    let classes = gate::known_classes();
    let (mut matched, mut accepted, mut class, mut differ) = (0, 0, 0, 0);
    for (s, reference) in probes.iter().zip(records) {
        let plain = run(s, false);
        let inspected = run(s, true);
        let mut engine = plain.clone();
        if let (Some(o), Some(f)) = (engine.as_object_mut(), inspected.get("frames")) {
            o.insert("frames".into(), f.clone());
        }
        println!("PROBE {}", s.name);
        println!("  reference {reference}");
        println!("  engine    {engine}");
        match gate::scenario_verdict(&known, &classes, s.name, reference, &plain, &inspected) {
            Verdict::Match => {
                matched += 1;
                println!("MATCH {}", s.name);
            }
            Verdict::Known => {
                accepted += 1;
                println!("KNOWN {}", s.name);
            }
            Verdict::Class(c) => {
                class += 1;
                println!("CLASS {} {c}", s.name);
            }
            Verdict::Differ(d) => {
                differ += 1;
                println!("DIFFER {} {d:?}", s.name);
                let (a, b) = (gate::canonical(reference), gate::canonical(&plain));
                for field in
                    d.iter().filter(|f| a.get(f.as_str()).is_some() || b.get(f.as_str()).is_some())
                {
                    let show =
                        |v: Option<&Value>| v.map_or("(absent)".to_string(), Value::to_string);
                    println!(
                        "  {field}: reference {} | engine {}",
                        show(a.get(field.as_str())),
                        show(b.get(field.as_str()))
                    );
                }
            }
        }
    }
    println!(
        "PROBE total={} match={matched} known={accepted} class={class} differ={differ}",
        probes.len()
    );
    differ > 0
}

fn fail(msg: &str) -> ! {
    eprintln!("probe (engine): {msg}");
    std::process::exit(2)
}
