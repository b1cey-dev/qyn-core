//! QYN Intelligence — AI-powered fraud detection at the protocol level.
//!
//! Every transaction is analysed before block inclusion. The system is fully
//! deterministic so all nodes reach the same conclusion.

pub mod fraud_detector;
pub mod models;
pub mod patterns;
pub mod risk_scorer;

pub use fraud_detector::FraudDetector;
pub use models::FraudConfig;
pub use patterns::PatternDatabase;
pub use risk_scorer::{FraudAnalysis, FraudFlag, FraudRecommendation, get_recommendation};
