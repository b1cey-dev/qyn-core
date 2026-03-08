//! Block and transaction validation logic.

use crate::block::{Block, BlockHeader};
use crate::error::CoreError;
use crate::state::StateDB;
use crate::transaction::SignedTransaction;
use crate::types::BLOCK_TIME_SECS;
use alloy_primitives::{Address, U256};
use std::collections::HashSet;

/// Validate a block header against parent and chain rules.
pub fn validate_block_header(
    header: &BlockHeader,
    parent: Option<&BlockHeader>,
    current_timestamp: u64,
) -> Result<(), CoreError> {
    if let Some(p) = parent {
        if header.parent_hash != p.hash() {
            return Err(CoreError::InvalidBlock("parent_hash mismatch".into()));
        }
        if header.number != p.number + 1 {
            return Err(CoreError::InvalidBlock("block number not parent+1".into()));
        }
        if header.timestamp <= p.timestamp {
            return Err(CoreError::InvalidBlock("timestamp not strictly after parent".into()));
        }
        if header.timestamp > current_timestamp + BLOCK_TIME_SECS * 2 {
            return Err(CoreError::InvalidBlock("timestamp too far in future".into()));
        }
    } else {
        if header.number != 0 {
            return Err(CoreError::InvalidBlock("genesis must have number 0".into()));
        }
    }
    Ok(())
}

/// Validate a signed transaction (signature, chain_id, basic fields).
pub fn validate_tx_basic(tx: &SignedTransaction, chain_id: u64) -> Result<(), CoreError> {
    if tx.chain_id() != chain_id {
        return Err(CoreError::InvalidTransaction(format!(
            "chain_id mismatch: expected {}, got {}",
            chain_id,
            tx.chain_id()
        )));
    }
    tx.sender().map_err(|e| CoreError::InvalidTransaction(e.to_string()))?;
    Ok(())
}

/// Validate transaction against state (balance, nonce). Call after validate_tx_basic.
pub fn validate_tx_against_state(
    tx: &SignedTransaction,
    state: &StateDB,
) -> Result<(), CoreError> {
    let sender = tx.sender()?;
    let balance = state.get_balance(&sender)?;
    let nonce = state.get_nonce(&sender)?;
    let cost = tx.value()
        .saturating_add(tx.gas_price().saturating_mul(U256::from(tx.gas_limit())));
    if balance < cost {
        return Err(CoreError::InvalidTransaction(format!(
            "insufficient balance: have {}, need {}",
            balance, cost
        )));
    }
    if tx.nonce() != nonce {
        return Err(CoreError::InvalidTransaction(format!(
            "invalid nonce: expected {}, got {}",
            nonce,
            tx.nonce()
        )));
    }
    Ok(())
}

/// Validate full block: self-consistency, then each tx basic validation.
/// State transition (and state_root) is done by the executor; this only checks structure.
pub fn validate_block(block: &Block) -> Result<(), CoreError> {
    block.validate_self()?;
    let mut seen_senders: HashSet<Address> = HashSet::new();
    for tx in &block.body.transactions {
        let sender = tx.sender().map_err(|e| CoreError::InvalidTransaction(e.to_string()))?;
        if !seen_senders.insert(sender) {
            return Err(CoreError::InvalidBlock(
                "duplicate sender in block (nonce ordering required)".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::BlockHeader;
    use alloy_primitives::{B256, U256};

    #[test]
    fn genesis_header_valid() {
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
        assert!(validate_block_header(&h, None, 1).is_ok());
    }
}
