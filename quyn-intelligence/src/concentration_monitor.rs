//! Token concentration monitoring: distribution across wallets for rug pull risk.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// token -> wallet -> balance
type HoldingsMap = HashMap<[u8; 20], HashMap<[u8; 20], u128>>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenRiskSummary {
    pub token: [u8; 20],
    pub total_holders: u32,
    pub top_holder_percent: f64,
    pub top_5_holders_percent: f64,
    pub is_high_concentration: bool,
    pub concentration_risk: ConcentrationRisk,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ConcentrationRisk {
    Low,    // Top holder < 10%
    Medium, // Top holder 10-25%
    High,   // Top holder 25-40%
    Critical, // Top holder > 40%
}

impl ConcentrationRisk {
    fn from_top_percent(pct: f64) -> Self {
        if pct > 40.0 {
            ConcentrationRisk::Critical
        } else if pct > 25.0 {
            ConcentrationRisk::High
        } else if pct > 10.0 {
            ConcentrationRisk::Medium
        } else {
            ConcentrationRisk::Low
        }
    }
}

pub struct ConcentrationMonitor {
    /// token_address -> wallet_address -> balance
    holdings: HoldingsMap,
}

impl ConcentrationMonitor {
    pub fn new() -> Self {
        Self {
            holdings: HashMap::new(),
        }
    }

    pub fn update_balance(&mut self, token: [u8; 20], wallet: [u8; 20], new_balance: u128) {
        self.holdings
            .entry(token)
            .or_default()
            .insert(wallet, new_balance);
    }

    fn total_supply(&self, token: &[u8; 20]) -> u128 {
        self.holdings
            .get(token)
            .map(|m| m.values().sum())
            .unwrap_or(0)
    }

    /// Returns percentage of total supply held by this wallet (0.0 to 100.0).
    pub fn get_concentration(&self, token: &[u8; 20], wallet: &[u8; 20]) -> f64 {
        let total = self.total_supply(token);
        if total == 0 {
            return 0.0;
        }
        let balance = self
            .holdings
            .get(token)
            .and_then(|m| m.get(wallet))
            .copied()
            .unwrap_or(0);
        (balance as f64 / total as f64) * 100.0
    }

    /// Returns top N holders with (address, percent). Address as 20 bytes.
    pub fn get_top_holders(
        &self,
        token: &[u8; 20],
        count: usize,
    ) -> Vec<([u8; 20], f64)> {
        let Some(wallets) = self.holdings.get(token) else {
            return Vec::new();
        };
        let total: u128 = wallets.values().sum();
        if total == 0 {
            return Vec::new();
        }
        let mut list: Vec<_> = wallets
            .iter()
            .map(|(addr, &bal)| (*addr, (bal as f64 / total as f64) * 100.0))
            .collect();
        list.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        list.into_iter().take(count).collect()
    }

    pub fn is_high_concentration(
        &self,
        token: &[u8; 20],
        wallet: &[u8; 20],
        threshold: f64,
    ) -> bool {
        self.get_concentration(token, wallet) >= threshold
    }

    pub fn get_token_risk_summary(&self, token: &[u8; 20]) -> TokenRiskSummary {
        let _total = self.total_supply(token);
        let holders = self
            .holdings
            .get(token)
            .map(|m| m.len() as u32)
            .unwrap_or(0);
        let top = self.get_top_holders(token, 5);
        let top_holder_percent = top.first().map(|(_, p)| *p).unwrap_or(0.0);
        let top_5_holders_percent: f64 = top.iter().take(5).map(|(_, p)| p).sum();
        let concentration_risk = ConcentrationRisk::from_top_percent(top_holder_percent);
        let is_high_concentration = top_holder_percent > 40.0;

        TokenRiskSummary {
            token: *token,
            total_holders: holders,
            top_holder_percent,
            top_5_holders_percent,
            is_high_concentration,
            concentration_risk,
        }
    }
}

impl Default for ConcentrationMonitor {
    fn default() -> Self {
        Self::new()
    }
}
