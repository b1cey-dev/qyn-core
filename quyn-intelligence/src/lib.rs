//! QYN Intelligence — protocol-level AI services.
//!
//! Phase 1: deterministic fraud detection.
//! Phase 2: reputation-based validator scoring and selection (disabled by default).

pub mod fraud_detector;
pub mod models;
pub mod patterns;
pub mod risk_scorer;
pub mod reputation_db;
pub mod validator_scorer;
pub mod validator_selection;

pub use fraud_detector::FraudDetector;
pub use models::FraudConfig;
pub use patterns::PatternDatabase;
pub use reputation_db::{ReputationDatabase, ValidatorRecord};
pub use risk_scorer::{FraudAnalysis, FraudFlag, FraudRecommendation, get_recommendation};
pub use validator_scorer::ValidatorScorer;
pub use validator_selection::{
    AIValidatorSelector, SelectorConfig, ValidatorCandidate, ValidatorScore,
};
