//! Lists the negative controls of a run, for `scripts/controls.sh`.
//!
//! Usage: `negative-controls <manifest> <engine source dir> sample|full`
//!
//! Prints `CONTROL <switch> <gate> <scope> <case>` for every control the run runs (see
//! `controls::in_scope`; a stale control is not run), then `UNLISTED <name>` for every switch of
//! the source without a control and `STALE <name>` for every control whose switch is not in the
//! source. A malformed manifest, an
//! unreadable source directory or a bad argument exits with status 2.

use mega_evm2_diff::controls::{self, Scope};
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let fail = |msg: String| -> ! {
        eprintln!("negative-controls: {msg}");
        std::process::exit(2)
    };
    let [manifest, src, scope] = args.as_slice() else {
        fail("usage: negative-controls <manifest> <engine source dir> sample|full".into())
    };
    let scope = match scope.as_str() {
        "sample" => Scope::Sample,
        "full" => Scope::Full,
        other => fail(format!("unknown scope `{other}`")),
    };
    let text =
        std::fs::read_to_string(manifest).unwrap_or_else(|e| fail(format!("{manifest}: {e}")));
    let list = controls::parse(&text).unwrap_or_else(|e| fail(e));
    let source =
        controls::source_switches(Path::new(src)).unwrap_or_else(|e| fail(format!("{src}: {e}")));
    if source.is_empty() {
        fail(format!("no `off(\"…\")` switch under {src}"));
    }
    let (unlisted, stale) = controls::completeness(&list, &source);
    for c in controls::in_scope(&list, scope) {
        if !stale.contains(&c.switch) {
            println!("CONTROL {} {} {} {}", c.switch, c.gate, c.scope, c.case);
        }
    }
    for name in unlisted {
        println!("UNLISTED {name}");
    }
    for name in stale {
        println!("STALE {name}");
    }
}
