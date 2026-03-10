use crate::reputation_db::ValidatorRecord;

/// Computes a deterministic reputation score for validators.
pub struct ValidatorScorer;

impl ValidatorScorer {
    /// Calculate a reputation score (0–1000).
    pub fn calculate_score(record: &ValidatorRecord, current_block: u64) -> u32 {
        let mut score: i32 = 500;

        // FACTOR 1 - Uptime score (0-200)
        let uptime = record.uptime_percentage.clamp(0.0, 100.0);
        let uptime_score = (uptime * 2.0).round() as i32; // 100% -> 200
        score += uptime_score;

        // FACTOR 2 - Block proposal success rate (0-200)
        let total = record
            .total_blocks_proposed
            .saturating_add(record.total_blocks_missed);
        if total > 0 {
            let success_ratio =
                record.total_blocks_proposed as f64 / total as f64;
            let success_score = (success_ratio * 200.0).round() as i32;
            score += success_score;
        }

        // FACTOR 3 - Invalid block penalty (0 to -300)
        let invalid_penalty = (record.total_invalid_blocks as i32 * 50).min(300);
        score -= invalid_penalty;

        // FACTOR 4 - Slash penalty (0 to -200)
        let slash_penalty = (record.slash_count as i32 * 100).min(200);
        score -= slash_penalty;

        // FACTOR 5 - Experience bonus (0-100)
        if current_block > record.joined_block {
            let active_blocks = current_block - record.joined_block;
            let experience_bonus = if active_blocks > 10_000 {
                100
            } else if active_blocks > 1_000 {
                50
            } else {
                0
            };
            score += experience_bonus;
        }

        // FACTOR 6 - Response time score (0-100)
        let rt = record.average_response_time_ms;
        let rt_score = if rt > 0 && rt < 100 {
            100
        } else if rt >= 100 && rt < 500 {
            50
        } else {
            0
        };
        score += rt_score;

        // FACTOR 7 - Recency bonus (0-100)
        if current_block > record.last_seen_block {
            let diff = current_block - record.last_seen_block;
            let recency_bonus = if diff <= 10 {
                100
            } else if diff <= 100 {
                50
            } else {
                0
            };
            score += recency_bonus;
        }

        // Clamp to [0, 1000]
        score = score.clamp(0, 1000);
        score as u32
    }
}

