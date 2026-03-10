//! Shared types for fraud analysis.

use serde::{Deserialize, Serialize};

/// Configuration for the fraud detection system.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FraudConfig {
    /// Risk score threshold: 0-30 = low
    pub low_threshold: u8,
    /// 31-60 = medium
    pub medium_threshold: u8,
    /// 61-85 = high
    pub high_threshold: u8,
    /// 86-100 = critical
    pub critical_threshold: u8,

    /// Max transactions per minute (velocity)
    pub max_tx_per_minute: u32,
    /// Max transactions per hour
    pub max_tx_per_hour: u32,
    /// Max value per hour (wei)
    pub max_value_per_hour: u128,

    /// Min blocks since first tx to consider wallet "aged"
    pub min_wallet_age_blocks: u64,
    /// Value (wei) above which new wallet tx is "large"
    pub new_wallet_large_tx: u128,
}

impl Default for FraudConfig {
    fn default() -> Self {
        Self {
            low_threshold: 30,
            medium_threshold: 60,
            high_threshold: 85,
            critical_threshold: 86,
            max_tx_per_minute: 10,
            max_tx_per_hour: 50,
            max_value_per_hour: u128::MAX, // not used in Phase 1
            min_wallet_age_blocks: 10,
            new_wallet_large_tx: 1000 * 10_u128.pow(18), // 1000 QYN
        }
    }
}
