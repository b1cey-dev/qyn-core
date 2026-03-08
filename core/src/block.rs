//! Block structure for Quyn blockchain.
//!
//! Blocks contain a header (hashes, timestamp, validator, signature) and a body (transactions).

use crate::error::CoreError;
use crate::transaction::SignedTransaction;
use alloy_primitives::{Address, B256, U256};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::SystemTime;

/// Block header - committed to in the chain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockHeader {
    /// Parent block hash.
    pub parent_hash: B256,
    /// State root after applying all transactions.
    pub state_root: B256,
    /// Root of the transaction trie.
    pub transactions_root: B256,
    /// Root of the receipts trie.
    pub receipts_root: B256,
    /// Block timestamp (Unix seconds).
    pub timestamp: u64,
    /// Block number (height).
    pub number: u64,
    /// Validator that produced this block.
    pub validator: Address,
    /// Validator's signature over the block (e.g. over header hash).
    pub signature: Vec<u8>,
    /// Extra data (optional).
    pub extra_data: Vec<u8>,
    /// Gas limit for this block.
    pub gas_limit: u64,
    /// Base fee per gas (for EIP-1559 style; can be 0 for legacy).
    pub base_fee_per_gas: U256,
}

impl BlockHeader {
    /// Compute the hash of the header (used as block hash).
    pub fn hash(&self) -> B256 {
        let mut hasher = Sha256::new();
        hasher.update(self.parent_hash.as_slice());
        hasher.update(self.state_root.as_slice());
        hasher.update(self.transactions_root.as_slice());
        hasher.update(self.receipts_root.as_slice());
        hasher.update(self.timestamp.to_be_bytes());
        hasher.update(self.number.to_be_bytes());
        hasher.update(self.validator.as_slice());
        hasher.update(&self.signature);
        hasher.update(&self.extra_data);
        hasher.update(self.gas_limit.to_be_bytes());
        hasher.update(self.base_fee_per_gas.to_be_bytes::<32>().as_slice());
        B256::from_slice(&hasher.finalize()[..])
    }

    /// Serialize for signing (canonical form without signature).
    pub fn signing_hash(&self) -> B256 {
        let mut h = self.clone();
        h.signature = vec![];
        h.hash()
    }
}

/// Block body - list of transactions.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockBody {
    pub transactions: Vec<SignedTransaction>,
}

impl BlockBody {
    /// Compute Merkle root of transactions (simplified: hash of concatenated tx hashes).
    pub fn transactions_root(&self) -> B256 {
        if self.transactions.is_empty() {
            return B256::ZERO;
        }
        let mut hasher = Sha256::new();
        for tx in &self.transactions {
            hasher.update(tx.hash().as_slice());
        }
        B256::from_slice(&hasher.finalize()[..])
    }
}

/// Full block: header + body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub body: BlockBody,
}

impl Block {
    /// Block hash (same as header hash).
    pub fn hash(&self) -> B256 {
        self.header.hash()
    }

    /// Validate header vs body (transactions_root match).
    pub fn validate_self(&self) -> Result<(), CoreError> {
        let computed = self.body.transactions_root();
        if computed != self.header.transactions_root {
            return Err(CoreError::InvalidBlock(format!(
                "transactions_root mismatch: got {:?}, expected {:?}",
                computed, self.header.transactions_root
            )));
        }
        Ok(())
    }

    /// Create a block (for block production).
    pub fn new(
        parent_hash: B256,
        number: u64,
        state_root: B256,
        receipts_root: B256,
        transactions: Vec<SignedTransaction>,
        validator: Address,
        signature: Vec<u8>,
        gas_limit: u64,
        base_fee_per_gas: U256,
    ) -> Result<Self, CoreError> {
        let body = BlockBody { transactions };
        let transactions_root = body.transactions_root();
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|e| CoreError::InvalidBlock(e.to_string()))?
            .as_secs();
        let header = BlockHeader {
            parent_hash,
            state_root,
            transactions_root,
            receipts_root,
            timestamp,
            number,
            validator,
            signature,
            extra_data: vec![],
            gas_limit,
            base_fee_per_gas,
        };
        let block = Block { header, body };
        block.validate_self()?;
        Ok(block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_header_hash_is_deterministic() {
        let h = BlockHeader {
            parent_hash: B256::ZERO,
            state_root: B256::ZERO,
            transactions_root: B256::ZERO,
            receipts_root: B256::ZERO,
            timestamp: 0,
            number: 0,
            validator: Address::ZERO,
            signature: vec![],
            extra_data: vec![],
            gas_limit: 30_000_000,
            base_fee_per_gas: U256::ZERO,
        };
        assert_eq!(h.hash(), h.hash());
    }

    #[test]
    fn empty_body_transactions_root_is_zero() {
        let body = BlockBody::default();
        assert_eq!(body.transactions_root(), B256::ZERO);
    }
}
