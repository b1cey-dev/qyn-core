use std::collections::VecDeque;

/// Per-block gas and congestion metrics captured by the optimiser.
#[derive(Clone, Debug)]
pub struct BlockMetrics {
    pub block_number: u64,
    pub transaction_count: u32,
    pub average_gas_used: u64,
    pub timestamp: u64,
    pub congestion_score: f64,
}

#[derive(Clone, Debug)]
pub struct GasConfig {
    pub history_size: usize,
    pub base_fee: u64,
    pub min_fee: u64,
    pub max_fee: u64,
    pub congestion_threshold: f64,
}

impl Default for GasConfig {
    fn default() -> Self {
        Self {
            history_size: 100,
            base_fee: 1_000_000_000,          // 1 gwei
            min_fee: 100_000_000,            // 0.1 gwei
            max_fee: 100_000_000_000,        // 100 gwei
            congestion_threshold: 0.8,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CongestionLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Clone, Debug)]
pub struct GasPrediction {
    pub recommended_gas_price: u64,
    pub confidence: f64,
    pub congestion_level: CongestionLevel,
    pub estimated_confirmation_blocks: u32,
    pub optimal_send_window: String,
}

/// Non-consensus gas optimiser – reads recent history to suggest prices.
pub struct GasOptimiser {
    history: VecDeque<BlockMetrics>,
    pub config: GasConfig,
}

impl GasOptimiser {
    pub fn new(config: GasConfig) -> Self {
        Self {
            history: VecDeque::with_capacity(config.history_size),
            config,
        }
    }

    pub fn record_block(&mut self, metrics: BlockMetrics) {
        if self.history.len() == self.config.history_size {
            self.history.pop_front();
        }
        self.history.push_back(metrics);
    }

    pub fn predict_gas_price(&self, _current_block: u64) -> GasPrediction {
        if self.history.is_empty() {
            // No history – return baseline defaults.
            return GasPrediction {
                recommended_gas_price: self.config.base_fee,
                confidence: 0.3,
                congestion_level: CongestionLevel::Low,
                estimated_confirmation_blocks: 1,
                optimal_send_window: "Now".to_string(),
            };
        }

        let window = self
            .history
            .iter()
            .rev()
            .take(100)
            .cloned()
            .collect::<Vec<_>>();

        let len = window.len() as f64;
        let avg_congestion: f64 =
            window.iter().map(|m| m.congestion_score).sum::<f64>() / len;
        let last_congestion = window.first().map(|m| m.congestion_score).unwrap_or(0.0);

        let level = if avg_congestion <= 0.3 {
            CongestionLevel::Low
        } else if avg_congestion <= 0.6 {
            CongestionLevel::Medium
        } else if avg_congestion <= 0.8 {
            CongestionLevel::High
        } else {
            CongestionLevel::Critical
        };

        let mut price = match level {
            CongestionLevel::Low => (self.config.base_fee as f64 * 1.0) as u64,
            CongestionLevel::Medium => (self.config.base_fee as f64 * 1.5) as u64,
            CongestionLevel::High => (self.config.base_fee as f64 * 2.0) as u64,
            CongestionLevel::Critical => (self.config.base_fee as f64 * 3.0) as u64,
        };

        // Clamp to [min_fee, max_fee].
        if price < self.config.min_fee {
            price = self.config.min_fee;
        }
        if price > self.config.max_fee {
            price = self.config.max_fee;
        }

        let estimated_confirmation_blocks = match level {
            CongestionLevel::Low => 1,
            CongestionLevel::Medium => 2,
            CongestionLevel::High => 5,
            CongestionLevel::Critical => 10,
        };

        let optimal_send_window = if avg_congestion < 0.3 {
            "Now".to_string()
        } else if last_congestion > avg_congestion && avg_congestion < self.config.congestion_threshold {
            "Wait 30 seconds".to_string()
        } else if avg_congestion >= 0.8 {
            "Network busy, retry in 2 minutes".to_string()
        } else {
            "Now".to_string()
        };

        let confidence = (len / self.config.history_size as f64).clamp(0.0, 1.0);

        GasPrediction {
            recommended_gas_price: price,
            confidence,
            congestion_level: level,
            estimated_confirmation_blocks,
            optimal_send_window,
        }
    }

    pub fn get_fee_history(&self) -> Vec<BlockMetrics> {
        self.history.iter().cloned().collect()
    }
}

