//! Adapter from quyn_core::StateDB to revm Database + DatabaseCommit for block execution.

use crate::executor::to_revm_u256;
use alloy_primitives::{keccak256, Address, B256, U256};
use quyn_core::StateDB;
use revm::db::{Database, DatabaseCommit};
use revm::primitives::{Account, AccountInfo, Bytecode, HashMap, KECCAK_EMPTY};
use std::cell::RefCell;

fn from_revm_u256(u: revm::primitives::U256) -> U256 {
    let bytes: [u8; 32] = u.to_be_bytes::<32>().as_slice().try_into().unwrap();
    U256::from_be_bytes::<32>(bytes)
}

/// Wraps StateDB + code-by-hash cache for revm Database.
pub struct StateDBAdapter<'a> {
    pub state: &'a StateDB,
    code_cache: RefCell<HashMap<B256, Bytecode>>,
}

impl<'a> StateDBAdapter<'a> {
    pub fn new(state: &'a StateDB) -> Self {
        Self {
            state,
            code_cache: RefCell::new(HashMap::new()),
        }
    }
}

impl Database for StateDBAdapter<'_> {
    type Error = quyn_core::CoreError;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        let balance = self.state.get_balance(&address)?;
        let nonce = self.state.get_nonce(&address)?;
        let code_bytes = self.state.get_code(&address)?;
        let code_hash = if code_bytes.is_empty() {
            KECCAK_EMPTY
        } else {
            B256::from(keccak256(&code_bytes))
        };
        let code = if code_bytes.is_empty() {
            None
        } else {
            let bc = Bytecode::new_raw(revm::primitives::Bytes::from(code_bytes.clone()));
            self.code_cache.borrow_mut().insert(code_hash, bc.clone());
            Some(bc)
        };
        Ok(Some(AccountInfo {
            balance: to_revm_u256(balance),
            nonce,
            code_hash,
            code,
        }))
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        if let Some(bc) = self.code_cache.borrow().get(&code_hash) {
            return Ok(bc.clone());
        }
        Err(quyn_core::CoreError::Storage(format!(
            "code_by_hash {} not in cache",
            hex::encode(code_hash.as_slice())
        )))
    }

    fn storage(&mut self, address: Address, index: revm::primitives::U256) -> Result<revm::primitives::U256, Self::Error> {
        let slot = from_revm_u256(index);
        let val = self.state.get_storage(&address, slot)?;
        Ok(to_revm_u256(val))
    }

    fn block_hash(&mut self, _number: revm::primitives::U256) -> Result<B256, Self::Error> {
        Ok(B256::ZERO)
    }
}

impl DatabaseCommit for StateDBAdapter<'_> {
    fn commit(&mut self, changes: HashMap<Address, Account>) {
        for (address, account) in changes {
            let info = &account.info;
            let balance = from_revm_u256(info.balance);
            let _ = self.state.set_balance(&address, balance);
            let _ = self.state.set_nonce(&address, info.nonce);
            if let Some(ref code) = info.code {
                let bytes = code.original_bytes();
                if !bytes.is_empty() {
                    let _ = self.state.set_code(&address, bytes.as_ref());
                    self.code_cache.borrow_mut().insert(info.code_hash, code.clone());
                }
            }
            for (slot, storage_slot) in account.storage.iter() {
                let s = from_revm_u256(*slot);
                let v = from_revm_u256(storage_slot.present_value());
                let _ = self.state.set_storage(&address, s, v);
            }
        }
    }
}
