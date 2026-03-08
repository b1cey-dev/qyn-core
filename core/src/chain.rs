//! Chain: block storage, current head, fork resolution, and double-sign tracking.

use crate::block::{Block, BlockBody, BlockHeader};
use crate::error::CoreError;
use crate::state::StateDB;
use crate::validation;
use alloy_primitives::{Address, B256};
use rocksdb::DB;
use std::path::Path;
use std::sync::Arc;

const COL_BLOCK_HEADER: &[u8] = b"block_header:";
const COL_BLOCK_BODY: &[u8] = b"block_body:";
const COL_BLOCK_NUMBER: &[u8] = b"block_number:";
const COL_TX_RECEIPT: &[u8] = b"tx_receipt:";
const COL_SIGNED_BLOCK: &[u8] = b"signed_block:";
const COL_SLASH_EVIDENCE: &[u8] = b"slash_evidence:";
const COL_CHILDREN: &[u8] = b"children:";
const KEY_HEAD: &[u8] = b"head_hash";
const KEY_FINALIZED_HEIGHT: &[u8] = b"finalized_height";
const KEY_FINALIZED_HASH: &[u8] = b"finalized_hash";
const KEY_VALIDATOR_SET: &[u8] = b"validator_set";

/// Depth (in blocks) before head that is considered finalized. Reorgs cannot go past this.
pub const FINALITY_DEPTH: u64 = 100;

/// Chain storage (blocks only). State is in StateDB.
pub struct ChainDB {
    db: Arc<rocksdb::DB>,
}

impl ChainDB {
    pub fn open(path: &Path) -> Result<Self, CoreError> {
        let db = DB::open_default(path).map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(Self { db: Arc::new(db) })
    }

    /// Get the block hash that a validator signed at the given height (if any).
    pub fn get_signed_block(&self, validator: &Address, height: u64) -> Result<Option<B256>, CoreError> {
        let key = [
            COL_SIGNED_BLOCK,
            hex::encode(validator.as_slice()).as_bytes(),
            b":",
            height.to_be_bytes().as_slice(),
        ]
        .concat();
        let val = self.db.get(&key).map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(val.and_then(|v| (v.len() == 32).then(|| B256::from_slice(&v))))
    }

    /// Persist slash evidence (e.g. after double-sign detection). Caller provides serialized evidence.
    pub fn put_slash_evidence(&self, validator: &Address, block_number: u64, evidence_bytes: &[u8]) -> Result<(), CoreError> {
        let key = [
            COL_SLASH_EVIDENCE,
            hex::encode(validator.as_slice()).as_bytes(),
            b":",
            block_number.to_be_bytes().as_slice(),
        ]
        .concat();
        self.db.put(key, evidence_bytes).map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn put_block(&self, block: &Block) -> Result<(), CoreError> {
        let hash = block.hash();
        let header_key = [COL_BLOCK_HEADER, hash.as_slice()].concat();
        let body_key = [COL_BLOCK_BODY, hash.as_slice()].concat();
        let number_key = [COL_BLOCK_NUMBER, block.header.number.to_be_bytes().as_slice()].concat();
        let header_bytes =
            bincode::serialize(&block.header).map_err(|e| CoreError::Serialization(e.to_string()))?;
        let body_bytes =
            bincode::serialize(&block.body).map_err(|e| CoreError::Serialization(e.to_string()))?;
        self.db.put(header_key, &header_bytes).map_err(|e| CoreError::Storage(e.to_string()))?;
        self.db.put(body_key, &body_bytes).map_err(|e| CoreError::Storage(e.to_string()))?;
        self.db.put(number_key, hash.as_slice()).map_err(|e| CoreError::Storage(e.to_string()))?;
        // Persist validator -> height -> block_hash for double-sign detection.
        let signed_key = [
            COL_SIGNED_BLOCK,
            hex::encode(block.header.validator.as_slice()).as_bytes(),
            b":",
            block.header.number.to_be_bytes().as_slice(),
        ]
        .concat();
        self.db.put(signed_key, hash.as_slice()).map_err(|e| CoreError::Storage(e.to_string()))?;
        // Index children for GHOST fork choice.
        let child_key = [COL_CHILDREN, block.header.parent_hash.as_slice()].concat();
        let mut existing = self.db.get(&child_key).map_err(|e| CoreError::Storage(e.to_string()))?.unwrap_or_default();
        existing.extend_from_slice(hash.as_slice());
        self.db.put(child_key, &existing).map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(())
    }

    /// Get child block hashes for a parent (for GHOST fork choice).
    pub fn get_children(&self, parent_hash: &B256) -> Result<Vec<B256>, CoreError> {
        let key = [COL_CHILDREN, parent_hash.as_slice()].concat();
        let val = self.db.get(&key).map_err(|e| CoreError::Storage(e.to_string()))?;
        let val = match val {
            Some(v) => v,
            None => return Ok(vec![]),
        };
        Ok(val.chunks(32).filter(|c| c.len() == 32).map(|c| B256::from_slice(c)).collect())
    }

    /// Get finalized block height (blocks at or below this cannot be reorged).
    pub fn get_finalized_height(&self) -> Result<Option<u64>, CoreError> {
        let v = self.db.get(KEY_FINALIZED_HEIGHT).map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(v.and_then(|b| {
            (b.len() == 8)
                .then(|| b[0..8].try_into().ok().map(u64::from_be_bytes))
                .flatten()
        }))
    }

    /// Get finalized block hash.
    pub fn get_finalized_hash(&self) -> Result<Option<B256>, CoreError> {
        let v = self.db.get(KEY_FINALIZED_HASH).map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(v.and_then(|b| (b.len() == 32).then(|| B256::from_slice(&b))))
    }

    /// Load persisted validator set bytes (node decodes with consensus::ValidatorSet).
    pub fn get_validator_set_bytes(&self) -> Result<Option<Vec<u8>>, CoreError> {
        self.db.get(KEY_VALIDATOR_SET).map_err(|e| CoreError::Storage(e.to_string()))
    }

    /// Persist validator set bytes after each change.
    pub fn put_validator_set_bytes(&self, bytes: &[u8]) -> Result<(), CoreError> {
        self.db.put(KEY_VALIDATOR_SET, bytes).map_err(|e| CoreError::Storage(e.to_string()))
    }

    /// Update finalized checkpoint (call after set_head when head_number >= FINALITY_DEPTH).
    pub fn update_finalized(&self, head_hash: &B256) -> Result<(), CoreError> {
        let block = match self.get_block(head_hash)? {
            Some(b) => b,
            None => return Ok(()),
        };
        let number = block.header.number;
        if number < FINALITY_DEPTH {
            return Ok(());
        }
        let finalized_number = number - FINALITY_DEPTH;
        let mut current = *head_hash;
        for _ in 0..=FINALITY_DEPTH {
            let b = self.get_block(&current)?.ok_or_else(|| CoreError::ChainValidation("block not found".into()))?;
            if b.header.number == finalized_number {
                self.db.put(KEY_FINALIZED_HEIGHT, finalized_number.to_be_bytes()).map_err(|e| CoreError::Storage(e.to_string()))?;
                self.db.put(KEY_FINALIZED_HASH, current.as_slice()).map_err(|e| CoreError::Storage(e.to_string()))?;
                return Ok(());
            }
            current = b.header.parent_hash;
        }
        Ok(())
    }

    pub fn get_block(&self, hash: &B256) -> Result<Option<Block>, CoreError> {
        let header_key = [COL_BLOCK_HEADER, hash.as_slice()].concat();
        let body_key = [COL_BLOCK_BODY, hash.as_slice()].concat();
        let header_bytes = self.db.get(&header_key).map_err(|e| CoreError::Storage(e.to_string()))?;
        let body_bytes = self.db.get(&body_key).map_err(|e| CoreError::Storage(e.to_string()))?;
        match (header_bytes, body_bytes) {
            (Some(h), Some(b)) => {
                let header: BlockHeader =
                    bincode::deserialize(&h).map_err(|e| CoreError::Serialization(e.to_string()))?;
                let body: BlockBody =
                    bincode::deserialize(&b).map_err(|e| CoreError::Serialization(e.to_string()))?;
                Ok(Some(Block { header, body }))
            }
            _ => Ok(None),
        }
    }

    pub fn get_block_by_number(&self, number: u64) -> Result<Option<Block>, CoreError> {
        let number_key = [COL_BLOCK_NUMBER, number.to_be_bytes().as_slice()].concat();
        let hash_bytes = self.db.get(&number_key).map_err(|e| CoreError::Storage(e.to_string()))?;
        let hash = match hash_bytes {
            Some(b) if b.len() == 32 => B256::from_slice(&b),
            _ => return Ok(None),
        };
        self.get_block(&hash)
    }

    pub fn set_head(&self, hash: &B256) -> Result<(), CoreError> {
        self.db
            .put(KEY_HEAD, hash.as_slice())
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn get_head(&self) -> Result<Option<B256>, CoreError> {
        let b = self.db.get(KEY_HEAD).map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(b.and_then(|b| (b.len() == 32).then(|| B256::from_slice(&b))))
    }

    /// Store tx hash -> (block_hash, block_number, index, gas_used) for receipt lookup.
    pub fn put_tx_receipt_index(
        &self,
        tx_hash: &B256,
        block_hash: B256,
        block_number: u64,
        index: u32,
        gas_used: u64,
    ) -> Result<(), CoreError> {
        let key = [COL_TX_RECEIPT, tx_hash.as_slice()].concat();
        let mut val = block_hash.as_slice().to_vec();
        val.extend_from_slice(&block_number.to_be_bytes());
        val.extend_from_slice(&index.to_be_bytes());
        val.extend_from_slice(&gas_used.to_be_bytes());
        self.db.put(key, &val).map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(())
    }

    /// Get receipt location for a tx, if included in a block. Returns (block_hash, block_number, index, gas_used).
    pub fn get_tx_receipt_index(&self, tx_hash: &B256) -> Result<Option<(B256, u64, u32, u64)>, CoreError> {
        let key = [COL_TX_RECEIPT, tx_hash.as_slice()].concat();
        let val = self.db.get(&key).map_err(|e| CoreError::Storage(e.to_string()))?;
        let val = match val {
            Some(v) if v.len() >= 32 + 8 + 4 => v,
            _ => return Ok(None),
        };
        let block_hash = B256::from_slice(&val[0..32]);
        let block_number_arr: [u8; 8] = val[32..40]
            .try_into()
            .map_err(|_| CoreError::Storage("receipt index format".into()))?;
        let index_arr: [u8; 4] = val[40..44]
            .try_into()
            .map_err(|_| CoreError::Storage("receipt index format".into()))?;
        let block_number = u64::from_be_bytes(block_number_arr);
        let index = u32::from_be_bytes(index_arr);
        let gas_used = if val.len() >= 52 {
            let arr: [u8; 8] = val[44..52]
                .try_into()
                .map_err(|_| CoreError::Storage("receipt index format".into()))?;
            u64::from_be_bytes(arr)
        } else {
            21000
        };
        Ok(Some((block_hash, block_number, index, gas_used)))
    }
}

/// Accept a new block: validate, check double-sign, append. Caller must have applied state transition and have new state_root.
pub fn accept_block(
    chain: &ChainDB,
    state: &StateDB,
    block: &Block,
    current_ts: u64,
) -> Result<(), CoreError> {
    validation::validate_block(block)?;
    let parent = block.header.number.checked_sub(1).and_then(|n| chain.get_block_by_number(n).ok().flatten());
    if let Some(ref p) = parent {
        validation::validate_block_header(&block.header, Some(&p.header), current_ts)?;
    } else if block.header.number != 0 {
        return Err(CoreError::ChainValidation("missing parent".into()));
    }

    // Checkpoint finality: reject reorgs that would go past finalized block.
    let current_head = chain.get_head()?;
    if let Some(head_hash) = current_head {
        if block.header.parent_hash != head_hash {
            let finalized_height = chain.get_finalized_height()?;
            if let Some(fh) = finalized_height {
                let common = crate::fork::common_ancestor(chain, &head_hash, &block.hash())?;
                if let Some(ca_hash) = common {
                    let ca_block = chain.get_block(&ca_hash)?.ok_or_else(|| CoreError::ChainValidation("common ancestor not found".into()))?;
                    if ca_block.header.number < fh {
                        return Err(CoreError::ChainValidation(format!(
                            "reorg would exceed finality: common ancestor {} < finalized {}",
                            ca_block.header.number, fh
                        )));
                    }
                }
            }
        }
    }

    // Double-sign detection: reject if validator already signed a different block at this height.
    // Caller (node) should record SlashEvidence and apply slash when this returns DoubleSign error.
    if let Some(existing_hash) = chain.get_signed_block(&block.header.validator, block.header.number)? {
        if existing_hash != block.hash() {
            return Err(CoreError::DoubleSign {
                validator: block.header.validator,
                height: block.header.number,
                first_block: existing_hash,
                second_block: block.hash(),
            });
        }
    }

    chain.put_block(block)?;
    chain.set_head(&block.hash())?;
    chain.update_finalized(&block.hash())?;
    state.save_state_root(&block.hash(), block.header.state_root)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn chain_put_and_get_block() {
        let dir = TempDir::new().unwrap();
        let chain = ChainDB::open(dir.path()).unwrap();
        let block = Block {
            header: BlockHeader {
                parent_hash: B256::ZERO,
                state_root: B256::ZERO,
                transactions_root: B256::ZERO,
                receipts_root: B256::ZERO,
                timestamp: 0,
                number: 0,
                validator: alloy_primitives::Address::ZERO,
                signature: vec![],
                extra_data: vec![],
                gas_limit: 30_000_000,
                base_fee_per_gas: alloy_primitives::U256::ZERO,
            },
            body: crate::block::BlockBody::default(),
        };
        chain.put_block(&block).unwrap();
        let hash = block.hash();
        let got = chain.get_block(&hash).unwrap().unwrap();
        assert_eq!(got.header.number, 0);
    }

    #[test]
    fn get_finalized_height_none_when_empty() {
        let dir = TempDir::new().unwrap();
        let chain = ChainDB::open(dir.path()).unwrap();
        assert!(chain.get_finalized_height().unwrap().is_none());
        assert!(chain.get_finalized_hash().unwrap().is_none());
    }

    #[test]
    fn tx_receipt_index_roundtrip() {
        let dir = TempDir::new().unwrap();
        let chain = ChainDB::open(dir.path()).unwrap();
        let tx_hash = B256::from_slice(&[1u8; 32]);
        let block_hash = B256::from_slice(&[2u8; 32]);
        chain.put_tx_receipt_index(&tx_hash, block_hash, 5, 3, 21000).unwrap();
        let got = chain.get_tx_receipt_index(&tx_hash).unwrap().unwrap();
        assert_eq!(got.0, block_hash);
        assert_eq!(got.1, 5);
        assert_eq!(got.2, 3);
        assert_eq!(got.3, 21000);
    }
}
