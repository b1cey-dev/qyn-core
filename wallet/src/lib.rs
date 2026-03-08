//! Quyn wallet: HD wallet (BIP39/BIP44), keys, signing, CLI.

pub mod cli;
pub mod error;
pub mod hd;
pub mod keys;
pub mod signing;

pub use cli::{run_new, run_sign_tx};
pub use error::WalletError;
pub use hd::{derive_keypair, generate_mnemonic};
pub use keys::{address_from_str, KeyPair};
pub use signing::sign_transaction;

/// Return address (0x-prefixed hex) for mnemonic and index.
pub fn address_for_mnemonic(mnemonic: &str, index: u32) -> Result<String, WalletError> {
    let kp = derive_keypair(mnemonic, index)?;
    Ok(format!("0x{}", hex::encode(kp.address().as_slice())))
}
