//! The chain configuration: the raw JSON reading of `satinTime` and the other keys
//! (`SATIN-ACT-002`), the base forks (`SATIN-ACT-003`), the Mega ladder (`SATIN-ACT-011`), the
//! registry's bootstrap values (`SATIN-REG-009`), genesis-active Satin (`SATIN-ACT-012`), the
//! activation facts a block's place in the schedule decides (`SATIN-ACT-001`, `SATIN-ACT-010`,
//! `SATIN-BASE-006`, `SATIN-BASE-007`) and the protocol values a reader sees before execution
//! (`SATIN-ACT-009`).

use alloy_primitives::{Address, address};
use mega_evm2::{
    chain::{
        BootstrapInput, ChainConfig, ChainConfigInput, Engine, Fork, ForkSchedule, ProtocolParams,
    },
    error::ConfigError,
};

const SEQUENCER: Address = address!("0x7a49197dd1ebb8d38c67e4eb7626af6ade432445");
const ADMIN: Address = address!("0x92e0e0b15e3e99b32c9ed9ad284f939553c7b7d6");

/// The members of a `config` object, in order; a key may repeat.
#[derive(Clone)]
struct Members(Vec<(String, String)>);

impl Members {
    /// Chain 4326 with every base fork and every Mega spec below Satin at `at`, Satin at `satin`,
    /// and the registry's bootstrap values.
    fn satin(at: u64, satin: u64) -> Self {
        let mut m = vec![("chainId".to_string(), "4326".to_string())];
        for fork in Fork::SATIN_BASE.into_iter().chain(Fork::LADDER) {
            let time = if fork == Fork::Satin { satin } else { at };
            m.push((fork.key().to_string(), time.to_string()));
        }
        m.push(("rex5InitialSequencer".into(), format!("\"{SEQUENCER}\"")));
        m.push(("rex5InitialAdmin".into(), format!("\"{ADMIN}\"")));
        m.push(("rex6MinRotationDelay".into(), "21600".into()));
        Self(m)
    }

    /// `self` with every copy of `key` removed.
    fn without(mut self, key: &str) -> Self {
        self.0.retain(|(k, _)| k != key);
        self
    }

    /// `self` with every copy of `key` given the raw JSON `value` (added when absent).
    fn with(mut self, key: &str, value: &str) -> Self {
        let mut found = false;
        for (k, v) in &mut self.0 {
            if k == key {
                *v = value.to_string();
                found = true;
            }
        }
        if !found {
            self.0.push((key.to_string(), value.to_string()));
        }
        self
    }

    /// `self` with one more member, written as given (a repeated key included).
    fn push(mut self, key: &str, value: &str) -> Self {
        self.0.push((key.to_string(), value.to_string()));
        self
    }

    fn json(&self) -> String {
        let members: Vec<String> = self.0.iter().map(|(k, v)| format!("\"{k}\": {v}")).collect();
        format!("{{{}}}", members.join(", "))
    }

    fn read(&self) -> Result<ChainConfig, ConfigError> {
        ChainConfig::from_config_json(&self.json(), 0)
    }
}

fn bootstrap() -> BootstrapInput {
    BootstrapInput {
        initial_sequencer: Some(SEQUENCER),
        initial_admin: Some(ADMIN),
        min_rotation_delay: Some(21_600),
    }
}

fn input(schedule: ForkSchedule) -> ChainConfigInput {
    ChainConfigInput { chain_id: 4326, genesis_timestamp: 0, schedule, bootstrap: bootstrap() }
}

#[test]
fn chain_config_reads_every_key_of_a_genesis_config() {
    let config = Members::satin(100, 200)
        .with("miniRexTime", "10")
        .with("rex6Time", "150")
        .push("optimism", r#"{"eip1559Elasticity": 6, "satinNested": true}"#)
        .push("homesteadBlock", "0")
        .push("terminalTotalDifficulty", "0")
        .read()
        .expect("a valid configuration");
    assert_eq!(config.chain_id(), 4326);
    assert_eq!(config.satin_time(), Some(200));
    assert_eq!(config.schedule().get(Fork::MiniRex), Some(10));
    assert_eq!(config.schedule().get(Fork::Rex6), Some(150));
    assert_eq!(config.schedule().get(Fork::Karst), Some(100));
    assert_eq!(config.schedule().get(Fork::Lagoon), None);
    let b = config.registry_bootstrap().expect("Satin is scheduled");
    assert_eq!((b.initial_sequencer(), b.initial_admin()), (SEQUENCER, ADMIN));
    assert_eq!(b.min_rotation_delay(), 21_600);
    assert_eq!(config.genesis_timestamp(), 0);
    assert!(!config.satin_from_genesis_block());
}

#[test]
fn chain_config_without_satin_time_leaves_satin_unscheduled() {
    // Without Satin, nothing of its rules applies: no ladder, base fork or bootstrap value.
    let config = ChainConfig::from_config_json(r#"{"chainId": 6343, "rexTime": 5}"#, 0)
        .expect("a configuration without Satin");
    assert_eq!(config.satin_time(), None);
    assert_eq!(config.registry_bootstrap(), None);
    for t in [0, 1, u64::MAX] {
        assert_eq!(config.engine_at(t), Engine::Legacy);
        assert!(!config.activation(t, Some(t.saturating_sub(1))).satin_activates());
    }
}

#[test]
fn chain_config_satin_time_takes_every_integer_up_to_the_largest_timestamp() {
    for satin in ["0", "1", "18446744073709551615"] {
        let config = Members::satin(0, 0).with("satinTime", satin).read().expect(satin);
        assert_eq!(config.satin_time(), Some(satin.parse().unwrap()));
    }
    let config = Members::satin(0, 0).with("satinTime", "18446744073709551615").read().unwrap();
    assert_eq!(config.engine_at(u64::MAX - 1), Engine::Legacy);
    assert_eq!(config.engine_at(u64::MAX), Engine::Satin);
}

#[test]
fn chain_config_satin_time_refuses_every_other_value() {
    for (value, found) in [
        ("null", "null"),
        ("-1", "a number that is not such an integer"),
        ("-0", "a number that is not such an integer"),
        ("1.0", "a number that is not such an integer"),
        ("1.5", "a number that is not such an integer"),
        ("1e3", "a number that is not such an integer"),
        ("18446744073709551616", "a number that is not such an integer"),
        ("100000000000000000000000000000", "a number that is not such an integer"),
        ("true", "a boolean"),
        ("false", "a boolean"),
        ("\"100\"", "a string"),
        ("\"0x64\"", "a string"),
        ("[100]", "an array"),
        // A deserializer's arbitrary-precision form is an object like any other.
        (r#"{"$serde_json::private::Number": "100"}"#, "an object"),
    ] {
        let error = Members::satin(0, 0).with("satinTime", value).read().unwrap_err();
        assert_eq!(
            error,
            ConfigError::InvalidValue {
                key: "satinTime",
                expected: "an integer from 0 to 2^64 - 1",
                found,
            },
            "{value}"
        );
    }
}

#[test]
fn chain_config_refuses_other_satin_keys_in_any_case() {
    for key in [
        "SatinTime",
        "satintime",
        "SATINTIME",
        "sAtInTiMe",
        "satin",
        "Satin",
        "satinTime ",
        "satinTimestamp",
        "satinInitialSequencer",
        "satinInitialSystemAddress",
        "satinInitialAdmin",
        "satinMinRotationDelay",
        "satinInitialFromBlock",
        "satinTxDataLimit",
    ] {
        let expected = ConfigError::UnknownSatinKey { key: key.to_string() };
        // With `satinTime` present too, and without it.
        let with = Members::satin(0, 0).push(key, "1").read().unwrap_err();
        assert_eq!(with, expected, "{key}");
        let without = Members::satin(0, 0).without("satinTime").push(key, "1").read();
        assert_eq!(without.unwrap_err(), expected, "{key} without satinTime");
    }
    // Keys that do not start with `satin` are another component's.
    for key in ["xsatinTime", "sati", "_satinTime", "satiNTime_x"] {
        let read = Members::satin(0, 0).push(key, "1").read();
        if key.to_ascii_lowercase().starts_with("satin") {
            assert!(read.is_err(), "{key}");
        } else {
            assert!(read.is_ok(), "{key}: {read:?}");
        }
    }
}

#[test]
fn chain_config_reads_a_repeated_key_as_its_last_copy() {
    // The last copy is read, then validated: an earlier invalid copy does not count...
    let config = Members::satin(0, 0).with("satinTime", "\"x\"").push("satinTime", "5").read();
    assert_eq!(config.unwrap().satin_time(), Some(5));
    // ...and a later invalid copy refuses the configuration.
    let error = Members::satin(0, 0).push("satinTime", "null").read().unwrap_err();
    assert!(matches!(error, ConfigError::InvalidValue { key: "satinTime", .. }), "{error:?}");
    // Every key alike: a fork's, a bootstrap value's, the chain id's.
    let config = Members::satin(0, 9)
        .push("rex6Time", "7")
        .push("rex6MinRotationDelay", "3")
        .push("chainId", "6343")
        .read()
        .unwrap();
    assert_eq!(config.schedule().get(Fork::Rex6), Some(7));
    assert_eq!(config.registry_bootstrap().unwrap().min_rotation_delay(), 3);
    assert_eq!(config.chain_id(), 6343);
    // A last copy that breaks the ladder is refused.
    let error = Members::satin(0, 9).push("rex6Time", "10").read().unwrap_err();
    assert!(matches!(error, ConfigError::LadderInverted { fork: Fork::Rex6, .. }), "{error:?}");
}

#[test]
fn chain_config_reads_an_escaped_key_as_the_key_it_spells() {
    // `a` is `a`: the key is `satinTime`.
    let json =
        Members::satin(0, 0).without("satinTime").json().replacen('{', r#"{"satinTime": 7, "#, 1);
    assert_eq!(ChainConfig::from_config_json(&json, 0).unwrap().satin_time(), Some(7));
    // `S` is `S`: the key is `SatinTime`, refused.
    let json = Members::satin(0, 0).json().replacen('{', r#"{"SatinTime": 7, "#, 1);
    assert_eq!(
        ChainConfig::from_config_json(&json, 0).unwrap_err(),
        ConfigError::UnknownSatinKey { key: "SatinTime".into() }
    );
}

#[test]
fn chain_config_reads_timestamps_without_floating_point_loss() {
    // 2^53 + 1 is the first integer a 64-bit float cannot hold; 2^64 - 2 rounds to 2^64.
    for t in [9_007_199_254_740_993u64, 18_446_744_073_709_551_614] {
        let config = Members::satin(t, t).read().unwrap();
        assert_eq!(config.satin_time(), Some(t));
        assert_eq!(config.schedule().get(Fork::Shanghai), Some(t));
        assert_eq!(config.engine_at(t - 1), Engine::Legacy);
        assert_eq!(config.engine_at(t), Engine::Satin);
    }
    let config = Members::satin(0, 0).with("rex6MinRotationDelay", "9007199254740993").read();
    assert_eq!(
        config.unwrap().registry_bootstrap().unwrap().min_rotation_delay(),
        9_007_199_254_740_993
    );
}

#[test]
fn chain_config_refuses_a_skipped_ladder_rung() {
    for fork in [
        Fork::MiniRex,
        Fork::Rex,
        Fork::Rex1,
        Fork::Rex2,
        Fork::Rex3,
        Fork::Rex4,
        Fork::Rex5,
        Fork::Rex6,
    ] {
        let error = Members::satin(0, 0).without(fork.key()).read().unwrap_err();
        assert_eq!(error, ConfigError::LadderGap { fork }, "{fork}");
        // A null time is no time.
        let error = Members::satin(0, 0).with(fork.key(), "null").read().unwrap_err();
        assert_eq!(error, ConfigError::LadderGap { fork }, "{fork} null");
    }
}

#[test]
fn chain_config_refuses_an_inverted_ladder() {
    // Rex3 after Rex4.
    let error = Members::satin(0, 100).with("rex3Time", "50").with("rex4Time", "40").read();
    assert_eq!(
        error.unwrap_err(),
        ConfigError::LadderInverted {
            fork: Fork::Rex3,
            time: 50,
            above: Fork::Rex4,
            above_time: 40
        }
    );
    // Rex6 after Satin.
    let error = Members::satin(0, 100).with("rex6Time", "101").read().unwrap_err();
    assert_eq!(
        error,
        ConfigError::LadderInverted {
            fork: Fork::Rex6,
            time: 101,
            above: Fork::Satin,
            above_time: 100
        }
    );
    // An alias rung, where scheduled, is on the ladder too: MiniRex1 after Rex.
    let error = Members::satin(0, 100).with("miniRex1Time", "60").with("rexTime", "50").read();
    assert_eq!(
        error.unwrap_err(),
        ConfigError::LadderInverted {
            fork: Fork::MiniRex1,
            time: 60,
            above: Fork::MiniRex2,
            above_time: 0,
        }
    );
    // MiniRex after an omitted MiniRex1 and MiniRex2: compared with Rex.
    let error = Members::satin(0, 100)
        .without("miniRex1Time")
        .without("miniRex2Time")
        .with("miniRexTime", "70")
        .with("rexTime", "60")
        .read();
    assert_eq!(
        error.unwrap_err(),
        ConfigError::LadderInverted {
            fork: Fork::MiniRex,
            time: 70,
            above: Fork::Rex,
            above_time: 60
        }
    );
}

#[test]
fn chain_config_allows_equal_timestamps_and_omitted_alias_rungs() {
    // Equal timestamps on every rung, and a climbing ladder with both alias rungs left out.
    assert!(Members::satin(7, 7).read().is_ok());
    let mut climbing = Members::satin(0, 100).without("miniRex1Time").without("miniRex2Time");
    for (i, fork) in Fork::LADDER.into_iter().filter(|f| !f.is_alias()).enumerate() {
        if fork != Fork::Satin {
            climbing = climbing.with(fork.key(), &(i as u64 * 10).to_string());
        }
    }
    let config = climbing.read().expect("a climbing ladder");
    assert_eq!(config.schedule().get(Fork::MiniRex1), None);
    assert_eq!(config.schedule().get(Fork::Rex6), Some(70));
}

#[test]
fn chain_config_requires_every_base_fork_active_at_satin_time() {
    for fork in Fork::SATIN_BASE {
        let missing = Members::satin(0, 100).without(fork.key()).read().unwrap_err();
        assert_eq!(
            missing,
            ConfigError::BaseForkInactive { fork, time: None, satin_time: 100 },
            "{fork}"
        );
        let later = Members::satin(0, 100).with(fork.key(), "101").read().unwrap_err();
        assert_eq!(
            later,
            ConfigError::BaseForkInactive { fork, time: Some(101), satin_time: 100 },
            "{fork}"
        );
        // Active at satinTime, whether before it or with it: no base fork has to activate at
        // Satin.
        assert!(Members::satin(0, 100).with(fork.key(), "100").read().is_ok(), "{fork}");
        assert!(Members::satin(0, 100).with(fork.key(), "3").read().is_ok(), "{fork}");
    }
    // A later OP fork the configuration schedules is no base fork.
    assert!(Members::satin(0, 100).with("lagoonTime", "500").read().is_ok());
}

#[test]
fn chain_config_requires_nonzero_bootstrap_values() {
    for key in ["rex5InitialSequencer", "rex5InitialAdmin", "rex6MinRotationDelay"] {
        let missing = Members::satin(0, 0).without(key).read().unwrap_err();
        assert_eq!(missing, ConfigError::MissingBootstrap { key }, "{key}");
        let null = Members::satin(0, 0).with(key, "null").read().unwrap_err();
        assert_eq!(null, ConfigError::MissingBootstrap { key }, "{key} null");
        let zero = if key == "rex6MinRotationDelay" {
            "0".to_string()
        } else {
            format!("\"{}\"", Address::ZERO)
        };
        let zero = Members::satin(0, 0).with(key, &zero).read().unwrap_err();
        assert_eq!(zero, ConfigError::ZeroBootstrap { key }, "{key} zero");
        // Without Satin the registry's bootstrap values are not Satin's to require.
        let unscheduled = Members::satin(0, 0).without("satinTime").without(key).read();
        assert!(unscheduled.is_ok(), "{key}: {unscheduled:?}");
    }
}

#[test]
fn chain_config_refuses_malformed_bootstrap_and_fork_values() {
    for (key, value) in [
        ("rex5InitialSequencer", "\"not-an-address\""),
        ("rex5InitialSequencer", "\"0x1234\""),
        ("rex5InitialSequencer", "42"),
        ("rex5InitialAdmin", "true"),
        ("rex6MinRotationDelay", "\"21600\""),
        ("rex6MinRotationDelay", "-1"),
        ("rex6MinRotationDelay", "1.5"),
        ("rex6Time", "\"0\""),
        ("karstTime", "1e2"),
        ("osakaTime", "{}"),
    ] {
        let error = Members::satin(0, 0).with(key, value).read().unwrap_err();
        assert!(
            matches!(error, ConfigError::InvalidValue { key: k, .. } if k == key),
            "{key}={value}: {error:?}"
        );
    }
}

#[test]
fn chain_config_reads_a_null_fork_time_as_unscheduled() {
    let config = Members::satin(0, 0).with("lagoonTime", "null").read().unwrap();
    assert_eq!(config.schedule().get(Fork::Lagoon), None);
    let error = Members::satin(0, 0).with("osakaTime", "null").read().unwrap_err();
    assert_eq!(
        error,
        ConfigError::BaseForkInactive { fork: Fork::Osaka, time: None, satin_time: 0 }
    );
}

#[test]
fn chain_config_in_memory_constructor_validates_as_the_json_reader_does() {
    let valid = ChainConfig::new(input(ForkSchedule::satin_from(5))).expect("valid");
    assert_eq!(valid, Members::satin(5, 5).read().unwrap());
    let gap = input(ForkSchedule::satin_from(5).with(Fork::Rex2, None));
    assert_eq!(ChainConfig::new(gap), Err(ConfigError::LadderGap { fork: Fork::Rex2 }));
    let base = input(ForkSchedule::satin_from(5).with(Fork::Jovian, Some(6)));
    assert_eq!(
        ChainConfig::new(base),
        Err(ConfigError::BaseForkInactive { fork: Fork::Jovian, time: Some(6), satin_time: 5 })
    );
    let mut zero = input(ForkSchedule::satin_from(5));
    zero.bootstrap.initial_admin = Some(Address::ZERO);
    assert_eq!(ChainConfig::new(zero), Err(ConfigError::ZeroBootstrap { key: "rex5InitialAdmin" }));
    let mut missing = input(ForkSchedule::satin_from(5));
    missing.bootstrap.min_rotation_delay = None;
    assert_eq!(
        ChainConfig::new(missing),
        Err(ConfigError::MissingBootstrap { key: "rex6MinRotationDelay" })
    );
    assert_eq!(
        ChainConfig::satin_from_genesis(1, BootstrapInput::default()),
        Err(ConfigError::MissingBootstrap { key: "rex5InitialSequencer" })
    );
}

#[test]
fn chain_config_genesis_active_satin_has_no_activation_block() {
    // Satin at 50, genesis at 100: active from genesis, not only at timestamp zero.
    let config = ChainConfig::from_config_json(&Members::satin(0, 50).json(), 100).unwrap();
    assert!(config.satin_from_genesis_block());
    assert_eq!(config.engine_at(100), Engine::Satin);
    // Block zero has no parent: no fork activates in it, and it admits every transaction.
    let zero = config.activation(100, None);
    assert!(zero.block_zero() && !zero.satin_activates() && !zero.only_deposits());
    // Block one, on a parent at the genesis timestamp, is no activation block either.
    let one = config.activation(101, Some(100));
    assert!(!one.block_zero() && !one.satin_activates() && !one.only_deposits());
    // Satin exactly at the genesis timestamp, and at zero, are genesis-active too.
    for (satin, genesis) in [(100, 100), (0, 0)] {
        let config = ChainConfig::from_config_json(&Members::satin(0, satin).json(), genesis);
        assert!(config.unwrap().satin_from_genesis_block(), "{satin} {genesis}");
    }
    let later = ChainConfig::from_config_json(&Members::satin(0, 101).json(), 100).unwrap();
    assert!(!later.satin_from_genesis_block());
}

#[test]
fn chain_config_activation_follows_the_parent_timestamp_across_gaps() {
    let config = Members::satin(0, 100).read().unwrap();
    assert_eq!(config.engine_at(99), Engine::Legacy);
    assert_eq!(config.engine_at(100), Engine::Satin);
    for (timestamp, parent, activates) in [
        (100, 99, true),
        // A gap between the parent and the block: still the activation block.
        (250, 40, true),
        (100, 0, true),
        // The parent already at or after satinTime: not.
        (100, 100, false),
        (101, 100, false),
        (500, 120, false),
    ] {
        let a = config.activation(timestamp, Some(parent));
        assert_eq!(a.engine(), Engine::Satin);
        assert_eq!(a.satin_activates(), activates, "{timestamp} on {parent}");
        assert_eq!(a.only_deposits(), activates, "{timestamp} on {parent}");
    }
    // Before satinTime the legacy engine runs, and Satin activates in no such block.
    let before = config.activation(99, Some(98));
    assert_eq!(before.engine(), Engine::Legacy);
    assert!(!before.satin_activates());
}

#[test]
fn chain_config_op_fork_activation_admits_only_deposits() {
    // Jovian and Karst activating with Satin: one deposit-only block.
    let config = Members::satin(0, 100).with("jovianTime", "100").with("karstTime", "100").read();
    let a = config.unwrap().activation(100, Some(99));
    assert!(a.satin_activates() && a.only_deposits());
    // Karst activating at Satin's block on a chain where Satin activates in it, Jovian earlier.
    let config = Members::satin(0, 100).with("karstTime", "100").read().unwrap();
    assert!(config.activation(100, Some(99)).only_deposits());
    // A later OP fork activating after Satin: deposit-only, though Satin does not activate.
    let config = Members::satin(0, 100).with("lagoonTime", "200").read().unwrap();
    let a = config.activation(200, Some(199));
    assert!(!a.satin_activates() && a.only_deposits());
    assert!(!config.activation(201, Some(200)).only_deposits());
    // An OP fork before Jovian activating admits every transaction (Holocene at a Satin block is
    // impossible: it is a base fork, active by satinTime); a fork not scheduled never activates.
    let config = Members::satin(0, 0).read().unwrap();
    assert!(!config.activation(1, Some(0)).only_deposits());
    let s = config.schedule();
    assert!(!s.activates(Fork::Lagoon, 1, Some(0)));
}

#[test]
fn chain_config_reads_the_genesis_file() {
    let config = Members::satin(0, 0x60).json();
    for (timestamp, expected) in
        [(r#""0x64""#, 100), (r#""0x0""#, 0), ("100", 100), (r#""0xffffffffffffffff""#, u64::MAX)]
    {
        let genesis = format!(
            r#"{{"nonce": "0x0", "timestamp": {timestamp}, "config": {config}, "alloc": {{}}}}"#
        );
        let read = ChainConfig::from_genesis_json(&genesis).expect(timestamp);
        assert_eq!(read.genesis_timestamp(), expected, "{timestamp}");
        assert_eq!(read.satin_time(), Some(0x60));
    }
    assert!(
        ChainConfig::from_genesis_json(&format!(r#"{{"timestamp": "0x64", "config": {config}}}"#))
            .unwrap()
            .satin_from_genesis_block()
    );
    let no_timestamp = ChainConfig::from_genesis_json(&format!(r#"{{"config": {config}}}"#));
    assert_eq!(no_timestamp.unwrap().genesis_timestamp(), 0);
    for timestamp in [r#""0x""#, r#""64""#, r#""0x1ffffffffffffffff""#, "-1", "null", "1.0"] {
        let genesis = format!(r#"{{"timestamp": {timestamp}, "config": {config}}}"#);
        let error = ChainConfig::from_genesis_json(&genesis).unwrap_err();
        assert!(matches!(error, ConfigError::InvalidValue { key: "timestamp", .. }), "{timestamp}");
    }
    assert_eq!(
        ChainConfig::from_genesis_json(r#"{"timestamp": "0x0"}"#),
        Err(ConfigError::MissingKey { key: "config" })
    );
    assert_eq!(
        ChainConfig::from_genesis_json(r#"{"config": []}"#),
        Err(ConfigError::NotAnObject { what: "the config object" })
    );
    assert_eq!(
        ChainConfig::from_genesis_json("[]"),
        Err(ConfigError::NotAnObject { what: "the genesis file" })
    );
    let trailing = ChainConfig::from_genesis_json(&format!(r#"{{"config": {config}}} x"#));
    assert!(matches!(trailing, Err(ConfigError::Json(_))), "{trailing:?}");
    let error = ChainConfig::from_config_json(r#"{"chainId": 1,"#, 0).unwrap_err();
    assert!(matches!(error, ConfigError::Json(_)), "{error:?}");
}

#[test]
fn chain_config_requires_a_chain_id() {
    let error = Members::satin(0, 0).without("chainId").read().unwrap_err();
    assert_eq!(error, ConfigError::MissingKey { key: "chainId" });
    for value in ["\"4326\"", "null", "-1", "4326.0"] {
        let error = Members::satin(0, 0).with("chainId", value).read().unwrap_err();
        assert!(matches!(error, ConfigError::InvalidValue { key: "chainId", .. }), "{value}");
    }
}

#[test]
fn chain_config_exports_the_protocol_values() {
    let config = Members::satin(0, 0).read().unwrap();
    // The values of the specification's pages on gas, limits and detention.
    let expected = ProtocolParams {
        chain_id: 4326,
        chain_id_check: true,
        execution_cap: 200_000_000,
        tx_total_gas_cap: None,
        max_code_size: 524_288,
        max_initcode_size: 1_048_576,
        tx_limits: [13_107_200, 500_000, 1_000],
        block_budgets: [13_107_200, 500_000, 1_000],
        frame_share: (98, 100),
        block_env_detention_cap: 20_000_000,
        oracle_detention_cap: 20_000_000,
        state_byte_price: 1_530,
        history_byte_price: 88,
        eip8037: true,
        eip2780: true,
        eip7708: true,
        slot_number: 0,
    };
    assert_eq!(config.protocol(), expected);
    assert_eq!(ProtocolParams::satin(4326), expected);
    // The engine runs on what it exports: the version's own values.
    let version = mega_evm2::satin_version();
    assert_eq!(version.tx_gas_limit_cap, expected.execution_cap);
    assert_eq!(version.max_code_size, expected.max_code_size);
    assert_eq!(version.max_initcode_size, expected.max_initcode_size);
}
