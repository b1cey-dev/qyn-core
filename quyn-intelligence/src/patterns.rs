//! Pattern database for suspicious addresses (governance-updatable).

use alloy_primitives::Address;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Database of flagged/suspicious address patterns. Deterministic: same input => same result.
/// Can be updated by governance; starts empty.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PatternDatabase {
    /// Addresses flagged as suspicious (stored as 20-byte arrays for serde).
    #[serde(with = "set_serde")]
    inner: HashSet<[u8; 20]>,
}

mod set_serde {
    use std::collections::HashSet;
    use serde::{Deserialize, Serialize};

    pub fn serialize<S>(set: &HashSet<[u8; 20]>, s: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let vec: Vec<String> = set.iter().map(|b| hex::encode(b)).collect();
        vec.serialize(s)
    }

    pub fn deserialize<'de, D>(d: D) -> Result<HashSet<[u8; 20]>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let vec: Vec<String> = Vec::deserialize(d)?;
        let mut set = HashSet::new();
        for s in vec {
            if let Ok(b) = hex::decode(s.trim_start_matches("0x")) {
                if b.len() == 20 {
                    let mut arr = [0u8; 20];
                    arr.copy_from_slice(&b);
                    set.insert(arr);
                }
            }
        }
        Ok(set)
    }
}

impl PatternDatabase {
    pub fn new() -> Self {
        Self {
            inner: HashSet::new(),
        }
    }

    /// Check if recipient address matches any known suspicious pattern.
    pub fn is_suspicious(&self, recipient: &Address) -> bool {
        let arr: [u8; 20] = recipient.0 .0;
        self.inner.contains(&arr)
    }

    /// Add address to suspicious list (governance / admin).
    pub fn add_suspicious(&mut self, addr: &Address) {
        let arr: [u8; 20] = addr.0 .0;
        self.inner.insert(arr);
    }

    /// Remove address from suspicious list.
    pub fn remove_suspicious(&mut self, addr: &Address) {
        let arr: [u8; 20] = addr.0 .0;
        self.inner.remove(&arr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_db_suspicious() {
        let mut db = PatternDatabase::new();
        let addr = Address::from_slice(&[1u8; 20]);
        assert!(!db.is_suspicious(&addr));
        db.add_suspicious(&addr);
        assert!(db.is_suspicious(&addr));
        db.remove_suspicious(&addr);
        assert!(!db.is_suspicious(&addr));
    }
}
