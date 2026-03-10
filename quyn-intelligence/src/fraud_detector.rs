//! Fraud detector: analyses a transaction and returns risk score and recommendation.

use alloy_primitives::Address;
use quyn_core::{ChainDB, SignedTransaction, StateDB};

use crate::models::FraudConfig;
use crate::patterns::PatternDatabase;
use crate::risk_scorer::{get_recommendation, FraudAnalysis, FraudFlag};

/// 1 QYN = 10^18 wei
const ONE_QYN_WEI: u128 = 10_u128.pow(18);
/// Dust threshold: 0.001 QYN
const DUST_WEI: u128 = 1_000_000_000_000_000; // 10^15
/// Velocity window: 60 blocks (~3 min)
const VELOCITY_WINDOW_BLOCKS: u64 = 60;
/// Rapid drain window: 10 blocks
const RAPID_DRAIN_WINDOW_BLOCKS: u64 = 10;
/// High velocity: more than 10 tx in 60 blocks
const HIGH_VELOCITY_THRESHOLD: u32 = 10;
/// Extreme velocity: more than 20 tx in 60 blocks
const EXTREME_VELOCITY_THRESHOLD: u32 = 20;
/// Dust attack: more than 50 dust-sized txs
const DUST_ATTACK_COUNT_THRESHOLD: u32 = 50;
/// Rapid drain: more than 5 tx in 10 blocks
const RAPID_DRAIN_TX_THRESHOLD: u32 = 5;
/// Rapid drain: balance decreased more than 50%
const RAPID_DRAIN_BALANCE_PCT: u128 = 50;

pub struct FraudDetector {
    pub pattern_db: PatternDatabase,
    pub config: FraudConfig,
}

impl FraudDetector {
    pub fn new(config: FraudConfig) -> Self {
        Self {
            pattern_db: PatternDatabase::new(),
            config,
        }
    }

    pub fn default_with_config(config: FraudConfig) -> Self {
        Self::new(config)
    }

    /// Analyse a transaction and return fraud analysis. Fully deterministic.
    pub fn analyse_transaction(
        &self,
        tx: &SignedTransaction,
        chain: &ChainDB,
        state: &StateDB,
        block_number: u64,
    ) -> Result<FraudAnalysis, quyn_core::error::CoreError> {
        let mut score: u8 = 0;
        let mut flags: Vec<FraudFlag> = vec![];

        let sender = tx.sender().map_err(|e| quyn_core::error::CoreError::InvalidTransaction(e.to_string()))?;

        // CHECK 1: New wallet large transfer
        score += self.check_new_wallet(tx, state, block_number, &mut flags);

        // CHECK 2: Velocity
        score += self.check_velocity(chain, state, &sender, block_number, &mut flags)?;

        // CHECK 3: Large value / balance drain
        score += self.check_large_value(tx, state, &mut flags)?;

        // CHECK 4: Round number
        score += self.check_round_numbers(tx, &mut flags);

        // CHECK 5: Dust attack
        score += self.check_dust_attack(chain, state, tx, block_number, &mut flags)?;

        // CHECK 6: Rapid wallet drain
        score += self.check_rapid_drain(chain, state, tx, block_number, &mut flags)?;

        // CHECK 7: Known suspicious patterns (recipient)
        score += self.check_known_patterns(tx, &mut flags);

        let score = score.min(100);
        let recommendation = get_recommendation(score, &self.config);
        let hash = tx.hash();
        let transaction_hash: [u8; 32] = hash.0;

        Ok(FraudAnalysis {
            transaction_hash,
            risk_score: score,
            flags,
            recommendation,
            timestamp: block_number,
        })
    }

    fn check_new_wallet(
        &self,
        tx: &SignedTransaction,
        state: &StateDB,
        _block_number: u64,
        flags: &mut Vec<FraudFlag>,
    ) -> u8 {
        let sender = match tx.sender() {
            Ok(s) => s,
            Err(_) => return 0,
        };
        let nonce = state.get_nonce(&sender).unwrap_or(0);
        // First tx from this wallet: nonce is 0 (they haven't confirmed any tx yet)
        if nonce != 0 {
            return 0;
        }
        let value_wei = tx.value().to::<u128>();
        if value_wei >= self.config.new_wallet_large_tx {
            flags.push(FraudFlag::NewWalletLargeTransfer);
            return 25;
        }
        0
    }

    fn check_velocity(
        &self,
        chain: &ChainDB,
        _state: &StateDB,
        sender: &Address,
        block_number: u64,
        flags: &mut Vec<FraudFlag>,
    ) -> Result<u8, quyn_core::error::CoreError> {
        if block_number < 2 {
            return Ok(0);
        }
        let start = block_number.saturating_sub(VELOCITY_WINDOW_BLOCKS);
        let mut count = 0u32;
        for n in start..block_number {
            if let Ok(Some(block)) = chain.get_block_by_number(n) {
                for tx in &block.body.transactions {
                    if tx.sender().ok().as_ref() == Some(sender) {
                        count += 1;
                    }
                }
            }
        }
        if count > EXTREME_VELOCITY_THRESHOLD {
            flags.push(FraudFlag::ExtremeVelocity);
            return Ok(40);
        }
        if count > HIGH_VELOCITY_THRESHOLD {
            flags.push(FraudFlag::HighVelocity);
            return Ok(20);
        }
        Ok(0)
    }

    fn check_large_value(
        &self,
        tx: &SignedTransaction,
        state: &StateDB,
        flags: &mut Vec<FraudFlag>,
    ) -> Result<u8, quyn_core::error::CoreError> {
        let sender = tx.sender().map_err(|e| quyn_core::error::CoreError::InvalidTransaction(e.to_string()))?;
        let balance = state.get_balance(&sender)?;
        let balance_wei = balance.to::<u128>();
        let value_wei = tx.value().to::<u128>();
        if balance_wei == 0 {
            return Ok(0);
        }
        if value_wei >= balance_wei {
            flags.push(FraudFlag::FullBalanceDrain);
            return Ok(25);
        }
        // value > 80% of balance
        if value_wei * 100 >= balance_wei * 80 {
            flags.push(FraudFlag::LargeBalanceDrain);
            return Ok(15);
        }
        Ok(0)
    }

    fn check_round_numbers(&self, tx: &SignedTransaction, flags: &mut Vec<FraudFlag>) -> u8 {
        let value_wei = tx.value().to::<u128>();
        if value_wei == 0 {
            return 0;
        }
        // Round in QYN: exact multiples of 1 QYN that are "round" (1000, 10000, 100000, etc.)
        if value_wei % ONE_QYN_WEI != 0 {
            return 0;
        }
        let qyn = value_wei / ONE_QYN_WEI;
        let round = [1u128, 10, 100, 1000, 10000, 100000, 1000000, 10000000, 100000000];
        if round.contains(&qyn) {
            flags.push(FraudFlag::RoundNumber);
            return 5;
        }
        0
    }

    fn check_dust_attack(
        &self,
        chain: &ChainDB,
        _state: &StateDB,
        tx: &SignedTransaction,
        block_number: u64,
        flags: &mut Vec<FraudFlag>,
    ) -> Result<u8, quyn_core::error::CoreError> {
        let value_wei = tx.value().to::<u128>();
        if value_wei >= DUST_WEI {
            return Ok(0);
        }
        let sender = tx.sender().map_err(|e| quyn_core::error::CoreError::InvalidTransaction(e.to_string()))?;
        if block_number < 2 {
            return Ok(0);
        }
        let start = block_number.saturating_sub(VELOCITY_WINDOW_BLOCKS * 2); // look back further for dust count
        let mut dust_count = 0u32;
        for n in start..block_number {
            if let Ok(Some(block)) = chain.get_block_by_number(n) {
                for t in &block.body.transactions {
                    if t.sender().ok().as_ref() == Some(&sender) && t.value().to::<u128>() < DUST_WEI {
                        dust_count += 1;
                    }
                }
            }
        }
        if dust_count >= DUST_ATTACK_COUNT_THRESHOLD {
            flags.push(FraudFlag::DustAttack);
            return Ok(30);
        }
        Ok(0)
    }

    fn check_rapid_drain(
        &self,
        chain: &ChainDB,
        state: &StateDB,
        tx: &SignedTransaction,
        block_number: u64,
        flags: &mut Vec<FraudFlag>,
    ) -> Result<u8, quyn_core::error::CoreError> {
        if block_number < RAPID_DRAIN_WINDOW_BLOCKS + 1 {
            return Ok(0);
        }
        let sender = tx.sender().map_err(|e| quyn_core::error::CoreError::InvalidTransaction(e.to_string()))?;
        let start = block_number.saturating_sub(RAPID_DRAIN_WINDOW_BLOCKS);
        let mut tx_count = 0u32;
        let mut value_sent = 0u128;
        for n in start..block_number {
            if let Ok(Some(block)) = chain.get_block_by_number(n) {
                for t in &block.body.transactions {
                    if t.sender().ok().as_ref() == Some(&sender) {
                        tx_count += 1;
                        value_sent += t.value().to::<u128>();
                    }
                }
            }
        }
        if tx_count < RAPID_DRAIN_TX_THRESHOLD {
            return Ok(0);
        }
        let current_balance = state.get_balance(&sender)?.to::<u128>();
        let balance_before_window = current_balance + value_sent;
        if balance_before_window == 0 {
            return Ok(0);
        }
        let pct_drained = value_sent * 100 / balance_before_window;
        if pct_drained >= RAPID_DRAIN_BALANCE_PCT {
            flags.push(FraudFlag::RapidWalletDrain);
            return Ok(35);
        }
        Ok(0)
    }

    fn check_known_patterns(&self, tx: &SignedTransaction, flags: &mut Vec<FraudFlag>) -> u8 {
        if let Some(to) = tx.to() {
            if self.pattern_db.is_suspicious(&to) {
                flags.push(FraudFlag::SuspiciousRecipient);
                return 40;
            }
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{Address, U256};
    use quyn_core::{ChainDB, StateDB, Transaction};
    use quyn_wallet::{sign_transaction, KeyPair};
    use tempfile::TempDir;

    fn devnet_keypair() -> KeyPair {
        let mut secret = [0u8; 32];
        secret[0] = 0xde;
        secret[1] = 0xad;
        secret[2] = 0xbe;
        secret[3] = 0xef;
        secret[4] = 0x11;
        KeyPair::from_secret(secret).expect("devnet key")
    }

    #[test]
    fn new_wallet_large_transfer_flag() {
        let dir_chain = TempDir::new().unwrap();
        let dir_state = TempDir::new().unwrap();
        let chain = ChainDB::open(dir_chain.path()).unwrap();
        let state = StateDB::open(dir_state.path()).unwrap();
        let kp = devnet_keypair();
        let sender = kp.address();
        let one_qyn = U256::from(10_u128.pow(18));
        state.set_balance(&sender, one_qyn * U256::from(2000)).unwrap();
        state.set_nonce(&sender, 0).unwrap();
        let tx = Transaction {
            nonce: 0,
            gas_price: U256::from(1),
            gas_limit: 21_000,
            to: Some(Address::ZERO),
            value: one_qyn * U256::from(1500),
            data: vec![],
            chain_id: 7779,
        };
        let signed = sign_transaction(&tx, &kp).unwrap();
        let detector = FraudDetector::new(FraudConfig::default());
        let analysis = detector.analyse_transaction(&signed, &chain, &state, 1).unwrap();
        assert!(analysis.risk_score >= 25);
        assert!(analysis.flags.iter().any(|f| matches!(f, FraudFlag::NewWalletLargeTransfer)));
    }

    #[test]
    fn round_number_flag() {
        let dir_chain = TempDir::new().unwrap();
        let dir_state = TempDir::new().unwrap();
        let chain = ChainDB::open(dir_chain.path()).unwrap();
        let state = StateDB::open(dir_state.path()).unwrap();
        let kp = devnet_keypair();
        let sender = kp.address();
        state.set_balance(&sender, U256::from(10_u128.pow(18) * 10000)).unwrap();
        state.set_nonce(&sender, 5).unwrap();
        let tx = Transaction {
            nonce: 5,
            gas_price: U256::from(1),
            gas_limit: 21_000,
            to: Some(Address::ZERO),
            value: U256::from(1000_u128 * 10_u128.pow(18)),
            data: vec![],
            chain_id: 7779,
        };
        let signed = sign_transaction(&tx, &kp).unwrap();
        let detector = FraudDetector::new(FraudConfig::default());
        let analysis = detector.analyse_transaction(&signed, &chain, &state, 100).unwrap();
        assert!(analysis.flags.iter().any(|f| matches!(f, FraudFlag::RoundNumber)));
    }

    #[test]
    fn full_balance_drain_flag() {
        let dir_chain = TempDir::new().unwrap();
        let dir_state = TempDir::new().unwrap();
        let chain = ChainDB::open(dir_chain.path()).unwrap();
        let state = StateDB::open(dir_state.path()).unwrap();
        let kp = devnet_keypair();
        let sender = kp.address();
        let bal = U256::from(500_u128 * 10_u128.pow(18));
        state.set_balance(&sender, bal).unwrap();
        state.set_nonce(&sender, 1).unwrap();
        let tx = Transaction {
            nonce: 1,
            gas_price: U256::from(1),
            gas_limit: 21_000,
            to: Some(Address::ZERO),
            value: bal,
            data: vec![],
            chain_id: 7779,
        };
        let signed = sign_transaction(&tx, &kp).unwrap();
        let detector = FraudDetector::new(FraudConfig::default());
        let analysis = detector.analyse_transaction(&signed, &chain, &state, 10).unwrap();
        assert!(analysis.flags.iter().any(|f| matches!(f, FraudFlag::FullBalanceDrain)));
    }

    #[test]
    fn suspicious_recipient_flag() {
        let dir_chain = TempDir::new().unwrap();
        let dir_state = TempDir::new().unwrap();
        let chain = ChainDB::open(dir_chain.path()).unwrap();
        let state = StateDB::open(dir_state.path()).unwrap();
        let kp = devnet_keypair();
        let mut bad = [0u8; 20];
        bad[19] = 99;
        let bad_addr = Address::from_slice(&bad);
        let mut detector = FraudDetector::new(FraudConfig::default());
        detector.pattern_db.add_suspicious(&bad_addr);
        state.set_balance(&kp.address(), U256::from(100_u128 * 10_u128.pow(18))).unwrap();
        state.set_nonce(&kp.address(), 0).unwrap();
        let tx = Transaction {
            nonce: 0,
            gas_price: U256::from(1),
            gas_limit: 21_000,
            to: Some(bad_addr),
            value: U256::from(10_u128 * 10_u128.pow(18)),
            data: vec![],
            chain_id: 7779,
        };
        let signed = sign_transaction(&tx, &kp).unwrap();
        let analysis = detector.analyse_transaction(&signed, &chain, &state, 1).unwrap();
        assert!(analysis.flags.iter().any(|f| matches!(f, FraudFlag::SuspiciousRecipient)));
    }
}
