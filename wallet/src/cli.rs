//! CLI wallet: new, import, balance, send, history, sign-tx.

use crate::hd::{derive_keypair, generate_mnemonic};
use crate::keys::address_from_str;
use clap::Subcommand;
use quyn_core::Transaction;
use alloy_primitives::U256;

#[derive(Subcommand, Debug)]
pub enum WalletCmd {
    /// Generate a new HD wallet (12-word mnemonic)
    New,
    /// Import from mnemonic (args: "word1 word2 ..." [index])
    Import {
        #[arg(long)]
        mnemonic: String,
        #[arg(long, default_value = "0")]
        index: u32,
    },
    /// Sign a transaction (offline). Args: nonce, gas_price, gas_limit, to, value_wei, data_hex, chain_id
    SignTx {
        #[arg(long)]
        nonce: u64,
        #[arg(long)]
        gas_price: String,
        #[arg(long)]
        gas_limit: u64,
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        value: String,
        #[arg(long, default_value = "")]
        data: String,
        #[arg(long, default_value = "7777")]
        chain_id: u64,
    },
}

pub fn run_new() -> Result<String, crate::error::WalletError> {
    let mnemonic = generate_mnemonic()?;
    let kp = derive_keypair(&mnemonic, 0)?;
    let addr = kp.address();
    Ok(format!("Mnemonic: {}\nAddress: 0x{}\n", mnemonic, hex::encode(addr.as_slice())))
}

pub fn run_import(mnemonic: String, index: u32) -> Result<String, crate::error::WalletError> {
    let kp = derive_keypair(&mnemonic, index)?;
    let addr = kp.address();
    Ok(format!("Address: 0x{}\n", hex::encode(addr.as_slice())))
}

pub fn run_sign_tx(
    nonce: u64,
    gas_price: String,
    gas_limit: u64,
    to: Option<String>,
    value: String,
    data: String,
    chain_id: u64,
    mnemonic: &str,
    index: u32,
) -> Result<Vec<u8>, crate::error::WalletError> {
    let kp = derive_keypair(mnemonic, index)?;
    let to_addr = to.as_ref().map(|s| address_from_str(s)).transpose()?;
    let value_u = U256::from_str_radix(value.trim_start_matches("0x"), 16)
        .or_else(|_| value.parse())
        .map_err(|_| crate::error::WalletError::InvalidKey("invalid value".into()))?;
    let gas_price_u = U256::from_str_radix(gas_price.trim_start_matches("0x"), 16)
        .or_else(|_| gas_price.parse())
        .map_err(|_| crate::error::WalletError::InvalidKey("invalid gas_price".into()))?;
    let data_bytes = if data.is_empty() {
        vec![]
    } else {
        hex::decode(data.trim_start_matches("0x")).map_err(|e| crate::error::WalletError::InvalidKey(e.to_string()))?
    };
    let tx = Transaction {
        nonce,
        gas_price: gas_price_u,
        gas_limit,
        to: to_addr,
        value: value_u,
        data: data_bytes,
        chain_id,
    };
    let signed = crate::signing::sign_transaction(&tx, &kp)?;
    let encoded = bincode::serialize(&signed).map_err(|e| crate::error::WalletError::Signing(e.to_string()))?;
    Ok(encoded)
}
