//! HD wallet: BIP-39 mnemonic, BIP-32/BIP-44 derivation.

use crate::error::WalletError;
use crate::keys::KeyPair;
use bip32::XPrv;
use bip39::Mnemonic;
use rand::RngCore;

/// Quyn derivation path: m/44'/7777'/0'/0/index
const DERIVATION_PATH_PREFIX: &str = "m/44'/7777'/0'/0/";

/// Generate a new BIP-39 mnemonic (12 words = 128 bits entropy).
pub fn generate_mnemonic() -> Result<String, WalletError> {
    let mut entropy = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut entropy);
    let mnemonic = Mnemonic::from_entropy(&entropy)
        .map_err(|e| WalletError::InvalidMnemonic(e.to_string()))?;
    Ok(mnemonic.to_string())
}

/// Derive keypair from mnemonic and index (path: m/44'/7777'/0'/0/index).
pub fn derive_keypair(mnemonic_phrase: &str, index: u32) -> Result<KeyPair, WalletError> {
    let mnemonic = Mnemonic::parse(mnemonic_phrase)
        .map_err(|e| WalletError::InvalidMnemonic(e.to_string()))?;
    let seed: [u8; 64] = mnemonic.to_seed("");
    let path = format!("{}{}", DERIVATION_PATH_PREFIX, index);
    let xprv = XPrv::derive_from_path(
        &seed,
        &path
            .parse()
            .map_err(|e: bip32::Error| WalletError::InvalidMnemonic(e.to_string()))?,
    )
    .map_err(|e| WalletError::InvalidMnemonic(e.to_string()))?;
    let secret: [u8; 32] = xprv.private_key().to_bytes().into();
    KeyPair::from_secret(secret)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_mnemonic_12_words() {
        let m = generate_mnemonic().unwrap();
        let words: Vec<_> = m.split_whitespace().collect();
        assert_eq!(words.len(), 12);
    }
}
