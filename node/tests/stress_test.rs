//! Stress tests for QYN: mempool load, block inclusion, TPS measurement.
//! Run with: cargo test -p quyn --test stress_test

use alloy_primitives::{Address, U256};
use quyn_core::{
    apply_genesis_alloc,
    block::{Block, BlockBody, BlockHeader},
    chain::ChainDB,
    Mempool, StateDB,
};
use quyn_wallet::sign_transaction;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

fn devnet_faucet_keypair() -> quyn_wallet::KeyPair {
    let mut secret = [0u8; 32];
    secret[0] = 0xde;
    secret[1] = 0xad;
    secret[2] = 0xbe;
    secret[3] = 0xef;
    secret[4] = 0x11;
    quyn_wallet::KeyPair::from_secret(secret).expect("devnet faucet key")
}

fn setup_genesis(chain: &ChainDB, state: &StateDB) {
    let mut validator_bytes = [0u8; 20];
    validator_bytes[19] = 1;
    let validator = Address::from_slice(&validator_bytes);
    let faucet = devnet_faucet_keypair().address();
    let one_qyn = 10_u128.pow(18);
    let mut alloc = HashMap::new();
    alloc.insert(format!("0x{}", hex::encode(validator.as_slice())), format!("0x{:x}", 1_000_000_000u128 * one_qyn));
    alloc.insert(format!("0x{}", hex::encode(faucet.as_slice())), format!("0x{:x}", 100_000_000u128 * one_qyn));
    apply_genesis_alloc(state, &alloc).unwrap();
    let state_root = state.compute_state_root().unwrap();
    let genesis = Block {
        header: BlockHeader {
            parent_hash: alloy_primitives::B256::ZERO,
            state_root,
            transactions_root: alloy_primitives::B256::ZERO,
            receipts_root: alloy_primitives::B256::ZERO,
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
}

/// Test 1: 1,000 transactions - mempool insert and ordering
#[test]
fn stress_1000_transactions() {
    let dir = tempfile::tempdir().unwrap();
    let chain = Arc::new(ChainDB::open(&dir.path().join("chain")).unwrap());
    let state = Arc::new(StateDB::open(&dir.path().join("state")).unwrap());
    let mempool = Arc::new(Mempool::with_capacity(10_000));
    setup_genesis(&chain, &state);

    let faucet = devnet_faucet_keypair();
    let chain_id = 7778u64;
    let start = Instant::now();
    for nonce in 0..1000 {
        let tx = quyn_core::Transaction {
            nonce,
            gas_price: U256::from(1),
            gas_limit: 21_000,
            to: Some(Address::ZERO),
            value: U256::from(1000),
            data: vec![],
            chain_id,
        };
        let signed = sign_transaction(&tx, &faucet).unwrap();
        mempool.insert(signed).unwrap();
    }
    let insert_time = start.elapsed();
    let count = mempool.len().unwrap();
    assert_eq!(count, 1000, "All 1000 txs should be in mempool");
    let tps = 1000.0 / insert_time.as_secs_f64();
    eprintln!("Stress 1k: inserted 1000 txs in {:?}, {:.0} insert/s", insert_time, tps);
}

/// Test 2: 10,000 transactions - mempool capacity
#[test]
fn stress_10000_transactions() {
    let dir = tempfile::tempdir().unwrap();
    let chain = Arc::new(ChainDB::open(&dir.path().join("chain")).unwrap());
    let state = Arc::new(StateDB::open(&dir.path().join("state")).unwrap());
    let mempool = Arc::new(Mempool::with_capacity(50_000));
    setup_genesis(&chain, &state);

    let faucet = devnet_faucet_keypair();
    let chain_id = 7778u64;
    let start = Instant::now();
    for nonce in 0..10_000 {
        let tx = quyn_core::Transaction {
            nonce,
            gas_price: U256::from(1),
            gas_limit: 21_000,
            to: Some(Address::ZERO),
            value: U256::from(1000),
            data: vec![],
            chain_id,
        };
        let signed = sign_transaction(&tx, &faucet).unwrap();
        mempool.insert(signed).unwrap();
    }
    let insert_time = start.elapsed();
    let count = mempool.len().unwrap();
    assert_eq!(count, 10_000);
    let tps = 10_000.0 / insert_time.as_secs_f64();
    eprintln!("Stress 10k: inserted 10000 txs in {:?}, {:.0} insert/s", insert_time, tps);
}

/// Test 3: Mempool eviction preserves nonce ordering (evict by sender)
#[test]
fn stress_eviction_preserves_nonce_ordering() {
    let mempool = Mempool::with_capacity(5);
    let mut addrs = Vec::new();
    for i in 0..3 {
        let mut secret = [0u8; 32];
        secret[0] = i as u8 + 1;
        let kp = quyn_wallet::KeyPair::from_secret(secret).unwrap();
        addrs.push(kp);
    }
    let chain_id = 7778u64;
    for (i, kp) in addrs.iter().enumerate() {
        for nonce in 0..3 {
            let tx = quyn_core::Transaction {
                nonce,
                gas_price: U256::from((i + 1) as u64),
                gas_limit: 21_000,
                to: Some(Address::ZERO),
                value: U256::ZERO,
                data: vec![],
                chain_id,
            };
            mempool.insert(sign_transaction(&tx, kp).unwrap()).unwrap();
        }
    }
    assert!(mempool.len().unwrap() <= 5);
    let best = mempool.get_best(20).unwrap();
    for tx in &best {
        let sender = tx.sender().unwrap();
        let nonces: Vec<_> = best.iter().filter(|t| t.sender().ok() == Some(sender)).map(|t| t.nonce()).collect();
        let mut sorted = nonces.clone();
        sorted.sort();
        assert_eq!(nonces, sorted, "Nonces must be ordered per sender");
    }
}
