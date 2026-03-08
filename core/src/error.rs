//! Core error types.

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

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Mempool error: {0}")]
    Mempool(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),
}
