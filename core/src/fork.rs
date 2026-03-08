//! Fork resolution: choose canonical chain by stake weight (or length for MVP).

use crate::chain::ChainDB;
use crate::error::CoreError;
use alloy_primitives::B256;
use std::collections::HashMap;

/// Fork choice: we use longest chain (by block number) as canonical.
/// When consensus provides stake weights, this can be extended to weight-by-stake.
pub fn canonical_head(chain: &ChainDB) -> Result<Option<B256>, CoreError> {
    chain.get_head()
}

/// Find common ancestor of two block hashes.
pub fn common_ancestor(chain: &ChainDB, a: &B256, b: &B256) -> Result<Option<B256>, CoreError> {
    let mut seen: HashMap<B256, ()> = HashMap::new();
    let mut current = *a;
    while let Some(block) = chain.get_block(&current)? {
        seen.insert(current, ());
        current = block.header.parent_hash;
        if current == B256::ZERO {
            break;
        }
    }
    current = *b;
    while let Some(block) = chain.get_block(&current)? {
        if seen.contains_key(&current) {
            return Ok(Some(current));
        }
        current = block.header.parent_hash;
        if current == B256::ZERO {
            break;
        }
    }
    Ok(None)
}

/// Blocks to reorg: from old head to new head (exclusive of common ancestor).
/// Returns (blocks to revert in order from old head, blocks to apply from common ancestor toward new head).
pub fn reorg_blocks(
    chain: &ChainDB,
    old_head: &B256,
    new_head: &B256,
) -> Result<(Vec<B256>, Vec<B256>), CoreError> {
    let common = common_ancestor(chain, old_head, new_head)?
        .ok_or_else(|| CoreError::ChainValidation("no common ancestor".into()))?;
    let mut to_revert = Vec::new();
    let mut current = *old_head;
    while current != common {
        to_revert.push(current);
        let block = chain
            .get_block(&current)?
            .ok_or_else(|| CoreError::ChainValidation("block not found".into()))?;
        current = block.header.parent_hash;
    }
    let mut stack = Vec::new();
    current = *new_head;
    while current != common {
        stack.push(current);
        let block = chain
            .get_block(&current)?
            .ok_or_else(|| CoreError::ChainValidation("block not found".into()))?;
        current = block.header.parent_hash;
    }
    let to_apply: Vec<B256> = stack.into_iter().rev().collect();
    Ok((to_revert, to_apply))
}
