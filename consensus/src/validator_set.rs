//! Validator set: registration, stake, and active set.

use alloy_primitives::{Address, U256};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Minimum stake to become a validator (10,000 QYN with 18 decimals).
pub const MIN_STAKE: u128 = 10_000 * 10_u128.pow(18);
/// Maximum number of active validators.
pub const MAX_VALIDATORS: usize = 1000;
/// Epoch length in blocks.
pub const EPOCH_LENGTH: u64 = 32;

/// Single validator info.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidatorInfo {
    pub address: Address,
    /// Self-stake.
    pub stake: U256,
    /// Total delegated to this validator.
    pub delegated: U256,
    /// Commission rate in basis points (0-10000).
    pub commission_bps: u16,
    pub active: bool,
}

impl ValidatorInfo {
    pub fn total_stake(&self) -> U256 {
        self.stake.saturating_add(self.delegated)
    }
}

/// Delegation from a delegator to a validator.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Delegation {
    pub delegator: Address,
    pub validator: Address,
    pub amount: U256,
}

/// In-memory validator set (persisted to chain via put_validator_set_bytes).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ValidatorSet {
    validators: BTreeMap<Address, ValidatorInfo>,
    delegations: Vec<Delegation>,
}

impl ValidatorSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, address: Address, stake: U256, commission_bps: u16) -> Result<(), crate::error::ConsensusError> {
        if stake.to::<u128>() < MIN_STAKE {
            return Err(crate::error::ConsensusError::Staking(format!(
                "stake {} below minimum {}",
                stake, MIN_STAKE
            )));
        }
        if self.validators.len() >= MAX_VALIDATORS && !self.validators.contains_key(&address) {
            return Err(crate::error::ConsensusError::Staking("validator set full".into()));
        }
        let delegated = self
            .delegations
            .iter()
            .filter(|d| d.validator == address)
            .map(|d| d.amount)
            .fold(U256::ZERO, |a, b| a.saturating_add(b));
        self.validators.insert(
            address,
            ValidatorInfo {
                address,
                stake,
                delegated,
                commission_bps,
                active: true,
            },
        );
        Ok(())
    }

    pub fn delegate(&mut self, delegator: Address, validator: Address, amount: U256) -> Result<(), crate::error::ConsensusError> {
        if amount.is_zero() {
            return Ok(());
        }
        let v = self
            .validators
            .get_mut(&validator)
            .ok_or_else(|| crate::error::ConsensusError::Staking("validator not found".into()))?;
        if !v.active {
            return Err(crate::error::ConsensusError::Staking("validator inactive".into()));
        }
        v.delegated = v.delegated.saturating_add(amount);
        self.delegations.push(Delegation {
            delegator,
            validator,
            amount,
        });
        Ok(())
    }

    pub fn get_validator(&self, address: &Address) -> Option<&ValidatorInfo> {
        self.validators.get(address)
    }

    pub fn active_validators(&self) -> Vec<ValidatorInfo> {
        self.validators
            .values()
            .filter(|v| v.active && v.total_stake().to::<u128>() >= MIN_STAKE)
            .cloned()
            .collect()
    }

    /// Slash validator: reduce stake and optionally deactivate.
    pub fn slash(&mut self, address: &Address, amount: U256, deactivate: bool) {
        if let Some(v) = self.validators.get_mut(address) {
            v.stake = v.stake.saturating_sub(amount);
            if deactivate {
                v.active = false;
            }
        }
    }
}

/// Select block proposer for a given slot (block number) deterministically.
/// Uses epoch and VRF-like hash(epoch_seed || block_number) mod total_stake.
pub fn select_proposer(validators: &[ValidatorInfo], block_number: u64, parent_block_hash: &[u8; 32]) -> Option<Address> {
    if validators.is_empty() {
        return None;
    }
    let epoch = block_number / EPOCH_LENGTH;
    let mut hasher = Sha256::new();
    hasher.update(epoch.to_be_bytes());
    hasher.update(parent_block_hash);
    hasher.update(block_number.to_be_bytes());
    let seed = hasher.finalize();
    let total_stake: u128 = validators.iter().map(|v| v.total_stake().to::<u128>()).sum();
    if total_stake == 0 {
        return Some(validators[0].address);
    }
    let mut seed_arr = [0u8; 8];
    seed_arr.copy_from_slice(&seed[0..8]);
    let mut idx = u64::from_be_bytes(seed_arr) as u128 % total_stake;
    for v in validators {
        let s = v.total_stake().to::<u128>();
        if idx < s {
            return Some(v.address);
        }
        idx -= s;
    }
    Some(validators.last()?.address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validator_set_register_and_select() {
        let mut set = ValidatorSet::new();
        let addr1 = Address::from_slice(&[1u8; 20]);
        set.register(addr1, U256::from(MIN_STAKE), 500).unwrap();
        let active = set.active_validators();
        assert_eq!(active.len(), 1);
        let hash = [0u8; 32];
        let proposer = select_proposer(&active, 1, &hash);
        assert_eq!(proposer, Some(addr1));
    }
}
