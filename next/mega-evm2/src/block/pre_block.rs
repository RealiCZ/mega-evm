//! A block's pre-block sequence (`SATIN-PBLK-003`), run by [`BlockSession::start`] before the
//! session is returned, in this order:
//!
//! 1. the EIP-2935 call with the parent's hash;
//! 2. the EIP-4788 call with the parent beacon block root, which the block must carry (block zero
//!    makes neither call, and its root must be zero: `SATIN-ACT-012`);
//! 3. in the block in which Satin activates, and no other, the EIP-7997 factory's check
//!    (`SATIN-ACT-004`, `SATIN-ACT-005`);
//! 4. the deployment of the Oracle, the High-Precision Timestamp, KeylessDeploy, MegaAccessControl
//!    and MegaLimitControl, each a state of its own (`SATIN-SYSC-003`);
//! 5. the SequencerRegistry's due-change read and, when a change is due, its
//!    `applyPendingChanges()` call (`SATIN-PBLK-004`);
//! 6. the SequencerRegistry's deployment (`SATIN-REG-008`);
//! 7. the resolution of the block's live system address (`SATIN-REG-002`, `SATIN-REG-004`).
//!
//! Each step's state, a read-only one included, is committed to the session before the next step
//! runs, so every step sees the ones before it. The pre-block read of the L1 block contract's
//! account and slots is still made, after step 6, so that the resolution is committed last.
//!
//! A deployment and the reads are charged nothing and count toward none of the block's counters.
//! A pre-block call runs from `SYSTEM_CALLER` on 30,000,000 of regular gas and a reservoir of
//! sixteen fresh slots' state gas at `m = 1`, 1,566,720, whatever the block's gas limit
//! (`SATIN-PBLK-001`, `SATIN-PBLK-002`; evm2's system call, whose reservoir is Satin's
//! `SstoreSetState` price times sixteen).

use super::{BEACON_ROOTS, BlockSession, HISTORY_STORAGE};
use crate::{
    SEQUENCER_REGISTRY,
    error::{BlockError, ErrorContext, Phase},
    system_contracts::{
        CREATE2_FACTORY, CREATE2_FACTORY_CODE_HASH, FLAT_SYSTEM_CONTRACTS, MEGA_SYSTEM_ADDRESS,
        RegistryRead, SEQUENCER_REGISTRY_CONTRACT, SEQUENCER_REGISTRY_V1_CODE_HASH, SystemContract,
        address_word, registry_slot,
    },
};
use alloy_primitives::{Address, B256, Bytes, U256, keccak256};
use evm2::{
    bytecode::Bytecode,
    evm::{AccountInfo, PendingState, StateChangeSink, StateChangeSource},
};
use op_evm2::constants::L1_BLOCK_CONTRACT;

/// The L1 block slots the transactions can be priced against (Satin `L1_BLOCK_INFO_SLOTS`).
const L1_BLOCK_INFO_SLOTS: [u64; 5] = [1, 3, 5, 7, 8];

/// A pre-block state whose account's earlier storage is cleared before the state's own changes
/// apply: a new registry seeded over an account without code (`SATIN-REG-008`).
struct Wiped<'s> {
    address: Address,
    state: &'s PendingState,
}

impl StateChangeSource for Wiped<'_> {
    fn visit<S: StateChangeSink>(&self, sink: &mut S) -> Result<(), S::Error> {
        sink.storage_wipe(self.address)?;
        self.state.visit(sink)
    }
}

/// `info` holding `contract`'s code, its nonce, balance and the rest kept.
fn with_code(mut info: AccountInfo, contract: &SystemContract) -> AccountInfo {
    info.code_hash = contract.code_hash;
    info.code = Some(Bytecode::new_raw(contract.code.clone()));
    info
}

/// Whether `info` holds no code.
fn has_no_code(info: &AccountInfo) -> bool {
    info.is_empty_code_hash() || info.code_hash == B256::ZERO
}

impl BlockSession<'_> {
    /// Runs the block's pre-block sequence, committing each step's state in order.
    pub(super) fn run_pre_block_sequence(&mut self) -> Result<(), BlockError> {
        // Steps 1 and 2.
        if self.activation.block_zero() {
            // Block zero has no parent: no EIP-2935 or EIP-4788 call, and its parent beacon block
            // root must be zero.
            match self.parent_beacon_block_root {
                Some(root) if root.is_zero() => {}
                Some(root) => return Err(BlockError::genesis_parent_beacon_root(root)),
                None => return Err(BlockError::missing_parent_beacon_root(BEACON_ROOTS)),
            }
        } else {
            let parent = Bytes::from(self.identity.parent_hash.0.to_vec());
            let state = self.system_call(HISTORY_STORAGE, parent)?;
            self.commit_pre_block_state(state);
            // A block without a parent beacon block root is invalid (EIP-4788).
            let root = self
                .parent_beacon_block_root
                .ok_or_else(|| BlockError::missing_parent_beacon_root(BEACON_ROOTS))?;
            let state = self.system_call(BEACON_ROOTS, Bytes::from(root.0.to_vec()))?;
            self.commit_pre_block_state(state);
        }
        // Step 3: the block in which Satin activates, and no other, checks the factory.
        if self.activation.satin_activates() {
            let state = self.check_create2_factory()?;
            self.commit_pre_block_state(state);
        }
        let deploys = !crate::under_reference_rule(
            self.evm.ext(),
            crate::ReferenceRule::NoPreBlockDeployment,
        );
        // Step 4: the five contracts, in order, each a state of its own.
        if deploys {
            for contract in &FLAT_SYSTEM_CONTRACTS {
                let state = self.deploy(contract)?;
                self.commit_pre_block_state(state);
            }
        }
        // Step 5: the due-change read, committed even when nothing is due, then the call.
        let (due, reads) = self.read_registry_due()?;
        self.commit_pre_block_state(reads);
        if due {
            let state = self.system_call(
                SEQUENCER_REGISTRY,
                Bytes::from(keccak256("applyPendingChanges()")[..4].to_vec()),
            )?;
            self.commit_pre_block_state(state);
        }
        // Step 6: the registry's deployment.
        if deploys {
            let (state, wipe) = self.deploy_registry()?;
            if wipe {
                self.commit_wiping(SEQUENCER_REGISTRY, state);
            } else {
                self.commit_pre_block_state(state);
            }
        }
        // The L1 block info read (read-only, as Satin's `read_l1_block_info`), before the
        // resolution so that the resolution is committed last.
        let state = self.read_l1_block_info()?;
        self.commit_pre_block_state(state);
        // Step 7: the live system address, committed last.
        let (address, reads) = self.resolve_system_address()?;
        self.commit_pre_block_state(reads);
        self.system_address = address;
        self.evm.ext_mut().system_address = Some(address);
        Ok(())
    }

    /// Commits `state` as [`Self::commit_pre_block_state`] does, `address`'s earlier storage
    /// cleared first in the session's state and in the block's change set.
    fn commit_wiping(&mut self, address: Address, state: PendingState) {
        let source = Wiped { address, state: &state };
        self.evm.commit_source(&source);
        let _ = source.visit(&mut self.changes);
        self.pre_block_states.push(state);
    }

    /// The EIP-7997 factory's check (`SATIN-ACT-005`): its address must hold an account with the
    /// factory's code hash; its nonce, balance and storage are not checked. The read is a
    /// read-only state. A read the database cannot serve is an internal error.
    fn check_create2_factory(&mut self) -> Result<PendingState, BlockError> {
        let info = self.evm.state_mut().account_info_untracked(&CREATE2_FACTORY).map_err(|e| {
            BlockError::read(Phase::FactoryCheck, e, ErrorContext::account(CREATE2_FACTORY))
        })?;
        match &info {
            Some(i) if i.code_hash == CREATE2_FACTORY_CODE_HASH => {}
            other => {
                return Err(BlockError::create2_factory(
                    CREATE2_FACTORY,
                    other.as_ref().map(|i| i.code_hash),
                ));
            }
        }
        let mut reads = PendingState::default();
        reads.insert_account(CREATE2_FACTORY, info.clone(), info);
        Ok(reads)
    }

    /// One of the five contracts' deployment (`SATIN-SYSC-003`): an address with no account is
    /// given one holding the contract's code; an account with another code hash has its code
    /// replaced, its nonce, balance and storage kept; an account with the contract's code hash is
    /// left as it is, a read-only state. A read the database cannot serve is an internal error.
    fn deploy(&mut self, contract: &SystemContract) -> Result<PendingState, BlockError> {
        let info = self.evm.state_mut().account_info_untracked(&contract.address).map_err(|e| {
            BlockError::read(Phase::Deployment, e, ErrorContext::account(contract.address))
        })?;
        let mut state = PendingState::default();
        match info {
            Some(info) if info.code_hash == contract.code_hash => {
                state.insert_account(contract.address, Some(info.clone()), Some(info));
            }
            info => {
                let installed = with_code(info.clone().unwrap_or_default(), contract);
                state.insert_account(contract.address, info, Some(installed));
            }
        }
        Ok(state)
    }

    /// The SequencerRegistry's deployment (`SATIN-REG-008`), by what its address holds now:
    /// version 2.0.0 is left as it is (a read-only state); version 1.0.0 is migrated in place, its
    /// code replaced and slot 13 written with the configured minimum rotation delay, every other
    /// slot, its nonce and its balance kept; an address with no account, or an account without
    /// code, receives version 2.0.0 as a new contract, an existing account's nonce and balance
    /// kept and its storage cleared (the second value), and its storage seeded; any other code
    /// makes the block invalid. A read the database cannot serve is an internal error.
    fn deploy_registry(&mut self) -> Result<(PendingState, bool), BlockError> {
        let registry = &SEQUENCER_REGISTRY_CONTRACT;
        let failed = |slot: Option<U256>| {
            move |e: evm2::DatabaseError| {
                let context = ErrorContext { slot, ..ErrorContext::account(SEQUENCER_REGISTRY) };
                BlockError::read(Phase::Deployment, e, context)
            }
        };
        let number = self.evm.block().number;
        let bootstrap = self.bootstrap;
        let state = self.evm.state_mut();
        let info = state.account_info_untracked(&SEQUENCER_REGISTRY).map_err(failed(None))?;
        let mut out = PendingState::default();
        let wipe = match info {
            Some(info) if info.code_hash == registry.code_hash => {
                out.insert_account(SEQUENCER_REGISTRY, Some(info.clone()), Some(info));
                false
            }
            Some(info) if info.code_hash == SEQUENCER_REGISTRY_V1_CODE_HASH => {
                let key = registry_slot::MIN_ROTATION_DELAY;
                let original = state
                    .storage_slot_untracked(&SEQUENCER_REGISTRY, &key)
                    .map_err(failed(Some(key)))?;
                out.insert_account(
                    SEQUENCER_REGISTRY,
                    Some(info.clone()),
                    Some(with_code(info, registry)),
                );
                let delay = U256::from(bootstrap.min_rotation_delay());
                out.insert_storage(SEQUENCER_REGISTRY, key, original, delay);
                false
            }
            Some(info) if !has_no_code(&info) => {
                return Err(BlockError::unmigratable_registry(SEQUENCER_REGISTRY, info.code_hash));
            }
            info => {
                let existed = info.is_some();
                let installed = with_code(info.clone().unwrap_or_default(), registry);
                out.insert_account(SEQUENCER_REGISTRY, info, Some(installed));
                let system = address_word(MEGA_SYSTEM_ADDRESS);
                let sequencer = address_word(bootstrap.initial_sequencer());
                for (key, value) in [
                    (registry_slot::CURRENT_SYSTEM_ADDRESS, system),
                    (registry_slot::CURRENT_SEQUENCER, sequencer),
                    (registry_slot::ADMIN, address_word(bootstrap.initial_admin())),
                    (registry_slot::INITIAL_SYSTEM_ADDRESS, system),
                    (registry_slot::INITIAL_SEQUENCER, sequencer),
                    (registry_slot::INITIAL_FROM_BLOCK, number),
                    (registry_slot::MIN_ROTATION_DELAY, U256::from(bootstrap.min_rotation_delay())),
                ] {
                    out.insert_storage(SEQUENCER_REGISTRY, key, U256::ZERO, value);
                }
                existed
            }
        };
        Ok((out, wipe))
    }

    /// Whether the registry has a role change due in this block (`SATIN-PBLK-004`), with the
    /// read-only state of what it read: the registry's account, each role's pending address slot
    /// and, when that is non-zero, its activation block slot. A change is due when the account
    /// exists and a role's pending address is non-zero and its activation block, its whole value,
    /// is at most the block's number. A read the database cannot serve is an internal error.
    pub(super) fn read_registry_due(&mut self) -> Result<(bool, PendingState), BlockError> {
        let number = self.evm.block().number;
        let state = self.evm.state_mut();
        let failed = |slot: Option<U256>| {
            move |e: evm2::DatabaseError| {
                let context = ErrorContext { slot, ..ErrorContext::account(SEQUENCER_REGISTRY) };
                BlockError::read(Phase::DueChangeRead, e, context)
            }
        };
        let info = state.account_info_untracked(&SEQUENCER_REGISTRY).map_err(failed(None))?;
        let mut reads = PendingState::default();
        reads.insert_account(SEQUENCER_REGISTRY, info.clone(), info.clone());
        let mut due = false;
        if info.is_some() {
            for (pending_slot, activation_slot) in registry_slot::PENDING_ROLES {
                let pending = state
                    .storage_slot_untracked(&SEQUENCER_REGISTRY, &pending_slot)
                    .map_err(failed(Some(pending_slot)))?;
                reads.insert_storage(SEQUENCER_REGISTRY, pending_slot, pending, pending);
                if pending.is_zero() {
                    continue;
                }
                let activation = state
                    .storage_slot_untracked(&SEQUENCER_REGISTRY, &activation_slot)
                    .map_err(failed(Some(activation_slot)))?;
                reads.insert_storage(SEQUENCER_REGISTRY, activation_slot, activation, activation);
                due |= activation <= number;
            }
        }
        Ok((due, reads))
    }

    /// The block's live system address (`SATIN-REG-002`): the registry's account and slot 0, read
    /// off the state the other steps left, its code hash compared and its code neither loaded nor
    /// run, with the read-only state of the read. A registry that is absent, holds other code than
    /// version 2.0.0 or whose slot 0 is zero makes the block invalid (`SATIN-REG-004`); a read the
    /// database cannot serve is an internal error (`SATIN-REG-007`).
    fn resolve_system_address(&mut self) -> Result<(Address, PendingState), BlockError> {
        let read = RegistryRead::read(&mut self.evm).map_err(|(slot, e)| {
            let context = ErrorContext { slot, ..ErrorContext::account(SEQUENCER_REGISTRY) };
            BlockError::read(Phase::SystemAddressResolution, e, context)
        })?;
        let address = read
            .resolve()
            .map_err(|fault| BlockError::system_address(SEQUENCER_REGISTRY, fault))?;
        Ok((address, read.state()))
    }

    /// Satin `read_l1_block_info`: the account, then its slots when it exists, read-only. A read
    /// the database cannot serve is an internal error.
    fn read_l1_block_info(&mut self) -> Result<PendingState, BlockError> {
        let state = self.evm.state_mut();
        let failed = |slot: Option<U256>| {
            move |e: evm2::DatabaseError| {
                let context = ErrorContext { slot, ..ErrorContext::account(L1_BLOCK_CONTRACT) };
                BlockError::read(Phase::L1BlockInfoRead, e, context)
            }
        };
        let info = state.account_info_untracked(&L1_BLOCK_CONTRACT).map_err(failed(None))?;
        let mut reads = PendingState::default();
        reads.insert_account(L1_BLOCK_CONTRACT, info.clone(), info.clone());
        if info.is_some() {
            for slot in L1_BLOCK_INFO_SLOTS {
                let key = U256::from(slot);
                let value = state
                    .storage_slot_untracked(&L1_BLOCK_CONTRACT, &key)
                    .map_err(failed(Some(key)))?;
                reads.insert_storage(L1_BLOCK_CONTRACT, key, value, value);
            }
        }
        Ok(reads)
    }
}
