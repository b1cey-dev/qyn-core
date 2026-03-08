//! Network error types.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum NetworkError {
    #[error("Connection failed: {0}")]
    Connection(String),

    #[error("Sync error: {0}")]
    Sync(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("IO: {0}")]
    Io(#[from] std::io::Error),
}
