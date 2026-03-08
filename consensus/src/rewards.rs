//! Reward distribution: 5% APY for validators, per-block rewards.

use alloy_primitives::{Address, U256};
use crate::validator_set::ValidatorInfo;

/// Approximate blocks per year at 3s block time.
const BLOCKS_PER_YEAR: u64 = 365 * 24 * 60 * 60 / 3;
/// 5% APY in basis points.
const APY_BPS: u64 = 500;
/// Reward pool from genesis (10% of supply = 100M QYN).
const REWARD_POOL: u128 = 100_000_000 * 10_u128.pow(18);

/// Per-block reward: (reward_pool * APY_BPS / 10000) / blocks_per_year.
pub fn block_reward_amount() -> U256 {
    let yearly = (REWARD_POOL as u64)
        .saturating_mul(APY_BPS)
        .checked_div(10_000)
        .unwrap_or(0);
    U256::from(yearly) / U256::from(BLOCKS_PER_YEAR)
}

/// Distribute block reward: proposer gets full reward (commission/delegation split can be applied by caller).
pub fn distribute_block_reward(proposer: Address, _validator: Option<&ValidatorInfo>, reward: U256) -> Vec<(Address, U256)> {
    vec![(proposer, reward)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_reward_non_zero() {
        let r = block_reward_amount();
        assert!(!r.is_zero());
    }
}
