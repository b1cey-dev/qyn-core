//! Quyn Proof of Stake consensus: validators, staking, slashing, rewards, delegation.

pub mod error;
pub mod rewards;
pub mod slashing;
pub mod validator_set;

pub use error::ConsensusError;
pub use rewards::{block_reward_amount, distribute_block_reward};
pub use slashing::{SlashEvidence, SlashReason, slash_penalty_bps};
pub use validator_set::{
    select_proposer, Delegation, ValidatorInfo, ValidatorSet, EPOCH_LENGTH, MAX_VALIDATORS, MIN_STAKE,
};
