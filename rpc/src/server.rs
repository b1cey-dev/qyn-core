//! JSON-RPC and REST server.

use axum::extract::ConnectInfo;
use axum::extract::DefaultBodyLimit;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use std::net::SocketAddr;
use quyn_core::{
    ChainDB, Mempool, StateDB,
    validation::{validate_tx_basic, validate_tx_against_state},
};
use quyn_intelligence::{
    ConcentrationMonitor, ContractScanner, FraudConfig, FraudDetector, FraudRecommendation,
    RugPullConfig, RugPullDetector,
    gas_optimiser::{BlockMetrics, CongestionLevel, GasConfig, GasOptimiser},
    AiGeneratedStatus, ContentType, CredibilityScore, ContentVerification,
};
use alloy_primitives::{Address, B256, U256};
use rlp::Rlp;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::{Duration, Instant};
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tower_http::timeout::TimeoutLayer;

pub type SharedChain = Arc<ChainDB>;
pub type SharedState = Arc<StateDB>;
pub type SharedMempool = Arc<Mempool>;

/// Max RPC requests per IP per second.
const RATE_LIMIT_PER_SEC: u32 = 100;
/// Max request body size (1MB).
const MAX_BODY_BYTES: usize = 1024 * 1024;
/// Request timeout.
const REQUEST_TIMEOUT_SECS: u64 = 30;

#[derive(Clone)]
pub struct AppState {
    pub chain: SharedChain,
    pub state: SharedState,
    pub mempool: SharedMempool,
    pub chain_id: u64,
    pub rate_limiter: Arc<tokio::sync::RwLock<HashMap<String, (u32, Instant)>>>,
    pub rug_pull_detector: Arc<RwLock<RugPullDetector>>,
    pub concentration_monitor: Arc<RwLock<ConcentrationMonitor>>,
}

/// Serve RPC and REST until shutdown. Pass chain_id so devnet can use 7779 and mainnet 7777.
/// In production (env QYN_PRODUCTION=1), CORS is restricted to getquyn.com and testnet.getquyn.com.
pub async fn serve(
    chain: SharedChain,
    state: SharedState,
    mempool: SharedMempool,
    chain_id: u64,
    addr: String,
) -> Result<(), crate::error::RpcError> {
    let rate_limiter = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
    let rug_pull_detector = Arc::new(RwLock::new(RugPullDetector::new(RugPullConfig::default())));
    let concentration_monitor = Arc::new(RwLock::new(ConcentrationMonitor::new()));
    let app_state = AppState {
        chain,
        state,
        mempool,
        chain_id,
        rate_limiter,
        rug_pull_detector,
        concentration_monitor,
    };

    let cors = if std::env::var("QYN_PRODUCTION").map(|v| v == "1").unwrap_or(false) {
        CorsLayer::new()
            .allow_origin([
                "https://getquyn.com".parse().expect("valid CORS origin"),
                "https://testnet.getquyn.com".parse().expect("valid CORS origin"),
            ])
            .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
    } else {
        CorsLayer::permissive()
    };

    let app = Router::new()
        .route("/", get(health).post(jsonrpc_handler))
        .route("/rpc", get(rpc_chain_id_get).post(jsonrpc_handler))
        .route("/health", get(health))
        .layer(
            ServiceBuilder::new()
                .layer(TimeoutLayer::new(Duration::from_secs(REQUEST_TIMEOUT_SECS)))
                .layer(DefaultBodyLimit::max(MAX_BODY_BYTES)),
        )
        .layer(cors)
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| crate::error::RpcError::Internal(e.to_string()))?;
    tracing::info!("RPC listening on {} (body limit {} bytes, timeout {}s)", addr, MAX_BODY_BYTES, REQUEST_TIMEOUT_SECS);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .map_err(|e| crate::error::RpcError::Internal(e.to_string()))?;
    Ok(())
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({"status":"ok","network":"quyn"})))
}

/// GET /rpc: return chain ID so wallets that probe with GET get a valid response.
async fn rpc_chain_id_get(State(state): State<AppState>) -> impl IntoResponse {
    let result = format!("0x{:x}", state.chain_id);
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": result
        })),
    )
}

async fn jsonrpc_handler(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let ip = peer.ip().to_string();
    {
        let now = Instant::now();
        let mut limiter = state.rate_limiter.write().await;
        let (count, window_start) = limiter.entry(ip.clone()).or_insert((0, now));
        if now.duration_since(*window_start) >= Duration::from_secs(1) {
            *count = 0;
            *window_start = now;
        }
        *count += 1;
        if *count > RATE_LIMIT_PER_SEC {
            tracing::warn!("Rate limit exceeded for suspicious IP: {} ({} req/s)", ip, *count);
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": body.get("id"),
                    "error": {"code": -32005, "message": "rate limit exceeded"}
                })),
            );
        }
    }
    let method = body.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = body.get("params").cloned().unwrap_or(Value::Array(vec![]));
    let id = body.get("id").cloned();
    tracing::info!("RPC REQUEST: method={}, params_len={}", method, params.to_string().len());
    let result = dispatch(state, method, params).await;
    let response = if result.get("error").is_some() {
        serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": result["error"] })
    } else {
        serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result })
    };
    let resp_str = response.to_string();
    tracing::info!("RPC RESPONSE: method={}, result_len={}", method, resp_str.len());
    (StatusCode::OK, Json(response))
}

fn param_str(params: &Value, i: usize) -> Option<&str> {
    params.get(i).and_then(|p| p.as_str())
}

fn require_params_array(params: &Value) -> Result<&Vec<Value>, Value> {
    params.as_array().ok_or_else(|| error_value("params must be an array"))
}

fn require_param_count(params: &Value, min_len: usize) -> Result<(), Value> {
    let arr = require_params_array(params)?;
    if arr.len() < min_len {
        return Err(error_value(format!(
            "method requires at least {} parameter(s), got {}",
            min_len,
            arr.len()
        )));
    }
    Ok(())
}

fn require_param_string(params: &Value, i: usize) -> Result<(), Value> {
    require_param_count(params, i + 1)?;
    if params.get(i).and_then(|p| p.as_str()).is_none() {
        return Err(error_value(format!("parameter {} must be a string", i)));
    }
    Ok(())
}

/// Decode RLP bytes to U256 (big-endian, left-padded).
fn rlp_bytes_to_u256(b: &[u8]) -> U256 {
    let mut arr = [0u8; 32];
    let len = b.len().min(32);
    let start = 32 - len;
    arr[start..].copy_from_slice(&b[b.len().saturating_sub(len)..]);
    U256::from_be_bytes(arr)
}

/// Parse Ethereum legacy RLP tx: [nonce, gasPrice, gasLimit, to, value, data, v, r, s]
/// raw_bytes: full RLP for tx hash (keccak256) so MetaMask receipt lookup works.
fn parse_legacy_tx(raw_bytes: &[u8]) -> Result<quyn_core::SignedTransaction, String> {
    let bytes = raw_bytes;
    let rlp = Rlp::new(bytes);
    if !rlp.is_list() {
        return Err("expected RLP list".into());
    }
    let item_count = rlp.item_count().map_err(|e| e.to_string())?;
    if item_count < 9 {
        return Err(format!("legacy tx expects 9 fields, got {}", item_count));
    }
    let nonce: u64 = rlp.val_at(0).map_err(|e| e.to_string())?;
    let gas_price = rlp_bytes_to_u256(rlp.at(1).map_err(|e| e.to_string())?.data().map_err(|e| e.to_string())?);
    let gas_limit: u64 = rlp.val_at(2).map_err(|e| e.to_string())?;
    let to_bytes = rlp.at(3).map_err(|e| e.to_string())?.data().map_err(|e| e.to_string())?;
    let to = if to_bytes.is_empty() {
        None
    } else if to_bytes.len() == 20 {
        Some(Address::from_slice(to_bytes))
    } else {
        return Err("invalid 'to' address length".into());
    };
    let value = rlp_bytes_to_u256(rlp.at(4).map_err(|e| e.to_string())?.data().map_err(|e| e.to_string())?);
    let data = rlp.at(5).map_err(|e| e.to_string())?.data().map_err(|e| e.to_string())?.to_vec();
    let v: u64 = rlp.val_at(6).map_err(|e| e.to_string())?;
    let r_bytes = rlp.at(7).map_err(|e| e.to_string())?.data().map_err(|e| e.to_string())?;
    let s_bytes = rlp.at(8).map_err(|e| e.to_string())?.data().map_err(|e| e.to_string())?;

    let chain_id = if v >= 35 { (v - 35) / 2 } else { 0 };
    let v_byte: u8 = if v >= 35 { ((v - 35) % 2 + 27) as u8 } else { v as u8 };

    let mut r = [0u8; 32];
    let mut s = [0u8; 32];
    let r_len = r_bytes.len().min(32);
    let s_len = s_bytes.len().min(32);
    r[32 - r_len..].copy_from_slice(&r_bytes[r_bytes.len().saturating_sub(r_len)..]);
    s[32 - s_len..].copy_from_slice(&s_bytes[s_bytes.len().saturating_sub(s_len)..]);

    let hash_override = Some(alloy_primitives::keccak256(raw_bytes));

    Ok(quyn_core::SignedTransaction {
        transaction: quyn_core::Transaction {
            nonce,
            gas_price,
            gas_limit,
            to,
            value,
            data,
            chain_id,
        },
        r,
        s,
        v: v_byte,
        hash_override,
    })
}

/// Parse EIP-1559 (type 2) tx: [chainId, nonce, maxPriorityFeePerGas, maxFeePerGas, gasLimit, to, value, data, accessList, signatureYParity, r, s]
/// raw_bytes: full payload including 0x02 prefix, used for tx hash.
fn parse_eip1559_tx(raw_bytes: &[u8]) -> Result<quyn_core::SignedTransaction, String> {
    let bytes = if raw_bytes.first() == Some(&0x02) { &raw_bytes[1..] } else { raw_bytes };
    if bytes.len() < 2 {
        return Err("EIP-1559 payload too short".into());
    }
    let rlp = Rlp::new(bytes);
    if !rlp.is_list() {
        return Err("EIP-1559 expected RLP list".into());
    }
    let item_count = rlp.item_count().map_err(|e| format!("EIP-1559 item_count: {}", e))?;
    if item_count < 11 {
        return Err(format!("EIP-1559 tx expects 11-12 fields, got {} (payload len={}, first_bytes={:?})", item_count, bytes.len(), &bytes[..bytes.len().min(4)]));
    }
    let chain_id: u64 = rlp.val_at(0).map_err(|e| e.to_string())?;
    let nonce: u64 = rlp.val_at(1).map_err(|e| e.to_string())?;
    let _max_priority = rlp_bytes_to_u256(rlp.at(2).map_err(|e| e.to_string())?.data().map_err(|e| e.to_string())?);
    let max_fee = rlp_bytes_to_u256(rlp.at(3).map_err(|e| e.to_string())?.data().map_err(|e| e.to_string())?);
    let gas_limit: u64 = rlp.val_at(4).map_err(|e| e.to_string())?;
    let to_bytes = rlp.at(5).map_err(|e| e.to_string())?.data().map_err(|e| e.to_string())?;
    let to = if to_bytes.is_empty() {
        None
    } else if to_bytes.len() == 20 {
        Some(Address::from_slice(to_bytes))
    } else {
        return Err("invalid 'to' address length".into());
    };
    let value = rlp_bytes_to_u256(rlp.at(6).map_err(|e| e.to_string())?.data().map_err(|e| e.to_string())?);
    let data = rlp.at(7).map_err(|e| e.to_string())?.data().map_err(|e| e.to_string())?.to_vec();
    let _access_list = rlp.at(8).map_err(|e| e.to_string())?;
    let parity: u64 = rlp.val_at(9).map_err(|e| e.to_string())?;
    let v_byte = 27 + (parity as u8);
    let r_bytes = rlp.at(10).map_err(|e| e.to_string())?.data().map_err(|e| e.to_string())?;
    let s_bytes = rlp.at(11).map_err(|e| e.to_string())?.data().map_err(|e| e.to_string())?;

    let mut r = [0u8; 32];
    let mut s = [0u8; 32];
    let r_len = r_bytes.len().min(32);
    let s_len = s_bytes.len().min(32);
    r[32 - r_len..].copy_from_slice(&r_bytes[r_bytes.len().saturating_sub(r_len)..]);
    s[32 - s_len..].copy_from_slice(&s_bytes[s_bytes.len().saturating_sub(s_len)..]);

    let hash_override = Some(alloy_primitives::keccak256(raw_bytes));

    Ok(quyn_core::SignedTransaction {
        transaction: quyn_core::Transaction {
            nonce,
            gas_price: max_fee,
            gas_limit,
            to,
            value,
            data,
            chain_id,
        },
        r,
        s,
        v: v_byte,
        hash_override,
    })
}

async fn dispatch(state: AppState, method: &str, params: Value) -> Value {
    match method {
        "eth_blockNumber" => {
            let head = state.chain.get_head().ok().flatten();
            let num = head
                .and_then(|h| state.chain.get_block(&h).ok().flatten())
                .map(|b| format!("0x{:x}", b.header.number))
                .unwrap_or_else(|| "0x0".into());
            Value::String(num)
        }
        "eth_chainId" => Value::String(format!("0x{:x}", state.chain_id)),
        "net_version" => Value::String(state.chain_id.to_string()),
        "eth_gasPrice" => Value::String("0x3B9ACA00".to_string()), // 1 gwei
        "eth_estimateGas" => Value::String("0x5208".to_string()),   // 21000 for simple transfers
        "eth_feeHistory" => serde_json::json!({
            "oldestBlock": "0x0",
            "gasUsedRatio": [0.0],
            "reward": [["0x3B9ACA00"]]
        }),
        "eth_maxPriorityFeePerGas" => Value::String("0x3B9ACA00".to_string()), // 1 gwei
        "quyn_health" => Value::String("ok".to_string()),
        "eth_getBalance" => {
            if let Err(e) = require_param_string(&params, 0) {
                return e;
            }
            let addr_hex = param_str(&params, 0).unwrap_or("");
            let addr = match parse_address(addr_hex) {
                Ok(a) => a,
                Err(e) => return error_value(e),
            };
            match state.state.get_balance(&addr) {
                Ok(bal) => Value::String(format!("0x{:x}", bal)),
                Err(e) => error_value(e.to_string()),
            }
        }
        "eth_getTransactionCount" => {
            if let Err(e) = require_param_string(&params, 0) {
                return e;
            }
            let addr_hex = param_str(&params, 0).unwrap_or("");
            let addr = match parse_address(addr_hex) {
                Ok(a) => a,
                Err(e) => return error_value(e),
            };
            match state.state.get_nonce(&addr) {
                Ok(n) => Value::String(format!("0x{:x}", n)),
                Err(e) => error_value(e.to_string()),
            }
        }
        "eth_sendRawTransaction" => {
            if let Err(e) = require_param_string(&params, 0) {
                return e;
            }
            let raw_hex = params
                .get(0)
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .trim_start_matches("0x");
            tracing::info!("eth_sendRawTransaction: raw hex len={}", raw_hex.len());
            if raw_hex.len() % 2 != 0 {
                tracing::error!("eth_sendRawTransaction: odd number of hex digits (len={})", raw_hex.len());
                return error_value_with_code(-32602, "Invalid hex: odd number of digits");
            }
            let bytes = match hex::decode(raw_hex) {
                Ok(b) => b,
                Err(e) => {
                    tracing::error!("eth_sendRawTransaction: hex decode error: {} (len={})", e, raw_hex.len());
                    return error_value_with_code(-32602, format!("Invalid hex: {}", e));
                }
            };
            if bytes.is_empty() {
                tracing::error!("eth_sendRawTransaction: empty transaction");
                return error_value("Empty transaction");
            }
            let tx_type = if bytes[0] == 0x02 { "EIP-1559" } else { "Legacy" };
            tracing::info!("eth_sendRawTransaction: bytes len={}, type={}, first_bytes={:?}", bytes.len(), tx_type, &bytes[..bytes.len().min(32)]);
            let tx_result: Result<quyn_core::SignedTransaction, String> = if bytes[0] == 0x02 {
                parse_eip1559_tx(&bytes)
            } else {
                parse_legacy_tx(&bytes).or_else(|_| {
                    bincode::deserialize(&bytes).map_err(|e| e.to_string())
                })
            };
            let tx = match tx_result {
                Ok(t) => t,
                Err(e) => {
                    tracing::error!("eth_sendRawTransaction: parse error: {}", e);
                    return error_value(format!("Failed to parse tx: {}", e));
                }
            };
            let tx_hash = tx.hash();
            let sender = match tx.sender() {
                Ok(a) => format!("0x{}", hex::encode(a.as_slice())),
                Err(e) => {
                    tracing::error!("eth_sendRawTransaction: signature recovery failed: {}", e);
                    return error_value(format!("Invalid signature: {}", e));
                }
            };
            tracing::info!("eth_sendRawTransaction: parsed tx hash=0x{}, sender={}, nonce={}", hex::encode(tx_hash.as_slice()), sender, tx.nonce());
            if let Err(e) = validate_tx_basic(&tx, state.chain_id) {
                tracing::error!("eth_sendRawTransaction: validate_tx_basic failed: {}", e);
                return error_value(e.to_string());
            }
            if let Err(e) = validate_tx_against_state(&tx, &state.state) {
                tracing::error!("eth_sendRawTransaction: validate_tx_against_state failed: {}", e);
                return error_value(e.to_string());
            }
            match state.mempool.insert(tx) {
                Ok(_) => {
                    let hash_hex = format!("0x{}", hex::encode(tx_hash.as_slice()));
                    tracing::info!("eth_sendRawTransaction: tx accepted, hash={}", hash_hex);
                    Value::String(hash_hex)
                }
                Err(e) => {
                    tracing::error!("eth_sendRawTransaction: mempool insert failed: {}", e);
                    error_value(e.to_string())
                }
            }
        }
        "eth_getCode" => {
            let addr_hex = param_str(&params, 0).unwrap_or("");
            let addr = match parse_address(addr_hex) {
                Ok(a) => a,
                Err(e) => return error_value(e),
            };
            match state.state.get_code(&addr) {
                Ok(code) => Value::String(format!("0x{}", hex::encode(&code))),
                Err(e) => error_value(e.to_string()),
            }
        }
        "quyn_getBlockByNumber" | "eth_getBlockByNumber" => {
            if let Err(e) = require_param_count(&params, 1) {
                return e;
            }
            let tag = param_str(&params, 0).unwrap_or("latest");
            let full_tx = params.get(1).and_then(|p| p.as_bool()).unwrap_or(false);
            let block_number = if tag == "latest" || tag == "pending" {
                state.chain.get_head().ok().flatten()
                    .and_then(|h| state.chain.get_block(&h).ok().flatten())
                    .map(|b| b.header.number)
            } else {
                u64::from_str_radix(tag.trim_start_matches("0x"), 16).ok()
            };
            let block_number = match block_number {
                Some(n) => n,
                None => return Value::Null,
            };
            let block = match state.chain.get_block_by_number(block_number) {
                Ok(Some(b)) => b,
                _ => return Value::Null,
            };
            let txs_value = if full_tx {
                Value::Array(block.body.transactions.iter().map(tx_to_json).collect())
            } else {
                Value::Array(
                    block.body.transactions.iter()
                        .map(|tx| Value::String(format!("0x{}", hex::encode(tx.hash().as_slice()))))
                        .collect()
                )
            };
            serde_json::json!({
                "number": format!("0x{:x}", block.header.number),
                "hash": format!("0x{}", hex::encode(block.hash().as_slice())),
                "parentHash": format!("0x{}", hex::encode(block.header.parent_hash.as_slice())),
                "stateRoot": format!("0x{}", hex::encode(block.header.state_root.as_slice())),
                "transactionsRoot": format!("0x{}", hex::encode(block.header.transactions_root.as_slice())),
                "receiptsRoot": format!("0x{}", hex::encode(block.header.receipts_root.as_slice())),
                "timestamp": format!("0x{:x}", block.header.timestamp),
                "miner": format!("0x{}", hex::encode(block.header.validator.as_slice())),
                "transactions": txs_value,
                "gasLimit": format!("0x{:x}", block.header.gas_limit),
                "gasUsed": "0x0",
                "sha3Uncles": "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
                "mixHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
                "nonce": "0x0000000000000000",
                "logsBloom": format!("0x{}", "00".repeat(256)),
                "difficulty": "0x0",
                "totalDifficulty": "0x0",
                "extraData": "0x",
                "size": "0x0",
                "uncles": []
            })
        }
        "quyn_getBlockByHash" | "eth_getBlockByHash" => {
            if let Err(e) = require_param_string(&params, 0) {
                return e;
            }
            let hash_hex = param_str(&params, 0).unwrap_or("");
            let block_hash = match parse_tx_hash(hash_hex) {
                Ok(h) => h,
                Err(e) => return error_value(e),
            };
            let full_tx = params.get(1).and_then(|p| p.as_bool()).unwrap_or(false);
            let block = match state.chain.get_block(&block_hash) {
                Ok(Some(b)) => b,
                _ => return Value::Null,
            };
            let txs_value = if full_tx {
                Value::Array(block.body.transactions.iter().map(tx_to_json).collect())
            } else {
                Value::Array(
                    block.body.transactions.iter()
                        .map(|tx| Value::String(format!("0x{}", hex::encode(tx.hash().as_slice()))))
                        .collect()
                )
            };
            serde_json::json!({
                "number": format!("0x{:x}", block.header.number),
                "hash": format!("0x{}", hex::encode(block.hash().as_slice())),
                "parentHash": format!("0x{}", hex::encode(block.header.parent_hash.as_slice())),
                "stateRoot": format!("0x{}", hex::encode(block.header.state_root.as_slice())),
                "transactionsRoot": format!("0x{}", hex::encode(block.header.transactions_root.as_slice())),
                "receiptsRoot": format!("0x{}", hex::encode(block.header.receipts_root.as_slice())),
                "timestamp": format!("0x{:x}", block.header.timestamp),
                "miner": format!("0x{}", hex::encode(block.header.validator.as_slice())),
                "transactions": txs_value,
                "gasLimit": format!("0x{:x}", block.header.gas_limit),
                "gasUsed": "0x0",
                "sha3Uncles": "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
                "mixHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
                "nonce": "0x0000000000000000",
                "logsBloom": format!("0x{}", "00".repeat(256)),
                "difficulty": "0x0",
                "totalDifficulty": "0x0",
                "extraData": "0x",
                "size": "0x0",
                "uncles": []
            })
        }
        "quyn_getTransactionByHash" | "eth_getTransactionByHash" => {
            if let Err(e) = require_param_string(&params, 0) {
                return e;
            }
            let hash_hex = param_str(&params, 0).unwrap_or("");
            let tx_hash = match parse_tx_hash(hash_hex) {
                Ok(h) => h,
                Err(e) => return error_value(e),
            };
            match state.chain.get_tx_receipt_index(&tx_hash) {
                Ok(Some((block_hash, block_number, index, _gas_used))) => {
                    match state.chain.get_block(&block_hash).ok().flatten()
                        .and_then(|b| b.body.transactions.get(index as usize).cloned())
                    {
                        Some(tx) => {
                            let mut j = tx_to_json(&tx);
                            if let Some(obj) = j.as_object_mut() {
                                obj.insert("blockHash".into(), serde_json::json!(format!("0x{}", hex::encode(block_hash.as_slice()))));
                                obj.insert("blockNumber".into(), serde_json::json!(format!("0x{:x}", block_number)));
                                obj.insert("transactionIndex".into(), serde_json::json!(format!("0x{:x}", index)));
                                obj.insert("input".into(), serde_json::json!(format!("0x{}", hex::encode(&tx.transaction.data))));
                                obj.insert("v".into(), serde_json::json!(format!("0x{:x}", tx.v)));
                                obj.insert("r".into(), serde_json::json!(format!("0x{}", hex::encode(&tx.r))));
                                obj.insert("s".into(), serde_json::json!(format!("0x{}", hex::encode(&tx.s))));
                            }
                            j
                        }
                        None => Value::Null,
                    }
                }
                Ok(None) => Value::Null,
                Err(e) => error_value(e.to_string()),
            }
        }
        "quyn_getAddressTransactions" => {
            if let Err(e) = require_param_string(&params, 0) {
                return e;
            }
            let addr_hex = param_str(&params, 0).unwrap_or("");
            let addr = match parse_address(addr_hex) {
                Ok(a) => a,
                Err(e) => return error_value(e),
            };
            let limit = params.get(1).and_then(|p| p.as_u64()).unwrap_or(100);
            let head = state.chain.get_head().ok().flatten();
            let head_num = head.and_then(|h| state.chain.get_block(&h).ok().flatten()).map(|b| b.header.number).unwrap_or(0);
            let mut txs = Vec::new();
            let mut count = 0u64;
            for n in (0..=head_num).rev() {
                if count >= limit {
                    break;
                }
                let block = match state.chain.get_block_by_number(n) {
                    Ok(Some(b)) => b,
                    _ => continue,
                };
                for (i, tx) in block.body.transactions.iter().enumerate() {
                    let from = tx.sender().ok().unwrap_or(Address::ZERO);
                    let to = tx.to();
                    if from == addr || to == Some(addr) {
                        let mut j = tx_to_json(tx);
                        if let Some(obj) = j.as_object_mut() {
                            obj.insert("blockNumber".into(), serde_json::json!(format!("0x{:x}", n)));
                            obj.insert("blockHash".into(), serde_json::json!(format!("0x{}", hex::encode(block.hash().as_slice()))));
                            obj.insert("transactionIndex".into(), serde_json::json!(format!("0x{:x}", i)));
                        }
                        txs.push(j);
                        count += 1;
                        if count >= limit {
                            break;
                        }
                    }
                }
            }
            Value::Array(txs)
        }
        "quyn_getNetworkStats" => {
            let head = state.chain.get_head().ok().flatten();
            let (block_count, total_txs) = head
                .and_then(|h| state.chain.get_block(&h).ok().flatten())
                .map(|b| {
                    let mut txs = 0u64;
                    for n in 0..=b.header.number {
                        if let Ok(Some(blk)) = state.chain.get_block_by_number(n) {
                            txs += blk.body.transactions.len() as u64;
                        }
                    }
                    (b.header.number + 1, txs)
                })
                .unwrap_or((0, 0));
            let block_time = 3u64;
            let tps = if block_count > 0 {
                (total_txs as f64) / ((block_count as f64) * (block_time as f64))
            } else {
                0.0
            };
            serde_json::json!({
                "blockCount": block_count,
                "totalTransactions": total_txs,
                "blockTimeSecs": block_time,
                "estimatedTps": format!("{:.2}", tps),
                "chainId": format!("0x{:x}", state.chain_id),
            })
        }
        "quyn_getValidatorList" => {
            let bytes = state.chain.get_validator_set_bytes().ok().flatten().unwrap_or_default();
            let validators: Vec<Value> = match bincode::deserialize::<quyn_consensus::ValidatorSet>(&bytes) {
                Ok(set) => set
                    .active_validators()
                    .iter()
                    .map(|v| serde_json::json!({
                        "address": format!("0x{}", hex::encode(v.address.as_slice())),
                        "stake": format!("0x{:x}", v.stake),
                        "delegated": format!("0x{:x}", v.delegated),
                        "active": v.active,
                    }))
                    .collect(),
                Err(_) => vec![],
            };
            Value::Array(validators)
        }
        "quyn_getValidatorStats" => {
            let bytes = state.chain.get_validator_set_bytes().ok().flatten().unwrap_or_default();
            let (count, total_stake) = match bincode::deserialize::<quyn_consensus::ValidatorSet>(&bytes) {
                Ok(set) => {
                    let active = set.active_validators();
                    let stake: u128 = active.iter().map(|v| v.total_stake().to::<u128>()).sum();
                    (active.len(), stake)
                }
                Err(_) => (0, 0u128),
            };
            serde_json::json!({
                "validatorCount": count,
                "totalStaked": format!("0x{:x}", total_stake),
            })
        }
        "quyn_getTransactionReceipt" | "eth_getTransactionReceipt" => {
            if let Err(e) = require_param_string(&params, 0) {
                return e;
            }
            let hash_hex = param_str(&params, 0).unwrap_or("");
            let tx_hash = match parse_tx_hash(hash_hex) {
                Ok(h) => h,
                Err(e) => return error_value(e),
            };
            match state.chain.get_tx_receipt_index(&tx_hash) {
                Ok(Some((block_hash, block_number, index, gas_used))) => {
                    let (from, to_val) = state.chain.get_block(&block_hash).ok().flatten()
                        .and_then(|b| b.body.transactions.get(index as usize).cloned())
                        .map(|tx| (
                            format!("0x{}", hex::encode(tx.sender().ok().unwrap_or(Address::ZERO).as_slice())),
                            tx.to().map(|a| serde_json::Value::String(format!("0x{}", hex::encode(a.as_slice())))),
                        ))
                        .unwrap_or((
                            format!("0x{}", hex::encode(Address::ZERO.as_slice())),
                            Some(serde_json::Value::String(format!("0x{}", hex::encode(Address::ZERO.as_slice())))),
                        ));
                    let to = to_val.unwrap_or(serde_json::Value::Null);
                    let logs_bloom = "0x".to_string() + &"0".repeat(512);
                    serde_json::json!({
                        "transactionHash": format!("0x{}", hex::encode(tx_hash.as_slice())),
                        "blockHash": format!("0x{}", hex::encode(block_hash.as_slice())),
                        "blockNumber": format!("0x{:x}", block_number),
                        "transactionIndex": format!("0x{:x}", index),
                        "from": from,
                        "to": to,
                        "status": "0x1",
                        "gasUsed": format!("0x{:x}", gas_used),
                        "cumulativeGasUsed": format!("0x{:x}", gas_used),
                        "logs": [],
                        "logsBloom": logs_bloom,
                    })
                },
                Ok(None) => Value::Null,
                Err(e) => error_value(e.to_string()),
            }
        }
        "qyn_getFraudAnalysis" => {
            if let Err(e) = require_param_string(&params, 0) {
                return e;
            }
            let hash_hex = param_str(&params, 0).unwrap_or("");
            let tx_hash = match parse_tx_hash(hash_hex) {
                Ok(h) => h,
                Err(e) => return error_value(e),
            };
            let tx_hash_arr: [u8; 32] = tx_hash.0;
            let (tx, block_number) = match state.chain.get_tx_receipt_index(&tx_hash) {
                Ok(Some((block_hash, block_number, index, _))) => {
                    match state.chain.get_block(&block_hash).ok().flatten()
                        .and_then(|b| b.body.transactions.get(index as usize).cloned())
                    {
                        Some(tx) => (tx, block_number),
                        None => return Value::Null,
                    }
                }
                Ok(None) => {
                    match state.mempool.get_by_hash(&tx_hash_arr).ok().flatten() {
                        Some(tx) => {
                            let head_num = state.chain.get_head().ok().flatten()
                                .and_then(|h| state.chain.get_block(&h).ok().flatten())
                                .map(|b| b.header.number)
                                .unwrap_or(0);
                            (tx, head_num + 1)
                        }
                        None => return Value::Null,
                    }
                }
                Err(e) => return error_value(e.to_string()),
            };
            let detector = FraudDetector::new_with_rug_pull(
                FraudConfig::default(),
                state.rug_pull_detector.clone(),
            );
            match detector.analyse_transaction(&tx, &state.chain, &state.state, block_number) {
                Ok(analysis) => serde_json::json!({
                    "transactionHash": format!("0x{}", hex::encode(analysis.transaction_hash)),
                    "riskScore": analysis.risk_score,
                    "flags": analysis.flags.iter().map(|f| f.as_str()).collect::<Vec<_>>(),
                    "recommendation": match analysis.recommendation {
                        FraudRecommendation::Include => "Include",
                        FraudRecommendation::IncludeWithLog => "IncludeWithLog",
                        FraudRecommendation::Delay => "Delay",
                        FraudRecommendation::Reject => "Reject",
                    },
                    "timestamp": analysis.timestamp,
                }),
                Err(e) => error_value(e.to_string()),
            }
        }
        "qyn_verifyContent" => {
            // Prototype of the QYN Verify RPC: analyse a content identifier (URL or text)
            // and return a structured verification result. Full on-chain recording can
            // extend this schema in a later phase.
            if let Err(e) = require_param_string(&params, 0) {
                return e;
            }
            let input = param_str(&params, 0).unwrap_or("").trim();
            let ctype_str = params
                .get(1)
                .and_then(|p| p.as_str())
                .unwrap_or("unknown")
                .to_lowercase();
            let content_type = match ctype_str.as_str() {
                "article" => ContentType::Article,
                "image" => ContentType::Image,
                "video" => ContentType::Video,
                "document" => ContentType::Document,
                "socialpost" | "social_post" | "post" => ContentType::SocialPost,
                "text" => ContentType::Text,
                _ => ContentType::Unknown,
            };

            // Derive a deterministic content hash from the input string for now.
            let content_hash = alloy_primitives::keccak256(input.as_bytes());

            // Placeholder scoring logic. This can later be replaced by a dedicated
            // content analysis pipeline inside quyn-intelligence.
            let len = input.len();
            let (trust_score, ai_generated, manipulation_detected, source_credibility) = if len < 32 {
                (50u8, AiGeneratedStatus::Unknown, false, CredibilityScore::Unknown)
            } else if len < 280 {
                (72u8, AiGeneratedStatus::Unknown, false, CredibilityScore::Medium)
            } else {
                (88u8, AiGeneratedStatus::LikelyAiGenerated, false, CredibilityScore::High)
            };

            let verified_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let verification = ContentVerification {
                verification_id: alloy_primitives::keccak256(
                    [content_hash.as_slice(), verified_at.to_be_bytes().as_slice()].concat().as_slice(),
                ),
                content_hash,
                content_url: if input.starts_with("http://") || input.starts_with("https://") {
                    Some(input.to_string())
                } else {
                    None
                },
                content_type,
                trust_score,
                ai_generated,
                manipulation_detected,
                source_credibility,
                original_source: None,
                alteration_history: None,
                verified_at,
            };

            serde_json::json!({
                "verificationId": format!("0x{}", hex::encode(verification.verification_id)),
                "contentHash": format!("0x{}", hex::encode(verification.content_hash)),
                "contentUrl": verification.content_url,
                "contentType": match verification.content_type {
                    ContentType::Article => "ARTICLE",
                    ContentType::Image => "IMAGE",
                    ContentType::Video => "VIDEO",
                    ContentType::Document => "DOCUMENT",
                    ContentType::SocialPost => "SOCIAL_POST",
                    ContentType::Text => "TEXT",
                    ContentType::Unknown => "UNKNOWN",
                },
                "trustScore": verification.trust_score,
                "aiGenerated": match verification.ai_generated {
                    AiGeneratedStatus::Human => "HUMAN",
                    AiGeneratedStatus::AiGenerated => "AI_GENERATED",
                    AiGeneratedStatus::LikelyAiGenerated => "LIKELY_AI_GENERATED",
                    AiGeneratedStatus::Unknown => "UNKNOWN",
                },
                "manipulationDetected": verification.manipulation_detected,
                "sourceCredibility": match verification.source_credibility {
                    CredibilityScore::High => "HIGH",
                    CredibilityScore::Medium => "MEDIUM",
                    CredibilityScore::Low => "LOW",
                    CredibilityScore::Unknown => "UNKNOWN",
                },
                "originalSource": verification.original_source,
                "alterationHistory": verification.alteration_history,
                "verifiedAt": verification.verified_at,
            })
        }
        "qyn_getValidatorScores" => {
            // Phase 2 placeholder: empty result until AI selector is wired.
            serde_json::json!({ "validators": [] })
        }
        "qyn_getValidatorRecord" => {
            if let Err(e) = require_param_string(&params, 0) {
                return e;
            }
            let addr_hex = param_str(&params, 0).unwrap_or("");
            // Phase 2 placeholder: return zeroed record shape.
            serde_json::json!({
                "address": addr_hex,
                "totalBlocksProposed": 0,
                "totalBlocksMissed": 0,
                "totalInvalidBlocks": 0,
                "averageResponseTimeMs": 0,
                "lastSeenBlock": 0,
                "slashCount": 0,
                "uptimePercentage": 0.0_f64,
                "joinedBlock": 0,
                "reputationScore": 0
            })
        }
        "qyn_getGasPrediction" => {
            // Build an in-memory GasOptimiser from recent on-chain history.
            let mut optimiser = GasOptimiser::new(GasConfig::default());
            let head = state.chain.get_head().ok().flatten();
            let current_block = head
                .and_then(|h| state.chain.get_block(&h).ok().flatten())
                .map(|b| b.header.number)
                .unwrap_or(0);

            if let Some(h) = head {
                if let Ok(Some(latest_block)) = state.chain.get_block(&h) {
                    let latest_number = latest_block.header.number;
                    let start = latest_number.saturating_sub(99);
                    for n in start..=latest_number {
                        if let Ok(Some(b)) = state.chain.get_block_by_number(n) {
                            let tx_count = b.body.transactions.len() as u32;
                            let congestion_score = (tx_count as f64 / 100.0).min(1.0);
                            let metrics = BlockMetrics {
                                block_number: b.header.number,
                                transaction_count: tx_count,
                                average_gas_used: 21_000,
                                timestamp: b.header.timestamp,
                                congestion_score,
                            };
                            optimiser.record_block(metrics);
                        }
                    }
                }
            }

            let prediction = optimiser.predict_gas_price(current_block);
            let history = optimiser.get_fee_history();

            let congestion_str = match prediction.congestion_level {
                CongestionLevel::Low => "Low",
                CongestionLevel::Medium => "Medium",
                CongestionLevel::High => "High",
                CongestionLevel::Critical => "Critical",
            };

            let trend = if history.len() >= 20 {
                let recent: f64 = history
                    .iter()
                    .rev()
                    .take(10)
                    .map(|b| b.congestion_score)
                    .sum::<f64>() / 10.0;
                let older: f64 = history
                    .iter()
                    .rev()
                    .skip(10)
                    .take(10)
                    .map(|b| b.congestion_score)
                    .sum::<f64>() / 10.0;
                if recent > older + 0.05 {
                    "Rising"
                } else if recent < older - 0.05 {
                    "Falling"
                } else {
                    "Stable"
                }
            } else {
                "Stable"
            };

            let gwei = prediction.recommended_gas_price as f64 / 1_000_000_000.0;

            serde_json::json!({
                "recommendedGasPrice": format!("0x{:x}", prediction.recommended_gas_price),
                "recommendedGasPriceGwei": format!("{:.1}", gwei),
                "confidence": prediction.confidence,
                "congestionLevel": congestion_str,
                "estimatedConfirmationBlocks": prediction.estimated_confirmation_blocks,
                "optimalSendWindow": prediction.optimal_send_window,
                "trend": trend
            })
        }
        "qyn_getCongestionHistory" => {
            let mut optimiser = GasOptimiser::new(GasConfig::default());
            let head = state.chain.get_head().ok().flatten();

            if let Some(h) = head {
                if let Ok(Some(latest_block)) = state.chain.get_block(&h) {
                    let latest_number = latest_block.header.number;
                    let start = latest_number.saturating_sub(99);
                    for n in start..=latest_number {
                        if let Ok(Some(b)) = state.chain.get_block_by_number(n) {
                            let tx_count = b.body.transactions.len() as u32;
                            let congestion_score = (tx_count as f64 / 100.0).min(1.0);
                            let metrics = BlockMetrics {
                                block_number: b.header.number,
                                transaction_count: tx_count,
                                average_gas_used: 21_000,
                                timestamp: b.header.timestamp,
                                congestion_score,
                            };
                            optimiser.record_block(metrics);
                        }
                    }
                }
            }

            let history = optimiser.get_fee_history();
            let blocks: Vec<serde_json::Value> = history
                .iter()
                .map(|b| {
                    serde_json::json!({
                        "blockNumber": b.block_number,
                        "transactionCount": b.transaction_count,
                        "congestionScore": b.congestion_score,
                        "timestamp": b.timestamp,
                    })
                })
                .collect();

            serde_json::json!({ "blocks": blocks })
        }
        "qyn_getContractRiskProfile" => {
            let contract_hex = params.get(0).and_then(|p| p.as_str()).or_else(|| params.get("contract").and_then(|v| v.as_str())).unwrap_or("");
            let contract_arr = match parse_address_array(contract_hex) {
                Ok(a) => a,
                Err(e) => return error_value(e),
            };
            let guard = state.rug_pull_detector.read().unwrap();
            match guard.get_contract_profile(&contract_arr) {
                Some(p) => {
                    let recommendation = match p.risk_score {
                        0..=30 => "Safe",
                        31..=60 => "Caution",
                        61..=85 => "HighRisk",
                        _ => "Dangerous",
                    };
                    serde_json::json!({
                        "contractAddress": format!("0x{}", hex::encode(p.contract_address)),
                        "deployer": format!("0x{}", hex::encode(p.deployer)),
                        "deployBlock": p.deploy_block,
                        "riskScore": p.risk_score,
                        "riskFactors": p.risk_factors.iter().map(|f| format!("{:?}", f)).collect::<Vec<_>>(),
                        "recommendation": recommendation,
                        "isVerified": p.is_verified,
                        "liquidityLocked": p.liquidity_locked,
                        "lockExpiryBlock": p.lock_expiry_block,
                        "topHolderPercent": p.top_holder_percent,
                        "holderCount": p.holder_count,
                    })
                }
                None => {
                    let empty_factors: Vec<String> = vec![];
                    serde_json::json!({
                        "contractAddress": format!("0x{}", hex::encode(contract_arr)),
                        "deployer": "0x0000000000000000000000000000000000000000",
                        "deployBlock": 0,
                        "riskScore": 0,
                        "riskFactors": empty_factors,
                        "recommendation": "Unknown",
                        "isVerified": false,
                        "liquidityLocked": false,
                        "lockExpiryBlock": Value::Null,
                        "topHolderPercent": 0.0_f64,
                        "holderCount": 0,
                    })
                }
            }
        }
        "qyn_scanContract" => {
            let source_code = params.get(0).and_then(|p| p.as_str()).or_else(|| params.get("sourceCode").and_then(|v| v.as_str())).unwrap_or("");
            let result = ContractScanner::scan_solidity(source_code);
            serde_json::json!({
                "riskScore": result.risk_score,
                "riskFactors": result.risk_factors.iter().map(|f| format!("{:?}", f)).collect::<Vec<_>>(),
                "recommendation": format!("{:?}", result.recommendation),
                "details": result.details,
            })
        }
        "qyn_getTokenConcentration" => {
            let token_hex = params.get(0).and_then(|p| p.as_str()).or_else(|| params.get("token").and_then(|v| v.as_str())).unwrap_or("");
            let token_arr = match parse_address_array(token_hex) {
                Ok(a) => a,
                Err(e) => return error_value(e),
            };
            let guard = state.concentration_monitor.read().unwrap();
            let summary = guard.get_token_risk_summary(&token_arr);
            let top_holders: Vec<Value> = guard
                .get_top_holders(&token_arr, 10)
                .into_iter()
                .map(|(addr, pct)| serde_json::json!({
                    "address": format!("0x{}", hex::encode(addr)),
                    "percent": pct,
                }))
                .collect();
            serde_json::json!({
                "totalHolders": summary.total_holders,
                "topHolderPercent": summary.top_holder_percent,
                "top5HoldersPercent": summary.top_5_holders_percent,
                "isHighConcentration": summary.is_high_concentration,
                "concentrationRisk": format!("{:?}", summary.concentration_risk),
                "topHolders": top_holders,
            })
        }
        "qyn_lockLiquidity" => {
            let contract_hex = params.get(0).and_then(|p| p.as_str()).or_else(|| params.get("contract").and_then(|v| v.as_str())).unwrap_or("");
            let amount_str = params.get(1).and_then(|p| p.as_str()).or_else(|| params.get("amount").and_then(|v| v.as_str())).unwrap_or("0");
            let period_str = params.get(2).and_then(|p| p.as_str()).or_else(|| params.get("lockPeriodBlocks").and_then(|v| v.as_str())).unwrap_or("0");
            let locker_hex = params.get(3).and_then(|p| p.as_str()).or_else(|| params.get("locker").and_then(|v| v.as_str())).unwrap_or("");
            let contract_arr = match parse_address_array(contract_hex) {
                Ok(a) => a,
                Err(e) => return error_value(e),
            };
            let amount = if amount_str.starts_with("0x") {
                u128::from_str_radix(amount_str.trim_start_matches("0x"), 16).unwrap_or(0)
            } else {
                amount_str.parse().unwrap_or(0)
            };
            let lock_period: u64 = period_str.parse().unwrap_or(0);
            let locker_arr = match parse_address_array(locker_hex) {
                Ok(a) => a,
                Err(e) => return error_value(e),
            };
            let current_block = state.chain.get_head().ok().flatten()
                .and_then(|h| state.chain.get_block(&h).ok().flatten())
                .map(|b| b.header.number)
                .unwrap_or(0);
            let mut guard = state.rug_pull_detector.write().unwrap();
            let lock = guard.lock_liquidity(contract_arr, amount, lock_period, locker_arr, current_block);
            let lock_id = alloy_primitives::keccak256(
                [lock.contract.as_ref(), lock.lock_start_block.to_be_bytes().as_slice()].concat().as_slice()
            );
            serde_json::json!({
                "lockId": format!("0x{}", hex::encode(lock_id.0)),
                "contract": format!("0x{}", hex::encode(lock.contract)),
                "lockedAmount": lock.locked_amount.to_string(),
                "lockStartBlock": lock.lock_start_block,
                "lockExpiryBlock": lock.lock_expiry_block,
                "isActive": lock.is_active,
            })
        }
        "qyn_getRugPullAlerts" => {
            let guard = state.rug_pull_detector.read().unwrap();
            let alerts: Vec<Value> = guard.get_all_alerts().into_iter().map(|a| {
                serde_json::json!({
                    "alertId": format!("0x{}", hex::encode(a.alert_id)),
                    "contract": format!("0x{}", hex::encode(a.contract)),
                    "deployer": format!("0x{}", hex::encode(a.deployer)),
                    "alertType": format!("{:?}", a.alert_type),
                    "severity": format!("{:?}", a.severity),
                    "description": a.description,
                    "triggeredBlock": a.triggered_block,
                })
            }).collect();
            serde_json::json!({ "alerts": alerts })
        }
        _ => Value::String(format!("method {} not implemented", method)),
    }
}

fn error_value(msg: impl Into<String>) -> Value {
    serde_json::json!({"error": {"message": msg.into()}})
}

fn error_value_with_code(code: i64, msg: impl Into<String>) -> Value {
    serde_json::json!({"error": {"code": code, "message": msg.into()}})
}

fn parse_address(s: &str) -> Result<Address, String> {
    let s = s.trim_start_matches("0x");
    let bytes = hex::decode(s).map_err(|e| e.to_string())?;
    if bytes.len() != 20 {
        return Err("address must be 20 bytes".into());
    }
    let mut arr = [0u8; 20];
    arr.copy_from_slice(&bytes);
    Ok(Address::from_slice(&arr))
}

/// Parse hex address to fixed 20-byte array for intelligence APIs.
fn parse_address_array(s: &str) -> Result<[u8; 20], String> {
    let addr = parse_address(s)?;
    addr.as_slice().try_into().map_err(|_| "address must be 20 bytes".into())
}

fn parse_tx_hash(s: &str) -> Result<B256, String> {
    let s = s.trim_start_matches("0x");
    let bytes = hex::decode(s).map_err(|e| e.to_string())?;
    if bytes.len() != 32 {
        return Err("tx hash must be 32 bytes".into());
    }
    Ok(B256::from_slice(&bytes))
}

fn tx_to_json(tx: &quyn_core::SignedTransaction) -> Value {
    serde_json::json!({
        "hash": format!("0x{}", hex::encode(tx.hash().as_slice())),
        "from": format!("0x{}", hex::encode(tx.sender().ok().unwrap_or(Address::ZERO).as_slice())),
        "to": tx.to().map(|a| format!("0x{}", hex::encode(a.as_slice()))),
        "value": format!("0x{:x}", tx.value()),
        "gasPrice": format!("0x{:x}", tx.gas_price()),
        "gas": format!("0x{:x}", tx.gas_limit()),
        "nonce": format!("0x{:x}", tx.nonce()),
    })
}
