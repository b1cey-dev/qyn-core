//! Scans smart contract bytecode and Solidity source for known rug pull patterns.

use serde::{Deserialize, Serialize};

use crate::rug_pull_detector::ContractRiskFactor;

/// Result of scanning a contract.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContractScanResult {
    pub risk_score: u8,
    pub risk_factors: Vec<ContractRiskFactor>,
    pub recommendation: ScanRecommendation,
    pub details: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ScanRecommendation {
    Safe,     // 0-30
    Caution,  // 31-60
    HighRisk, // 61-85
    Dangerous, // 86-100
}

impl ContractScanResult {
    fn recommendation_from_score(score: u8) -> ScanRecommendation {
        if score <= 30 {
            ScanRecommendation::Safe
        } else if score <= 60 {
            ScanRecommendation::Caution
        } else if score <= 85 {
            ScanRecommendation::HighRisk
        } else {
            ScanRecommendation::Dangerous
        }
    }
}

pub struct ContractScanner;

impl ContractScanner {
    /// Scan bytecode for dangerous patterns (e.g. selfdestruct selector).
    pub fn scan_bytecode(bytecode: &[u8]) -> ContractScanResult {
        let mut score: i32 = 0;
        let mut risk_factors = Vec::new();
        let mut details = Vec::new();

        // Check for selfdestruct (opcode 0xff) or common selector patterns
        if bytecode.windows(1).any(|w| w[0] == 0xff) {
            score += 40;
            risk_factors.push(ContractRiskFactor::HiddenBackdoor);
            details.push(
                "Contract contains selfdestruct function which can destroy contract and take funds"
                    .to_string(),
            );
        }

        let score = (score).clamp(0, 100) as u8;
        let recommendation = ContractScanResult::recommendation_from_score(score);
        ContractScanResult {
            risk_score: score,
            risk_factors,
            recommendation,
            details,
        }
    }

    /// Scan Solidity source code for rug pull patterns.
    pub fn scan_solidity(source_code: &str) -> ContractScanResult {
        let mut score: i32 = 0;
        let mut risk_factors = Vec::new();
        let mut details = Vec::new();
        let s = source_code.to_lowercase();

        // Rule 1 - Unlimited mint
        if (s.contains("function mint") || s.contains("function _mint"))
            && !s.contains("maxsupply")
            && !s.contains("max_supply")
            && !s.contains("cap")
        {
            score += 25;
            risk_factors.push(ContractRiskFactor::UnlimitedMintFunction);
            details.push(
                "Contract has unlimited mint function with no supply cap".to_string(),
            );
        }

        // Rule 2 - Instant owner withdraw (transfer/send to owner, no timelock)
        if (s.contains("transfer") || s.contains("send") || s.contains("call")) && s.contains("owner")
            && !s.contains("timelock")
            && !s.contains("time lock")
            && !s.contains("delay")
        {
            score += 30;
            risk_factors.push(ContractRiskFactor::InstantOwnerWithdraw);
            details.push(
                "Owner can withdraw funds instantly without timelock".to_string(),
            );
        }

        // Rule 3 - Hidden selfdestruct
        if s.contains("selfdestruct") || s.contains("suicide") {
            score += 40;
            risk_factors.push(ContractRiskFactor::HiddenBackdoor);
            details.push(
                "Contract contains selfdestruct function which can destroy contract and take funds"
                    .to_string(),
            );
        }

        // Rule 4 - Blacklist
        if s.contains("blacklist") || s.contains("_isblacklisted") {
            score += 15;
            details.push(
                "Contract can blacklist wallet addresses preventing them from selling".to_string(),
            );
        }

        // Rule 5 - Pausable transfers
        if s.contains("pause") && (s.contains("transfer") || s.contains("_beforetokentransfer")) {
            score += 20;
            details.push("Owner can pause all transfers freezing funds".to_string());
        }

        // Rule 6 - Fee manipulation (owner can set fee to 100%)
        if (s.contains("fee") || s.contains("_fee")) && s.contains("owner") && s.contains("set") {
            score += 25;
            details.push(
                "Owner can set transfer fees to 100% effectively stealing all transferred funds"
                    .to_string(),
            );
        }

        // Rule 7 - No timelock on withdrawal
        if (s.contains("withdraw") || s.contains("claim")) && s.contains("owner")
            && !s.contains("timelock")
            && !s.contains("delay")
        {
            score += 15;
            risk_factors.push(ContractRiskFactor::NoTimeLockOnWithdraw);
        }

        // Safe patterns that reduce score
        if s.contains("renounceownership") || s.contains("ownershiprenounced") {
            score -= 20;
        }
        if s.contains("timelock") || s.contains("time lock") {
            score -= 15;
        }
        if s.contains("audit") || s.contains("verified") {
            score -= 10;
        }
        if s.contains("liquiditylock") || s.contains("liquidity locked") {
            score -= 15;
        }

        let score = (score).clamp(0, 100) as u8;
        let recommendation = ContractScanResult::recommendation_from_score(score);
        ContractScanResult {
            risk_score: score,
            risk_factors,
            recommendation,
            details,
        }
    }
}
