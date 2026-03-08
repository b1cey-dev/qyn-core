//! Slashing: conditions and evidence for bad validators.

use alloy_primitives::Address;
use serde::{Deserialize, Serialize};

/// Slashing reason.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SlashReason {
    DoubleSign,
    InvalidBlock,
    Liveness,
}

/// Evidence of misbehavior (e.g. two signed blocks at same height).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SlashEvidence {
    pub validator: Address,
    pub reason: SlashReason,
    pub block_number: u64,
    pub payload: Vec<u8>,
}

/// Slashing penalty: fraction of stake to slash (basis points). 10000 = 100%.
pub fn slash_penalty_bps(reason: &SlashReason) -> u16 {
    match reason {
        SlashReason::DoubleSign => 10000,
        SlashReason::InvalidBlock => 5000,
        SlashReason::Liveness => 1000,
    }
}
