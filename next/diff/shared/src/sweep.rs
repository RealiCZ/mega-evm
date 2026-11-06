//! The pre-frame sweep (group "edge"): every point where a transaction can end before its first
//! frame runs, crossed with the states of its sender and its recipient, so that both sides are
//! compared on what each loads before or instead of that frame (the read set) as well as on the
//! outcome.
//!
//! - Deposits ([`deposits`]): the failure points are the refusal of a deposit from the live system
//!   address, the body over the data-size limit, the intrinsic gas, the calldata floor, the account
//!   state gas of a created caller, the first frame's runtime gas phase (a value's new recipient, a
//!   delegated recipient's warm and cold delegate access, a creation's account), and a gas limit
//!   that runs the frame (which also reaches a value the caller cannot fund and a creation's
//!   collision). They are crossed with the sender (absent, empty, a funded EOA, delegated, a
//!   contract), the recipient (absent, existing, delegated, a contract, a precompile, a system
//!   contract, a creation onto an absent or an occupied address) and the mint and value (neither, a
//!   mint, a value the mint funds, a value nothing funds).
//! - Types 0, 1 and 2 ([`user_transactions`]): the intrinsic gas with the body's history (a
//!   refusal), the first frame's start record a transaction cannot pay (included, no frame), the
//!   runtime gas phase and a gas limit that runs the frame, crossed with the sender (a funded EOA,
//!   delegated, absent at gas price 0), the same recipients and a value or none.
//! - EIP-7702 ([`authorization_lists`]): the authority's charges one by one (its new account, its
//!   account write, its delegation bytes), the history of its record with the start record, the
//!   runtime gas phase, the list rule at a KV or state-growth limit of 0 and a frame that runs.
//! - Keyless deployment ([`keyless`]): the call to `KeylessDeploy` with or without a value from
//!   each sender, the signer absent, at nonce 1 or the sender, at the start record and with gas to
//!   run.
//!
//! The gas limits are placed one below each charge, from Satin's intrinsic gas (EIP-2780's
//! decomposed base, 4 and 16 per zero and non-zero calldata byte, EIP-3860's 2 per init-code word,
//! a floor of 64 per byte), its history (88 per byte of the 310-byte body, the calldata and 101 per
//! authorization) and its state prices at the minimum SALT bucket. Each combination is a scenario
//! of its own, named after its parts.

use super::{
    A, Account, Auth, B, IDENTITY, KEYLESS, Limits, SENDER, Scenario, acct, funded, init_returning,
    op, scenario,
};
use alloy_primitives::{Address, U256};

const DEPOSITOR: Address = super::q2::DEPOSITOR;
/// The sender of a user transaction whose account is absent (gas price 0).
const ABSENT_SENDER: Address = Address::with_last_byte(0x5e);
/// An EIP-7702 authority no scenario gives an account.
const AUTHORITY: Address = Address::with_last_byte(0xc7);
const ONE_ETHER: u64 = 1_000_000_000_000_000_000;
const PLENTY: u64 = 1_000_000;
const NEW_ACCOUNT: u64 = 183_600;
const CREATE_ACCOUNT: u64 = 183_600;
const WARM_ACCESS: u64 = 100;
const COLD_ACCESS: u64 = 2_900;
const CPHB: u64 = 88;
const RECORD: u64 = 40;
const ACCOUNT_WRITE: u64 = 9_000;
const DELEGATION_BYTES: u64 = 23 * 1_530;
const PER_AUTHORIZATION: u64 = 7_816;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sender {
    Absent,
    Empty,
    Funded,
    Delegated,
    Contract,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Target {
    Absent,
    Existing,
    Delegated,
    Contract,
    Precompile,
    System,
    Create,
    CreateOccupied,
}

const TARGETS: [Target; 8] = [
    Target::Absent,
    Target::Existing,
    Target::Delegated,
    Target::Contract,
    Target::Precompile,
    Target::System,
    Target::Create,
    Target::CreateOccupied,
];

fn delegation(target: Address) -> Vec<u8> {
    let mut c = vec![0xef, 0x01, 0x00];
    c.extend_from_slice(target.as_slice());
    c
}

fn lower(v: impl core::fmt::Debug) -> String {
    format!("{v:?}").to_lowercase()
}

fn leak(name: String) -> &'static str {
    Box::leak(name.into_boxed_str())
}

/// The sender's account in state `state`, at `address`, or none.
fn sender_account(address: Address, state: Sender) -> Option<Account> {
    let rich = U256::from(ONE_ETHER);
    Some(match state {
        Sender::Absent => return None,
        Sender::Empty => Account { address, ..Account::default() },
        Sender::Funded => Account { address, balance: rich, ..Account::default() },
        Sender::Delegated => {
            Account { address, balance: rich, code: delegation(B), ..Account::default() }
        }
        Sender::Contract => {
            Account { address, balance: rich, code: vec![op::STOP], ..Account::default() }
        }
    })
}

/// The recipient's accounts, `to`, the calldata (the init code of a creation) and the first
/// frame's runtime charges for `value` (each a point of its own), for a sender at `sender` with
/// nonce 0.
fn target(t: Target, sender: Address, value: bool) -> (Vec<Account>, Address, Vec<u8>, Vec<u64>) {
    // B, the delegate: a contract that writes a slot.
    let delegate = acct(B, super::Asm::new().sstore(1, 1).stop().build());
    let store = super::Asm::new().sstore(1, 1).stop().build();
    let new_account = if value { vec![NEW_ACCOUNT] } else { vec![] };
    match t {
        Target::Absent => (vec![], A, vec![], new_account),
        Target::Existing => (vec![funded(A, vec![], 1)], A, vec![], vec![]),
        Target::Delegated => {
            (vec![acct(A, delegation(B)), delegate], A, vec![], vec![WARM_ACCESS, COLD_ACCESS])
        }
        Target::Contract => (vec![acct(A, store)], A, vec![], vec![]),
        Target::Precompile => (vec![], IDENTITY, vec![0xff, 0x00], new_account),
        Target::System => (
            vec![],
            super::q8::ACCESS_CONTROL,
            super::q8::sel("isVolatileDataAccessDisabled()").to_vec(),
            new_account,
        ),
        Target::Create => (vec![], A, init_returning(1), vec![CREATE_ACCOUNT]),
        Target::CreateOccupied => {
            let occupant = Account {
                address: sender.create(0),
                nonce: 1,
                code: vec![op::STOP],
                ..Account::default()
            };
            (vec![occupant], A, init_returning(1), vec![])
        }
    }
}

/// Replaces the default sender's account of `sc` with `from`'s in state `state`.
fn with_sender(sc: &mut Scenario, from: Address, state: Sender) {
    sc.accounts.retain(|a| a.address != SENDER);
    sc.accounts.extend(sender_account(from, state));
    sc.sender = from;
}

fn is_create(t: Target) -> bool {
    matches!(t, Target::Create | Target::CreateOccupied)
}

/// Satin's intrinsic gas and calldata floor of a transaction with calldata `data`.
fn intrinsic(create: bool, value: bool, data: &[u8]) -> (u64, u64) {
    let base = 12_000 + if create { 12_000 } else { 3_000 + if value { 6_000 } else { 0 } };
    let tokens: u64 = data.iter().map(|b| if *b == 0 { 4 } else { 16 }).sum();
    let initcode = if create { 2 * (data.len() as u64).div_ceil(32) } else { 0 };
    (base + tokens + initcode, base + 64 * data.len() as u64)
}

/// The pre-frame points of a deposit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DepositPoint {
    System,
    Body,
    Intrinsic,
    Floor,
    Caller,
    Runtime(usize),
    Runs,
}

/// Every deposit cell: failure point × sender × recipient × mint and value.
pub fn deposits() -> Vec<Scenario> {
    let senders =
        [Sender::Absent, Sender::Empty, Sender::Funded, Sender::Delegated, Sender::Contract];
    let fundings: [(&str, u64, u64); 4] =
        [("plain", 0, 0), ("mint", 1_000_000, 0), ("value", 1_000_000, 5), ("unfunded", 0, 5)];
    let mut v = Vec::new();
    for s in senders {
        for t in TARGETS {
            for (fname, mint, value) in fundings {
                let created = matches!(s, Sender::Absent | Sender::Empty);
                let (_, _, _, runtime) = target(t, DEPOSITOR, value > 0);
                let mut points = vec![DepositPoint::System, DepositPoint::Body];
                points.extend([DepositPoint::Intrinsic, DepositPoint::Floor]);
                if created {
                    points.push(DepositPoint::Caller);
                }
                points.extend((0..runtime.len()).map(DepositPoint::Runtime));
                points.push(DepositPoint::Runs);
                for p in points {
                    v.push(deposit(s, t, fname, mint, value, p));
                }
            }
        }
    }
    v
}

fn deposit(s: Sender, t: Target, fname: &str, mint: u64, value: u64, p: DepositPoint) -> Scenario {
    let from = if p == DepositPoint::System { super::edge::SYSTEM } else { DEPOSITOR };
    let (mut accounts, to, mut data, runtime) = target(t, from, value > 0);
    if p == DepositPoint::Floor {
        // Calldata whose floor is above the intrinsic gas (after a creation's init code returns).
        data.extend([0xff; 64]);
    }
    if p == DepositPoint::System {
        accounts.push(super::edge::registry_naming(super::edge::SYSTEM));
    }
    let created = matches!(s, Sender::Absent | Sender::Empty);
    let (i, floor) = intrinsic(is_create(t), value > 0, &data);
    let caller = if created { NEW_ACCOUNT } else { 0 };
    let gas = match p {
        DepositPoint::System | DepositPoint::Body | DepositPoint::Runs => PLENTY,
        DepositPoint::Intrinsic => i - 1,
        DepositPoint::Floor => floor - 1,
        DepositPoint::Caller => i + caller - 1,
        DepositPoint::Runtime(k) => i + caller + runtime[..=k].iter().sum::<u64>() - 1,
    };
    let limits = if p == DepositPoint::Body {
        Limits { data: Some(309), ..Limits::default() }
    } else {
        Limits::default()
    };
    let point = match p {
        DepositPoint::Runtime(k) => format!("runtime{k}"),
        other => lower(other),
    };
    let name = format!("edge_sweep_dep_{}_{}_{fname}_{point}", lower(s), lower(t));
    let mut sc = scenario(leak(name), accounts, gas, limits);
    sc.group = "edge";
    with_sender(&mut sc, from, s);
    sc.deposit = Some(mint);
    sc.value = U256::from(value);
    sc.to = to;
    sc.create = is_create(t);
    sc.data = data;
    sc
}

/// The pre-frame points of a user transaction.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum UserPoint {
    Intrinsic,
    Start,
    Runtime(usize),
    Runs,
}

/// Every type 0, 1 and 2 cell: point × sender × recipient × value.
pub fn user_transactions() -> Vec<Scenario> {
    let mut v = Vec::new();
    for ty in 0..=2u8 {
        for s in [Sender::Funded, Sender::Delegated, Sender::Absent] {
            for t in TARGETS {
                for value in [0u64, 5] {
                    // An absent sender at gas price 0 cannot fund a value: refused, nothing more.
                    if s == Sender::Absent && value > 0 {
                        continue;
                    }
                    let start = start_charge(t, value > 0);
                    let (_, _, _, runtime) = target(t, SENDER, value > 0);
                    let mut points = vec![UserPoint::Intrinsic];
                    if start > 0 {
                        points.push(UserPoint::Start);
                    }
                    points.extend((0..runtime.len()).map(UserPoint::Runtime));
                    points.push(UserPoint::Runs);
                    for p in points {
                        v.push(user(ty, s, t, value, p));
                    }
                }
            }
        }
    }
    v
}

/// The first frame's start record charge: a value's recipient or a creation's account.
fn start_charge(t: Target, value: bool) -> u64 {
    if value || is_create(t) { RECORD * CPHB } else { 0 }
}

fn user(ty: u8, s: Sender, t: Target, value: u64, p: UserPoint) -> Scenario {
    let from = if s == Sender::Absent { ABSENT_SENDER } else { SENDER };
    let (accounts, to, data, runtime) = target(t, from, value > 0);
    let (i, _) = intrinsic(is_create(t), value > 0, &data);
    let history = (310 + data.len() as u64) * CPHB;
    let start = start_charge(t, value > 0);
    let gas = match p {
        UserPoint::Intrinsic => i + history - 1,
        UserPoint::Start => i + history + start - 1,
        UserPoint::Runtime(k) => i + history + start + runtime[..=k].iter().sum::<u64>() - 1,
        UserPoint::Runs => PLENTY,
    };
    let point = match p {
        UserPoint::Runtime(k) => format!("runtime{k}"),
        other => lower(other),
    };
    let name = format!("edge_sweep_tx{ty}_{}_{}_v{value}_{point}", lower(s), lower(t));
    let mut sc = scenario(leak(name), accounts, gas, Limits::default());
    sc.group = "edge";
    with_sender(&mut sc, from, s);
    sc.tx_type = ty;
    sc.value = U256::from(value);
    sc.to = to;
    sc.create = is_create(t);
    sc.data = data;
    sc
}

/// The pre-frame points of an EIP-7702 transaction.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AuthPoint {
    Intrinsic,
    NewAuthority,
    AccountWrite,
    DelegationBytes,
    History,
    Runtime(usize),
    Runs,
    ListKv,
    ListGrowth,
}

/// Every EIP-7702 cell: point × sender × recipient × value, with one authorization of an absent
/// authority delegating to B.
pub fn authorization_lists() -> Vec<Scenario> {
    let mut v = Vec::new();
    for s in [Sender::Funded, Sender::Delegated] {
        for t in TARGETS.into_iter().filter(|t| !is_create(*t)) {
            for value in [0u64, 5] {
                let (_, _, _, runtime) = target(t, SENDER, value > 0);
                let mut points = vec![
                    AuthPoint::Intrinsic,
                    AuthPoint::NewAuthority,
                    AuthPoint::AccountWrite,
                    AuthPoint::DelegationBytes,
                    AuthPoint::History,
                ];
                points.extend((0..runtime.len()).map(AuthPoint::Runtime));
                points.extend([AuthPoint::Runs, AuthPoint::ListKv, AuthPoint::ListGrowth]);
                for p in points {
                    v.push(authorization_list(s, t, value, p));
                }
            }
        }
    }
    v
}

fn authorization_list(s: Sender, t: Target, value: u64, p: AuthPoint) -> Scenario {
    let (accounts, to, data, runtime) = target(t, SENDER, value > 0);
    let (i, _) = intrinsic(false, value > 0, &data);
    let s0 = i + PER_AUTHORIZATION + (310 + data.len() as u64 + 101) * CPHB;
    let authority = NEW_ACCOUNT + ACCOUNT_WRITE + DELEGATION_BYTES;
    let history = (RECORD + start_charge(t, value > 0) / CPHB) * CPHB;
    let gas = match p {
        AuthPoint::Intrinsic => s0 - 1,
        AuthPoint::NewAuthority => s0 + NEW_ACCOUNT - 1,
        AuthPoint::AccountWrite => s0 + NEW_ACCOUNT + ACCOUNT_WRITE - 1,
        AuthPoint::DelegationBytes => s0 + authority - 1,
        AuthPoint::History => s0 + authority + history - 1,
        AuthPoint::Runtime(k) => s0 + authority + history + runtime[..=k].iter().sum::<u64>() - 1,
        AuthPoint::Runs | AuthPoint::ListKv | AuthPoint::ListGrowth => PLENTY,
    };
    let limits = match p {
        AuthPoint::ListKv => Limits { kv: Some(0), ..Limits::default() },
        AuthPoint::ListGrowth => Limits { growth: Some(0), ..Limits::default() },
        _ => Limits::default(),
    };
    let point = match p {
        AuthPoint::Runtime(k) => format!("runtime{k}"),
        other => lower(other),
    };
    let name = format!("edge_sweep_auth_{}_{}_v{value}_{point}", lower(s), lower(t));
    let mut sc = scenario(leak(name), accounts, gas, limits);
    sc.group = "edge";
    with_sender(&mut sc, SENDER, s);
    sc.value = U256::from(value);
    sc.to = to;
    sc.data = data;
    sc.auths = vec![Auth { chain_id: 1, address: B, nonce: 0, authority: Some(AUTHORITY) }];
    sc
}

/// Every keyless cell: sender × value × signer state × point (the intrinsic gas with the body's
/// history, the start record of a value, gas to run).
pub fn keyless() -> Vec<Scenario> {
    let mut v = Vec::new();
    let (data, signer, _) = super::q8::keyless_call("ok", 1_000_000);
    for s in [Sender::Funded, Sender::Delegated, Sender::Absent] {
        for value in [0u64, 1] {
            if s == Sender::Absent && value > 0 {
                continue;
            }
            for signer_state in ["absent", "nonce1", "sender"] {
                if signer_state == "sender" && s != Sender::Funded {
                    continue;
                }
                let (i, _) = intrinsic(false, value > 0, &data);
                let history = (310 + data.len() as u64) * CPHB;
                let start = start_charge(Target::Existing, value > 0);
                let mut points = vec![("intrinsic", i + history - 1)];
                if start > 0 {
                    points.push(("start", i + history + start - 1));
                }
                points.push(("runs", 2_000_000));
                for (point, gas) in points {
                    let from = match (signer_state, s) {
                        ("sender", _) => signer,
                        (_, Sender::Absent) => ABSENT_SENDER,
                        _ => SENDER,
                    };
                    let mut accounts = Vec::new();
                    if signer_state == "nonce1" {
                        accounts.push(Account { address: signer, nonce: 1, ..Account::default() });
                    }
                    let name = format!(
                        "edge_sweep_keyless_{}_v{value}_signer_{signer_state}_{point}",
                        lower(s)
                    );
                    let mut sc = scenario(leak(name), accounts, gas, Limits::default());
                    sc.group = "edge";
                    with_sender(&mut sc, from, s);
                    sc.to = KEYLESS;
                    sc.value = U256::from(value);
                    sc.data = data.clone();
                    v.push(sc);
                }
            }
        }
    }
    v
}

/// The whole sweep.
pub fn all() -> Vec<Scenario> {
    let mut v = deposits();
    v.extend(user_transactions());
    v.extend(authorization_lists());
    v.extend(keyless());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sweep's names are unique and its account lists hold each address once.
    #[test]
    fn test_the_sweep_names_each_cell_once() {
        let all = all();
        let mut names: Vec<_> = all.iter().map(|s| s.name).collect();
        names.sort_unstable();
        let n = names.len();
        names.dedup();
        assert_eq!(names.len(), n, "duplicate names");
        for s in &all {
            let mut addresses: Vec<_> = s.accounts.iter().map(|a| a.address).collect();
            addresses.sort_unstable();
            let k = addresses.len();
            addresses.dedup();
            assert_eq!(addresses.len(), k, "{}: an address twice", s.name);
        }
    }

    /// Satin's intrinsic gas and floor, as `mega-evm2`'s tests print them.
    #[test]
    fn test_the_sweep_prices_intrinsic_gas_as_satin() {
        assert_eq!(intrinsic(false, false, &[]), (15_000, 15_000));
        assert_eq!(intrinsic(false, true, &[]), (21_000, 21_000));
        assert_eq!(intrinsic(false, false, &[0, 0xff]), (15_020, 15_128));
        assert_eq!(intrinsic(true, false, &[0x60, 1, 0x60, 0, 0xf3, 0]), (24_074, 24_384));
    }
}
