//! Fork resolution: GHOST stake-weighted fork choice and checkpoint finality.

use crate::chain::ChainDB;
use crate::error::CoreError;
use crate::state::StateDB;
use alloy_primitives::B256;
use std::collections::HashMap;

/// Re-export for callers that need the constant.
pub use crate::chain::FINALITY_DEPTH;

/// Fork choice: GHOST (Greediest Heaviest Observed SubTree). Weight each block by the
/// validator's stake (balance in state); choose the child with heaviest subtree at each step.
pub fn canonical_head(chain: &ChainDB, state: &StateDB) -> Result<Option<B256>, CoreError> {
    let genesis = match chain.get_block_by_number(0)? {
        Some(b) => b.hash(),
        None => return chain.get_head(),
    };
    // Walk from genesis: at each step pick the child with heaviest subtree (GHOST).
    let mut current = genesis;
    loop {
        let children = chain.get_children(&current)?;
        if children.is_empty() {
            return Ok(Some(current));
        }
        let best = children
            .into_iter()
            .max_by_key(|h| subtree_stake(chain, state, h).unwrap_or(0))
            .unwrap_or(current);
        current = best;
    }
}

/// Total stake (validator balance) in the subtree rooted at this block (for GHOST).
fn subtree_stake(chain: &ChainDB, state: &StateDB, block_hash: &B256) -> Result<u128, CoreError> {
    let block = match chain.get_block(block_hash)? {
        Some(b) => b,
        None => return Ok(0),
    };
    let validator_stake = state.get_balance(&block.header.validator)?.to::<u128>();
    let children = chain.get_children(block_hash)?;
    let child_stake: u128 = children
        .iter()
        .map(|h| subtree_stake(chain, state, h))
        .try_fold(0u128, |acc, r| r.map(|s| acc.saturating_add(s)))?;
    Ok(validator_stake.saturating_add(child_stake))
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
