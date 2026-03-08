//! Mempool: in-memory pool of pending transactions.
//!
//! Keyed by sender; eviction by gas price or age when at capacity.

use crate::error::CoreError;
use crate::transaction::SignedTransaction;
use alloy_primitives::Address;
use std::collections::{BTreeMap, HashMap};
use std::sync::RwLock;

/// Default maximum number of transactions in the mempool.
pub const DEFAULT_MAX_POOL_SIZE: usize = 100_000;

/// Mempool for pending transactions. Thread-safe.
pub struct Mempool {
    /// sender -> nonce -> tx (ordered by nonce per sender)
    by_sender: RwLock<HashMap<Address, BTreeMap<u64, SignedTransaction>>>,
    /// tx hash -> (sender, nonce) for quick lookup and dedup
    by_hash: RwLock<HashMap<[u8; 32], (Address, u64)>>,
    max_size: usize,
}

impl Mempool {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_MAX_POOL_SIZE)
    }

    pub fn with_capacity(max_size: usize) -> Self {
        Self {
            by_sender: RwLock::new(HashMap::new()),
            by_hash: RwLock::new(HashMap::new()),
            max_size,
        }
    }

    /// Insert a transaction. Replaces existing if same sender+nonce. Returns evicted count if over capacity.
    pub fn insert(&self, tx: SignedTransaction) -> Result<Option<usize>, CoreError> {
        let sender = tx.sender().map_err(|_| CoreError::InvalidTransaction("Invalid signature".into()))?;
        let nonce = tx.nonce();
        let hash = tx.hash();
        let hash_arr: [u8; 32] = hash.0.into();

        let mut by_sender = self.by_sender.write().map_err(|e| CoreError::Mempool(e.to_string()))?;
        let mut by_hash = self.by_hash.write().map_err(|e| CoreError::Mempool(e.to_string()))?;

        if by_hash.contains_key(&hash_arr) {
            return Ok(None);
        }

        let entry = by_sender.entry(sender).or_default();
        if let Some(old) = entry.insert(nonce, tx) {
            let old_arr: [u8; 32] = old.hash().0.into();
            by_hash.remove(&old_arr);
        }
        by_hash.insert(hash_arr, (sender, nonce));

        let total: usize = by_sender.values().map(|m| m.len()).sum();
        if total > self.max_size {
            let to_evict = total - self.max_size;
            let evicted = self.evict_lowest_fee(&mut by_sender, &mut by_hash, to_evict);
            Ok(Some(evicted))
        } else {
            Ok(None)
        }
    }

    /// Evict `n` transactions with lowest gas price (and oldest first).
    fn evict_lowest_fee(
        &self,
        by_sender: &mut HashMap<Address, BTreeMap<u64, SignedTransaction>>,
        by_hash: &mut HashMap<[u8; 32], (Address, u64)>,
        n: usize,
    ) -> usize {
        let mut list: Vec<(u128, Address, u64)> = by_hash
            .iter()
            .map(|(_, (addr, nonce))| {
                let tx = by_sender.get(addr).and_then(|m| m.get(nonce)).unwrap();
                (tx.gas_price().to::<u128>(), *addr, *nonce)
            })
            .collect();
        list.sort_by_key(|(price, _, _)| *price);
        let mut evicted = 0;
        for (_, addr, nonce) in list.into_iter().take(n) {
            if let Some(m) = by_sender.get_mut(&addr) {
                if let Some(tx) = m.remove(&nonce) {
                    let arr: [u8; 32] = tx.hash().0.into();
                    by_hash.remove(&arr);
                    evicted += 1;
                }
            }
        }
        evicted
    }

    /// Remove transaction by hash (e.g. when included in a block).
    pub fn remove(&self, tx_hash: &[u8; 32]) -> Result<bool, CoreError> {
        let mut by_sender = self.by_sender.write().map_err(|e| CoreError::Mempool(e.to_string()))?;
        let mut by_hash = self.by_hash.write().map_err(|e| CoreError::Mempool(e.to_string()))?;
        if let Some((sender, nonce)) = by_hash.remove(tx_hash) {
            if let Some(m) = by_sender.get_mut(&sender) {
                m.remove(&nonce);
                if m.is_empty() {
                    by_sender.remove(&sender);
                }
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Get best transactions for block building: up to `limit` txs, ordered by gas price desc.
    pub fn get_best(&self, limit: usize) -> Result<Vec<SignedTransaction>, CoreError> {
        let by_sender = self.by_sender.read().map_err(|e| CoreError::Mempool(e.to_string()))?;
        let mut all: Vec<SignedTransaction> = by_sender
            .values()
            .flat_map(|m| m.values().cloned())
            .collect();
        all.sort_by(|a, b| b.gas_price().cmp(&a.gas_price()));
        Ok(all.into_iter().take(limit).collect())
    }

    /// Get pending count.
    pub fn len(&self) -> Result<usize, CoreError> {
        let by_sender = self.by_sender.read().map_err(|e| CoreError::Mempool(e.to_string()))?;
        Ok(by_sender.values().map(|m| m.len()).sum())
    }

    pub fn is_empty(&self) -> Result<bool, CoreError> {
        Ok(self.len()? == 0)
    }
}

impl Default for Mempool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mempool_empty_len_and_get_best() {
        let pool = Mempool::with_capacity(10);
        assert_eq!(pool.len().unwrap(), 0);
        assert!(pool.get_best(5).unwrap().is_empty());
    }

    #[test]
    fn mempool_remove_nonexistent() {
        let pool = Mempool::new();
        let hash = [0u8; 32];
        assert!(!pool.remove(&hash).unwrap());
    }
}
