use alloy_primitives::Address;
use quyn_intelligence::{
    reputation_db::ReputationDatabase,
    validator_scorer::ValidatorScorer,
    validator_selection::{AIValidatorSelector, SelectorConfig, ValidatorCandidate},
};

fn addr(byte: u8) -> Address {
    let mut a = [0u8; 20];
    a[19] = byte;
    Address::from_slice(&a)
}

#[test]
fn perfect_validator_scores_high() {
    let mut db = ReputationDatabase::new();
    let a = addr(1);
    // perfect history
    for b in 1..=1_100 {
        db.update_proposed(a, b, 50);
    }
    let rec = db.get_record(&a).unwrap();
    let score = ValidatorScorer::calculate_score(rec, 11_000);
    assert!(score > 900, "expected high score, got {}", score);
}

#[test]
fn poor_validator_scores_low() {
    let mut db = ReputationDatabase::new();
    let a = addr(2);
    for b in 1..=500 {
        db.update_missed(a, b);
    }
    for b in 501..=600 {
        db.update_invalid(a, b);
    }
    let mut rec = db.get_record(&a).unwrap().clone();
    rec.slash_count = 2;
    let score = ValidatorScorer::calculate_score(&rec, 2_000);
    assert!(score < 300, "expected low score, got {}", score);
}

#[test]
fn grace_period_uses_stake_only() {
    let cfg = SelectorConfig {
        grace_period_blocks: 100,
        ..SelectorConfig::default()
    };
    let mut selector = AIValidatorSelector::new(cfg);
    let a = addr(3);
    // no history => grace period
    let cands = vec![
        ValidatorCandidate { address: a, stake_amount: 1_000 },
        ValidatorCandidate { address: addr(4), stake_amount: 100 },
    ];
    let chosen = selector.select_validator(cands, 10, [0u8; 32]);
    assert_eq!(chosen, a);
}

#[test]
fn minimum_reputation_filters() {
    let mut db = ReputationDatabase::new();
    let good = addr(5);
    let bad = addr(6);
    for b in 1..=200 {
        db.update_proposed(good, b, 80);
    }
    for b in 1..=50 {
        db.update_missed(bad, b);
    }
    let cfg = SelectorConfig {
        min_reputation_score: 100,
        grace_period_blocks: 0,
        ..SelectorConfig::default()
    };
    let mut selector = AIValidatorSelector::new(cfg);
    selector.reputation_db = db;

    let cands = vec![
        ValidatorCandidate { address: good, stake_amount: 1_000 },
        ValidatorCandidate { address: bad, stake_amount: 1_000_000 },
    ];
    let chosen = selector.select_validator(cands, 5_000, [1u8; 32]);
    assert_eq!(chosen, good);
}

#[test]
fn deterministic_selection_same_hash() {
    let cfg = SelectorConfig::default();
    let selector = AIValidatorSelector::new(cfg);
    let cands = vec![
        ValidatorCandidate { address: addr(10), stake_amount: 1_000 },
        ValidatorCandidate { address: addr(11), stake_amount: 2_000 },
        ValidatorCandidate { address: addr(12), stake_amount: 3_000 },
    ];
    let hash = [42u8; 32];
    let a = selector.select_validator(cands.clone(), 0, hash);
    let b = selector.select_validator(cands, 0, hash);
    assert_eq!(a, b);
}

