//! Anti Rug Pull System (ARPS): protocol-level rug pull detection.

use quyn_core::SignedTransaction;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for rug pull detection thresholds.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RugPullConfig {
    /// Maximum % of supply one wallet can sell in single tx (e.g. 30).
    pub max_single_tx_sell_percent: f64,
    /// Maximum % supply concentration (e.g. 40).
    pub max_concentration_percent: f64,
    /// Minimum liquidity lock period in blocks (e.g. 100_000 ~34 days).
    pub min_lock_period_blocks: u64,
    /// Threshold for flagging contract (0-100, e.g. 60).
    pub contract_risk_threshold: u8,
}

impl Default for RugPullConfig {
    fn default() -> Self {
        Self {
            max_single_tx_sell_percent: 30.0,
            max_concentration_percent: 40.0,
            min_lock_period_blocks: 100_000,
            contract_risk_threshold: 60,
        }
    }
}

/// Risk factors identified in a contract.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ContractRiskFactor {
    UnlimitedMintFunction,
    InstantOwnerWithdraw,
    HiddenBackdoor,
    NoTimeLockOnWithdraw,
    HighOwnerConcentration,
    NoLiquidityLock,
    SuspiciousDeployer,
    UnverifiedContract,
    RecentDeployment,
    MultipleRisksCombined,
}

/// Risk profile for a deployed contract.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContractRiskProfile {
    pub contract_address: [u8; 20],
    pub deployer: [u8; 20],
    pub deploy_block: u64,
    pub risk_score: u8,
    pub risk_factors: Vec<ContractRiskFactor>,
    pub is_verified: bool,
    pub liquidity_locked: bool,
    pub lock_expiry_block: Option<u64>,
    pub total_supply: u128,
    pub holder_count: u32,
    pub top_holder_percent: f64,
}

/// On-chain liquidity lock record.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiquidityLock {
    pub contract: [u8; 20],
    pub locked_amount: u128,
    pub lock_start_block: u64,
    pub lock_expiry_block: u64,
    pub locker: [u8; 20],
    pub is_active: bool,
}

/// Rug pull alert raised by the detector.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RugPullAlert {
    pub alert_id: [u8; 32],
    pub contract: [u8; 20],
    pub deployer: [u8; 20],
    pub alert_type: RugPullAlertType,
    pub severity: AlertSeverity,
    pub description: String,
    pub triggered_block: u64,
    pub transaction_hash: Option<[u8; 32]>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum RugPullAlertType {
    LargeSellDetected,
    LiquidityDrainAttempt,
    HighConcentrationSell,
    ContractBackdoorDetected,
    SuspiciousMintBeforeSell,
    RapidDeployAndSell,
    CoordinatedDump,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum AlertSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// Generates a deterministic alert id from contract + block + description.
fn make_alert_id(contract: &[u8; 20], block: u64, desc: &str) -> [u8; 32] {
    use alloy_primitives::keccak256;
    let mut input = Vec::with_capacity(20 + 8 + desc.len());
    input.extend_from_slice(contract);
    input.extend_from_slice(&block.to_be_bytes());
    input.extend_from_slice(desc.as_bytes());
    let h = keccak256(&input);
    h.0
}

/// Anti Rug Pull detector: analyses transactions and maintains contract/lock state.
pub struct RugPullDetector {
    pub config: RugPullConfig,
    pub contract_registry: HashMap<[u8; 20], ContractRiskProfile>,
    pub liquidity_locks: HashMap<[u8; 20], LiquidityLock>,
    /// Stored alerts (e.g. for RPC get_all_alerts).
    pub alerts: Vec<RugPullAlert>,
}

impl RugPullDetector {
    pub fn new(config: RugPullConfig) -> Self {
        Self {
            config,
            contract_registry: HashMap::new(),
            liquidity_locks: HashMap::new(),
            alerts: Vec::new(),
        }
    }

    /// Analyse a transaction for rug pull patterns. Caller should pass token/sender/supply
    /// when available; otherwise checks that need them are skipped (e.g. total_supply 0).
    /// If an alert is raised it is stored and returned.
    pub fn analyse_transaction(
        &mut self,
        tx: &SignedTransaction,
        contract_profile: Option<&ContractRiskProfile>,
        sender_balance: u128,
        total_supply: u128,
        current_block: u64,
    ) -> Option<RugPullAlert> {
        let sender = tx.sender().ok()?;
        let sender_bytes: [u8; 20] = sender.as_slice().try_into().ok()?;
        let value_wei = tx.value().to::<u128>();

        // CHECK 1 - Large sell: tx value > 30% of total supply AND sender is deployer
        if total_supply > 0 && contract_profile.is_some() {
            let profile = contract_profile.unwrap();
            let sell_pct = (value_wei as f64 / total_supply as f64) * 100.0;
            if sell_pct > self.config.max_single_tx_sell_percent
                && sender_bytes == profile.deployer
            {
                let alert = RugPullAlert {
                    alert_id: make_alert_id(&profile.contract_address, current_block, "large_sell"),
                    contract: profile.contract_address,
                    deployer: profile.deployer,
                    alert_type: RugPullAlertType::LargeSellDetected,
                    severity: AlertSeverity::Critical,
                    description: format!(
                        "Deployer selling {:.1}% of supply in one tx (max {:.0}%)",
                        sell_pct, self.config.max_single_tx_sell_percent
                    ),
                    triggered_block: current_block,
                    transaction_hash: Some(tx.hash().0),
                };
                self.alerts.push(alert.clone());
                return Some(alert);
            }
        }

        // CHECK 2 - Liquidity drain: contract has liquidity and tx drains >50% in one tx
        // (We don't have liquidity amount in this API; treat as no alert if no contract_profile with liquidity data.)
        // Skip unless we have liquidity info on profile; for now we don't add liquidity_balance to profile.
        // So CHECK 2 is effectively skipped without more data.

        // CHECK 3 - Concentration sell: sender holds >40% and is selling >20%
        if total_supply > 0 && sender_balance > 0 {
            let holder_pct = (sender_balance as f64 / total_supply as f64) * 100.0;
            let sell_pct = (value_wei as f64 / total_supply as f64) * 100.0;
            if holder_pct > self.config.max_concentration_percent && sell_pct > 20.0 {
                let contract = contract_profile.map(|p| p.contract_address).unwrap_or([0u8; 20]);
                let deployer = contract_profile.map(|p| p.deployer).unwrap_or([0u8; 20]);
                let alert = RugPullAlert {
                    alert_id: make_alert_id(&contract, current_block, "high_concentration_sell"),
                    contract,
                    deployer,
                    alert_type: RugPullAlertType::HighConcentrationSell,
                    severity: AlertSeverity::High,
                    description: format!(
                        "Wallet holding {:.1}% selling {:.1}% of supply",
                        holder_pct, sell_pct
                    ),
                    triggered_block: current_block,
                    transaction_hash: Some(tx.hash().0),
                };
                self.alerts.push(alert.clone());
                return Some(alert);
            }
        }

        // CHECK 4 - Rapid deploy and sell: deployed <1000 blocks ago, deployer selling large
        if let Some(profile) = contract_profile {
            let blocks_since_deploy = current_block.saturating_sub(profile.deploy_block);
            if blocks_since_deploy < 1000 && sender_bytes == profile.deployer && total_supply > 0 {
                let sell_pct = (value_wei as f64 / total_supply as f64) * 100.0;
                if sell_pct > 10.0 {
                    let alert = RugPullAlert {
                        alert_id: make_alert_id(
                            &profile.contract_address,
                            current_block,
                            "rapid_deploy_sell",
                        ),
                        contract: profile.contract_address,
                        deployer: profile.deployer,
                        alert_type: RugPullAlertType::RapidDeployAndSell,
                        severity: AlertSeverity::High,
                        description: format!(
                            "Deployer selling {:.1}% within {} blocks of deployment",
                            sell_pct, blocks_since_deploy
                        ),
                        triggered_block: current_block,
                        transaction_hash: Some(tx.hash().0),
                    };
                    self.alerts.push(alert.clone());
                    return Some(alert);
                }
            }
        }

        // CHECK 5 - Suspicious mint before sell: mint in last 10 blocks then large sell
        // (Requires chain/mint history; not available here. Skip or no-op.)

        None
    }

    pub fn register_contract(
        &mut self,
        address: [u8; 20],
        _deployer: [u8; 20],
        _block: u64,
        profile: ContractRiskProfile,
    ) {
        self.contract_registry.insert(address, profile);
    }

    pub fn lock_liquidity(
        &mut self,
        contract: [u8; 20],
        amount: u128,
        lock_period_blocks: u64,
        locker: [u8; 20],
        current_block: u64,
    ) -> LiquidityLock {
        let lock = LiquidityLock {
            contract,
            locked_amount: amount,
            lock_start_block: current_block,
            lock_expiry_block: current_block + lock_period_blocks,
            locker,
            is_active: true,
        };
        self.liquidity_locks.insert(contract, lock.clone());
        lock
    }

    pub fn get_contract_profile(&self, contract: &[u8; 20]) -> Option<&ContractRiskProfile> {
        self.contract_registry.get(contract)
    }

    pub fn get_all_alerts(&self) -> Vec<&RugPullAlert> {
        self.alerts.iter().collect()
    }

    pub fn get_alerts_for_contract(&self, contract: &[u8; 20]) -> Vec<&RugPullAlert> {
        self.alerts
            .iter()
            .filter(|a| a.contract == *contract)
            .collect()
    }
}
