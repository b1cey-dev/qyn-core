//! Wallet error types.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum WalletError {
    #[error("Invalid mnemonic: {0}")]
    InvalidMnemonic(String),

    #[error("Invalid key: {0}")]
    InvalidKey(String),

    #[error("Signing failed: {0}")]
    Signing(String),

    #[error("IO: {0}")]
    Io(#[from] std::io::Error),
}
