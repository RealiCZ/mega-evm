//! L1 block info and L1 data / operator fee computation.
//!
//! Port of `op-revm/src/l1block.rs`.

use crate::{
    OpSpecId,
    constants::{
        BASE_FEE_SCALAR_OFFSET, BLOB_BASE_FEE_SCALAR_OFFSET, DA_FOOTPRINT_GAS_SCALAR_OFFSET,
        DA_FOOTPRINT_GAS_SCALAR_SLOT, DEPOSIT_TRANSACTION_TYPE, ECOTONE_L1_BLOB_BASE_FEE_SLOT,
        ECOTONE_L1_FEE_SCALARS_SLOT, EMPTY_SCALARS, L1_BASE_FEE_SLOT, L1_BLOCK_CONTRACT,
        L1_OVERHEAD_SLOT, L1_SCALAR_SLOT, NON_ZERO_BYTE_COST, OPERATOR_FEE_CONSTANT_OFFSET,
        OPERATOR_FEE_JOVIAN_MULTIPLIER, OPERATOR_FEE_SCALAR_DECIMAL, OPERATOR_FEE_SCALAR_OFFSET,
        OPERATOR_FEE_SCALARS_SLOT,
    },
    transaction::estimate_tx_compressed_size,
};
use alloy_primitives::U256;
use evm2::{DatabaseError, evm::State};

/// Calldata token cost (same as Istanbul's `STANDARD_TOKEN_COST`).
const STANDARD_TOKEN_COST: u64 = 4;
/// Non-zero byte token multiplier since Istanbul.
const NON_ZERO_BYTE_MULTIPLIER_ISTANBUL: u64 = 4;

/// L1 block info, as stored in the `L1Block` predeploy.
///
/// This is the data needed to compute the L1 data fee and the operator fee of a transaction. It is
/// cached per L2 block in [`OpEvmExt`](crate::OpEvmExt) and reloaded from the `L1Block` storage
/// when [`Self::l2_block`] does not match the current block number, exactly like op-revm's
/// `L1BlockInfo`. Callers may also set it directly (set `l2_block` to the current block number to
/// prevent a reload).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct L1BlockInfo {
    /// The L2 block number this info was loaded for. If it differs from the current block number,
    /// the info is reloaded from the database before the next non-deposit transaction.
    pub l2_block: Option<U256>,
    /// The base fee of the L1 origin block.
    pub l1_base_fee: U256,
    /// The current L1 fee overhead. `None` if Ecotone is activated.
    pub l1_fee_overhead: Option<U256>,
    /// The current L1 fee scalar.
    pub l1_base_fee_scalar: U256,
    /// The current L1 blob base fee. `None` if Ecotone is not activated.
    pub l1_blob_base_fee: Option<U256>,
    /// The current L1 blob base fee scalar. `None` if Ecotone is not activated.
    pub l1_blob_base_fee_scalar: Option<U256>,
    /// The operator fee scalar. `None` if Isthmus is not activated.
    pub operator_fee_scalar: Option<U256>,
    /// The operator fee constant. `None` if Isthmus is not activated.
    pub operator_fee_constant: Option<U256>,
    /// DA footprint gas scalar. Always `None` prior to the Jovian hardfork.
    pub da_footprint_gas_scalar: Option<u16>,
    /// True if Ecotone is activated, but the L1 fee scalars have not yet been set.
    pub empty_ecotone_scalars: bool,
    /// Last calculated L1 fee. Used as a cache between validation and settlement of a transaction.
    pub tx_l1_cost: Option<U256>,
}

/// A callback handed every slot of the `L1Block` predeploy a fetch reads, with the value read
/// ([`L1BlockInfo::try_fetch_with`], [`L1BlockInfo::fetch_da_footprint_gas_scalar_with`]).
pub type L1SlotRead<'r> = &'r mut dyn FnMut(U256, U256);

/// Reads a storage slot of the `L1Block` predeploy without loading it into the transaction
/// overlay, and hands it to `read`.
fn l1_block_slot(
    state: &mut State<'_>,
    slot: U256,
    read: L1SlotRead<'_>,
) -> Result<U256, DatabaseError> {
    let value = state.storage_slot_untracked(&L1_BLOCK_CONTRACT, &slot)?;
    read(slot, value);
    Ok(value)
}

impl L1BlockInfo {
    /// Fetches the DA footprint gas scalar from the `L1Block` predeploy.
    pub fn fetch_da_footprint_gas_scalar(state: &mut State<'_>) -> Result<u16, DatabaseError> {
        Self::fetch_da_footprint_gas_scalar_with(state, &mut |_, _| {})
    }

    /// [`Self::fetch_da_footprint_gas_scalar`], handing `read` the slot it reads with its value:
    /// the read is made outside any transaction's journal, so a caller that reports what a
    /// transaction read records it here.
    pub fn fetch_da_footprint_gas_scalar_with(
        state: &mut State<'_>,
        read: L1SlotRead<'_>,
    ) -> Result<u16, DatabaseError> {
        let slot = l1_block_slot(state, DA_FOOTPRINT_GAS_SCALAR_SLOT, read)?.to_be_bytes::<32>();
        Ok(u16::from_be_bytes([
            slot[DA_FOOTPRINT_GAS_SCALAR_OFFSET],
            slot[DA_FOOTPRINT_GAS_SCALAR_OFFSET + 1],
        ]))
    }

    fn try_fetch_isthmus(
        &mut self,
        state: &mut State<'_>,
        read: L1SlotRead<'_>,
    ) -> Result<(), DatabaseError> {
        let scalars = l1_block_slot(state, OPERATOR_FEE_SCALARS_SLOT, read)?.to_be_bytes::<32>();
        self.operator_fee_scalar = Some(U256::from_be_slice(
            &scalars[OPERATOR_FEE_SCALAR_OFFSET..OPERATOR_FEE_SCALAR_OFFSET + 4],
        ));
        self.operator_fee_constant = Some(U256::from_be_slice(
            &scalars[OPERATOR_FEE_CONSTANT_OFFSET..OPERATOR_FEE_CONSTANT_OFFSET + 8],
        ));
        Ok(())
    }

    fn try_fetch_ecotone(
        &mut self,
        state: &mut State<'_>,
        read: L1SlotRead<'_>,
    ) -> Result<(), DatabaseError> {
        self.l1_blob_base_fee = Some(l1_block_slot(state, ECOTONE_L1_BLOB_BASE_FEE_SLOT, read)?);

        let scalars = l1_block_slot(state, ECOTONE_L1_FEE_SCALARS_SLOT, read)?.to_be_bytes::<32>();
        self.l1_base_fee_scalar =
            U256::from_be_slice(&scalars[BASE_FEE_SCALAR_OFFSET..BASE_FEE_SCALAR_OFFSET + 4]);
        let l1_blob_base_fee_scalar = U256::from_be_slice(
            &scalars[BLOB_BASE_FEE_SCALAR_OFFSET..BLOB_BASE_FEE_SCALAR_OFFSET + 4],
        );
        self.l1_blob_base_fee_scalar = Some(l1_blob_base_fee_scalar);

        // If the L1 fee scalars are empty the Bedrock cost function is used, which needs the
        // (otherwise deprecated) overhead.
        self.empty_ecotone_scalars = l1_blob_base_fee_scalar.is_zero()
            && scalars[BASE_FEE_SCALAR_OFFSET..BLOB_BASE_FEE_SCALAR_OFFSET + 4] == EMPTY_SCALARS;
        self.l1_fee_overhead = if self.empty_ecotone_scalars {
            Some(l1_block_slot(state, L1_OVERHEAD_SLOT, read)?)
        } else {
            None
        };
        Ok(())
    }

    /// Loads the L1 block info from the `L1Block` predeploy storage for `spec_id`.
    pub fn try_fetch(
        state: &mut State<'_>,
        l2_block: U256,
        spec_id: OpSpecId,
    ) -> Result<Self, DatabaseError> {
        Self::try_fetch_with(state, l2_block, spec_id, &mut |_, _| {})
    }

    /// [`Self::try_fetch`], handing `read` every slot it reads, in the order read, with its value:
    /// the base fee slot, then by `spec_id` the scalar and overhead slots (before Ecotone), or the
    /// blob base fee slot, the fee scalars slot, the overhead slot only when the scalars are
    /// empty, the operator fee scalars slot (Isthmus) and the footprint scalar slot (Jovian).
    /// The reads are made outside any transaction's journal, so a caller that reports what a
    /// transaction read records them here.
    pub fn try_fetch_with(
        state: &mut State<'_>,
        l2_block: U256,
        spec_id: OpSpecId,
        read: L1SlotRead<'_>,
    ) -> Result<Self, DatabaseError> {
        let mut out = Self {
            l2_block: Some(l2_block),
            l1_base_fee: l1_block_slot(state, L1_BASE_FEE_SLOT, read)?,
            ..Default::default()
        };

        if !spec_id.is_enabled_in(OpSpecId::ECOTONE) {
            out.l1_base_fee_scalar = l1_block_slot(state, L1_SCALAR_SLOT, read)?;
            out.l1_fee_overhead = Some(l1_block_slot(state, L1_OVERHEAD_SLOT, read)?);
            return Ok(out);
        }

        out.try_fetch_ecotone(state, read)?;

        if spec_id.is_enabled_in(OpSpecId::ISTHMUS) {
            out.try_fetch_isthmus(state, read)?;
        }

        if spec_id.is_enabled_in(OpSpecId::JOVIAN) {
            out.da_footprint_gas_scalar =
                Some(Self::fetch_da_footprint_gas_scalar_with(state, read)?);
        }

        Ok(out)
    }

    /// Calculates the operator fee charged upfront for a transaction with `gas_limit`.
    ///
    /// Zero for deposits and empty inputs. Introduced in Isthmus.
    pub fn operator_fee_charge(&self, input: &[u8], gas_limit: U256, spec_id: OpSpecId) -> U256 {
        if input.is_empty() || input.first() == Some(&DEPOSIT_TRANSACTION_TYPE) {
            return U256::ZERO;
        }
        self.operator_fee_charge_inner(gas_limit, spec_id)
    }

    /// Calculates the operator fee for `gas`.
    ///
    /// op-revm panics when the Isthmus operator fee parameters are missing; this port treats a
    /// missing parameter as zero.
    fn operator_fee_charge_inner(&self, gas: U256, spec_id: OpSpecId) -> U256 {
        let scalar = self.operator_fee_scalar.unwrap_or_default();
        let constant = self.operator_fee_constant.unwrap_or_default();

        let product = if spec_id.is_enabled_in(OpSpecId::JOVIAN) {
            gas.saturating_mul(scalar).saturating_mul(U256::from(OPERATOR_FEE_JOVIAN_MULTIPLIER))
        } else {
            gas.saturating_mul(scalar) / U256::from(OPERATOR_FEE_SCALAR_DECIMAL)
        };

        product.saturating_add(constant)
    }

    /// Calculates the operator fee refunded to the caller after execution.
    ///
    /// `gas_used` is the final transaction gas used (after refunds and the calldata floor, without
    /// the unused EIP-8037 reservoir), which is what op-revm derives from its `Gas` as
    /// `limit - (remaining + reservoir + refunded)`. Zero before Isthmus.
    pub fn operator_fee_refund(&self, gas_limit: u64, gas_used: u64, spec_id: OpSpecId) -> U256 {
        if !spec_id.is_enabled_in(OpSpecId::ISTHMUS) {
            return U256::ZERO;
        }
        let charged = self.operator_fee_charge_inner(U256::from(gas_limit), spec_id);
        let used = self.operator_fee_charge_inner(U256::from(gas_used), spec_id);
        charged.saturating_sub(used)
    }

    /// Calculates the data gas for posting the transaction on L1.
    ///
    /// Since Fjord this is the FastLZ size estimate times 16. Before Fjord calldata costs 16 gas
    /// per non-zero byte and 4 gas per zero byte, and before Regolith an extra 68 non-zero bytes
    /// were included to account for the signature.
    pub fn data_gas(&self, input: &[u8], spec_id: OpSpecId) -> U256 {
        if spec_id.is_enabled_in(OpSpecId::FJORD) {
            let estimated_size = U256::from(estimate_tx_compressed_size(input));
            return estimated_size
                .saturating_mul(U256::from(NON_ZERO_BYTE_COST))
                .wrapping_div(U256::from(1_000_000));
        }

        let zero_bytes = input.iter().filter(|b| **b == 0).count() as u64;
        let mut tokens = zero_bytes + (input.len() as u64 - zero_bytes) * 4;

        if !spec_id.is_enabled_in(OpSpecId::REGOLITH) {
            tokens += 68 * NON_ZERO_BYTE_MULTIPLIER_ISTANBUL;
        }

        U256::from(tokens.saturating_mul(STANDARD_TOKEN_COST))
    }

    /// Returns the Jovian DA footprint of a transaction:
    /// `estimated_compressed_size / 1e6 * da_footprint_gas_scalar`.
    ///
    /// Port of alloy-op-evm's block-level `jovian_da_footprint_estimation`. Enforcing the block DA
    /// footprint limit is a block-executor concern and is not done by the EVM handlers.
    pub fn tx_da_footprint(enveloped_tx: &[u8], da_footprint_gas_scalar: u16) -> u64 {
        (estimate_tx_compressed_size(enveloped_tx) / 1_000_000)
            .saturating_mul(u64::from(da_footprint_gas_scalar))
    }

    /// Clears the cached L1 cost of the current transaction.
    pub const fn clear_tx_l1_cost(&mut self) {
        self.tx_l1_cost = None;
    }

    /// Calculates the total additional cost (L1 data fee plus operator fee) charged upfront for a
    /// transaction with `enveloped_tx` and `gas_limit`.
    pub fn tx_cost(&mut self, enveloped_tx: &[u8], gas_limit: U256, spec: OpSpecId) -> U256 {
        let mut additional_cost = self.calculate_tx_l1_cost(enveloped_tx, spec);
        if spec.is_enabled_in(OpSpecId::ISTHMUS) {
            let operator_fee_charge = self.operator_fee_charge(enveloped_tx, gas_limit, spec);
            additional_cost = additional_cost.saturating_add(operator_fee_charge);
        }
        additional_cost
    }

    /// Calculates the L1 data fee of a transaction, caching the result in
    /// [`Self::tx_l1_cost`].
    pub fn calculate_tx_l1_cost(&mut self, input: &[u8], spec_id: OpSpecId) -> U256 {
        if let Some(tx_l1_cost) = self.tx_l1_cost {
            return tx_l1_cost;
        }
        // Deposits and empty inputs cost nothing. Like op-revm, this is not cached.
        if input.is_empty() || input.first() == Some(&DEPOSIT_TRANSACTION_TYPE) {
            return U256::ZERO;
        }
        let tx_l1_cost = if spec_id.is_enabled_in(OpSpecId::FJORD) {
            self.calculate_tx_l1_cost_fjord(input)
        } else if spec_id.is_enabled_in(OpSpecId::ECOTONE) {
            self.calculate_tx_l1_cost_ecotone(input, spec_id)
        } else {
            self.calculate_tx_l1_cost_bedrock(input, spec_id)
        };

        self.tx_l1_cost = Some(tx_l1_cost);
        tx_l1_cost
    }

    /// Pre-Ecotone L1 cost function.
    fn calculate_tx_l1_cost_bedrock(&self, input: &[u8], spec_id: OpSpecId) -> U256 {
        self.data_gas(input, spec_id)
            .saturating_add(self.l1_fee_overhead.unwrap_or_default())
            .saturating_mul(self.l1_base_fee)
            .saturating_mul(self.l1_base_fee_scalar)
            .wrapping_div(U256::from(1_000_000))
    }

    /// Ecotone L1 cost function:
    /// `calldataGas*(l1BaseFee*16*l1BaseFeeScalar + l1BlobBaseFee*l1BlobBaseFeeScalar)/16e6`.
    fn calculate_tx_l1_cost_ecotone(&self, input: &[u8], spec_id: OpSpecId) -> U256 {
        // The first Ecotone block (unless activated at genesis) still uses the Bedrock function.
        if self.empty_ecotone_scalars {
            return self.calculate_tx_l1_cost_bedrock(input, spec_id);
        }

        let rollup_data_gas_cost = self.data_gas(input, spec_id);
        self.calculate_l1_fee_scaled_ecotone()
            .saturating_mul(rollup_data_gas_cost)
            .wrapping_div(U256::from(1_000_000 * NON_ZERO_BYTE_COST))
    }

    /// Fjord L1 cost function:
    /// `estimatedSize*(baseFeeScalar*l1BaseFee*16 + blobFeeScalar*l1BlobBaseFee)/1e12`.
    fn calculate_tx_l1_cost_fjord(&self, input: &[u8]) -> U256 {
        let l1_fee_scaled = self.calculate_l1_fee_scaled_ecotone();
        if l1_fee_scaled.is_zero() {
            return U256::ZERO;
        }
        let estimated_size = U256::from(estimate_tx_compressed_size(input));
        estimated_size.saturating_mul(l1_fee_scaled).wrapping_div(U256::from(1_000_000_000_000u64))
    }

    /// `l1BaseFee*16*l1BaseFeeScalar + l1BlobBaseFee*l1BlobBaseFeeScalar`.
    fn calculate_l1_fee_scaled_ecotone(&self) -> U256 {
        let calldata_cost_per_byte = self
            .l1_base_fee
            .saturating_mul(U256::from(NON_ZERO_BYTE_COST))
            .saturating_mul(self.l1_base_fee_scalar);
        let blob_cost_per_byte = self
            .l1_blob_base_fee
            .unwrap_or_default()
            .saturating_mul(self.l1_blob_base_fee_scalar.unwrap_or_default());
        calldata_cost_per_byte.saturating_add(blob_cost_per_byte)
    }
}

#[cfg(test)]
mod tests {
    //! Vectors from op-revm's `l1block.rs` tests.

    use super::*;
    use crate::test_vectors::{ECOTONE_MAINNET_TX, FJORD_MAINNET_TX, FJORD_SAMPLE_TX};
    use alloy_primitives::hex;

    fn bedrock_info() -> L1BlockInfo {
        L1BlockInfo {
            l1_base_fee: U256::from(1_000),
            l1_fee_overhead: Some(U256::from(1_000)),
            l1_base_fee_scalar: U256::from(1_000),
            ..Default::default()
        }
    }

    #[test]
    fn data_gas_per_era() {
        let info = L1BlockInfo {
            l1_base_fee: U256::from(1_000_000),
            l1_fee_overhead: Some(U256::from(1_000_000)),
            l1_base_fee_scalar: U256::from(1_000_000),
            ..Default::default()
        };
        let input = hex!("FACADE");
        assert_eq!(info.data_gas(&input, OpSpecId::BEDROCK), U256::from(1136));
        assert_eq!(info.data_gas(&input, OpSpecId::REGOLITH), U256::from(48));
        assert_eq!(info.data_gas(&input, OpSpecId::FJORD), U256::from(1600));

        let input = hex!("FA00CA00DE");
        assert_eq!(info.data_gas(&input, OpSpecId::BEDROCK), U256::from(1144));
        assert_eq!(info.data_gas(&input, OpSpecId::REGOLITH), U256::from(56));
        assert_eq!(info.data_gas(&input, OpSpecId::FJORD), U256::from(1600));
    }

    #[test]
    fn l1_cost_regolith() {
        let mut info = bedrock_info();
        assert_eq!(
            info.calculate_tx_l1_cost(&hex!("FACADE"), OpSpecId::REGOLITH),
            U256::from(1048)
        );
        info.clear_tx_l1_cost();
        assert_eq!(info.calculate_tx_l1_cost(&[], OpSpecId::REGOLITH), U256::ZERO);
        info.clear_tx_l1_cost();
        assert_eq!(info.calculate_tx_l1_cost(&hex!("7EFACADE"), OpSpecId::REGOLITH), U256::ZERO);
    }

    #[test]
    fn l1_cost_is_cached_until_cleared() {
        let mut info = bedrock_info();
        assert_eq!(
            info.calculate_tx_l1_cost(&hex!("FACADE"), OpSpecId::REGOLITH),
            U256::from(1048)
        );
        // A different input returns the cached value until the cache is cleared.
        assert_eq!(
            info.calculate_tx_l1_cost(&hex!("FA00CA00DE"), OpSpecId::REGOLITH),
            U256::from(1048)
        );
        info.clear_tx_l1_cost();
        assert_ne!(
            info.calculate_tx_l1_cost(&hex!("FA00CA00DE"), OpSpecId::REGOLITH),
            U256::from(1048)
        );
    }

    #[test]
    fn l1_cost_ecotone() {
        let mut info = L1BlockInfo {
            l1_blob_base_fee: Some(U256::from(1_000)),
            l1_blob_base_fee_scalar: Some(U256::from(1_000)),
            ..bedrock_info()
        };
        assert_eq!(info.calculate_tx_l1_cost(&hex!("FACADE"), OpSpecId::ECOTONE), U256::from(51));
        info.clear_tx_l1_cost();
        assert_eq!(info.calculate_tx_l1_cost(&[], OpSpecId::ECOTONE), U256::ZERO);
        info.clear_tx_l1_cost();
        assert_eq!(info.calculate_tx_l1_cost(&hex!("7EFACADE"), OpSpecId::ECOTONE), U256::ZERO);
        info.clear_tx_l1_cost();
        info.empty_ecotone_scalars = true;
        assert_eq!(info.calculate_tx_l1_cost(&hex!("FACADE"), OpSpecId::ECOTONE), U256::from(1048));
    }

    #[test]
    fn l1_cost_ecotone_op_mainnet_block_118024092() {
        let info = L1BlockInfo {
            l1_base_fee: U256::from(47036678951u64),
            l1_base_fee_scalar: U256::from(1368),
            l1_blob_base_fee: Some(U256::from(57422457042u64)),
            l1_blob_base_fee_scalar: Some(U256::from(810949)),
            ..Default::default()
        };
        assert_eq!(info.data_gas(&ECOTONE_MAINNET_TX, OpSpecId::ECOTONE), U256::from(2456));
        assert_eq!(
            info.calculate_tx_l1_cost_ecotone(&ECOTONE_MAINNET_TX, OpSpecId::ECOTONE),
            U256::from(7306020222001u64)
        );
    }

    #[test]
    fn l1_cost_fjord() {
        let mut info = L1BlockInfo {
            l1_base_fee: U256::from(1_000),
            l1_base_fee_scalar: U256::from(1_000),
            l1_blob_base_fee: Some(U256::from(1_000)),
            l1_blob_base_fee_scalar: Some(U256::from(1_000)),
            ..Default::default()
        };
        assert_eq!(info.calculate_tx_l1_cost(&hex!("FACADE"), OpSpecId::FJORD), U256::from(1700));
        info.clear_tx_l1_cost();
        assert_eq!(info.calculate_tx_l1_cost(&FJORD_SAMPLE_TX, OpSpecId::FJORD), U256::from(2148));
        info.clear_tx_l1_cost();
        assert_eq!(info.calculate_tx_l1_cost(&[], OpSpecId::FJORD), U256::ZERO);
        info.clear_tx_l1_cost();
        assert_eq!(info.calculate_tx_l1_cost(&hex!("7EFACADE"), OpSpecId::FJORD), U256::ZERO);
    }

    #[test]
    fn l1_cost_fjord_op_mainnet_block_124665056() {
        let info = L1BlockInfo {
            l1_base_fee: U256::from(1055991687),
            l1_base_fee_scalar: U256::from(5227),
            l1_blob_base_fee_scalar: Some(U256::from(1014213)),
            l1_blob_base_fee: Some(U256::from(1)),
            ..Default::default()
        };
        assert_eq!(info.data_gas(&FJORD_MAINNET_TX, OpSpecId::FJORD), U256::from(4471));
        assert_eq!(info.calculate_tx_l1_cost_fjord(&FJORD_MAINNET_TX), U256::from(0x5bf1ab43du64));
    }

    #[test]
    fn da_footprint() {
        // FastLZ size 4 is below the 100-byte minimum; the sample call compresses to 202 bytes:
        // (836500 * 202 - 42585600) / 1e6 = 126.
        assert_eq!(L1BlockInfo::tx_da_footprint(&hex!("FACADE"), 7), 700);
        assert_eq!(L1BlockInfo::tx_da_footprint(&FJORD_SAMPLE_TX, 400), 126 * 400);
    }

    #[test]
    fn operator_fee_formulas() {
        let info = L1BlockInfo {
            operator_fee_scalar: Some(U256::from(1_000u64)),
            operator_fee_constant: Some(U256::from(10u64)),
            ..Default::default()
        };
        let input = [0x01u8];
        assert_eq!(
            info.operator_fee_charge(&input, U256::from(1_000u64), OpSpecId::ISTHMUS),
            U256::from(11u64)
        );
        assert_eq!(
            info.operator_fee_charge(&input, U256::from(1_000u64), OpSpecId::JOVIAN),
            U256::from(100_000_010u64)
        );
        assert_eq!(
            info.operator_fee_charge(&[], U256::from(1_000u64), OpSpecId::JOVIAN),
            U256::ZERO
        );
        assert_eq!(
            info.operator_fee_charge(&[0x7E], U256::from(1_000u64), OpSpecId::JOVIAN),
            U256::ZERO
        );
    }

    #[test]
    fn operator_fee_refund_isthmus() {
        // op-revm `test_operator_fee_refund` (l1block.rs): `Gas::new(50000)` with nothing spent.
        let info = L1BlockInfo {
            l1_base_fee: U256::from(1055991687),
            l1_base_fee_scalar: U256::from(5227),
            operator_fee_scalar: Some(U256::from(2000)),
            operator_fee_constant: Some(U256::from(5)),
            ..Default::default()
        };
        assert_eq!(info.operator_fee_refund(50_000, 0, OpSpecId::ISTHMUS), U256::from(100));
        assert_eq!(info.operator_fee_refund(50_000, 0, OpSpecId::HOLOCENE), U256::ZERO);
    }
}

#[cfg(test)]
mod read_report_tests {
    //! What [`L1BlockInfo::try_fetch_with`] and [`L1BlockInfo::fetch_da_footprint_gas_scalar_with`]
    //! report: every slot they read, in the order read, with its value, and nothing else.

    use super::*;
    use alloc::vec::Vec;
    use evm2::evm::{AccountInfo, InMemoryDB};

    /// An `L1Block` predeploy whose slot `n` holds `n * 10`, its fee scalars slot (3) `scalars`.
    fn l1_block(scalars: U256) -> InMemoryDB {
        let mut db = InMemoryDB::default();
        db.insert_account_info(&L1_BLOCK_CONTRACT, AccountInfo::default().with_nonce(1));
        for slot in 1..=8u64 {
            db.insert_account_storage(
                &L1_BLOCK_CONTRACT,
                &U256::from(slot),
                &U256::from(slot * 10),
            );
        }
        db.insert_account_storage(&L1_BLOCK_CONTRACT, &ECOTONE_L1_FEE_SCALARS_SLOT, &scalars);
        db
    }

    fn reported(db: InMemoryDB, spec: OpSpecId) -> (L1BlockInfo, Vec<(u64, U256)>) {
        let mut state = State::new(db);
        let mut reads = Vec::new();
        let info = L1BlockInfo::try_fetch_with(&mut state, U256::from(1), spec, &mut |slot, v| {
            reads.push((slot.to::<u64>(), v));
        })
        .unwrap();
        (info, reads)
    }

    #[test]
    fn witness_l1_fee_fetch_reports_every_slot_it_reads() {
        // A blob base fee scalar of 1 (bytes 20 to 23): the scalars are not empty.
        let scalars = U256::from(1) << 64;
        // Jovian and later: the base fee, the blob base fee, the scalars, then the operator fee
        // scalars and the footprint scalar, both of slot 8; no overhead.
        for spec in [OpSpecId::JOVIAN, OpSpecId::KARST] {
            let (info, reads) = reported(l1_block(scalars), spec);
            assert!(!info.empty_ecotone_scalars);
            let ten = |n: u64| U256::from(n * 10);
            assert_eq!(reads, [(1, ten(1)), (7, ten(7)), (3, scalars), (8, ten(8)), (8, ten(8))]);
        }
        // Empty scalars: the Bedrock fallback's overhead, slot 5, is read too.
        let (info, reads) = reported(l1_block(U256::ZERO), OpSpecId::KARST);
        assert!(info.empty_ecotone_scalars);
        let slots: Vec<u64> = reads.iter().map(|(slot, _)| *slot).collect();
        assert_eq!(slots, [1, 7, 3, 5, 8, 8]);
        // Isthmus: no footprint scalar; Ecotone: no operator fee scalars; before Ecotone: the
        // scalar and the overhead.
        let slots =
            |spec| reported(l1_block(scalars), spec).1.iter().map(|r| r.0).collect::<Vec<_>>();
        assert_eq!(slots(OpSpecId::ISTHMUS), [1, 7, 3, 8]);
        assert_eq!(slots(OpSpecId::ECOTONE), [1, 7, 3]);
        assert_eq!(slots(OpSpecId::REGOLITH), [1, 6, 5]);
        // The unreported fetch reads the same values.
        let mut state = State::new(l1_block(scalars));
        let plain = L1BlockInfo::try_fetch(&mut state, U256::from(1), OpSpecId::KARST).unwrap();
        assert_eq!(plain, reported(l1_block(scalars), OpSpecId::KARST).0);
    }

    #[test]
    fn witness_footprint_fetch_reports_slot_8() {
        let mut state = State::new(l1_block(U256::ZERO));
        let mut reads = Vec::new();
        let scalar =
            L1BlockInfo::fetch_da_footprint_gas_scalar_with(&mut state, &mut |slot, value| {
                reads.push((slot, value));
            })
            .unwrap();
        assert_eq!(reads, [(DA_FOOTPRINT_GAS_SCALAR_SLOT, U256::from(80))]);
        // 80 holds no footprint scalar in its bytes 12 and 13.
        assert_eq!(scalar, 0);
        assert_eq!(L1BlockInfo::fetch_da_footprint_gas_scalar(&mut state).unwrap(), scalar);
    }
}
