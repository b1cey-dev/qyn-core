//! Core error types.

use alloy_primitives::{Address, B256};
use thiserror::Error;

/// Errors that can occur in the Quyn core.
#[derive(Error, Debug)]
pub enum CoreError {
    #[error("Invalid block: {0}")]
    InvalidBlock(String),

    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),

    #[error("Chain validation failed: {0}")]
    ChainValidation(String),

    /// Double-sign detected: validator signed two different blocks at the same height.
    #[error("double-sign: validator {validator:?} at height {height}")]
    DoubleSign {
        validator: Address,
        height: u64,
        first_block: B256,
        second_block: B256,
    },

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Mempool error: {0}")]
    Mempool(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),
}
