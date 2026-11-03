//! Lists the negative controls of a run, for `scripts/controls.sh`.
//!
//! Usage: `negative-controls <manifest> <engine source dir> <harness source dir> sample|full`
//!
//! Prints `CONTROL <switch> <gate> <scope> <case>` for every control the run runs (see
//! `controls::in_scope`; a stale control is not run), then `UNLISTED <name> <table>` for every
//! switch of a source without a control in its table (`control` for the engine source, `harness`
//! for the harness source) and `STALE <name> <table>` for every control whose switch is not in its
//! table's source. A malformed manifest, an unreadable source directory, an engine source without
//! a switch or a bad argument exits with status 2.

use mega_evm2_diff::controls::{self, Kind, Scope};
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let fail = |msg: String| -> ! {
        eprintln!("negative-controls: {msg}");
        std::process::exit(2)
    };
    let [manifest, engine, harness, scope] = args.as_slice() else {
        fail("usage: negative-controls <manifest> <engine source dir> <harness source dir> sample|full".into())
    };
    let scope = match scope.as_str() {
        "sample" => Scope::Sample,
        "full" => Scope::Full,
        other => fail(format!("unknown scope `{other}`")),
    };
    let text =
        std::fs::read_to_string(manifest).unwrap_or_else(|e| fail(format!("{manifest}: {e}")));
    let list = controls::parse(&text).unwrap_or_else(|e| fail(e));
    let mut unlisted = Vec::new();
    let mut stale = Vec::new();
    for (kind, src) in [(Kind::Engine, engine), (Kind::Harness, harness)] {
        let source = controls::source_switches(Path::new(src))
            .unwrap_or_else(|e| fail(format!("{src}: {e}")));
        if kind == Kind::Engine && source.is_empty() {
            fail(format!("no `off(\"…\")` switch under {src}"));
        }
        let (u, s) = controls::completeness(&list, kind, &source);
        unlisted.extend(u.into_iter().map(|name| (name, kind)));
        stale.extend(s.into_iter().map(|name| (name, kind)));
    }
    for c in controls::in_scope(&list, scope) {
        if !stale.contains(&(c.switch.clone(), c.kind)) {
            println!("CONTROL {} {} {} {}", c.switch, c.gate, c.scope, c.case);
        }
    }
    for (name, kind) in unlisted {
        println!("UNLISTED {name} {}", kind.table());
    }
    for (name, kind) in stale {
        println!("STALE {name} {}", kind.table());
    }
}
