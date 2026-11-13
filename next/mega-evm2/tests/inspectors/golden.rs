//! The golden files: each scenario's tracer output, pinned in `golden/<scenario>.json` and read
//! back, so that a change in a trace's shape or figures fails the scenario's test.
//!
//! `MEGA_EVM2_BLESS_TRACES=1` writes the files instead of comparing them. A blessed change is a
//! change in what a node returns to its callers: review the diff of every file it moves.

use serde_json::Value;
use std::path::PathBuf;

/// The golden file of `scenario`.
pub fn path(scenario: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/inspectors/golden")
        .join(format!("{scenario}.json"))
}

fn bless() -> bool {
    std::env::var_os("MEGA_EVM2_BLESS_TRACES").is_some()
}

/// `value` with every object's keys sorted, rendered as pretty JSON with a final newline.
pub fn render(value: &Value) -> String {
    fn sorted(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                Value::Object(keys.into_iter().map(|k| (k.clone(), sorted(&map[k]))).collect())
            }
            Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
            other => other.clone(),
        }
    }
    let mut text = serde_json::to_string_pretty(&sorted(value)).expect("JSON");
    text.push('\n');
    text
}

/// Up to `max` JSON pointers at which `got` differs from `want`, with both values.
fn differences(want: &Value, got: &Value, at: &str, out: &mut Vec<String>, max: usize) {
    if out.len() >= max || want == got {
        return;
    }
    match (want, got) {
        (Value::Object(w), Value::Object(g)) => {
            let mut keys: Vec<&String> = w.keys().chain(g.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                let (w, g) = (w.get(k).unwrap_or(&Value::Null), g.get(k).unwrap_or(&Value::Null));
                differences(w, g, &format!("{at}/{k}"), out, max);
            }
        }
        (Value::Array(w), Value::Array(g)) if w.len() == g.len() => {
            for (i, (w, g)) in w.iter().zip(g).enumerate() {
                differences(w, g, &format!("{at}/{i}"), out, max);
            }
        }
        _ => {
            let short = |v: &Value| {
                let s = v.to_string();
                if s.len() > 200 { format!("{}…", &s[..200]) } else { s }
            };
            out.push(format!("{at}: golden {} got {}", short(want), short(got)));
        }
    }
}

/// Holds `traces`, the output of every tracer for `scenario`, to its golden file (or writes the
/// file, under `MEGA_EVM2_BLESS_TRACES`).
pub fn check(scenario: &str, traces: &Value) {
    let path = path(scenario);
    let text = render(traces);
    if bless() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
        return;
    }
    let golden = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}; MEGA_EVM2_BLESS_TRACES=1 writes it", path.display()));
    if golden == text {
        return;
    }
    let want: Value = serde_json::from_str(&golden).expect("a golden file is JSON");
    let mut out = Vec::new();
    differences(&want, traces, "", &mut out, 12);
    panic!(
        "{scenario}: the traces differ from {} ({} shown; MEGA_EVM2_BLESS_TRACES=1 rewrites it, \
         review the diff):\n{}",
        path.display(),
        out.len(),
        out.join("\n")
    );
}
