//! Probe inputs: one scenario per line of JSON, which both harnesses' probe binaries read
//! (`next/diff/scripts/probe.sh` runs them and compares). A line is an object:
//!
//! - `base`: the name of a scenario to start from ([`crate::by_name`]: a hand-written scenario, or
//!   a fuzz seed of any mode such as `fqh0042`); the other keys override its fields. Without `base`
//!   the line is a whole scenario, built on the defaults of [`crate::scenario`] (the funded
//!   [`crate::SENDER`] calling [`crate::A`] with gas 1,000,000, no other account), group `probe`.
//! - `name`: the probe's name, unique in its file. Default: the base's name when nothing is
//!   overridden, `<base>~<line>` when something is, `probe<line>` for a whole scenario. The gate's
//!   accepted differences apply by name, so a probe named like a listed scenario is held to its
//!   entry.
//! - Transaction: `gas` (the gas limit), `value`, `data` (hex), `to`, `create` (a creation; `to`
//!   ignored), `sender`, `deposit` (an OP deposit's mint, or null for none), `gas_price` (0 for a
//!   deposit, which carries none), `tx_type` (0, 1 or 2), `chain_id` (a legacy transaction's, or
//!   null), `auths` (an EIP-7702 list: `{chain_id, address, nonce, authority}`, `authority` null
//!   for an invalid signature), `l1_fee` (`[l1 base fee, base fee scalar, enveloped bytes]`, or
//!   null).
//! - Block and environment: `beneficiary`; `salt` (`[address, slot or null, capacity in minimum
//!   buckets]` per bucket), `salt_capacity` (the same with the capacity in entries), `oracle`
//!   (`[slot, value]` per answer).
//! - Limits, each a number or null for the Satin default: `cap` (the block environment's detention
//!   cap), `oracle_cap`, `data_limit`, `kv_limit`, `growth_limit`.
//! - `accounts`: `{address, code, balance, nonce, storage}` per account (`storage` as `[key,
//!   value]` pairs; the others default to empty or zero). An entry replaces the account at its
//!   address, or adds one; the others stay.
//!
//! Also `group` (the record's group, default the base's) and `comment` (ignored).
//!
//! A list replaces the base's list. Numbers are JSON integers or strings (`0x` hex or decimal),
//! byte strings and addresses `0x` hex. An unknown key, a repeated name or a malformed value is an
//! error naming the line. In a file, blank lines and lines starting with `#` are skipped.
//! Every probe gets the chain's accounts ([`crate::add_chain_accounts`]) after its overrides.

use crate::{
    Account, Auth, BELOW_CAP, Limits, Scenario, add_chain_accounts, by_name,
    json::{self, Json},
    scenario,
};
use alloy_primitives::{Address, U256, hex};

/// The keys a probe line may hold.
pub const KEYS: [&str; 26] = [
    "name",
    "base",
    "gas",
    "value",
    "data",
    "to",
    "create",
    "sender",
    "deposit",
    "gas_price",
    "tx_type",
    "chain_id",
    "auths",
    "l1_fee",
    "beneficiary",
    "salt",
    "salt_capacity",
    "oracle",
    "cap",
    "oracle_cap",
    "data_limit",
    "kv_limit",
    "growth_limit",
    "accounts",
    "group",
    "comment",
];

/// Reads the probes of a file: one per line that is neither blank nor a `#` comment. Lines are
/// numbered from 1; names must be unique.
pub fn parse_file(text: &str) -> Result<Vec<Scenario>, String> {
    let mut out: Vec<Scenario> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let s = parse_line(trimmed, i + 1)?;
        if out.iter().any(|o| o.name == s.name) {
            return Err(format!("line {}: the name {} is taken by an earlier line", i + 1, s.name));
        }
        out.push(s);
    }
    if out.is_empty() {
        return Err("no probe in the file".into());
    }
    Ok(out)
}

/// Reads the probe on line `number` of its file.
pub fn parse_line(line: &str, number: usize) -> Result<Scenario, String> {
    let at = |e: String| format!("line {number}: {e}");
    let Json::Obj(members) = json::parse(line).map_err(at)? else {
        return Err(at("a probe is a JSON object".into()));
    };
    if let Some((k, _)) = members.iter().find(|(k, _)| !KEYS.contains(&k.as_str())) {
        return Err(at(format!("unknown key `{k}` (keys: {})", KEYS.join(", "))));
    }
    let get = |k: &str| members.iter().find(|(m, _)| m == k).map(|(_, v)| v);
    let base = get("base").map(|v| string(v, "base")).transpose().map_err(at)?;
    let mut s = match &base {
        Some(name) => by_name(name).ok_or_else(|| at(format!("no scenario is named {name}")))?,
        None => {
            let mut s = scenario("", Vec::new(), BELOW_CAP, Limits::default());
            s.group = "probe";
            s
        }
    };
    let overrides =
        members.iter().filter(|(k, _)| !matches!(k.as_str(), "name" | "base" | "comment")).count();
    let name = match (get("name"), &base) {
        (Some(v), _) => string(v, "name").map_err(at)?,
        (None, Some(b)) if overrides == 0 => b.clone(),
        (None, Some(b)) => format!("{b}~{number}"),
        (None, None) => format!("probe{number}"),
    };
    if name.is_empty() || name.contains(char::is_whitespace) {
        return Err(at(format!("the name {name:?} is empty or holds a space")));
    }
    s.name = Box::leak(name.into_boxed_str());
    for (k, v) in &members {
        apply(&mut s, k, v).map_err(|e| at(format!("`{k}`: {e}")))?;
    }
    if s.deposit.is_some() && s.gas_price != 0 {
        return Err(at("a deposit carries no gas price (`gas_price` must be 0)".into()));
    }
    add_chain_accounts(&mut s);
    Ok(s)
}

fn apply(s: &mut Scenario, key: &str, v: &Json) -> Result<(), String> {
    match key {
        "name" | "base" | "comment" => {}
        "group" => s.group = Box::leak(string(v, key)?.into_boxed_str()),
        "gas" => s.gas_limit = int(v)?,
        "value" => s.value = u256(v)?,
        "data" => s.data = bytes(v)?,
        "to" => s.to = address(v)?,
        "create" => s.create = boolean(v)?,
        "sender" => s.sender = address(v)?,
        "deposit" => s.deposit = opt(v, int)?,
        "gas_price" => s.gas_price = int(v)?,
        "tx_type" => {
            s.tx_type = match int(v)? {
                t @ 0..=2 => t as u8,
                t => return Err(format!("{t} is not 0, 1 or 2")),
            }
        }
        "chain_id" => s.chain_id = opt(v, int)?,
        "auths" => s.auths = list(v, auth)?,
        "l1_fee" => {
            s.l1_fee = opt(v, |v| {
                let [base_fee, scalar, enveloped] = fixed::<3>(v)?;
                Ok((int(base_fee)?, int(scalar)?, bytes(enveloped)?))
            })?
        }
        "beneficiary" => s.beneficiary = address(v)?,
        "salt" => s.salt = list(v, bucket)?,
        "salt_capacity" => s.salt_capacity = list(v, bucket)?,
        "oracle" => {
            s.oracle = list(v, |v| {
                let [slot, value] = fixed::<2>(v)?;
                Ok((int(slot)?, int(value)?))
            })?
        }
        "cap" => s.limits.block_env_cap = opt(v, int)?,
        "oracle_cap" => s.limits.oracle_cap = opt(v, int)?,
        "data_limit" => s.limits.data = opt(v, int)?,
        "kv_limit" => s.limits.kv = opt(v, int)?,
        "growth_limit" => s.limits.growth = opt(v, int)?,
        "accounts" => {
            for a in list(v, account)? {
                match s.accounts.iter_mut().find(|x| x.address == a.address) {
                    Some(x) => *x = a,
                    None => s.accounts.push(a),
                }
            }
        }
        _ => unreachable!("checked against KEYS"),
    }
    Ok(())
}

fn string(v: &Json, what: &str) -> Result<String, String> {
    match v {
        Json::Str(s) => Ok(s.clone()),
        other => Err(format!("{what} must be a string, not {}", other.kind())),
    }
}

fn boolean(v: &Json) -> Result<bool, String> {
    match v {
        Json::Bool(b) => Ok(*b),
        other => Err(format!("expected true or false, not {}", other.kind())),
    }
}

/// A number: a JSON integer, or a string of `0x` hex or decimal digits.
fn u256(v: &Json) -> Result<U256, String> {
    let text = match v {
        Json::Int(d) => d.as_str(),
        Json::Str(s) => s.as_str(),
        other => return Err(format!("expected a number, not {}", other.kind())),
    };
    let parsed = match text.strip_prefix("0x") {
        Some(h) => U256::from_str_radix(h, 16),
        None => U256::from_str_radix(text, 10),
    };
    parsed.map_err(|e| format!("{text:?} is not a number of 256 bits: {e}"))
}

fn int(v: &Json) -> Result<u64, String> {
    let n = u256(v)?;
    u64::try_from(n).map_err(|_| format!("{n} does not fit 64 bits"))
}

fn opt<T>(v: &Json, f: impl Fn(&Json) -> Result<T, String>) -> Result<Option<T>, String> {
    if *v == Json::Null { Ok(None) } else { f(v).map(Some) }
}

fn list<T>(v: &Json, f: impl Fn(&Json) -> Result<T, String>) -> Result<Vec<T>, String> {
    match v {
        Json::Arr(items) => items
            .iter()
            .enumerate()
            .map(|(i, x)| f(x).map_err(|e| format!("entry {i}: {e}")))
            .collect(),
        other => Err(format!("expected a list, not {}", other.kind())),
    }
}

fn fixed<const N: usize>(v: &Json) -> Result<[&Json; N], String> {
    match v {
        Json::Arr(items) if items.len() == N => Ok(core::array::from_fn(|i| &items[i])),
        _ => Err(format!("expected a list of {N}")),
    }
}

fn bytes(v: &Json) -> Result<Vec<u8>, String> {
    let s = string(v, "a byte string")?;
    hex::decode(s.strip_prefix("0x").unwrap_or(&s)).map_err(|e| format!("{s:?}: {e}"))
}

fn address(v: &Json) -> Result<Address, String> {
    let s = string(v, "an address")?;
    let h = s.strip_prefix("0x").ok_or(format!("{s:?} has no 0x prefix"))?;
    let b = hex::decode(h).map_err(|e| format!("{s:?}: {e}"))?;
    if b.len() != 20 {
        return Err(format!("{s:?} is not 20 bytes"));
    }
    Ok(Address::from_slice(&b))
}

fn bucket(v: &Json) -> Result<(Address, Option<u64>, u64), String> {
    let [a, slot, capacity] = fixed::<3>(v)?;
    Ok((address(a)?, opt(slot, int)?, int(capacity)?))
}

fn members<'a>(v: &'a Json, keys: &[&str]) -> Result<&'a [(String, Json)], String> {
    let Json::Obj(m) = v else {
        return Err(format!("expected an object, not {}", v.kind()));
    };
    if let Some((k, _)) = m.iter().find(|(k, _)| !keys.contains(&k.as_str())) {
        return Err(format!("unknown key `{k}` (keys: {})", keys.join(", ")));
    }
    Ok(m)
}

fn field<'a>(m: &'a [(String, Json)], k: &str) -> Option<&'a Json> {
    m.iter().find(|(x, _)| x == k).map(|(_, v)| v)
}

fn account(v: &Json) -> Result<Account, String> {
    let m = members(v, &["address", "code", "balance", "nonce", "storage"])?;
    let address = address(field(m, "address").ok_or("an account needs an `address`")?)?;
    let mut a = Account { address, ..Account::default() };
    if let Some(c) = field(m, "code") {
        a.code = bytes(c)?;
    }
    if let Some(b) = field(m, "balance") {
        a.balance = u256(b)?;
    }
    if let Some(n) = field(m, "nonce") {
        a.nonce = int(n)?;
    }
    if let Some(st) = field(m, "storage") {
        a.storage = list(st, |e| {
            let [k, v] = fixed::<2>(e)?;
            Ok((int(k)?, int(v)?))
        })?;
    }
    Ok(a)
}

fn auth(v: &Json) -> Result<Auth, String> {
    let m = members(v, &["chain_id", "address", "nonce", "authority"])?;
    let need = |k: &str| field(m, k).ok_or(format!("an authorization needs `{k}`"));
    Ok(Auth {
        chain_id: int(need("chain_id")?)?,
        address: address(need("address")?)?,
        nonce: int(need("nonce")?)?,
        authority: opt(need("authority")?, address)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{A, B, SENDER};

    #[test]
    fn test_a_base_with_overrides_changes_only_what_is_named() {
        let base = by_name("ctl_sstore").unwrap();
        let s = parse_line(
            r#"{"base": "ctl_sstore", "gas": 100000, "cap": 10, "growth_limit": "18446744073709551615", "beneficiary": "0x5050505050505050505050505050505050505050", "salt": [["0x00000000000000000000000000000000000000bb", 0, 0]]}"#,
            7,
        )
        .unwrap();
        assert_eq!(s.name, "ctl_sstore~7");
        assert_eq!(s.group, base.group);
        assert_eq!(s.gas_limit, 100_000);
        assert_eq!(s.limits.block_env_cap, Some(10));
        assert_eq!(s.limits.growth, Some(u64::MAX));
        assert_eq!(s.limits.kv, base.limits.kv);
        assert_eq!(s.beneficiary, SENDER);
        assert_eq!(s.salt, vec![(Address::with_last_byte(0xbb), Some(0), 0)]);
        assert_eq!(s.to, base.to);
        assert_eq!(s.accounts.len(), base.accounts.len());
        // Nothing overridden: the base itself, under its own name.
        let same = parse_line(r#"{"base": "ctl_sstore"}"#, 1).unwrap();
        assert_eq!(same.name, "ctl_sstore");
        assert_eq!(same.accounts.len(), base.accounts.len());
    }

    #[test]
    fn test_an_account_entry_replaces_its_address_and_adds_new_ones() {
        let s = parse_line(
            r#"{"base": "ctl_sstore", "name": "x", "accounts": [{"address": "0x10000000000000000000000000000000000000a0", "code": "0x00", "balance": 10, "nonce": 1, "storage": [[1, 2]]}, {"address": "0x10000000000000000000000000000000000000b0"}]}"#,
            1,
        )
        .unwrap();
        let a = s.accounts.iter().filter(|x| x.address == A).collect::<Vec<_>>();
        assert_eq!(a.len(), 1);
        assert_eq!(
            (a[0].code.clone(), a[0].balance, a[0].nonce, a[0].storage.clone()),
            (vec![0], U256::from(10), 1, vec![(1, 2)])
        );
        assert!(s.accounts.iter().any(|x| x.address == B));
        assert!(s.accounts.iter().any(|x| x.address == SENDER));
    }

    #[test]
    fn test_a_whole_scenario_starts_from_the_defaults() {
        let s = parse_line(
            r#"{"to": "0x0000000000000000000000000000000000000004", "data": "0x00ff"}"#,
            3,
        )
        .unwrap();
        assert_eq!((s.name, s.group, s.gas_limit), ("probe3", "probe", BELOW_CAP));
        assert_eq!(s.data, vec![0, 0xff]);
        assert_eq!(s.accounts[0].address, SENDER);
        assert!(s.accounts.iter().any(|a| a.address == crate::SEQUENCER_REGISTRY));
    }

    #[test]
    fn test_a_fuzz_seed_of_any_mode_is_a_base() {
        let s = parse_line(r#"{"base": "fqh0042"}"#, 1).unwrap();
        assert_eq!(s.name, "fqh0042");
        assert_eq!(s.group, "fz");
        let direct = crate::fz::seed_scenario(4, 42);
        assert_eq!(s.accounts.len(), direct.accounts.len() + 2);
        assert_eq!(s.accounts[1].code, direct.accounts[1].code);
    }

    #[test]
    fn test_malformed_lines_are_refused_with_their_line() {
        for (line, needle) in [
            (r#"{"base": "nope"}"#, "no scenario is named nope"),
            (r#"{"gass": 1}"#, "unknown key `gass`"),
            (r#"{"gas": -1}"#, "negative"),
            (r#"{"gas": "18446744073709551616"}"#, "does not fit 64 bits"),
            (r#"{"to": "0x01"}"#, "not 20 bytes"),
            (r#"{"tx_type": 3}"#, "not 0, 1 or 2"),
            (r#"{"accounts": [{"code": "0x00"}]}"#, "needs an `address`"),
            (r#"{"salt": [["0x0000000000000000000000000000000000000001", 0]]}"#, "a list of 3"),
            (r#"[1]"#, "a JSON object"),
            (r#"{"deposit": 0, "gas_price": 1}"#, "no gas price"),
        ] {
            let e = parse_line(line, 9).unwrap_err();
            assert!(e.starts_with("line 9: ") && e.contains(needle), "{line}: {e}");
        }
        let e = parse_file("{\"name\": \"a\"}\n\n# c\n{\"name\": \"a\"}\n").unwrap_err();
        assert!(e.contains("line 4") && e.contains("taken"), "{e}");
        assert!(parse_file("# only a comment\n").is_err());
    }
}
