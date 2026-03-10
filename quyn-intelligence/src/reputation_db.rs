use std::collections::HashMap;

use alloy_primitives::Address;

/// Historical performance data for a validator.
#[derive(Clone, Debug, Default)]
pub struct ValidatorRecord {
    pub address: Address,
    pub total_blocks_proposed: u64,
    pub total_blocks_missed: u64,
    pub total_invalid_blocks: u64,
    pub average_response_time_ms: u64,
    pub last_seen_block: u64,
    pub slash_count: u32,
    pub uptime_percentage: f64,
    pub joined_block: u64,
}

#[derive(Default)]
pub struct ReputationDatabase {
    records: HashMap<Address, ValidatorRecord>,
}

impl ReputationDatabase {
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
        }
    }

    fn entry_mut(&mut self, address: Address, block: u64) -> &mut ValidatorRecord {
        self.records.entry(address).or_insert_with(|| ValidatorRecord {
            address,
            joined_block: block,
            ..Default::default()
        })
    }

    pub fn update_proposed(&mut self, address: Address, block: u64, response_time_ms: u64) {
        let rec = self.entry_mut(address, block);
        rec.total_blocks_proposed = rec.total_blocks_proposed.saturating_add(1);
        // simple running average
        if rec.average_response_time_ms == 0 {
            rec.average_response_time_ms = response_time_ms;
        } else {
            rec.average_response_time_ms =
                ((rec.average_response_time_ms as u128 * 3 + response_time_ms as u128) / 4) as u64;
        }
        rec.last_seen_block = block;
        Self::recompute_uptime(rec);
    }

    pub fn update_missed(&mut self, address: Address, block: u64) {
        let rec = self.entry_mut(address, block);
        rec.total_blocks_missed = rec.total_blocks_missed.saturating_add(1);
        rec.last_seen_block = block;
        Self::recompute_uptime(rec);
    }

    pub fn update_invalid(&mut self, address: Address, block: u64) {
        let rec = self.entry_mut(address, block);
        rec.total_invalid_blocks = rec.total_invalid_blocks.saturating_add(1);
        rec.last_seen_block = block;
        Self::recompute_uptime(rec);
    }

    fn recompute_uptime(rec: &mut ValidatorRecord) {
        let total = rec
            .total_blocks_proposed
            .saturating_add(rec.total_blocks_missed);
        if total == 0 {
            rec.uptime_percentage = 0.0;
        } else {
            rec.uptime_percentage =
                (rec.total_blocks_proposed as f64 / total as f64) * 100.0;
        }
    }

    pub fn get_record(&self, address: &Address) -> Option<&ValidatorRecord> {
        self.records.get(address)
    }

    pub fn all_records(&self) -> Vec<&ValidatorRecord> {
        self.records.values().collect()
    }
}

