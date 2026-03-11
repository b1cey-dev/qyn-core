//! QYN Intelligence — protocol-level AI services.
//!
//! Phase 1: deterministic fraud detection.
//! Phase 2: reputation-based validator scoring and selection (disabled by default).
//! Phase 4: Anti Rug Pull System (ARPS).

pub mod concentration_monitor;
pub mod contract_scanner;
pub mod fraud_detector;
pub mod liquidity_monitor;
pub mod models;
pub mod patterns;
pub mod reputation_db;
pub mod risk_scorer;
pub mod rug_pull_detector;
pub mod gas_optimiser;
pub mod validator_scorer;
pub mod validator_selection;
pub mod content_verifier;

pub use fraud_detector::FraudDetector;
pub use models::FraudConfig;
pub use patterns::PatternDatabase;
pub use reputation_db::{ReputationDatabase, ValidatorRecord};
pub use gas_optimiser::{
    BlockMetrics,
    CongestionLevel,
    GasConfig,
    GasOptimiser,
    GasPrediction,
};
pub use risk_scorer::{FraudAnalysis, FraudFlag, FraudRecommendation, get_recommendation};
pub use rug_pull_detector::{
    AlertSeverity, ContractRiskFactor, ContractRiskProfile, LiquidityLock, RugPullAlert,
    RugPullAlertType, RugPullConfig, RugPullDetector,
};
pub use contract_scanner::{ContractScanResult, ContractScanner, ScanRecommendation};
pub use concentration_monitor::{ConcentrationMonitor, ConcentrationRisk, TokenRiskSummary};
pub use liquidity_monitor::LiquidityMonitor;
pub use validator_scorer::ValidatorScorer;
pub use validator_selection::{
    AIValidatorSelector, SelectorConfig, ValidatorCandidate, ValidatorScore,
};
pub use content_verifier::{
    AiGeneratedStatus,
    ContentType,
    CredibilityScore,
    ContentVerification,
};
