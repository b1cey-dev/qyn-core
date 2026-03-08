//! Transaction signing (EIP-155 style).

use crate::error::WalletError;
use crate::keys::KeyPair;
use quyn_core::{SignedTransaction, Transaction};

/// Sign an unsigned transaction with the given keypair. chain_id is included in signature.
pub fn sign_transaction(
    tx: &Transaction,
    keypair: &KeyPair,
) -> Result<SignedTransaction, WalletError> {
    let hash = tx.signing_hash();
    let (r, s, v) = keypair.sign_hash(hash.as_slice().try_into().map_err(|_| WalletError::Signing("hash length".into()))?)?;
    Ok(SignedTransaction {
        transaction: tx.clone(),
        r,
        s,
        v,
        hash_override: None,
    })
}
