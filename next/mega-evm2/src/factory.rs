//! The engine's constructor: a factory over a validated [`ChainConfig`] that makes every EVM the
//! engine runs, a block session ([`SatinFactory::start_block`]) or a single transaction's EVM
//! ([`SatinFactory::evm`]).
//!
//! The factory installs what the spec fixes, whatever a caller supplies (`SATIN-ACT-009`): Satin's
//! version (its gas schedule, state and history byte prices, EIP-8037/2780/7708, the execution
//! cap, the code and initcode limits, the chain-id check), the chain id of the configuration,
//! Satin's transaction limits and detention caps, the transaction registry and the precompile
//! set. A block's activation facts — whether Satin activates in it, whether it admits only
//! deposits, whether it is block zero — are worked out from the configuration and the parent
//! context ([`ChainConfig::activation`]), never taken from the caller (`SATIN-ACT-010`).
//!
//! Every EVM reads SALT's bucket capacities from a snapshot the caller gives it
//! ([`SaltSnapshot`]); there is no default that answers a bucket the snapshot cannot read. A block
//! session's snapshot must be of the block's parent ([`SnapshotId::parent_hash`]): the same height
//! under another parent is another snapshot.
//!
//! Simulations — a tool's or a test's counterfactual run — use the separately named entry points
//! ([`SatinFactory::start_simulated_block`], [`SatinFactory::simulated_evm`]) with the switches of
//! a [`Simulation`]; a session reports whether it is one ([`BlockSession::is_simulation`]). The
//! consensus entry points take no switch.

use crate::{
    Cfg, MegaSpec, MegaTypes, TxLimits,
    block::BlockSession,
    chain::{Activation, ChainConfig, Engine, Fork, ProtocolParams},
    error::{BlockError, BlockIdentity, InputError},
    salt::{SaltCache, SaltSnapshot},
};
use alloy_primitives::{Address, B256, U256};
use evm2::{Evm, ExecutionConfig, Inspector, env::BlockEnv, evm::DynDatabase};
use std::{boxed::Box, sync::Arc};

/// The header fields of a block the engine reads, from the consumer's authenticated header.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BlockHeader {
    pub number: u64,
    pub timestamp: u64,
    pub beneficiary: Address,
    pub gas_limit: u64,
    pub base_fee: u64,
    pub difficulty: U256,
    /// The header's mix hash, the block's `PREVRANDAO`.
    pub prevrandao: B256,
    /// Zero on every OP chain header; the engine executes no other.
    pub excess_blob_gas: u64,
    pub parent_hash: B256,
    pub parent_beacon_block_root: Option<B256>,
}

/// What precedes the block: nothing for block zero, otherwise its authenticated parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParentContext {
    /// The block is block zero, which has no parent.
    BlockZero,
    /// The block's parent, from the consumer's authenticated chain.
    Parent { number: u64, hash: B256, timestamp: u64 },
}

/// A block to execute: its header and what precedes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockInput {
    pub header: BlockHeader,
    pub parent: ParentContext,
}

/// What the node supplies a block beside its state.
pub struct BlockServices<'a> {
    /// SALT's bucket capacities in the block's parent state, the state the database reads: the
    /// factory refuses a snapshot of another parent. Shared, so a node can hand the same snapshot
    /// to several sessions of a block.
    pub salt: Arc<dyn SaltSnapshot>,
    /// The node's environment: the Oracle service's answers.
    pub environment: Cfg,
    /// An inspector that observes the whole block, its pre-block calls included. It observes; it
    /// does not change what the block executes.
    pub inspector: Option<Box<dyn Inspector<MegaTypes> + 'a>>,
}

impl BlockServices<'_> {
    /// The services of a block over the SALT snapshot `salt`, with an empty environment and no
    /// inspector.
    pub fn new(salt: Arc<dyn SaltSnapshot>) -> Self {
        Self { salt, environment: Cfg::default(), inspector: None }
    }
}

/// The explicitly named switches of a simulation: a run that is not the chain's, which a tool or a
/// test asks for by name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Simulation {
    /// Transactions run without their sender's nonce check.
    pub skip_nonce_check: bool,
    /// Transactions run without EIP-3607's refusal of a sender with code.
    pub skip_eip3607: bool,
    /// Per-transaction limits and detention caps other than Satin's.
    pub limits: Option<TxLimits>,
    /// Comparison only: the rules of the implementation under review to run under
    /// (`Lite::reference_rules`), the version [`crate::satin_version_under`] builds.
    #[cfg(feature = "compare")]
    pub reference_rules: Vec<crate::ReferenceRule>,
}

/// The engine's factory over one validated chain configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SatinFactory {
    config: ChainConfig,
}

impl SatinFactory {
    pub const fn new(config: ChainConfig) -> Self {
        Self { config }
    }

    pub const fn config(&self) -> &ChainConfig {
        &self.config
    }

    /// The protocol values every Satin block of the chain executes on (`SATIN-ACT-009`).
    pub fn protocol(&self) -> ProtocolParams {
        self.config.protocol()
    }

    /// Starts the block `input` over `database` under consensus rules: checks the input against
    /// the configuration and its parent, and the SALT snapshot against the parent, derives its
    /// activation facts, and runs its pre-block sequence. The session returned is ready for
    /// transactions.
    pub fn start_block<'a>(
        &self,
        input: &BlockInput,
        database: impl DynDatabase + 'a,
        services: BlockServices<'a>,
    ) -> Result<BlockSession<'a>, BlockError> {
        self.start(input, database, services, None)
    }

    /// [`Self::start_block`] under a simulation's switches.
    pub fn start_simulated_block<'a>(
        &self,
        input: &BlockInput,
        database: impl DynDatabase + 'a,
        services: BlockServices<'a>,
        simulation: &Simulation,
    ) -> Result<BlockSession<'a>, BlockError> {
        self.start(input, database, services, Some(simulation))
    }

    fn start<'a>(
        &self,
        input: &BlockInput,
        database: impl DynDatabase + 'a,
        services: BlockServices<'a>,
        simulation: Option<&Simulation>,
    ) -> Result<BlockSession<'a>, BlockError> {
        let activation = self.check_input(input)?;
        let h = &input.header;
        let snapshot = services.salt.id();
        if snapshot.parent_hash != h.parent_hash {
            return Err(BlockError::input(InputError::SaltSnapshotParent {
                parent_hash: h.parent_hash,
                snapshot,
            }));
        }
        let block = BlockEnv::<MegaTypes> {
            number: U256::from(h.number),
            beneficiary: h.beneficiary,
            timestamp: U256::from(h.timestamp),
            gas_limit: U256::from(h.gas_limit),
            basefee: U256::from(h.base_fee),
            difficulty: h.difficulty,
            prevrandao: U256::from_be_bytes(h.prevrandao.0),
            // An OP header has no excess blob gas ([`Self::check_input`]): the minimum.
            blob_basefee: U256::from(1),
            slot_num: U256::ZERO,
            ..BlockEnv::<MegaTypes>::default()
        };
        let mut evm = self.build(block, database, services.salt, simulation);
        evm.ext_mut().cfg = services.environment;
        if let Some(inspector) = services.inspector {
            evm.set_boxed_inspector(inspector);
        }
        let identity = BlockIdentity { number: h.number, parent_hash: h.parent_hash };
        let limits = simulation.and_then(|s| s.limits).unwrap_or(TxLimits::SATIN);
        BlockSession::start(
            evm,
            identity,
            snapshot,
            h.parent_beacon_block_root,
            activation,
            limits,
            simulation.is_some(),
        )
    }

    /// A single transaction's EVM in the block environment `block`, under Satin's rules: for a
    /// transaction executed outside a block session (a call, an estimate). It reads SALT from
    /// `salt`, which the caller binds to the state `database` reads (the environment carries no
    /// parent to check it against); its environment (`Lite::cfg`) starts empty.
    pub fn evm<'a>(
        &self,
        block: BlockEnv<MegaTypes>,
        database: impl DynDatabase + 'a,
        salt: Arc<dyn SaltSnapshot>,
    ) -> Result<Evm<'a, MegaTypes>, BlockError> {
        let timestamp = u64::try_from(block.timestamp)
            .map_err(|_| BlockError::input(InputError::TimestampTooLarge))?;
        self.check_timestamp(timestamp)?;
        Ok(self.build(block, database, salt, None))
    }

    /// [`Self::evm`] under a simulation's switches.
    pub fn simulated_evm<'a>(
        &self,
        block: BlockEnv<MegaTypes>,
        database: impl DynDatabase + 'a,
        salt: Arc<dyn SaltSnapshot>,
        simulation: &Simulation,
    ) -> Result<Evm<'a, MegaTypes>, BlockError> {
        let timestamp = u64::try_from(block.timestamp)
            .map_err(|_| BlockError::input(InputError::TimestampTooLarge))?;
        self.check_timestamp(timestamp)?;
        Ok(self.build(block, database, salt, Some(simulation)))
    }

    /// A Satin timestamp, at which no fork the engine has no rules for is active.
    fn check_timestamp(&self, timestamp: u64) -> Result<(), BlockError> {
        if self.config.engine_at(timestamp) != Engine::Satin {
            return Err(BlockError::input(InputError::NotSatin {
                timestamp,
                satin_time: self.config.satin_time(),
            }));
        }
        if let Some(fork) = Fork::UNSUPPORTED_UNDER_SATIN
            .into_iter()
            .find(|fork| self.config.schedule().is_active(*fork, timestamp))
        {
            return Err(BlockError::input(InputError::UnsupportedFork { fork, timestamp }));
        }
        Ok(())
    }

    /// The block's input against the configuration and its parent, and its activation facts.
    fn check_input(&self, input: &BlockInput) -> Result<Activation, BlockError> {
        let h = &input.header;
        let genesis_timestamp = self.config.genesis_timestamp();
        self.check_timestamp(h.timestamp)?;
        let fail = |e| Err(BlockError::input(e));
        let parent_timestamp = match input.parent {
            ParentContext::BlockZero => {
                if h.number != 0 {
                    return fail(InputError::Number { number: h.number, parent: None });
                }
                if h.timestamp != genesis_timestamp {
                    return fail(InputError::GenesisTimestamp {
                        timestamp: h.timestamp,
                        genesis_timestamp,
                    });
                }
                None
            }
            ParentContext::Parent { number, hash, timestamp } => {
                if number.checked_add(1) != Some(h.number) {
                    return fail(InputError::Number { number: h.number, parent: Some(number) });
                }
                if h.parent_hash != hash {
                    return fail(InputError::ParentHash {
                        parent_hash: h.parent_hash,
                        parent: hash,
                    });
                }
                if timestamp > h.timestamp {
                    return fail(InputError::ParentAfterBlock {
                        parent_timestamp: timestamp,
                        timestamp: h.timestamp,
                    });
                }
                if timestamp < genesis_timestamp || number == 0 && timestamp != genesis_timestamp {
                    return fail(InputError::GenesisTimestamp { timestamp, genesis_timestamp });
                }
                Some(timestamp)
            }
        };
        if h.excess_blob_gas != 0 {
            return fail(InputError::ExcessBlobGas { excess_blob_gas: h.excess_blob_gas });
        }
        Ok(self.config.activation(h.timestamp, parent_timestamp))
    }

    /// An EVM with what the spec fixes installed, reading SALT from `salt`, plus a simulation's
    /// switches.
    fn build<'a>(
        &self,
        block: BlockEnv<MegaTypes>,
        database: impl DynDatabase + 'a,
        salt: Arc<dyn SaltSnapshot>,
        simulation: Option<&Simulation>,
    ) -> Evm<'a, MegaTypes> {
        #[cfg(feature = "compare")]
        let mut version = crate::satin_version_under(
            simulation.map(|s| s.reference_rules.as_slice()).unwrap_or_default(),
        );
        #[cfg(not(feature = "compare"))]
        let mut version = crate::satin_version();
        version.chain_id = self.config.chain_id();
        if let Some(s) = simulation {
            if s.skip_nonce_check {
                version.features.remove(evm2::EvmFeatures::NONCE_CHECK);
            }
            if s.skip_eip3607 {
                version.features.remove(evm2::EvmFeatures::EIP3607);
            }
        }
        let config = ExecutionConfig::<MegaTypes>::for_spec_and_version(MegaSpec::Satin, version);
        let mut evm = Evm::<MegaTypes>::new_with_execution_config(
            config,
            MegaSpec::Satin,
            block,
            crate::tx::registry(),
            database,
            crate::precompiles(),
        );
        // Read storage of an account the database lacks, as revm does: the read set holds it.
        evm.state_mut().set_read_absent_storage(true);
        let ext = evm.ext_mut();
        ext.salt = SaltCache::new(salt);
        ext.limits = simulation.and_then(|s| s.limits).unwrap_or(TxLimits::SATIN);
        #[cfg(feature = "compare")]
        if let Some(s) = simulation {
            ext.reference_rules = s.reference_rules.clone();
        }
        evm
    }
}
