//! Quyn Core - Blockchain engine: blocks, transactions, chain state, mempool, validation.

pub mod block;
pub mod chain;
pub mod error;
pub mod fork;
pub mod genesis;
pub mod mempool;
pub mod state;
pub mod transaction;
pub mod types;
pub mod validation;

pub use block::{Block, BlockBody, BlockHeader};
pub use chain::{accept_block, ChainDB};
pub use error::CoreError;
pub use fork::{canonical_head, common_ancestor, reorg_blocks};
pub use genesis::{apply_genesis_alloc, default_mainnet_alloc, split_fees, GenesisConfig, DECIMALS, ONE_QYN};
pub use mempool::Mempool;
pub use state::{apply_transfer, StateDB};
pub use transaction::{SignedTransaction, Transaction};
pub use types::*;
pub use validation::{validate_block, validate_block_header, validate_tx_against_state, validate_tx_basic};
