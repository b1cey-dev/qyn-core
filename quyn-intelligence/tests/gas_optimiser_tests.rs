use quyn_intelligence::gas_optimiser::{
    BlockMetrics,
    CongestionLevel,
    GasConfig,
    GasOptimiser,
};

fn make_metrics(block: u64, congestion: f64) -> BlockMetrics {
    BlockMetrics {
        block_number: block,
        transaction_count: 10,
        average_gas_used: 21_000,
        timestamp: block * 3,
        congestion_score: congestion,
    }
}

#[test]
fn test_empty_history_defaults() {
    let opt = GasOptimiser::new(GasConfig::default());
    let pred = opt.predict_gas_price(0);
    assert_eq!(pred.recommended_gas_price, opt.config.base_fee);
    assert_eq!(pred.congestion_level, CongestionLevel::Low);
}

#[test]
fn test_low_congestion_prediction() {
    let mut opt = GasOptimiser::new(GasConfig::default());
    for i in 1..=50 {
        opt.record_block(make_metrics(i, 0.1));
    }
    let pred = opt.predict_gas_price(100);
    assert_eq!(pred.congestion_level, CongestionLevel::Low);
    assert_eq!(pred.estimated_confirmation_blocks, 1);
}

#[test]
fn test_high_congestion_prediction() {
    let mut opt = GasOptimiser::new(GasConfig::default());
    for i in 1..=50 {
        opt.record_block(make_metrics(i, 0.7));
    }
    let pred = opt.predict_gas_price(100);
    assert_eq!(pred.congestion_level, CongestionLevel::High);
}

#[test]
fn test_critical_congestion_prediction() {
    let mut opt = GasOptimiser::new(GasConfig::default());
    for i in 1..=50 {
        opt.record_block(make_metrics(i, 0.9));
    }
    let pred = opt.predict_gas_price(100);
    assert_eq!(pred.congestion_level, CongestionLevel::Critical);
    assert!(
        pred.optimal_send_window.contains("Network busy"),
        "expected busy message, got {}",
        pred.optimal_send_window
    );
}

#[test]
fn test_gas_price_scaling() {
    let mut cfg = GasConfig::default();
    cfg.base_fee = 1_000_000_000;
    let mut opt = GasOptimiser::new(cfg.clone());

    // low
    opt.record_block(make_metrics(1, 0.1));
    let low = opt.predict_gas_price(2).recommended_gas_price;

    // medium
    opt.record_block(make_metrics(2, 0.4));
    let med = opt.predict_gas_price(3).recommended_gas_price;

    // critical
    for i in 3..10 {
        opt.record_block(make_metrics(i, 0.9));
    }
    let crit = opt.predict_gas_price(20).recommended_gas_price;

    assert!(med >= low);
    assert!(crit >= med);
}

