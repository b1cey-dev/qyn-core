use alloy_primitives::Address;

use crate::reputation_db::ReputationDatabase;
use crate::validator_scorer::ValidatorScorer;

pub struct AIValidatorSelector {
    pub reputation_db: ReputationDatabase,
    pub scorer: ValidatorScorer,
    pub config: SelectorConfig,
}

#[derive(Clone, Copy)]
pub struct SelectorConfig {
    /// Relative weight of reputation in [0.0, 1.0]
    pub reputation_weight: f64,
    /// Relative weight of stake in [0.0, 1.0]
    pub stake_weight: f64,
    /// Minimum reputation to participate.
    pub min_reputation_score: u32,
    /// Blocks since joined during which stake-only selection is used.
    pub grace_period_blocks: u64,
}

#[derive(Clone, Copy)]
pub struct ValidatorCandidate {
    pub address: Address,
    pub stake_amount: u128,
}

#[derive(Clone, Debug)]
pub struct ValidatorScore {
    pub address: Address,
    pub reputation_score: u32,
    pub stake_score: u32,
    pub combined_score: u32,
    pub uptime_percentage: f64,
    pub blocks_proposed: u64,
    pub blocks_missed: u64,
    pub slash_count: u32,
}

impl Default for SelectorConfig {
    fn default() -> Self {
        Self {
            reputation_weight: 0.5,
            stake_weight: 0.5,
            min_reputation_score: 100,
            grace_period_blocks: 100,
        }
    }
}

impl AIValidatorSelector {
    pub fn new(config: SelectorConfig) -> Self {
        Self {
            reputation_db: ReputationDatabase::new(),
            scorer: ValidatorScorer,
            config,
        }
    }

    /// Deterministically select a validator using combined reputation+stake.
    pub fn select_validator(
        &self,
        candidates: Vec<ValidatorCandidate>,
        current_block: u64,
        block_hash: [u8; 32],
    ) -> Address {
        // If no candidates, default to zero address.
        if candidates.is_empty() {
            return Address::ZERO;
        }

        // Determine max stake for normalisation.
        let max_stake: u128 = candidates
            .iter()
            .map(|c| c.stake_amount)
            .max()
            .unwrap_or(1)
            .max(1);

        let mut scored: Vec<(ValidatorCandidate, u64)> = Vec::with_capacity(candidates.len());

        for cand in candidates.iter().cloned() {
            let mut reputation_score = 0u32;
            let mut stake_score = 0u32;

            // Stake score: scale to 0..1000 linearly.
            if cand.stake_amount > 0 {
                let ratio = cand.stake_amount as f64 / max_stake as f64;
                stake_score = (ratio * 1000.0).round() as u32;
            }

            let record = self.reputation_db.get_record(&cand.address);
            let in_grace = record
                .map(|r| current_block.saturating_sub(r.joined_block) <= self.config.grace_period_blocks)
                .unwrap_or(true);

            if let Some(rec) = record {
                reputation_score = ValidatorScorer::calculate_score(rec, current_block);
            }

            // Exclude if reputation below minimum (unless still in grace).
            if !in_grace && reputation_score < self.config.min_reputation_score {
                continue;
            }

            // Combined score.
            let combined = if in_grace {
                // Stake-only weighting during grace period.
                (stake_score as f64).round() as u32
            } else {
                let rep_w = self.config.reputation_weight;
                let stake_w = self.config.stake_weight;
                let combined_f = (stake_score as f64 * stake_w) + (reputation_score as f64 * rep_w);
                combined_f.round().clamp(0.0, 1000.0) as u32
            };

            if combined == 0 {
                continue;
            }

            scored.push((cand, combined as u64));
        }

        if scored.is_empty() {
            // Fallback: pick highest stake if everything filtered out.
            let mut best = &candidates[0];
            for cand in &candidates {
                if cand.stake_amount > best.stake_amount {
                    best = cand;
                }
            }
            return best.address;
        }

        // Deterministic weighted selection using block_hash as seed.
        let total_weight: u64 = scored.iter().map(|(_, w)| *w).sum();
        let seed = u64::from_be_bytes(block_hash[0..8].try_into().unwrap());
        let mut target = seed % total_weight.max(1);

        for (cand, weight) in &scored {
            if target < *weight {
                return cand.address;
            }
            target -= weight;
        }

        // Fallback, should be unreachable.
        scored[0].0.address
    }

    pub fn record_block_produced(
        &mut self,
        validator: Address,
        block: u64,
        response_time_ms: u64,
    ) {
        self.reputation_db
            .update_proposed(validator, block, response_time_ms);
    }

    pub fn record_block_missed(&mut self, validator: Address, block: u64) {
        self.reputation_db.update_missed(validator, block);
    }

    pub fn record_invalid_block(&mut self, validator: Address, block: u64) {
        self.reputation_db.update_invalid(validator, block);
    }

    pub fn get_validator_scores(&self, current_block: u64) -> Vec<ValidatorScore> {
        self.reputation_db
            .all_records()
            .into_iter()
            .map(|rec| {
                let reputation_score = ValidatorScorer::calculate_score(rec, current_block);
                // derive stake_score and combined_score externally in consensus later;
                ValidatorScore {
                    address: rec.address,
                    reputation_score,
                    stake_score: 0,
                    combined_score: reputation_score,
                    uptime_percentage: rec.uptime_percentage,
                    blocks_proposed: rec.total_blocks_proposed,
                    blocks_missed: rec.total_blocks_missed,
                    slash_count: rec.slash_count,
                }
            })
            .collect()
    }
}

