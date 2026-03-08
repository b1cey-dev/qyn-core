//! VM error types.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum VmError {
    #[error("Execution reverted: {0}")]
    Revert(String),

    #[error("Out of gas")]
    OutOfGas,

    #[error("Invalid contract: {0}")]
    InvalidContract(String),

    #[error("ABI error: {0}")]
    Abi(String),
}

