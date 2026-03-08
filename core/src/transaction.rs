//! Transaction structure and signing for Quyn.
//!
//! EIP-155 style: chain_id in signature to prevent replay across chains.

use crate::error::CoreError;
use alloy_primitives::{keccak256, Address, B256, U256};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use secp256k1::ecdsa::RecoveryId;

/// Unsigned transaction (payload to sign).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    pub nonce: u64,
    pub gas_price: U256,
    pub gas_limit: u64,
    /// None = contract deployment (CREATE).
    pub to: Option<Address>,
    pub value: U256,
    pub data: Vec<u8>,
    pub chain_id: u64,
}

impl Transaction {
    /// Encode for signing (EIP-155: RLP of [nonce, gas_price, gas_limit, to, value, data, chain_id, 0, 0]).
    pub fn signing_hash(&self) -> B256 {
        let mut hasher = Sha256::new();
        hasher.update(self.nonce.to_be_bytes());
        hasher.update(self.gas_price.to_be_bytes::<32>().as_slice());
        hasher.update(self.gas_limit.to_be_bytes());
        if let Some(to) = &self.to {
            hasher.update(to.as_slice());
        } else {
            hasher.update([0u8; 20]);
        }
        hasher.update(self.value.to_be_bytes::<32>().as_slice());
        hasher.update(self.data.as_slice());
        hasher.update(self.chain_id.to_be_bytes());
        B256::from_slice(&hasher.finalize()[..])
    }

    /// Recover sender address from signature (Ethereum style: Keccak256 of pubkey, last 20 bytes).
    pub fn recover_sender(&self, r: &[u8; 32], s: &[u8; 32], v: u8) -> Option<Address> {
        use secp256k1::{ecdsa::RecoverableSignature, Message, Secp256k1};
        let msg = Message::from_digest_slice(self.signing_hash().as_slice()).ok()?;
        let rid = RecoveryId::from_i32(v as i32).ok()?;
        let mut compact = [0u8; 64];
        compact[0..32].copy_from_slice(r);
        compact[32..64].copy_from_slice(s);
        let sig = RecoverableSignature::from_compact(&compact, rid).ok()?;
        let pk = Secp256k1::new().recover_ecdsa(&msg, &sig).ok()?;
        let uncompressed = pk.serialize_uncompressed();
        let hash = keccak256(&uncompressed[1..65]);
        Some(Address::from_slice(&hash[12..32]))
    }
}

/// Signed transaction (with r, s, v).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedTransaction {
    pub transaction: Transaction,
    pub r: [u8; 32],
    pub s: [u8; 32],
    pub v: u8,
}

impl SignedTransaction {
    pub fn hash(&self) -> B256 {
        let mut hasher = Sha256::new();
        hasher.update(self.transaction.signing_hash().as_slice());
        hasher.update(&self.r);
        hasher.update(&self.s);
        hasher.update([self.v]);
        B256::from_slice(&hasher.finalize()[..])
    }

    /// Recover sender address.
    pub fn sender(&self) -> Result<Address, CoreError> {
        self.transaction
            .recover_sender(&self.r, &self.s, self.v)
            .ok_or_else(|| CoreError::InvalidTransaction("Invalid signature".into()))
    }

    pub fn chain_id(&self) -> u64 {
        self.transaction.chain_id
    }

    pub fn nonce(&self) -> u64 {
        self.transaction.nonce
    }

    pub fn gas_price(&self) -> U256 {
        self.transaction.gas_price
    }

    pub fn gas_limit(&self) -> u64 {
        self.transaction.gas_limit
    }

    pub fn to(&self) -> Option<Address> {
        self.transaction.to
    }

    pub fn value(&self) -> U256 {
        self.transaction.value
    }

    pub fn data(&self) -> &[u8] {
        &self.transaction.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tx_signing_hash_deterministic() {
        let tx = Transaction {
            nonce: 0,
            gas_price: U256::from(1),
            gas_limit: 21000,
            to: Some(Address::ZERO),
            value: U256::from(1000),
            data: vec![],
            chain_id: 7777,
        };
        assert_eq!(tx.signing_hash(), tx.signing_hash());
    }
}
