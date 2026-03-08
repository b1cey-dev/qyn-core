//! Node runner: full node that runs chain, state, mempool, and RPC.

use quyn_core::{ChainDB, Mempool, StateDB};
use std::path::Path;
use std::sync::Arc;

/// Full node state: chain, state DB, mempool.
pub struct FullNode {
    pub chain: Arc<ChainDB>,
    pub state: Arc<StateDB>,
    pub mempool: Arc<Mempool>,
}

impl FullNode {
    pub fn open(data_dir: &Path) -> Result<Self, quyn_core::CoreError> {
        let chain_path = data_dir.join("chain");
        let state_path = data_dir.join("state");
        std::fs::create_dir_all(&chain_path).ok();
        std::fs::create_dir_all(&state_path).ok();
        let chain = Arc::new(ChainDB::open(&chain_path)?);
        let state = Arc::new(StateDB::open(&state_path)?);
        let mempool = Arc::new(Mempool::new());
        Ok(Self { chain, state, mempool })
    }
}
