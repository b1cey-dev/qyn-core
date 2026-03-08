//! Key generation and Ethereum-style address derivation.

use crate::error::WalletError;
use alloy_primitives::{keccak256, Address};
use secp256k1::Secp256k1;
use std::str::FromStr;

/// Secp256k1 keypair; secret key is 32 bytes.
#[derive(Clone)]
pub struct KeyPair {
    pub secret: [u8; 32],
    pub public: [u8; 65],
}

impl KeyPair {
    pub fn from_secret(secret: [u8; 32]) -> Result<Self, WalletError> {
        let secp = Secp256k1::new();
        let sk = secp256k1::SecretKey::from_slice(&secret)
            .map_err(|e| WalletError::InvalidKey(e.to_string()))?;
        let pk = sk.public_key(&secp);
        let uncompressed = pk.serialize_uncompressed();
        let mut public = [0u8; 65];
        public.copy_from_slice(&uncompressed);
        Ok(Self { secret, public })
    }

    /// Ethereum address: Keccak256(public_key[1..65])[12..32].
    pub fn address(&self) -> Address {
        let hash = keccak256(&self.public[1..65]);
        Address::from_slice(&hash[12..32])
    }

    pub fn sign_hash(&self, hash: &[u8; 32]) -> Result<([u8; 32], [u8; 32], u8), WalletError> {
        let secp = Secp256k1::new();
        let sk = secp256k1::SecretKey::from_slice(&self.secret)
            .map_err(|e| WalletError::InvalidKey(e.to_string()))?;
        let msg = secp256k1::Message::from_digest_slice(hash)
            .map_err(|e| WalletError::Signing(e.to_string()))?;
        let sig = secp.sign_ecdsa_recoverable(&msg, &sk);
        let (recovery_id, compact) = sig.serialize_compact();
        let r: [u8; 32] = compact[0..32].try_into().unwrap();
        let s: [u8; 32] = compact[32..64].try_into().unwrap();
        let v = recovery_id.to_i32() as u8;
        Ok((r, s, v))
    }
}

/// Parse hex address string (0x...).
pub fn address_from_str(s: &str) -> Result<Address, WalletError> {
    Address::from_str(s).map_err(|e| WalletError::InvalidKey(e.to_string()))
}
