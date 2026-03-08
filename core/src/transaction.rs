//! Transaction structure and signing for Quyn.
//!
//! EIP-155: chain_id in RLP signing payload and in v for replay protection.
//! Transaction hash is Keccak256(RLP(signed_tx)) for Ethereum compatibility.

use crate::error::CoreError;
use alloy_primitives::{keccak256, Address, B256, U256};
use rlp::RlpStream;
use serde::{Deserialize, Serialize};
use secp256k1::ecdsa::RecoveryId;

/// Minimal big-endian bytes for RLP (no leading zeros).
fn u256_to_rlp_bytes(u: &U256) -> Vec<u8> {
    let b = u.to_be_bytes::<32>();
    let start = b.iter().position(|&x| x != 0).unwrap_or(32);
    b[start..].to_vec()
}

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
    /// Encode for signing (EIP-155: RLP of [nonce, gas_price, gas_limit, to, value, data, chain_id, 0, 0]), then Keccak256.
    pub fn signing_hash(&self) -> B256 {
        let mut stream = RlpStream::new_list(9);
        stream.append(&self.nonce);
        stream.append(&u256_to_rlp_bytes(&self.gas_price).as_slice());
        stream.append(&self.gas_limit);
        let to_bytes: &[u8] = self.to.as_ref().map(|a| a.as_slice()).unwrap_or(&[]);
        stream.append(&to_bytes);
        stream.append(&u256_to_rlp_bytes(&self.value).as_slice());
        stream.append(&self.data.as_slice());
        stream.append(&self.chain_id);
        stream.append_empty_data();
        stream.append_empty_data();
        keccak256(stream.out().as_ref())
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
    /// Transaction hash: Keccak256(RLP([nonce, gas_price, gas_limit, to, value, data, v, r, s])) for Ethereum compatibility.
    pub fn hash(&self) -> B256 {
        let mut stream = RlpStream::new_list(9);
        let t = &self.transaction;
        stream.append(&t.nonce);
        stream.append(&u256_to_rlp_bytes(&t.gas_price).as_slice());
        stream.append(&t.gas_limit);
        let to_bytes: &[u8] = t.to.as_ref().map(|a| a.as_slice()).unwrap_or(&[]);
        stream.append(&to_bytes);
        stream.append(&u256_to_rlp_bytes(&t.value).as_slice());
        stream.append(&t.data.as_slice());
        stream.append(&self.v);
        stream.append(&self.r.as_slice());
        stream.append(&self.s.as_slice());
        keccak256(stream.out().as_ref())
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
