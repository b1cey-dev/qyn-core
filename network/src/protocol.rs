//! Quyn P2P protocol types for block and transaction propagation.

use serde::{Deserialize, Serialize};

/// Protocol name for block sync.
pub const PROTOCOL_BLOCKS: &str = "/quyn/block/1";
/// Protocol name for transaction gossip.
pub const PROTOCOL_TX: &str = "/quyn/tx/1";

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
