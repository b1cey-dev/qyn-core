//! Quyn P2P protocol types for block and transaction propagation.
//!
//! Message size limits and rate limiting are enforced to prevent DoS.

use serde::{Deserialize, Serialize};

/// Protocol name for block sync.
pub const PROTOCOL_BLOCKS: &str = "/quyn/block/1";
/// Protocol name for transaction gossip.
pub const PROTOCOL_TX: &str = "/quyn/tx/1";

/// Max block size (2MB). Reject blocks larger than this.
pub const MAX_BLOCK_SIZE: usize = 2 * 1024 * 1024;
/// Max transaction size (128KB). Reject txs larger than this.
pub const MAX_TX_SIZE: usize = 128 * 1024;
/// Max peer message size (4MB). Reject messages larger than this.
pub const MAX_PEER_MESSAGE_SIZE: usize = 4 * 1024 * 1024;
/// Max messages per peer per second. Ban if exceeded.
pub const MAX_MESSAGES_PER_PEER_PER_SEC: u32 = 100;
/// Ban duration in seconds when rate limit exceeded.
pub const PEER_BAN_DURATION_SECS: u64 = 3600;
/// Initial peer reputation score.
pub const INITIAL_PEER_REPUTATION: i32 = 100;
/// Max connections per IP for Sybil resistance.
pub const MAX_CONNECTIONS_PER_IP: usize = 3;

/// Request: get blocks by range.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockRequest {
    pub from: u64,
    pub count: u64,
}

/// Response: serialized block payloads.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockResponse {
    pub blocks: Vec<Vec<u8>>,
}

/// Transaction gossip message.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TxMessage {
    pub raw: Vec<u8>,
}
