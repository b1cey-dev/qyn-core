//! Consensus error types.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConsensusError {
    #[error("Invalid validator: {0}")]
    InvalidValidator(String),

    #[error("Slashing: {0}")]
    Slashing(String),

    #[error("Staking: {0}")]
    Staking(String),

    #[error("Block production: {0}")]
    BlockProduction(String),
}
