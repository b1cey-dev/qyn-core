//! Quyn Virtual Machine - EVM compatible execution via revm.

pub mod abi;
pub mod error;
pub mod executor;
pub mod state_db_adapter;

pub use error::VmError;
pub use executor::{block_env, execute_call, execute_tx, ExecutionResult, Log, QYN_CHAIN_ID};
pub use state_db_adapter::StateDBAdapter;
