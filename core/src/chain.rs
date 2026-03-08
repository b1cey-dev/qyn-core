//! Chain: block storage, current head, and fork resolution.

use crate::block::{Block, BlockBody, BlockHeader};
use crate::error::CoreError;
use crate::state::StateDB;
use crate::validation;
use alloy_primitives::B256;
use rocksdb::DB;
use std::path::Path;
use std::sync::Arc;

const COL_BLOCK_HEADER: &[u8] = b"block_header:";
const COL_BLOCK_BODY: &[u8] = b"block_body:";
const COL_BLOCK_NUMBER: &[u8] = b"block_number:";
const COL_TX_RECEIPT: &[u8] = b"tx_receipt:";
const KEY_HEAD: &[u8] = b"head_hash";

/// Chain storage (blocks only). State is in StateDB.
pub struct ChainDB {
    db: Arc<rocksdb::DB>,
}

impl ChainDB {
    pub fn open(path: &Path) -> Result<Self, CoreError> {
        let db = DB::open_default(path).map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(Self { db: Arc::new(db) })
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

    /// Store tx hash -> (block_hash, block_number, index) for receipt lookup.
    pub fn put_tx_receipt_index(
        &self,
        tx_hash: &B256,
        block_hash: B256,
        block_number: u64,
        index: u32,
    ) -> Result<(), CoreError> {
        let key = [COL_TX_RECEIPT, tx_hash.as_slice()].concat();
        let mut val = block_hash.as_slice().to_vec();
        val.extend_from_slice(&block_number.to_be_bytes());
        val.extend_from_slice(&index.to_be_bytes());
        self.db.put(key, &val).map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(())
    }

    /// Get receipt location for a tx, if included in a block.
    pub fn get_tx_receipt_index(&self, tx_hash: &B256) -> Result<Option<(B256, u64, u32)>, CoreError> {
        let key = [COL_TX_RECEIPT, tx_hash.as_slice()].concat();
        let val = self.db.get(&key).map_err(|e| CoreError::Storage(e.to_string()))?;
        let val = match val {
            Some(v) if v.len() >= 32 + 8 + 4 => v,
            _ => return Ok(None),
        };
        let block_hash = B256::from_slice(&val[0..32]);
        let block_number = u64::from_be_bytes(val[32..40].try_into().unwrap());
        let index = u32::from_be_bytes(val[40..44].try_into().unwrap());
        Ok(Some((block_hash, block_number, index)))
    }
}

/// Accept a new block: validate and append. Caller must have applied state transition and have new state_root.
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
    chain.put_block(block)?;
    chain.set_head(&block.hash())?;
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
}
