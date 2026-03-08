//! Integration tests for Quyn node. Run with: cargo test -p quyn --test integration

use quyn_consensus::{ValidatorSet, MIN_STAKE};
use quyn_core::{apply_genesis_alloc, block::{Block, BlockBody, BlockHeader}, chain::ChainDB, state::StateDB};
use alloy_primitives::{Address, B256, U256};
use std::collections::HashMap;

#[test]
fn full_node_opens_data_dir() {
    let dir = tempfile::tempdir().unwrap();
    let node = quyn::runner::FullNode::open(dir.path());
    assert!(node.is_ok());
}

/// L4: Verify validator set persists across "restart" (reopen chain from same dir)
#[test]
fn validator_set_persists_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let chain_path = dir.path().join("chain");
    let state_path = dir.path().join("state");
    std::fs::create_dir_all(&chain_path).unwrap();
    std::fs::create_dir_all(&state_path).unwrap();

    let chain = ChainDB::open(&chain_path).unwrap();
    let state = StateDB::open(&state_path).unwrap();
    let mut validator_bytes = [0u8; 20];
    validator_bytes[19] = 1;
    let validator = Address::from_slice(&validator_bytes);
    let one_qyn = 10_u128.pow(18);
    let mut alloc = HashMap::new();
    alloc.insert(format!("0x{}", hex::encode(validator.as_slice())), format!("0x{:x}", 1_000_000_000u128 * one_qyn));
    apply_genesis_alloc(&state, &alloc).unwrap();
    let state_root = state.compute_state_root().unwrap();
    let genesis = Block {
        header: BlockHeader {
            parent_hash: B256::ZERO,
            state_root,
            transactions_root: B256::ZERO,
            receipts_root: B256::ZERO,
            timestamp: 0,
            number: 0,
            validator,
            signature: vec![],
            extra_data: vec![],
            gas_limit: 30_000_000,
            base_fee_per_gas: U256::ZERO,
        },
        body: BlockBody::default(),
    };
    chain.put_block(&genesis).unwrap();
    chain.set_head(&genesis.hash()).unwrap();
    state.save_state_root(&genesis.hash(), genesis.header.state_root).unwrap();

    let mut validator_set = ValidatorSet::new();
    validator_set.register(validator, U256::from(MIN_STAKE), 0).unwrap();
    let bytes = bincode::serialize(&validator_set).unwrap();
    chain.put_validator_set_bytes(&bytes).unwrap();

    drop(chain);
    drop(state);

    let chain2 = ChainDB::open(&chain_path).unwrap();
    let loaded = chain2.get_validator_set_bytes().unwrap();
    assert!(loaded.is_some());
    let loaded_bytes = loaded.unwrap();
    let restored: ValidatorSet = bincode::deserialize(&loaded_bytes).unwrap();
    let active = restored.active_validators();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].address, validator);
}
