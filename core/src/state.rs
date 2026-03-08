//! Chain state: account balances, nonces, and contract storage.
//!
//! Backed by RocksDB for persistence. State root is computed from a simple
//! Merkle-style structure (or hash of state for MVP).

use crate::error::CoreError;
use alloy_primitives::{Address, B256, U256};
use rocksdb::{Direction, IteratorMode, DB};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;

const COL_BALANCE: &str = "balance:";
const COL_NONCE: &str = "nonce:";
const COL_CODE: &str = "code:";
#[allow(dead_code)]
const COL_STORAGE: &str = "storage:";
const PREFIX_STATE_ROOT: &[u8] = b"state_root:";

/// Account state (balance + nonce). Contract storage and code in separate keys.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AccountState {
    pub balance: U256,
    pub nonce: u64,
}

/// In-memory state for execution; can be committed to StateDB.
pub struct StateDB {
    db: Arc<rocksdb::DB>,
}

impl StateDB {
    pub fn open(path: &Path) -> Result<Self, CoreError> {
        let db = DB::open_default(path)
            .map_err(|e| CoreError::Storage(format!("RocksDB open: {}", e)))?;
        Ok(Self { db: Arc::new(db) })
    }

    /// Get balance for address.
    pub fn get_balance(&self, address: &Address) -> Result<U256, CoreError> {
        let key = format!("{}{}", COL_BALANCE, hex::encode(address.as_slice()));
        match self.db.get(key.as_bytes()) {
            Ok(Some(v)) => {
                let mut buf = [0u8; 32];
                let len = v.len().min(32);
                buf[32 - len..].copy_from_slice(&v);
                Ok(U256::from_be_bytes(buf))
            }
            Ok(None) => Ok(U256::ZERO),
            Err(e) => Err(CoreError::Storage(e.to_string())),
        }
    }

    /// Set balance.
    pub fn set_balance(&self, address: &Address, balance: U256) -> Result<(), CoreError> {
        let key = format!("{}{}", COL_BALANCE, hex::encode(address.as_slice()));
        let val = balance.to_be_bytes::<32>();
        self.db
            .put(key.as_bytes(), val.as_slice())
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(())
    }

    /// Get nonce.
    pub fn get_nonce(&self, address: &Address) -> Result<u64, CoreError> {
        let key = format!("{}{}", COL_NONCE, hex::encode(address.as_slice()));
        match self.db.get(key.as_bytes()) {
            Ok(Some(v)) => {
                let mut buf = [0u8; 8];
                let len = v.len().min(8);
                buf[8 - len..].copy_from_slice(&v);
                Ok(u64::from_be_bytes(buf))
            }
            Ok(None) => Ok(0),
            Err(e) => Err(CoreError::Storage(e.to_string())),
        }
    }

    /// Set nonce.
    pub fn set_nonce(&self, address: &Address, nonce: u64) -> Result<(), CoreError> {
        let key = format!("{}{}", COL_NONCE, hex::encode(address.as_slice()));
        self.db
            .put(key.as_bytes(), nonce.to_be_bytes())
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(())
    }

    /// Get contract code at address.
    pub fn get_code(&self, address: &Address) -> Result<Vec<u8>, CoreError> {
        let key = format!("{}{}", COL_CODE, hex::encode(address.as_slice()));
        match self.db.get(key.as_bytes()) {
            Ok(Some(v)) => Ok(v),
            Ok(None) => Ok(vec![]),
            Err(e) => Err(CoreError::Storage(e.to_string())),
        }
    }

    /// Set contract code.
    pub fn set_code(&self, address: &Address, code: &[u8]) -> Result<(), CoreError> {
        let key = format!("{}{}", COL_CODE, hex::encode(address.as_slice()));
        self.db
            .put(key.as_bytes(), code)
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(())
    }

    /// Get storage slot at address.
    pub fn get_storage(&self, address: &Address, slot: U256) -> Result<U256, CoreError> {
        let key = format!(
            "{}{}:{}",
            COL_STORAGE,
            hex::encode(address.as_slice()),
            hex::encode(slot.to_be_bytes::<32>().as_slice())
        );
        match self.db.get(key.as_bytes()) {
            Ok(Some(v)) => {
                let mut buf = [0u8; 32];
                let len = v.len().min(32);
                buf[32 - len..].copy_from_slice(&v);
                Ok(U256::from_be_bytes(buf))
            }
            Ok(None) => Ok(U256::ZERO),
            Err(e) => Err(CoreError::Storage(e.to_string())),
        }
    }

    /// Set storage slot at address.
    pub fn set_storage(&self, address: &Address, slot: U256, value: U256) -> Result<(), CoreError> {
        let key = format!(
            "{}{}:{}",
            COL_STORAGE,
            hex::encode(address.as_slice()),
            hex::encode(slot.to_be_bytes::<32>().as_slice())
        );
        self.db
            .put(key.as_bytes(), value.to_be_bytes::<32>().as_slice())
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(())
    }

    /// Compute a simple state root (hash of all balance+nonce entries). Not a full Merkle trie.
    pub fn compute_state_root(&self) -> Result<B256, CoreError> {
        let mut hasher = Sha256::new();
        let mut keys: Vec<_> = Vec::new();
        let iter = self.db.iterator(IteratorMode::From(COL_BALANCE.as_bytes(), Direction::Forward));
        for item in iter {
            let (k, v) = item.map_err(|e| CoreError::Storage(e.to_string()))?;
            if !k.starts_with(COL_BALANCE.as_bytes()) {
                break;
            }
            keys.push((k.to_vec(), v.to_vec()));
        }
        keys.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, v) in keys {
            hasher.update(&k);
            hasher.update(&v);
        }
        let iter = self.db.iterator(IteratorMode::From(COL_NONCE.as_bytes(), Direction::Forward));
        for item in iter {
            let (k, v) = item.map_err(|e| CoreError::Storage(e.to_string()))?;
            if !k.starts_with(COL_NONCE.as_bytes()) {
                break;
            }
            hasher.update(&k);
            hasher.update(&v);
        }
        Ok(B256::from_slice(&hasher.finalize()[..]))
    }

    /// Persist state root for a block.
    pub fn save_state_root(&self, block_hash: &B256, state_root: B256) -> Result<(), CoreError> {
        let key = [PREFIX_STATE_ROOT, block_hash.as_slice()].concat();
        self.db
            .put(key, state_root.as_slice())
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(())
    }
}

/// Apply a transfer: subtract from sender, add to recipient. Caller must ensure balance >= value.
pub fn apply_transfer(
    db: &StateDB,
    from: &Address,
    to: &Address,
    value: U256,
) -> Result<(), CoreError> {
    if value.is_zero() {
        return Ok(());
    }
    let from_bal = db.get_balance(from)?;
    let to_bal = db.get_balance(to)?;
    db.set_balance(from, from_bal.saturating_sub(value))?;
    db.set_balance(to, to_bal.saturating_add(value))?;
    Ok(())
}

/// Apply a simple transfer tx: deduct value+gas from sender, add value to to, fee to validator, increment nonce.
/// Caller must have validated tx (balance, nonce, chain_id). Only supports value transfers (no contract calls).
pub fn apply_simple_transfer_tx(
    db: &StateDB,
    tx: &crate::transaction::SignedTransaction,
    validator: &Address,
) -> Result<(), CoreError> {
    let sender = tx.sender()?;
    let gas_fee = tx.gas_price().saturating_mul(U256::from(tx.gas_limit()));
    let (burn, proposer_fee) = crate::genesis::split_fees(gas_fee);
    let _ = burn; // burn not credited to anyone
    let from_bal = db.get_balance(&sender)?;
    db.set_balance(&sender, from_bal.saturating_sub(tx.value()).saturating_sub(gas_fee))?;
    if let Some(to) = tx.to() {
        let to_bal = db.get_balance(&to)?;
        db.set_balance(&to, to_bal.saturating_add(tx.value()))?;
    }
    let val_bal = db.get_balance(validator)?;
    db.set_balance(validator, val_bal.saturating_add(proposer_fee))?;
    let nonce = db.get_nonce(&sender)?;
    db.set_nonce(&sender, nonce + 1)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn state_balance_nonce_roundtrip() {
        let dir = TempDir::new().unwrap();
        let db = StateDB::open(dir.path()).unwrap();
        let addr = Address::from_slice(&[1u8; 20]);
        db.set_balance(&addr, U256::from(1000)).unwrap();
        db.set_nonce(&addr, 5).unwrap();
        assert_eq!(db.get_balance(&addr).unwrap(), U256::from(1000));
        assert_eq!(db.get_nonce(&addr).unwrap(), 5);
    }
}
