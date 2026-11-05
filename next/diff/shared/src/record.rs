//! The outcome record of a scenario, as both harnesses render it (a JSON object; the reference
//! outputs are lists of them): its field names and how each value is written. A harness fills
//! every field of [`FIELDS`] from its own engine's result, with the encoders below, and adds its
//! own extra fields ([`REFERENCE_EXTRA`], [`ENGINE_EXTRA`]); [`check_fields`] states that a
//! rendered record has exactly those ([`fields`]). A transaction the engine refuses is the record
//! [`REFUSED_FIELDS`] instead, `error` holding the refusal's text.

use alloy_primitives::{Address, B256, U256};

/// The fields of an executed scenario's record, in the order the reference writes them:
/// - `name`, `group`: the scenario's ([`crate::Scenario`]);
/// - `status`: [`SUCCESS`], [`REVERT`] or [`HALT`]; `halt`: the halt reason's text, or null;
/// - `output`: [`hex`] of the transaction's output;
/// - `gas_used`, and its parts `regular`, `state` (state gas but history), `history` and
///   `history_bytes`; `reservoir_remaining`: the EIP-8037 reservoir left;
/// - `data_size`, `kv` (write records), `growth` (state growth): the transaction's usage;
/// - `limit_exceeded`: `"<kind>:<limit>"` of the limit that stopped it, or null;
/// - `logs`: `{address, topics, data}` per log ([`LOG`]);
/// - `accounts`: every changed account, `address` → [`account`]; `storage`: every changed slot,
///   [`slot_key`] → [`slot_value`];
/// - `reads`: the read set ([`READS`]); `hints`: the Oracle hints, `[from, topic, hex(data)]`;
/// - `ledger`: the start-of-frame settlements ([`LEDGER`]); `frames`: the inspected run's frames
///   ([`FRAME`]).
pub const FIELDS: [&str; 24] = [
    "name",
    "group",
    "status",
    "halt",
    "output",
    "gas_used",
    "regular",
    "state",
    "history",
    "history_bytes",
    "reservoir_remaining",
    "data_size",
    "kv",
    "growth",
    "limit_exceeded",
    "logs",
    "accounts",
    "storage",
    "reads",
    "hints",
    "ledger",
    "frames",
    "inspected_equal",
    "inspected",
];

/// The fields of a refused scenario's record: `name`, `group` and the refusal's text.
pub const REFUSED_FIELDS: [&str; 3] = ["name", "group", "error"];

/// What only the reference adds: `inspected_equal` (whether its plain and inspected runs rendered
/// the same record) and, when they did not, `inspected` (the inspected run's record). Both are the
/// last two entries of [`FIELDS`]; the reference's refused record carries them too.
pub const REFERENCE_EXTRA: [&str; 2] = ["inspected_equal", "inspected"];

/// What only the engine adds: `priced`, its debug record of the precompile prices it quoted.
pub const ENGINE_EXTRA: [&str; 1] = ["priced"];

/// The read set (`reads`): the accounts, slots (`address:key`), code hashes and block hashes the
/// database served, the SALT buckets read and the number of Oracle reads.
pub const READS: [&str; 6] =
    ["accounts", "storage", "codes", "block_hashes", "buckets", "oracle_reads"];

/// A frame of `frames`, recorded as it opens (`kind`, `level`, `gas_limit`, `reservoir`, `value`)
/// and as it closes (`status`, `output`, `gas_left`, `reservoir_left`).
pub const FRAME: [&str; 9] = [
    "kind",
    "level",
    "gas_limit",
    "reservoir",
    "value",
    "status",
    "output",
    "gas_left",
    "reservoir_left",
];

/// A settlement of `ledger`: the frame's depth, the frame's and its caller's pools
/// (`{remaining, net}`) and the refund.
pub const LEDGER: [&str; 4] = ["depth", "frame", "caller", "refund"];

/// A log of `logs`.
pub const LOG: [&str; 3] = ["address", "topics", "data"];

/// `status` of a successful transaction or frame.
pub const SUCCESS: &str = "success";
/// `status` of a reverted one.
pub const REVERT: &str = "revert";
/// `status` of a halted one.
pub const HALT: &str = "halt";

/// Bytes as `0x`-prefixed lower-case hex (`0x` for none).
pub fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(2 + 2 * b.len());
    s.push_str("0x");
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}

/// A changed account's entry in `accounts`: `<balance>/n<nonce>/<code hash>`, a destroyed or
/// removed account as zero balance, nonce 0 and the zero hash.
pub fn account(balance: U256, nonce: u64, code_hash: B256) -> String {
    format!("{balance}/n{nonce}/{code_hash}")
}

/// A slot's key in `storage` (and in the read set's `storage`): `<address>:<key in hex>`.
pub fn slot_key(address: Address, key: U256) -> String {
    format!("{address}:{key:#x}")
}

/// A slot's value in `storage`.
pub fn slot_value(value: U256) -> String {
    format!("{value:#x}")
}

/// Which harness rendered a record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Harness {
    /// The reference (`next/reference`).
    Reference,
    /// The Satin engine's harness (`next/diff`).
    Engine,
}

/// The top-level fields a harness's record has: an executed scenario's ([`FIELDS`] without the
/// other side's extra fields), or a refused one's ([`REFUSED_FIELDS`]; the reference also writes
/// `frames` and `inspected_equal` there). `inspected` is listed for the reference, which writes it
/// only when its two runs differ.
pub fn fields(harness: Harness, refused: bool) -> Vec<&'static str> {
    match (harness, refused) {
        (Harness::Reference, false) => FIELDS.to_vec(),
        (Harness::Engine, false) => FIELDS
            .iter()
            .copied()
            .filter(|k| !REFERENCE_EXTRA.contains(k))
            .chain(ENGINE_EXTRA)
            .collect(),
        (Harness::Reference, true) => {
            REFUSED_FIELDS.iter().copied().chain(["frames"]).chain(REFERENCE_EXTRA).collect()
        }
        (Harness::Engine, true) => REFUSED_FIELDS.to_vec(),
    }
}

/// Whether a record with the top-level `keys` has exactly the fields `harness` renders
/// ([`fields`]; a record with `error` is a refused one). `Err` names the missing and the unexpected
/// fields.
pub fn check_fields<'a>(
    harness: Harness,
    keys: impl IntoIterator<Item = &'a str>,
) -> Result<(), String> {
    let keys: std::collections::BTreeSet<&str> = keys.into_iter().collect();
    let want = fields(harness, keys.contains("error"));
    let optional = |k: &str| harness == Harness::Reference && k == "inspected";
    let missing: Vec<_> = want.iter().filter(|k| !keys.contains(*k) && !optional(k)).collect();
    let unexpected: Vec<_> = keys.iter().filter(|k| !want.contains(*k)).collect();
    if missing.is_empty() && unexpected.is_empty() {
        Ok(())
    } else {
        Err(format!("record fields: missing {missing:?}, unexpected {unexpected:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_values_are_written_as_both_harnesses_write_them() {
        assert_eq!(hex(&[]), "0x");
        assert_eq!(hex(&[0x00, 0xab, 0x0f]), "0x00ab0f");
        assert_eq!(
            account(U256::from(10), 1, B256::ZERO),
            "10/n1/0x0000000000000000000000000000000000000000000000000000000000000000"
        );
        // EIP-55's first example address: written with its checksum.
        let a: Address = "0x5aaeb6053f3e94c9b9a09f33669435e7ef1beaed".parse().unwrap();
        assert_eq!(
            slot_key(a, U256::from(0x1f)),
            "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed:0x1f"
        );
        assert_eq!(slot_value(U256::ZERO), "0x0");
        assert_eq!(slot_value(U256::from(255)), "0xff");
    }

    #[test]
    fn test_a_record_must_carry_exactly_its_harness_fields() {
        let engine = fields(Harness::Engine, false);
        assert!(engine.contains(&"priced") && !engine.contains(&"inspected_equal"));
        assert_eq!(check_fields(Harness::Engine, engine.iter().copied()), Ok(()));
        // The reference writes `inspected` only when its runs differ.
        assert_eq!(check_fields(Harness::Reference, FIELDS[..23].iter().copied()), Ok(()));
        assert_eq!(check_fields(Harness::Reference, FIELDS), Ok(()));
        let without_gas = engine.iter().copied().filter(|k| *k != "gas_used");
        let e = check_fields(Harness::Engine, without_gas).unwrap_err();
        assert!(e.contains("missing [\"gas_used\"]"), "{e}");
        let e = check_fields(Harness::Engine, FIELDS[..23].iter().copied()).unwrap_err();
        assert!(e.contains("unexpected [\"inspected_equal\"]"), "{e}");
        let refused = ["name", "group", "error", "frames", "inspected_equal"];
        assert_eq!(check_fields(Harness::Reference, refused), Ok(()));
        assert_eq!(check_fields(Harness::Engine, ["name", "group", "error"]), Ok(()));
        assert!(check_fields(Harness::Engine, refused).is_err());
    }
}
