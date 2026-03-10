//! Risk score and recommendation types.

use serde::{Deserialize, Serialize};

use crate::models::FraudConfig;

/// Recommendation for a transaction based on risk score.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum FraudRecommendation {
    /// 0-30: include normally
    Include,
    /// 31-60: include with log
    IncludeWithLog,
    /// 61-85: delay and review
    Delay,
    /// 86-100: reject
    Reject,
}

/// Result of fraud analysis for a single transaction.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FraudAnalysis {
    pub transaction_hash: [u8; 32],
    pub risk_score: u8,
    pub flags: Vec<FraudFlag>,
    pub recommendation: FraudRecommendation,
    pub timestamp: u64,
}

/// Individual fraud indicator.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FraudFlag {
    NewWalletLargeTransfer,
    HighVelocity,
    ExtremeVelocity,
    LargeBalanceDrain,
    FullBalanceDrain,
    RoundNumber,
    DustAttack,
    RapidWalletDrain,
    SuspiciousRecipient,
}

impl FraudFlag {
    pub fn as_str(&self) -> &'static str {
        match self {
            FraudFlag::NewWalletLargeTransfer => "NEW_WALLET_LARGE_TRANSFER",
            FraudFlag::HighVelocity => "HIGH_VELOCITY",
            FraudFlag::ExtremeVelocity => "EXTREME_VELOCITY",
            FraudFlag::LargeBalanceDrain => "LARGE_BALANCE_DRAIN",
            FraudFlag::FullBalanceDrain => "FULL_BALANCE_DRAIN",
            FraudFlag::RoundNumber => "ROUND_NUMBER",
            FraudFlag::DustAttack => "DUST_ATTACK",
            FraudFlag::RapidWalletDrain => "RAPID_WALLET_DRAIN",
            FraudFlag::SuspiciousRecipient => "SUSPICIOUS_RECIPIENT",
        }
    }
}

/// Map risk score to recommendation using config thresholds.
pub fn get_recommendation(score: u8, config: &FraudConfig) -> FraudRecommendation {
    if score <= config.low_threshold {
        FraudRecommendation::Include
    } else if score <= config.medium_threshold {
        FraudRecommendation::IncludeWithLog
    } else if score <= config.high_threshold {
        FraudRecommendation::Delay
    } else {
        FraudRecommendation::Reject
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommendation_thresholds() {
        let config = FraudConfig::default();
        assert_eq!(get_recommendation(0, &config), FraudRecommendation::Include);
        assert_eq!(get_recommendation(30, &config), FraudRecommendation::Include);
        assert_eq!(get_recommendation(31, &config), FraudRecommendation::IncludeWithLog);
        assert_eq!(get_recommendation(60, &config), FraudRecommendation::IncludeWithLog);
        assert_eq!(get_recommendation(61, &config), FraudRecommendation::Delay);
        assert_eq!(get_recommendation(85, &config), FraudRecommendation::Delay);
        assert_eq!(get_recommendation(86, &config), FraudRecommendation::Reject);
        assert_eq!(get_recommendation(100, &config), FraudRecommendation::Reject);
    }
}
