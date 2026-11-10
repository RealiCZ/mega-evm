//! The chain configuration the engine executes a chain by: its chain id, its complete fork
//! schedule, its genesis timestamp and the `SequencerRegistry`'s bootstrap values, validated once,
//! before any block runs (`SATIN-ACT-001`–`003`, `SATIN-ACT-011`, `SATIN-ACT-012`,
//! `SATIN-REG-009`).
//!
//! Every reader — a node, a validator, a prover, a CLI — builds its [`ChainConfig`] here, from the
//! genesis file's text ([`ChainConfig::from_genesis_json`], [`ChainConfig::from_config_json`]) or
//! in memory ([`ChainConfig::new`]); both paths run the same validation, so no reader can hold a
//! configuration another refuses. A validated configuration is read-only.
//!
//! # The keys
//!
//! The keys sit at the top level of the genesis file's `config` object, beside every other fork's:
//!
//! | Key | Value | Meaning |
//! | --- | --- | --- |
//! | `chainId` | integer | the chain id; required |
//! | `satinTime` | integer | Satin's activation timestamp; absent: unscheduled |
//! | `shanghaiTime`, `cancunTime`, `pragueTime`, `osakaTime` | integer or null | Ethereum forks |
//! | `regolithTime` … `karstTime`, `lagoonTime` | integer or null | OP forks |
//! | `miniRexTime`, `miniRex1Time`, `miniRex2Time`, `rexTime`, `rex1Time` … `rex6Time` | integer or null | the Mega ladder |
//! | `rex5InitialSequencer`, `rex5InitialAdmin` | address string or null | registry bootstrap |
//! | `rex6MinRotationDelay` | integer or null | registry bootstrap |
//!
//! An *integer* is a JSON number written as digits alone, at most `2^64 − 1`, read exactly from
//! the text (no floating-point step). The rules:
//!
//! - `satinTime`, when present, must be an integer: null, a negative, fractional, exponent or
//!   overflowing number, a boolean, a string, an array or an object is refused (`SATIN-ACT-002`).
//! - A key starting with `satin`, in any ASCII letter case, that is not exactly `satinTime` is
//!   refused, whether or not `satinTime` is also present: a misspelling or a key of an earlier
//!   draft cannot load as if it were valid (`SATIN-ACT-002`).
//! - A key given twice is read as its last copy, then validated (`SATIN-ACT-002`).
//! - Every other fork key and the bootstrap keys read null as absent, as the node's own genesis
//!   reader does; any other value of the wrong type is refused.
//! - Every other key is another component's and is not read.
//!
//! The reader works on the JSON text itself ([`json`]): a large number is never handed over in a
//! deserializer-specific form, so `{"$serde_json::private::Number": "<digits>"}` is an object, and
//! refused as a timestamp, under every build.
//!
//! # Validation of a configuration that schedules Satin
//!
//! - every Mega spec below Satin on the ladder — MiniRex, Rex, Rex1 … Rex6 — is scheduled, and the
//!   ladder, the alias specs MiniRex1 and MiniRex2 included where scheduled, does not decrease
//!   (`SATIN-ACT-011`); equal timestamps are allowed;
//! - every base fork Satin runs on — Shanghai, Cancun, Prague and Osaka, and Regolith, Canyon,
//!   Ecotone, Fjord, Granite, Holocene, Isthmus, Jovian and Karst — is active at `satinTime`
//!   (`SATIN-ACT-003`); none has to activate exactly at Satin;
//! - the registry's bootstrap values are present and non-zero (`SATIN-REG-009`).
//!
//! A configuration that does not schedule Satin is not held to these rules: the engine never runs
//! one of its blocks.

pub(crate) mod json;

pub use json::JsonError;

use crate::{
    TxLimits,
    block::BLOCK_BUDGETS,
    error::ConfigError,
    ledger::{CPHB, CPSB, EXECUTION_CAP, FRAME_SHARE},
};
use alloy_primitives::Address;
use json::Json;
use std::string::ToString;

/// A fork the chain configuration schedules by timestamp: the Ethereum forks Satin runs on, the OP
/// forks from Regolith on, and the Mega ladder through Satin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Fork {
    Shanghai,
    Cancun,
    Prague,
    Osaka,
    Regolith,
    Canyon,
    Ecotone,
    Fjord,
    Granite,
    Holocene,
    Isthmus,
    Jovian,
    Karst,
    /// The OP fork after Karst: the engine has no rules for it.
    Lagoon,
    MiniRex,
    MiniRex1,
    MiniRex2,
    Rex,
    Rex1,
    Rex2,
    Rex3,
    Rex4,
    Rex5,
    Rex6,
    Satin,
}

impl Fork {
    /// Every fork, in declaration order.
    pub const ALL: [Self; 25] = [
        Self::Shanghai,
        Self::Cancun,
        Self::Prague,
        Self::Osaka,
        Self::Regolith,
        Self::Canyon,
        Self::Ecotone,
        Self::Fjord,
        Self::Granite,
        Self::Holocene,
        Self::Isthmus,
        Self::Jovian,
        Self::Karst,
        Self::Lagoon,
        Self::MiniRex,
        Self::MiniRex1,
        Self::MiniRex2,
        Self::Rex,
        Self::Rex1,
        Self::Rex2,
        Self::Rex3,
        Self::Rex4,
        Self::Rex5,
        Self::Rex6,
        Self::Satin,
    ];

    /// The base forks that must be active at `satinTime` (`SATIN-ACT-003`).
    pub const SATIN_BASE: [Self; 13] = [
        Self::Shanghai,
        Self::Cancun,
        Self::Prague,
        Self::Osaka,
        Self::Regolith,
        Self::Canyon,
        Self::Ecotone,
        Self::Fjord,
        Self::Granite,
        Self::Holocene,
        Self::Isthmus,
        Self::Jovian,
        Self::Karst,
    ];

    /// The Mega spec ladder through Satin, bottom first (`SATIN-ACT-011`).
    pub const LADDER: [Self; 11] = [
        Self::MiniRex,
        Self::MiniRex1,
        Self::MiniRex2,
        Self::Rex,
        Self::Rex1,
        Self::Rex2,
        Self::Rex3,
        Self::Rex4,
        Self::Rex5,
        Self::Rex6,
        Self::Satin,
    ];

    /// The OP forks at or after Jovian in the OP fork order, whose activation block admits only
    /// deposits (`SATIN-BASE-006`).
    pub const DEPOSIT_ONLY_OP: [Self; 3] = [Self::Jovian, Self::Karst, Self::Lagoon];

    /// The scheduled forks a Satin block cannot run under: the engine has no rules for them, so
    /// a block at which one is active fails explicitly rather than running Karst's.
    pub const UNSUPPORTED_UNDER_SATIN: [Self; 1] = [Self::Lagoon];

    /// The chain-configuration key of the fork's activation timestamp.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Shanghai => "shanghaiTime",
            Self::Cancun => "cancunTime",
            Self::Prague => "pragueTime",
            Self::Osaka => "osakaTime",
            Self::Regolith => "regolithTime",
            Self::Canyon => "canyonTime",
            Self::Ecotone => "ecotoneTime",
            Self::Fjord => "fjordTime",
            Self::Granite => "graniteTime",
            Self::Holocene => "holoceneTime",
            Self::Isthmus => "isthmusTime",
            Self::Jovian => "jovianTime",
            Self::Karst => "karstTime",
            Self::Lagoon => "lagoonTime",
            Self::MiniRex => "miniRexTime",
            Self::MiniRex1 => "miniRex1Time",
            Self::MiniRex2 => "miniRex2Time",
            Self::Rex => "rexTime",
            Self::Rex1 => "rex1Time",
            Self::Rex2 => "rex2Time",
            Self::Rex3 => "rex3Time",
            Self::Rex4 => "rex4Time",
            Self::Rex5 => "rex5Time",
            Self::Rex6 => "rex6Time",
            Self::Satin => SATIN_TIME_KEY,
        }
    }

    /// Whether the fork is an alias rung of the ladder, which a configuration may omit.
    pub const fn is_alias(self) -> bool {
        matches!(self, Self::MiniRex1 | Self::MiniRex2)
    }

    const fn index(self) -> usize {
        self as usize
    }
}

impl core::fmt::Display for Fork {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(self, f)
    }
}

/// The key that schedules Satin (`SATIN-ACT-002`).
pub const SATIN_TIME_KEY: &str = "satinTime";
/// The chain id's key.
pub const CHAIN_ID_KEY: &str = "chainId";
/// The registry bootstrap keys the Rex era introduced (`SATIN-REG-009`).
pub const INITIAL_SEQUENCER_KEY: &str = "rex5InitialSequencer";
pub const INITIAL_ADMIN_KEY: &str = "rex5InitialAdmin";
pub const MIN_ROTATION_DELAY_KEY: &str = "rex6MinRotationDelay";

/// When each fork activates: a timestamp, or `None` for an unscheduled fork.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ForkSchedule([Option<u64>; 25]);

impl ForkSchedule {
    /// A schedule with nothing scheduled.
    pub const fn new() -> Self {
        Self([None; 25])
    }

    /// Every base fork of Satin and every Mega spec through Satin, the alias specs included, active
    /// from `time`; no fork the engine has no rules for.
    pub fn satin_from(time: u64) -> Self {
        let mut schedule = Self::new();
        for fork in Fork::SATIN_BASE.into_iter().chain(Fork::LADDER) {
            schedule.set(fork, Some(time));
        }
        schedule
    }

    /// `self` with `fork` at `time` (`None`: unscheduled).
    pub const fn with(mut self, fork: Fork, time: Option<u64>) -> Self {
        self.0[fork.index()] = time;
        self
    }

    pub const fn set(&mut self, fork: Fork, time: Option<u64>) {
        self.0[fork.index()] = time;
    }

    /// The fork's activation timestamp, `None` when it is not scheduled.
    pub const fn get(&self, fork: Fork) -> Option<u64> {
        self.0[fork.index()]
    }

    /// Whether `fork` is active at `timestamp`: scheduled, at or before it.
    pub fn is_active(&self, fork: Fork, timestamp: u64) -> bool {
        self.get(fork).is_some_and(|time| time <= timestamp)
    }

    /// Whether `fork` activates in a block at `timestamp` whose parent is at `parent`: scheduled,
    /// active at the block and not at its parent (`SATIN-BASE-007`). Block zero, which has no
    /// parent, is no fork's activation block.
    pub fn activates(&self, fork: Fork, timestamp: u64, parent: Option<u64>) -> bool {
        self.is_active(fork, timestamp) && parent.is_some_and(|p| !self.is_active(fork, p))
    }
}

/// The registry's bootstrap values, the keys the Rex era introduced (`SATIN-REG-009`), as a
/// configuration gives them: each may be missing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BootstrapInput {
    pub initial_sequencer: Option<Address>,
    pub initial_admin: Option<Address>,
    pub min_rotation_delay: Option<u64>,
}

/// The registry's bootstrap values of a configuration that schedules Satin: each present and
/// non-zero. A new registry is seeded with them (`SATIN-REG-008`); once the registry holds its
/// roles, its storage decides them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryBootstrap {
    initial_sequencer: Address,
    initial_admin: Address,
    min_rotation_delay: u64,
}

impl RegistryBootstrap {
    pub const fn initial_sequencer(&self) -> Address {
        self.initial_sequencer
    }
    pub const fn initial_admin(&self) -> Address {
        self.initial_admin
    }
    pub const fn min_rotation_delay(&self) -> u64 {
        self.min_rotation_delay
    }
}

/// An unvalidated configuration, for [`ChainConfig::new`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChainConfigInput {
    pub chain_id: u64,
    /// The genesis block's timestamp.
    pub genesis_timestamp: u64,
    pub schedule: ForkSchedule,
    pub bootstrap: BootstrapInput,
}

/// Which engine executes a block: Satin's, or the frozen legacy engine of the spec the block's
/// timestamp resolves to (`SATIN-ACT-001`, `SATIN-ACT-008`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    Legacy,
    Satin,
}

/// What a block's place in the schedule decides, worked out from the configuration and the
/// parent's timestamp alone (`SATIN-ACT-010`): never an input a caller can omit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Activation {
    engine: Engine,
    satin_activates: bool,
    deposits_only: bool,
    block_zero: bool,
}

impl Activation {
    pub const fn engine(&self) -> Engine {
        self.engine
    }

    /// Whether Satin activates in the block: scheduled, active at the block, not at its parent.
    pub const fn satin_activates(&self) -> bool {
        self.satin_activates
    }

    /// Whether the block admits only deposits: Satin, or an OP fork at or after Jovian, activates
    /// in it (`SATIN-BASE-006`).
    pub const fn deposits_only(&self) -> bool {
        self.deposits_only
    }

    /// Whether the block is block zero, which has no parent.
    pub const fn block_zero(&self) -> bool {
        self.block_zero
    }
}

/// A validated chain configuration ([`ChainConfig::new`] and the readers of the genesis file are
/// the only ways to make one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainConfig {
    chain_id: u64,
    genesis_timestamp: u64,
    schedule: ForkSchedule,
    bootstrap: BootstrapInput,
}

impl ChainConfig {
    /// Validates `input` as the readers of the genesis file do.
    pub fn new(input: ChainConfigInput) -> Result<Self, ConfigError> {
        let ChainConfigInput { chain_id, genesis_timestamp, schedule, bootstrap } = input;
        if let Some(satin) = schedule.get(Fork::Satin) {
            validate_ladder(&schedule)?;
            validate_base_forks(&schedule, satin)?;
            validate_bootstrap(&bootstrap)?;
        }
        Ok(Self { chain_id, genesis_timestamp, schedule, bootstrap })
    }

    /// A chain with Satin active from genesis: every base fork and every Mega spec at timestamp
    /// zero, genesis at timestamp zero. For development chains and simulations.
    pub fn satin_from_genesis(
        chain_id: u64,
        bootstrap: BootstrapInput,
    ) -> Result<Self, ConfigError> {
        Self::new(ChainConfigInput {
            chain_id,
            genesis_timestamp: 0,
            schedule: ForkSchedule::satin_from(0),
            bootstrap,
        })
    }

    /// Reads a genesis file's text: its `config` object ([`Self::from_config_json`]'s rules) and
    /// its `timestamp`, a JSON integer or a `0x`-prefixed hexadecimal string (absent: zero).
    pub fn from_genesis_json(text: &str) -> Result<Self, ConfigError> {
        let genesis = json::parse(text).map_err(ConfigError::Json)?;
        if genesis.members().is_none() {
            return Err(ConfigError::NotAnObject { what: "the genesis file" });
        }
        let config = genesis.get("config").ok_or(ConfigError::MissingKey { key: "config" })?;
        let genesis_timestamp = match genesis.get("timestamp") {
            None => 0,
            Some(value) => quantity(value).ok_or_else(|| ConfigError::InvalidValue {
                key: "timestamp",
                expected: "an integer or a 0x-prefixed hexadecimal string of at most 2^64 - 1",
                found: value.kind(),
            })?,
        };
        Self::from_config(config, genesis_timestamp)
    }

    /// Reads the text of a genesis file's `config` object; the genesis timestamp is the genesis
    /// block's.
    pub fn from_config_json(text: &str, genesis_timestamp: u64) -> Result<Self, ConfigError> {
        Self::from_config(&json::parse(text).map_err(ConfigError::Json)?, genesis_timestamp)
    }

    fn from_config(config: &Json<'_>, genesis_timestamp: u64) -> Result<Self, ConfigError> {
        let members =
            config.members().ok_or(ConfigError::NotAnObject { what: "the config object" })?;
        // `SATIN-ACT-002`: a `satin` key that is not exactly `satinTime`, whatever its case, is
        // refused, whether or not `satinTime` is present too.
        if let Some((key, _)) = members.iter().find(|(key, _)| {
            key != SATIN_TIME_KEY &&
                key.len() >= 5 &&
                key.as_bytes()[..5].eq_ignore_ascii_case(b"satin")
        }) {
            return Err(ConfigError::UnknownSatinKey { key: key.to_string() });
        }
        let chain_id = match config.get(CHAIN_ID_KEY) {
            None => return Err(ConfigError::MissingKey { key: CHAIN_ID_KEY }),
            Some(value) => value.as_u64().ok_or_else(|| integer_expected(CHAIN_ID_KEY, value))?,
        };
        let mut schedule = ForkSchedule::new();
        for fork in Fork::ALL {
            let key = fork.key();
            let time = match config.get(key) {
                None => None,
                // Only `satinTime` takes no null (`SATIN-ACT-002`).
                Some(Json::Null) if fork != Fork::Satin => None,
                Some(value) => Some(value.as_u64().ok_or_else(|| integer_expected(key, value))?),
            };
            schedule.set(fork, time);
        }
        let address = |key: &'static str| -> Result<Option<Address>, ConfigError> {
            match config.get(key) {
                None | Some(Json::Null) => Ok(None),
                Some(Json::String(text)) => {
                    text.parse().map(Some).map_err(|_| ConfigError::InvalidValue {
                        key,
                        expected: "a 20-byte hexadecimal address",
                        found: "a string",
                    })
                }
                Some(value) => Err(ConfigError::InvalidValue {
                    key,
                    expected: "a 20-byte hexadecimal address",
                    found: value.kind(),
                }),
            }
        };
        let bootstrap = BootstrapInput {
            initial_sequencer: address(INITIAL_SEQUENCER_KEY)?,
            initial_admin: address(INITIAL_ADMIN_KEY)?,
            min_rotation_delay: match config.get(MIN_ROTATION_DELAY_KEY) {
                None | Some(Json::Null) => None,
                Some(value) => Some(
                    value
                        .as_u64()
                        .ok_or_else(|| integer_expected(MIN_ROTATION_DELAY_KEY, value))?,
                ),
            },
        };
        Self::new(ChainConfigInput { chain_id, genesis_timestamp, schedule, bootstrap })
    }

    pub const fn chain_id(&self) -> u64 {
        self.chain_id
    }

    pub const fn genesis_timestamp(&self) -> u64 {
        self.genesis_timestamp
    }

    pub const fn schedule(&self) -> &ForkSchedule {
        &self.schedule
    }

    /// `satinTime`, `None` when Satin is not scheduled.
    pub const fn satin_time(&self) -> Option<u64> {
        self.schedule.get(Fork::Satin)
    }

    /// Whether Satin is active from genesis: `satinTime` at or before the genesis timestamp
    /// (`SATIN-ACT-012`). Such a chain has no block in which Satin activates.
    pub fn satin_from_genesis_block(&self) -> bool {
        self.satin_time().is_some_and(|time| time <= self.genesis_timestamp)
    }

    /// The registry's bootstrap values, present when the configuration schedules Satin.
    pub fn registry_bootstrap(&self) -> Option<RegistryBootstrap> {
        self.satin_time()?;
        let b = self.bootstrap;
        Some(RegistryBootstrap {
            initial_sequencer: b.initial_sequencer?,
            initial_admin: b.initial_admin?,
            min_rotation_delay: b.min_rotation_delay?,
        })
    }

    /// The engine that executes a block at `timestamp` (`SATIN-ACT-001`).
    pub fn engine_at(&self, timestamp: u64) -> Engine {
        if self.schedule.is_active(Fork::Satin, timestamp) { Engine::Satin } else { Engine::Legacy }
    }

    /// What the schedule decides for a block at `timestamp` whose parent is at `parent` (`None`:
    /// block zero) (`SATIN-ACT-010`, `SATIN-BASE-006`, `SATIN-BASE-007`).
    pub fn activation(&self, timestamp: u64, parent: Option<u64>) -> Activation {
        let s = &self.schedule;
        let satin_activates = s.activates(Fork::Satin, timestamp, parent);
        let deposits_only = satin_activates ||
            Fork::DEPOSIT_ONLY_OP.into_iter().any(|fork| s.activates(fork, timestamp, parent));
        Activation {
            engine: self.engine_at(timestamp),
            satin_activates,
            deposits_only,
            block_zero: parent.is_none(),
        }
    }

    /// The protocol values every Satin block executes on, whatever a node or a block names
    /// (`SATIN-ACT-009`): what a gas estimator or a transaction pool reads before execution.
    pub fn protocol(&self) -> ProtocolParams {
        ProtocolParams::satin(self.chain_id)
    }
}

/// The configuration's validation of the Mega ladder (`SATIN-ACT-011`).
fn validate_ladder(schedule: &ForkSchedule) -> Result<(), ConfigError> {
    let mut below: Option<(Fork, u64)> = None;
    for fork in Fork::LADDER {
        let Some(time) = schedule.get(fork) else {
            if fork.is_alias() {
                continue;
            }
            return Err(ConfigError::LadderGap { fork });
        };
        if let Some((lower, lower_time)) = below &&
            lower_time > time
        {
            return Err(ConfigError::LadderInverted {
                fork: lower,
                time: lower_time,
                above: fork,
                above_time: time,
            });
        }
        below = Some((fork, time));
    }
    Ok(())
}

/// The base forks active at `satinTime` (`SATIN-ACT-003`).
fn validate_base_forks(schedule: &ForkSchedule, satin: u64) -> Result<(), ConfigError> {
    for fork in Fork::SATIN_BASE {
        if !schedule.is_active(fork, satin) {
            return Err(ConfigError::BaseForkInactive {
                fork,
                time: schedule.get(fork),
                satin_time: satin,
            });
        }
    }
    Ok(())
}

/// The registry's bootstrap values present and non-zero (`SATIN-REG-009`).
fn validate_bootstrap(b: &BootstrapInput) -> Result<(), ConfigError> {
    let checks = [
        (INITIAL_SEQUENCER_KEY, b.initial_sequencer.map(|a| a.is_zero())),
        (INITIAL_ADMIN_KEY, b.initial_admin.map(|a| a.is_zero())),
        (MIN_ROTATION_DELAY_KEY, b.min_rotation_delay.map(|d| d == 0)),
    ];
    for (key, zero) in checks {
        match zero {
            None => return Err(ConfigError::MissingBootstrap { key }),
            Some(true) => return Err(ConfigError::ZeroBootstrap { key }),
            Some(false) => {}
        }
    }
    Ok(())
}

fn integer_expected(key: &'static str, value: &Json<'_>) -> ConfigError {
    ConfigError::InvalidValue {
        key,
        expected: "an integer from 0 to 2^64 - 1",
        found: match value {
            Json::Number(_) => "a number that is not such an integer",
            other => other.kind(),
        },
    }
}

/// A genesis quantity: a JSON integer, or a `0x`-prefixed hexadecimal string (as genesis files
/// write their header fields).
fn quantity(value: &Json<'_>) -> Option<u64> {
    match value {
        Json::Number(_) => value.as_u64(),
        Json::String(text) => {
            let digits = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X"))?;
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            u64::from_str_radix(digits, 16).ok()
        }
        _ => None,
    }
}

/// The execution configuration the spec fixes for Satin (`SATIN-ACT-009`), whatever a node
/// supplies: the values the engine runs every Satin block on, exported so that a reader that needs
/// them before execution — a gas estimator against the execution cap, a pool's admission — reads
/// the engine's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProtocolParams {
    /// The chain id transactions are validated against and `CHAINID` pushes.
    pub chain_id: u64,
    /// Whether a transaction's chain id is checked: always on.
    pub chain_id_check: bool,
    /// The most regular gas a transaction runs on.
    pub execution_cap: u64,
    /// No total gas cap bounds a transaction's gas limit (EIP-7825 and EIP-8037's are removed):
    /// only the block's gas limit does.
    pub tx_total_gas_cap: Option<u64>,
    /// Deployed code and initcode size limits, in bytes.
    pub max_code_size: usize,
    pub max_initcode_size: usize,
    /// The per-transaction data size, KV update and state growth limits.
    pub tx_limits: [u64; 3],
    /// The block's packing budgets: data size, KV updates, state growth.
    pub block_budgets: [u64; 3],
    /// The share of its remaining resource budgets a frame passes to a child, as a fraction.
    pub frame_share: (u64, u64),
    /// Gas detention's caps on compute after a block-environment and an Oracle read.
    pub block_env_detention_cap: u64,
    pub oracle_detention_cap: u64,
    /// The byte prices of state and history gas.
    pub state_byte_price: u64,
    pub history_byte_price: u64,
    /// EIP-8037, EIP-2780 and EIP-7708 are on.
    pub eip8037: bool,
    pub eip2780: bool,
    pub eip7708: bool,
    /// `SLOTNUM` pushes zero in every block.
    pub slot_number: u64,
}

impl ProtocolParams {
    /// Satin's values for `chain_id`, read off the version and the limits the engine executes
    /// with.
    pub fn satin(chain_id: u64) -> Self {
        let version = crate::satin_version();
        let features = version.features;
        let limits = TxLimits::SATIN;
        Self {
            chain_id,
            chain_id_check: features.contains(evm2::EvmFeatures::TX_CHAIN_ID_CHECK),
            execution_cap: EXECUTION_CAP,
            tx_total_gas_cap: (version.tx_total_gas_cap != u64::MAX)
                .then_some(version.tx_total_gas_cap),
            max_code_size: version.max_code_size,
            max_initcode_size: version.max_initcode_size,
            tx_limits: limits.tx,
            block_budgets: BLOCK_BUDGETS,
            frame_share: FRAME_SHARE,
            block_env_detention_cap: limits.cap,
            oracle_detention_cap: limits.oracle_cap,
            state_byte_price: CPSB,
            history_byte_price: CPHB,
            eip8037: features.contains(evm2::EvmFeatures::EIP8037),
            eip2780: features.contains(evm2::EvmFeatures::EIP2780),
            eip7708: features.contains(evm2::EvmFeatures::EIP7708),
            slot_number: 0,
        }
    }
}
