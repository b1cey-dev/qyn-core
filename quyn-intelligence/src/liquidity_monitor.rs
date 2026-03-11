//! Liquidity monitoring for the Anti Rug Pull System.
//! Tracks liquidity locks and drain attempts per contract.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::rug_pull_detector::{LiquidityLock, RugPullConfig};

/// Per-contract liquidity state (for monitoring drain attempts).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LiquidityState {
    pub total_liquidity: u128,
    pub last_known_block: u64,
}

/// Monitors liquidity per contract and detects drain attempts.
pub struct LiquidityMonitor {
    pub config: RugPullConfig,
    /// contract -> liquidity state
    pub liquidity: HashMap<[u8; 20], LiquidityState>,
    /// contract -> lock (mirrors RugPullDetector for querying)
    pub locks: HashMap<[u8; 20], LiquidityLock>,
}

impl LiquidityMonitor {
    pub fn new(config: RugPullConfig) -> Self {
        Self {
            config,
            liquidity: HashMap::new(),
            locks: HashMap::new(),
        }
    }

    pub fn set_liquidity(&mut self, contract: [u8; 20], total_liquidity: u128, block: u64) {
        self.liquidity.insert(
            contract,
            LiquidityState {
                total_liquidity,
                last_known_block: block,
            },
        );
    }

    pub fn register_lock(&mut self, lock: LiquidityLock) {
        self.locks.insert(lock.contract, lock);
    }

    /// Returns true if a drain of `amount` would exceed 50% of current liquidity (drain attempt).
    pub fn would_drain_more_than_half(
        &self,
        contract: &[u8; 20],
        amount: u128,
    ) -> bool {
        let Some(state) = self.liquidity.get(contract) else {
            return false;
        };
        if state.total_liquidity == 0 {
            return false;
        }
        amount * 100 >= state.total_liquidity * 50
    }

    pub fn get_lock(&self, contract: &[u8; 20]) -> Option<&LiquidityLock> {
        self.locks.get(contract)
    }

    pub fn is_locked(&self, contract: &[u8; 20], current_block: u64) -> bool {
        self.locks.get(contract).map_or(false, |l| {
            l.is_active && current_block < l.lock_expiry_block
        })
    }
}
