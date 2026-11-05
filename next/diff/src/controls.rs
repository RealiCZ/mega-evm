//! The negative-control manifest (`next/diff/negative-controls.toml`): for every switch
//! (`mega_evm2::off("…")`), the gate case that must fail when the switch is off. The engine's
//! switches (`next/mega-evm2/src`) are `[[control]]` tables, the gate harness's own fixture
//! switches (`next/diff`) are `[[harness]]` tables.
//!
//! `scripts/controls.sh` runs the controls; this module parses and validates the manifest and
//! checks each table against the switch names in its source.

use std::{collections::BTreeSet, fmt, path::Path};

/// Where a control's switch is: a fix of the engine (`[[control]]`, `next/mega-evm2/src`) or a
/// fixture choice of the gate harness (`[[harness]]`, `next/diff`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Engine,
    Harness,
}

impl Kind {
    /// The manifest table of this kind.
    pub fn table(self) -> &'static str {
        match self {
            Kind::Engine => "control",
            Kind::Harness => "harness",
        }
    }
}

/// The gate a control runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    Main,
    Fuzz,
    Replay,
    Block,
}

/// The data a control's case is in: the in-repo sample, or only the full corpus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Sample,
    Full,
}

/// The one case a control runs its gate on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Case {
    /// A scenario of the main gate, by name.
    Scenario(String),
    /// One seed of a fuzz mode.
    Fuzz { mode: u8, seed: u64 },
    /// A recorded block, `set:number`.
    Block { set: String, number: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Control {
    pub kind: Kind,
    pub switch: String,
    pub gate: Gate,
    pub case: Case,
    pub scope: Scope,
    pub why: String,
}

impl fmt::Display for Gate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Gate::Main => "main",
            Gate::Fuzz => "fuzz",
            Gate::Replay => "replay",
            Gate::Block => "block",
        })
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Scope::Sample => "sample",
            Scope::Full => "full",
        })
    }
}

impl fmt::Display for Case {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Case::Scenario(name) => f.write_str(name),
            Case::Fuzz { mode, seed } => write!(f, "{mode}:{seed}:{}", fuzz_name(*mode, *seed)),
            Case::Block { set, number } => write!(f, "{set}:{number}"),
        }
    }
}

/// The scenario name of fuzz `mode`'s seed `seed`, as the generator names it (a control selects its
/// one scenario by this name; a different name selects nothing, and the control's baseline fails).
pub fn fuzz_name(mode: u8, seed: u64) -> String {
    mega_evm2_scenarios::fz::name(mode, seed)
}

/// Parses the manifest, the `[[control]]` tables first. Fails on an unknown table, a missing or
/// unknown key, a selector of another gate, an unknown gate or scope, an empty `why`, and a
/// switch listed twice (in one table or in both).
pub fn parse(text: &str) -> Result<Vec<Control>, String> {
    let doc: toml::Table = text.parse().map_err(|e| format!("negative-controls.toml: {e}"))?;
    let kinds = [Kind::Engine, Kind::Harness];
    if let Some(key) = doc.keys().find(|k| !kinds.iter().any(|kind| kind.table() == *k)) {
        return Err(format!("negative-controls.toml: unknown table `{key}`"));
    }
    let mut seen = BTreeSet::new();
    let mut controls = Vec::new();
    for kind in kinds {
        let name = kind.table();
        let entries =
            doc.get(name).map(|v| v.as_array().ok_or(format!("`{name}` is not a list of tables")));
        for (i, entry) in entries.transpose()?.into_iter().flatten().enumerate() {
            let table = entry.as_table().ok_or(format!("{name} {i}: not a table"))?;
            let control = control(kind, table).map_err(|e| format!("{name} {i}: {e}"))?;
            if !seen.insert(control.switch.clone()) {
                return Err(format!("{name} {i}: switch `{}` is listed twice", control.switch));
            }
            controls.push(control);
        }
    }
    if controls.is_empty() {
        return Err("negative-controls.toml: no control".into());
    }
    Ok(controls)
}

fn control(kind: Kind, t: &toml::Table) -> Result<Control, String> {
    let text = |k: &str| -> Result<String, String> {
        t.get(k).and_then(toml::Value::as_str).map(str::to_owned).ok_or(format!("missing `{k}`"))
    };
    let int = |k: &str| -> Result<i64, String> {
        t.get(k).and_then(toml::Value::as_integer).ok_or(format!("missing integer `{k}`"))
    };
    let switch = text("switch")?;
    let (gate, selectors): (Gate, &[&str]) = match text("gate")?.as_str() {
        "main" => (Gate::Main, &["scenario"]),
        "fuzz" => (Gate::Fuzz, &["mode", "seed"]),
        "replay" => (Gate::Replay, &["block"]),
        "block" => (Gate::Block, &["block"]),
        other => return Err(format!("unknown gate `{other}` (main, fuzz, replay or block)")),
    };
    let common = ["switch", "gate", "scope", "why"];
    if let Some(key) =
        t.keys().find(|k| !common.contains(&k.as_str()) && !selectors.contains(&k.as_str()))
    {
        return Err(format!("`{key}` is not a key of a {gate} control"));
    }
    let case = match gate {
        Gate::Main => Case::Scenario(text("scenario")?),
        Gate::Fuzz => {
            let mode = int("mode")?;
            let seed = int("seed")?;
            if !mega_evm2_scenarios::fz::MODES.iter().any(|m| i64::from(m.0) == mode) {
                return Err(format!("fuzz `mode` {mode} is not a mode of the generator"));
            }
            if seed < 1 {
                return Err(format!("fuzz `seed` {seed} is not positive"));
            }
            Case::Fuzz { mode: mode as u8, seed: seed as u64 }
        }
        Gate::Replay | Gate::Block => {
            let block = text("block")?;
            let parsed = block
                .split_once(':')
                .and_then(|(set, n)| Some((set, n.parse::<u64>().ok()?)))
                .filter(|(set, _)| !set.is_empty());
            let Some((set, number)) = parsed else {
                return Err(format!("`block` {block:?} is not `<set>:<number>`"));
            };
            Case::Block { set: set.to_owned(), number }
        }
    };
    let scope = match text("scope")?.as_str() {
        "sample" => Scope::Sample,
        "full" => Scope::Full,
        other => return Err(format!("unknown scope `{other}` (sample or full)")),
    };
    let why = text("why")?;
    if why.trim().is_empty() {
        return Err("empty `why`".into());
    }
    Ok(Control { kind, switch, gate, case, scope, why })
}

/// Every name that occurs as a call `off("<name>")` (a name of `a-z`, `0-9` and `_`) in the
/// `.rs` files under `dir`. Whitespace, line breaks included, may stand on either side of the
/// string literal, and a trailing comma after it, as rustfmt writes a call it breaks across
/// lines. The scan is textual: a comment that spells such a call counts as one.
pub fn source_switches(dir: &Path) -> std::io::Result<BTreeSet<String>> {
    let mut found = BTreeSet::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for entry in std::fs::read_dir(&d)? {
            let path = entry?.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path)?;
                found.extend(off_calls(&text));
            }
        }
    }
    Ok(found)
}

/// The names of the `off("…")` calls in `text` (see `source_switches`).
fn off_calls(text: &str) -> impl Iterator<Item = String> + '_ {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    text.match_indices("off(").filter_map(move |(at, _)| {
        // `off` itself, not the end of a longer name such as `cutoff`.
        if text[..at].chars().next_back().is_some_and(ident) {
            return None;
        }
        let rest = text[at + 4..].trim_start().strip_prefix('"')?;
        let len = rest
            .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
            .unwrap_or(rest.len());
        let after = rest[len..].strip_prefix('"')?.trim_start();
        let after = after.strip_prefix(',').map_or(after, str::trim_start);
        (len > 0 && after.starts_with(')')).then(|| rest[..len].to_owned())
    })
}

/// The switches of `source` (the switches of `kind`'s source) without a control of `kind`
/// (unlisted), and the controls of `kind` whose switch is not in `source` (stale).
pub fn completeness(
    controls: &[Control],
    kind: Kind,
    source: &BTreeSet<String>,
) -> (Vec<String>, Vec<String>) {
    let listed: BTreeSet<String> =
        controls.iter().filter(|c| c.kind == kind).map(|c| c.switch.clone()).collect();
    (source.difference(&listed).cloned().collect(), listed.difference(source).cloned().collect())
}

/// The controls a run of `scope` runs: `sample` runs the sample's, `full` runs all of them (the
/// full corpus holds every sample case).
pub fn in_scope(controls: &[Control], scope: Scope) -> Vec<&Control> {
    controls.iter().filter(|c| scope == Scope::Full || c.scope == Scope::Sample).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(body: &str) -> Result<Vec<Control>, String> {
        parse(&format!("[[control]]\nswitch = \"s\"\nscope = \"sample\"\nwhy = \"w\"\n{body}"))
    }

    #[test]
    fn test_each_gate_reads_its_case() {
        let main = one("gate = \"main\"\nscenario = \"p6_add\"").unwrap();
        assert_eq!(
            main,
            vec![Control {
                kind: Kind::Engine,
                switch: "s".into(),
                gate: Gate::Main,
                case: Case::Scenario("p6_add".into()),
                scope: Scope::Sample,
                why: "w".into(),
            }]
        );
        let fuzz = one("gate = \"fuzz\"\nmode = 4\nseed = 1788").unwrap();
        assert_eq!(fuzz[0].case, Case::Fuzz { mode: 4, seed: 1788 });
        assert_eq!(fuzz[0].case.to_string(), "4:1788:fqh1788");
        for gate in ["replay", "block"] {
            let c = one(&format!("gate = \"{gate}\"\nblock = \"d4:2045117\"")).unwrap();
            assert_eq!(c[0].case, Case::Block { set: "d4".into(), number: 2045117 });
            assert_eq!(c[0].gate.to_string(), gate);
        }
    }

    #[test]
    fn test_fuzz_names_follow_the_generator() {
        assert_eq!(fuzz_name(1, 87), "fz087");
        assert_eq!(fuzz_name(2, 323), "fh0323");
        assert_eq!(fuzz_name(3, 663), "fq0663");
        assert_eq!(fuzz_name(4, 1788), "fqh1788");
    }

    #[test]
    fn test_malformed_controls_are_rejected() {
        for (body, needle) in [
            ("gate = \"main\"", "scenario"),
            ("gate = \"main\"\nscenario = \"a\"\nblock = \"d4:1\"", "block"),
            ("gate = \"main\"\nscenario = \"a\"\nseeds = 1", "seeds"),
            ("gate = \"fuzz\"\nmode = 4", "seed"),
            ("gate = \"fuzz\"\nmode = 5\nseed = 1", "mode"),
            ("gate = \"fuzz\"\nmode = 4\nseed = 0", "seed"),
            ("gate = \"replay\"\nblock = \"2045117\"", "block"),
            ("gate = \"block\"\nblock = \"d4:x\"", "block"),
            ("gate = \"other\"\nscenario = \"a\"", "gate"),
        ] {
            let err = one(body).expect_err(body);
            assert!(err.contains(needle), "{body}: {err}");
        }
        let err = parse("[[control]]\nswitch = \"s\"\ngate = \"main\"\nscenario = \"a\"\nscope = \"all\"\nwhy = \"w\"")
            .unwrap_err();
        assert!(err.contains("scope"), "{err}");
        let err = parse("[[control]]\nswitch = \"s\"\ngate = \"main\"\nscenario = \"a\"\nscope = \"full\"\nwhy = \" \"")
            .unwrap_err();
        assert!(err.contains("why"), "{err}");
        let err =
            parse("[[control]]\ngate = \"main\"\nscenario = \"a\"\nscope = \"full\"\nwhy = \"w\"")
                .unwrap_err();
        assert!(err.contains("switch"), "{err}");
        assert!(parse("[[other]]\nx = 1").unwrap_err().contains("other"));
        assert!(parse("").unwrap_err().contains("no control"));
    }

    #[test]
    fn test_a_switch_listed_twice_is_rejected() {
        let c = "[[control]]\nswitch = \"s\"\ngate = \"main\"\nscenario = \"a\"\nscope = \"sample\"\nwhy = \"w\"\n";
        let err = parse(&format!("{c}{c}")).unwrap_err();
        assert!(err.contains("`s`") && err.contains("twice"), "{err}");
    }

    #[test]
    fn test_source_switches_are_every_off_call_under_the_directory() {
        let dir = std::env::temp_dir().join(format!("mega-evm2-controls-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(
            dir.join("a.rs"),
            "if crate::off(\"alpha\") || !off(\"beta_2\") {}\n// off(\"gamma\") counts too\n",
        )
        .unwrap();
        std::fs::write(dir.join("sub/b.rs"), "let x = off(\"alpha\"); off(\"Bad\"); off(name);")
            .unwrap();
        std::fs::write(dir.join("notes.txt"), "off(\"delta\")").unwrap();
        let found = source_switches(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(found, ["alpha", "beta_2", "gamma"].map(String::from).into_iter().collect());
    }

    #[test]
    fn test_source_switches_tolerate_whitespace_and_rustfmt_splits() {
        let dir =
            std::env::temp_dir().join(format!("mega-evm2-controls-ws-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("a.rs"),
            concat!(
                // rustfmt breaks a long argument list: one argument per line, trailing comma.
                "if !crate::off(\n        \"split_by_rustfmt\",\n    ) {}\n",
                "let a = off( \"spaced\" );\n",
                "let b = off(\t\"tab_comma\" , );\n",
                "let c = off(\r\n\"crlf\"\r\n);\n",
                // Not switches: another function, two arguments, an unclosed call, a variable.
                "cutoff(\"other_fn\"); off(\"two\", 2); off(\"open\"; off(name);\n",
            ),
        )
        .unwrap();
        let found = source_switches(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            found,
            ["crlf", "spaced", "split_by_rustfmt", "tab_comma"]
                .map(String::from)
                .into_iter()
                .collect()
        );
    }

    #[test]
    fn test_completeness_reports_unlisted_and_stale_switches() {
        let controls = one("gate = \"main\"\nscenario = \"a\"").unwrap();
        let mut renamed = controls[0].clone();
        renamed.switch = "old".into();
        let source: BTreeSet<String> = ["s", "new"].map(String::from).into_iter().collect();
        let (unlisted, stale) =
            completeness(&[controls[0].clone(), renamed], Kind::Engine, &source);
        assert_eq!((unlisted, stale), (vec!["new".to_string()], vec!["old".to_string()]));
    }

    #[test]
    fn test_harness_controls_are_checked_against_their_own_source() {
        let c = |table: &str, switch: &str| {
            format!(
                "[[{table}]]\nswitch = \"{switch}\"\ngate = \"main\"\nscenario = \"a\"\nscope = \"sample\"\nwhy = \"w\"\n"
            )
        };
        let controls =
            parse(&format!("{}{}", c("control", "fix"), c("harness", "fixture"))).unwrap();
        assert_eq!(
            controls.iter().map(|c| (c.kind, c.switch.as_str())).collect::<Vec<_>>(),
            [(Kind::Engine, "fix"), (Kind::Harness, "fixture")]
        );
        let engine: BTreeSet<String> = ["fix".to_string()].into();
        let harness: BTreeSet<String> = ["fixture".to_string(), "new_fixture".to_string()].into();
        assert_eq!(completeness(&controls, Kind::Engine, &engine), (vec![], vec![]));
        assert_eq!(
            completeness(&controls, Kind::Harness, &harness),
            (vec!["new_fixture".to_string()], vec![])
        );
        // A harness switch listed as an engine control is stale there and unlisted here.
        let misplaced = parse(&c("control", "fixture")).unwrap();
        assert_eq!(
            completeness(&misplaced, Kind::Engine, &engine),
            (vec!["fix".to_string()], vec!["fixture".to_string()])
        );
        assert_eq!(
            completeness(&misplaced, Kind::Harness, &harness),
            (vec!["fixture".to_string(), "new_fixture".to_string()], vec![])
        );
        // One name, one switch: listed in both tables is listed twice.
        let err = parse(&format!("{}{}", c("control", "x"), c("harness", "x"))).unwrap_err();
        assert!(err.contains("`x`") && err.contains("twice"), "{err}");
    }

    #[test]
    fn test_sample_runs_sample_controls_and_full_runs_all() {
        let mut controls = one("gate = \"main\"\nscenario = \"a\"").unwrap();
        let mut full = controls[0].clone();
        full.switch = "t".into();
        full.scope = Scope::Full;
        controls.push(full);
        let names =
            |scope| in_scope(&controls, scope).iter().map(|c| c.switch.clone()).collect::<Vec<_>>();
        assert_eq!(names(Scope::Sample), ["s"]);
        assert_eq!(names(Scope::Full), ["s", "t"]);
    }
}
