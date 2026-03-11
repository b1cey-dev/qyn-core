//! Content verification types for QYN Verify (protocol-level truth layer).
//!
//! These types are used by the RPC layer to return structured verification
//! results for arbitrary content (articles, images, videos, documents, text).
//!
//! NOTE: In the current phase, QYN Verify reuses the existing intelligence
//! pipeline and exposes verification as an off-chain service via RPC. On-chain
//! storage of content verification records can be added later using these
//! structures as the canonical schema.

use serde::{Deserialize, Serialize};
use alloy_primitives::B256;

/// High-level type of content being verified.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContentType {
    Article,
    Image,
    Video,
    Document,
    SocialPost,
    Text,
    Unknown,
}

/// AI generation status for the analysed content.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AiGeneratedStatus {
    Human,
    AiGenerated,
    LikelyAiGenerated,
    Unknown,
}

/// Source credibility bucket.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CredibilityScore {
    High,
    Medium,
    Low,
    Unknown,
}

/// Summary result for a single content verification.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContentVerification {
    /// Unique ID for this verification (can be derived from tx hash or content hash).
    pub verification_id: B256,
    /// SHA-256 or Keccak hash of the canonical content.
    pub content_hash: B256,
    /// Optional URL associated with the content (e.g. article URL).
    pub content_url: Option<String>,
    /// Declared or inferred content type.
    pub content_type: ContentType,
    /// Overall trust score (0-100, higher = more trustworthy).
    pub trust_score: u8,
    /// Whether AI generation is suspected.
    pub ai_generated: AiGeneratedStatus,
    /// Whether manipulation (e.g. deepfake, heavy editing) was detected.
    pub manipulation_detected: bool,
    /// Credibility bucket for the primary source.
    pub source_credibility: CredibilityScore,
    /// Optional description of the original source, if discovered.
    pub original_source: Option<String>,
    /// Optional human-readable alteration / duplicate history.
    pub alteration_history: Option<String>,
    /// UNIX timestamp (seconds) when verification was produced.
    pub verified_at: u64,
}

