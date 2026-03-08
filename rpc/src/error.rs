//! RPC error types.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum RpcError {
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Method not found: {0}")]
    MethodNotFound(String),

    #[error("Internal error: {0}")]
    Internal(String),
}
