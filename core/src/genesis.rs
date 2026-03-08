//! Genesis block configuration and initial allocations.

use crate::error::CoreError;
use crate::state::StateDB;
use alloy_primitives::{Address, U256};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 18 decimals.
pub const DECIMALS: u32 = 18;
/// 1 QYN = 10^18 units.
pub const ONE_QYN: u128 = 10_u128.pow(18);

/// Genesis allocation: address -> balance (in wei/smallest unit).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GenesisAlloc(pub HashMap<String, serde_json::Value>);

/// Genesis config (from genesis.json).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenesisConfig {
    pub chain_id: u64,
    pub network_name: String,
    pub timestamp: u64,
    pub gas_limit: u64,
    #[serde(default)]
    pub alloc: HashMap<String, String>,
    #[serde(default)]
    pub validators: Vec<String>,
}

/// Apply genesis allocations to state DB.
pub fn apply_genesis_alloc(state: &StateDB, alloc: &HashMap<String, String>) -> Result<(), CoreError> {
    for (addr_hex, balance_str) in alloc {
        let addr = parse_address(addr_hex)?;
        let balance = parse_u256(balance_str)?;
        state.set_balance(&addr, balance)?;
        state.set_nonce(&addr, 0)?;
    }
    Ok(())
}

fn parse_address(s: &str) -> Result<Address, CoreError> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(s).map_err(|e| CoreError::Serialization(e.to_string()))?;
    if bytes.len() != 20 {
        return Err(CoreError::Serialization("address must be 20 bytes".into()));
    }
    let mut arr = [0u8; 20];
    arr.copy_from_slice(&bytes);
    Ok(Address::from(arr))
}

fn parse_u256(s: &str) -> Result<U256, CoreError> {
    let s = s.trim_start_matches("0x").trim_start_matches("0X");
    U256::from_str_radix(s, 16).map_err(|_| CoreError::Serialization("invalid hex balance".into()))
}

/// Standard genesis allocations (plan): 40% public, 20% team, 20% reserve, 10% marketing, 10% validators.
pub fn default_mainnet_alloc() -> HashMap<String, String> {
    let mut m = HashMap::new();
    let one = 10_u128.pow(18);
    m.insert(
        "0x0000000000000000000000000000000000000001".into(),
        format!("0x{:x}", 400_000_000u128 * one),
    );
    m.insert(
        "0x0000000000000000000000000000000000000002".into(),
        format!("0x{:x}", 200_000_000u128 * one),
    );
    m.insert(
        "0x0000000000000000000000000000000000000003".into(),
        format!("0x{:x}", 200_000_000u128 * one),
    );
    m.insert(
        "0x0000000000000000000000000000000000000004".into(),
        format!("0x{:x}", 100_000_000u128 * one),
    );
    m.insert(
        "0x0000000000000000000000000000000000000005".into(),
        format!("0x{:x}", 100_000_000u128 * one),
    );
    m
}

/// Fee burn: 50% of gas fees burned (subtract from supply). Returns (amount_to_burn, amount_to_proposer).
pub fn split_fees(total_gas_fee: U256) -> (U256, U256) {
    let half = total_gas_fee / U256::from(2);
    let proposer = total_gas_fee.saturating_sub(half);
    (half, proposer)
}
