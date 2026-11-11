//! The MegaETH system contracts every Satin block deploys before its transactions, and the
//! EIP-7997 factory the block in which Satin activates checks: their addresses, the code hash each
//! address must hold (`SATIN-SYSC-001`, `SATIN-SYSC-002`, `SATIN-REG-001`, `SATIN-ACT-005`) and the
//! runtime bytecode a deployment installs (`SATIN-SYSC-003`, `SATIN-REG-008`).
//!
//! The deployment itself is a step of the block's pre-block sequence (`crate::block`).

pub use crate::system_code::*;
use crate::{
    MegaTypes, ORACLE, SEQUENCER_REGISTRY, error::SystemAddressFault, intercept::LIMIT_CONTROL,
    keyless::KEYLESS, satin_ext::ACCESS_CONTROL,
};
use alloy_primitives::{Address, B256, Bytes, U256, address, b256};
use evm2::{
    DatabaseError, Evm,
    evm::{AccountInfo, PendingState},
};

/// The High-Precision Timestamp's address.
pub const HIGH_PRECISION_TIMESTAMP: Address =
    address!("0x6342000000000000000000000000000000000002");

/// Oracle version 2.0.0 (`ORACLE_CODE_HASH`).
pub const ORACLE_CODE_HASH: B256 =
    b256!("71a65239db8d0f1bb765fad36e34f57600420d103a4401ef7555bd50b229dc55");
/// The High-Precision Timestamp, the Oracle's address embedded
/// (`HIGH_PRECISION_TIMESTAMP_CODE_HASH`).
pub const HIGH_PRECISION_TIMESTAMP_CODE_HASH: B256 =
    b256!("1b2df8ca5350cd7106d67ed95532584526f97a5ca267c8eef73a1831f53720f2");
/// KeylessDeploy version 1.0.0 (`KEYLESS_DEPLOY_CODE_HASH`).
pub const KEYLESS_DEPLOY_CODE_HASH: B256 =
    b256!("55020d41649acf7a84add6e628b887f802218d9ac86f142ef0994da43ea5eeb6");
/// MegaAccessControl version 1.0.0 (`MEGA_ACCESS_CONTROL_CODE_HASH`).
pub const MEGA_ACCESS_CONTROL_CODE_HASH: B256 =
    b256!("96d0f3ba6b474e9684a97e4388ec8dbb7929818b51e6b4f19f885ccbdf642f9b");
/// MegaLimitControl version 1.0.0 (`MEGA_LIMIT_CONTROL_CODE_HASH`).
pub const MEGA_LIMIT_CONTROL_CODE_HASH: B256 =
    b256!("3927f2a4803c5e18153ff5742d0fa1acd9ad04538e4e6037cb4a9b28694ca87f");
/// SequencerRegistry version 2.0.0 (`SEQUENCER_REGISTRY_CODE_HASH`).
pub const SEQUENCER_REGISTRY_CODE_HASH: B256 = crate::tx::SEQUENCER_REGISTRY_CODE_HASH;
/// SequencerRegistry version 1.0.0, which a block migrates in place
/// (`SEQUENCER_REGISTRY_V1_CODE_HASH`).
pub const SEQUENCER_REGISTRY_V1_CODE_HASH: B256 =
    b256!("63cd411a379be1c198613ef1d15c3058e7b0db4a5d07d4bcf07014af90040315");

/// The initial system address a new registry is seeded with (`MEGA_SYSTEM_ADDRESS`).
pub const MEGA_SYSTEM_ADDRESS: Address = address!("0xA887dCB9D5f39Ef79272801d05Abdf707CFBbD1d");

/// The EIP-7997 deterministic deployment proxy (`CREATE2_FACTORY_ADDRESS`).
pub const CREATE2_FACTORY: Address = address!("0x4e59b44847b379578588920ca78fbf26c0b4956c");
/// The code hash of the factory's runtime (`CREATE2_FACTORY_CODE_HASH`).
pub const CREATE2_FACTORY_CODE_HASH: B256 =
    b256!("2fa86add0aed31f33a762c9d88e807c475bd51d0f52bd0955754b2608f7e4989");

/// A system contract a deployment installs: its address, the code hash the address must hold and
/// the code installed when it does not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemContract {
    pub name: &'static str,
    pub address: Address,
    pub code_hash: B256,
    pub code: Bytes,
}

/// The five contracts deployed by the same rule, in their deployment order (`SATIN-SYSC-003`): the
/// Oracle, the High-Precision Timestamp, KeylessDeploy, MegaAccessControl and MegaLimitControl.
pub const FLAT_SYSTEM_CONTRACTS: [SystemContract; 5] = [
    SystemContract {
        name: "Oracle",
        address: ORACLE,
        code_hash: ORACLE_CODE_HASH,
        code: ORACLE_CODE,
    },
    SystemContract {
        name: "High-Precision Timestamp",
        address: HIGH_PRECISION_TIMESTAMP,
        code_hash: HIGH_PRECISION_TIMESTAMP_CODE_HASH,
        code: HIGH_PRECISION_TIMESTAMP_CODE,
    },
    SystemContract {
        name: "KeylessDeploy",
        address: KEYLESS,
        code_hash: KEYLESS_DEPLOY_CODE_HASH,
        code: KEYLESS_DEPLOY_CODE,
    },
    SystemContract {
        name: "MegaAccessControl",
        address: ACCESS_CONTROL,
        code_hash: MEGA_ACCESS_CONTROL_CODE_HASH,
        code: MEGA_ACCESS_CONTROL_CODE,
    },
    SystemContract {
        name: "MegaLimitControl",
        address: LIMIT_CONTROL,
        code_hash: MEGA_LIMIT_CONTROL_CODE_HASH,
        code: MEGA_LIMIT_CONTROL_CODE,
    },
];

/// The SequencerRegistry, deployed by its own rule (`SATIN-REG-008`).
pub const SEQUENCER_REGISTRY_CONTRACT: SystemContract = SystemContract {
    name: "SequencerRegistry",
    address: SEQUENCER_REGISTRY,
    code_hash: SEQUENCER_REGISTRY_CODE_HASH,
    code: SEQUENCER_REGISTRY_CODE,
};

/// The SequencerRegistry's storage slots the pre-block sequence reads or writes
/// (`SATIN-REG-001`).
pub mod registry_slot {
    use alloy_primitives::U256;

    /// `_currentSystemAddress`, the live system address.
    pub const CURRENT_SYSTEM_ADDRESS: U256 = U256::from_limbs([0, 0, 0, 0]);
    /// `_currentSequencer`.
    pub const CURRENT_SEQUENCER: U256 = U256::from_limbs([1, 0, 0, 0]);
    /// `_admin`.
    pub const ADMIN: U256 = U256::from_limbs([2, 0, 0, 0]);
    /// `_initialSystemAddress`.
    pub const INITIAL_SYSTEM_ADDRESS: U256 = U256::from_limbs([4, 0, 0, 0]);
    /// `_initialSequencer`.
    pub const INITIAL_SEQUENCER: U256 = U256::from_limbs([5, 0, 0, 0]);
    /// `_initialFromBlock`.
    pub const INITIAL_FROM_BLOCK: U256 = U256::from_limbs([6, 0, 0, 0]);
    /// `_pendingSystemAddress` and `_systemAddressActivationBlock`.
    pub const PENDING_SYSTEM_ADDRESS: U256 = U256::from_limbs([7, 0, 0, 0]);
    pub const SYSTEM_ADDRESS_ACTIVATION_BLOCK: U256 = U256::from_limbs([8, 0, 0, 0]);
    /// `_pendingSequencer` and `_sequencerActivationBlock`.
    pub const PENDING_SEQUENCER: U256 = U256::from_limbs([9, 0, 0, 0]);
    pub const SEQUENCER_ACTIVATION_BLOCK: U256 = U256::from_limbs([10, 0, 0, 0]);
    /// `_minRotationDelay`, version 2.0.0's slot.
    pub const MIN_ROTATION_DELAY: U256 = U256::from_limbs([13, 0, 0, 0]);

    /// Each role's (pending address, activation block) slots: the system address's, then the
    /// sequencer's (`SATIN-PBLK-004`).
    pub const PENDING_ROLES: [(U256, U256); 2] = [
        (PENDING_SYSTEM_ADDRESS, SYSTEM_ADDRESS_ACTIVATION_BLOCK),
        (PENDING_SEQUENCER, SEQUENCER_ACTIVATION_BLOCK),
    ];
}

/// An address as a storage word (Solidity's address-in-slot layout).
pub fn address_word(address: Address) -> U256 {
    U256::from_be_slice(address.as_slice())
}

/// What the resolution of the live system address reads (`SATIN-REG-002`): the registry's account
/// and its slot 0, read directly off the state, neither loaded into a transaction, warmed nor
/// charged. A block reads them once, after its other pre-block steps; a single transaction's EVM
/// once, when it is made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryRead {
    pub account: Option<AccountInfo>,
    /// Slot 0, `_currentSystemAddress`.
    pub current_system_address: U256,
}

impl RegistryRead {
    /// Reads the registry's account, then its slot 0, off `evm`'s state. A read the database cannot
    /// serve is returned with the slot it was of (`None`: the account).
    pub fn read(evm: &mut Evm<'_, MegaTypes>) -> Result<Self, (Option<U256>, DatabaseError)> {
        let key = registry_slot::CURRENT_SYSTEM_ADDRESS;
        let state = evm.state_mut();
        let account = state.account_info_untracked(&SEQUENCER_REGISTRY).map_err(|e| (None, e))?;
        let current_system_address =
            state.storage_slot_untracked(&SEQUENCER_REGISTRY, &key).map_err(|e| (Some(key), e))?;
        Ok(Self { account, current_system_address })
    }

    /// The live system address: slot 0's, when the account holds version 2.0.0's code and the slot
    /// is not zero; otherwise why there is none (`SATIN-REG-004`).
    pub fn resolve(&self) -> Result<Address, SystemAddressFault> {
        let Some(account) = &self.account else {
            return Err(SystemAddressFault::RegistryAbsent);
        };
        if account.code_hash != SEQUENCER_REGISTRY_CODE_HASH {
            return Err(SystemAddressFault::RegistryCode(account.code_hash));
        }
        if self.current_system_address.is_zero() {
            return Err(SystemAddressFault::ZeroSystemAddress);
        }
        Ok(Address::from_word(B256::from(self.current_system_address)))
    }

    /// The read as a read-only state: the account and slot 0 as they stood.
    pub fn state(&self) -> PendingState {
        let mut state = PendingState::default();
        state.insert_account(SEQUENCER_REGISTRY, self.account.clone(), self.account.clone());
        let (key, value) = (registry_slot::CURRENT_SYSTEM_ADDRESS, self.current_system_address);
        state.insert_storage(SEQUENCER_REGISTRY, key, value, value);
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::keccak256;

    /// Every installed bytecode is the code its code hash names, and the addresses are the six
    /// system contracts' in their deployment order.
    #[test]
    fn test_the_installed_code_is_the_code_its_hash_names() {
        for c in FLAT_SYSTEM_CONTRACTS.iter().chain([&SEQUENCER_REGISTRY_CONTRACT]) {
            assert_eq!(keccak256(&c.code), c.code_hash, "{}", c.name);
        }
        let addresses: Vec<u8> = FLAT_SYSTEM_CONTRACTS
            .iter()
            .chain([&SEQUENCER_REGISTRY_CONTRACT])
            .map(|c| {
                assert_eq!(c.address.as_slice()[..19], ORACLE.as_slice()[..19], "{}", c.name);
                c.address.as_slice()[19]
            })
            .collect();
        assert_eq!(addresses, [1, 2, 3, 4, 5, 6]);
    }

    /// An address fills the low 20 bytes of its word.
    #[test]
    fn test_an_address_is_the_low_bytes_of_its_word() {
        let word = address_word(MEGA_SYSTEM_ADDRESS);
        assert_eq!(Address::from_word(B256::from(word)), MEGA_SYSTEM_ADDRESS);
        assert!(word < U256::from(1) << 160);
    }
}
