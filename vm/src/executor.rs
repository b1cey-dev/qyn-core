//! QVM executor: wrap revm for transaction and contract execution.

use crate::error::VmError;
use alloy_primitives::{Address, B256, U256};
use quyn_core::SignedTransaction;
use revm::db::{Database, DatabaseCommit};
use revm::primitives::{BlockEnv, CfgEnv, Env, EnvWithHandlerCfg, HandlerCfg, SpecId, TxEnv};

/// Chain ID for Quyn mainnet (used in EVM when chain_id not passed).
pub const QYN_CHAIN_ID: u64 = 7777;

/// Result of executing a transaction.
#[derive(Debug)]
pub struct ExecutionResult {
    pub gas_used: u64,
    pub success: bool,
    pub output: Vec<u8>,
    pub logs: Vec<Log>,
}

#[derive(Clone, Debug)]
pub struct Log {
    pub address: Address,
    pub topics: Vec<B256>,
    pub data: Vec<u8>,
}

pub(crate) fn to_revm_u256(u: U256) -> revm::primitives::U256 {
    let bytes: [u8; 32] = u.to_be_bytes::<32>().as_slice().try_into().unwrap();
    revm::primitives::U256::from_be_bytes::<32>(bytes)
}

/// Execute a signed transaction against the given state. State is modified in place.
/// Use transact_commit so that DB implementing DatabaseCommit (e.g. StateDBAdapter) is persisted.
/// chain_id must match the node (e.g. 7778 for testnet, 7777 for mainnet).
pub fn execute_tx<DB>(db: &mut DB, tx: &SignedTransaction, block_env: &BlockEnv, chain_id: u64) -> Result<ExecutionResult, VmError>
where
    DB: Database<Error = quyn_core::CoreError> + DatabaseCommit,
{
    let sender = tx.sender().map_err(|e| VmError::InvalidContract(e.to_string()))?;
    let to = tx.to();
    let value = to_revm_u256(tx.value());
    let data = revm::primitives::Bytes::from(tx.data().to_vec());
    let gas_limit = tx.gas_limit();
    let gas_price = to_revm_u256(tx.gas_price());

    let transact_to = match to {
        Some(addr) => revm::primitives::TransactTo::Call(addr),
        None => revm::primitives::TransactTo::create(),
    };

    let tx_env = TxEnv {
        caller: sender,
        gas_limit,
        gas_price,
        gas_priority_fee: None,
        transact_to,
        value,
        data,
        chain_id: Some(chain_id),
        nonce: Some(tx.nonce()),
        access_list: vec![],
        blob_hashes: vec![],
        max_fee_per_blob_gas: None,
    };
    let mut cfg_env = CfgEnv::default();
    cfg_env.chain_id = chain_id;
    let env = Env {
        cfg: cfg_env,
        block: block_env.clone(),
        tx: tx_env,
    };
    let handler_cfg = HandlerCfg::new(SpecId::CANCUN);
    let env_with_cfg = EnvWithHandlerCfg {
        env: Box::new(env),
        handler_cfg,
    };
    let mut evm = revm::Evm::builder()
        .with_db(db)
        .with_env_with_handler_cfg(env_with_cfg)
        .build();

    let result = evm.transact_commit().map_err(|e| VmError::Revert(format!("{:?}", e)))?;
    let gas_used = result.gas_used();
    let success = result.is_success();
    let output = result
        .output()
        .map(|o| o.as_ref().to_vec())
        .unwrap_or_default();
    let logs = result
        .logs()
        .iter()
        .map(|l| Log {
            address: l.address,
            topics: l.topics().to_vec(),
            data: l.data.data.to_vec(),
        })
        .collect();

    Ok(ExecutionResult {
        gas_used,
        success,
        output,
        logs,
    })
}

/// Execute a call without committing state (for eth_call). Returns output or error.
pub fn execute_call<DB>(db: &mut DB, tx: &SignedTransaction, block_env: &BlockEnv, chain_id: u64) -> Result<Vec<u8>, VmError>
where
    DB: Database<Error = quyn_core::CoreError>,
{
    let sender = tx.sender().map_err(|e| VmError::InvalidContract(e.to_string()))?;
    let to = tx.to();
    let value = to_revm_u256(tx.value());
    let data = revm::primitives::Bytes::from(tx.data().to_vec());
    let gas_limit = tx.gas_limit();
    let gas_price = to_revm_u256(tx.gas_price());
    let transact_to = match to {
        Some(addr) => revm::primitives::TransactTo::Call(addr),
        None => revm::primitives::TransactTo::create(),
    };
    let tx_env = TxEnv {
        caller: sender,
        gas_limit,
        gas_price,
        gas_priority_fee: None,
        transact_to,
        value,
        data,
        chain_id: Some(chain_id),
        nonce: Some(tx.nonce()),
        access_list: vec![],
        blob_hashes: vec![],
        max_fee_per_blob_gas: None,
    };
    let mut cfg_env = CfgEnv::default();
    cfg_env.chain_id = chain_id;
    let env = Env {
        cfg: cfg_env,
        block: block_env.clone(),
        tx: tx_env,
    };
    let handler_cfg = HandlerCfg::new(SpecId::CANCUN);
    let env_with_cfg = EnvWithHandlerCfg {
        env: Box::new(env),
        handler_cfg,
    };
    let mut evm = revm::Evm::builder()
        .with_db(db)
        .with_env_with_handler_cfg(env_with_cfg)
        .build();
    let result = evm.transact().map_err(|e| VmError::Revert(format!("{:?}", e)))?;
    let output = result
        .result
        .output()
        .map(|o| o.as_ref().to_vec())
        .unwrap_or_default();
    Ok(output)
}

/// Build a minimal BlockEnv for execution. Coinbase receives gas fees.
pub fn block_env(
    number: u64,
    timestamp: u64,
    gas_limit: u64,
    base_fee: U256,
    coinbase: Address,
) -> BlockEnv {
    BlockEnv {
        number: to_revm_u256(U256::from(number)),
        timestamp: to_revm_u256(U256::from(timestamp)),
        gas_limit: to_revm_u256(U256::from(gas_limit)),
        basefee: to_revm_u256(base_fee),
        coinbase,
        difficulty: to_revm_u256(U256::ZERO),
        prevrandao: None,
        blob_excess_gas_and_price: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_env_creation() {
        let env = block_env(1, 1000, 30_000_000, U256::ZERO, Address::ZERO);
        assert_eq!(env.number, revm::primitives::U256::from(1));
    }
}
