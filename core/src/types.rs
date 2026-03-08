//! Core types - placeholder until Part 2 implementation.
//! Block, Transaction, and chain types will be implemented in the core data structures phase.

use serde::{Deserialize, Serialize};

/// Chain constants for Quyn.
pub const BLOCK_TIME_SECS: u64 = 3;
/// Target max TPS - used for block gas limit and mempool sizing.
pub const MAX_TPS: u64 = 50_000;
/// 1 billion QYN, 18 decimals.
pub const TOTAL_SUPPLY: u128 = 1_000_000_000 * 10_u128.pow(18);
/// Mainnet chain ID.
pub const CHAIN_ID_MAINNET: u64 = 7777;
/// Testnet chain ID.
pub const CHAIN_ID_TESTNET: u64 = 7779;

/// Placeholder block identifier (will be replaced with full Block type).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockId(pub [u8; 32]);

/// Placeholder transaction identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TxId(pub [u8; 32]);
