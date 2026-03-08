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
use alloy_primitives::{Address, B256, U256};
use rlp::Rlp;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
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
    let app_state = AppState {
        chain,
        state,
        mempool,
        chain_id,
        rate_limiter,
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
    let rlp = Rlp::new(bytes);
    if !rlp.is_list() {
        return Err("expected RLP list".into());
    }
    let item_count = rlp.item_count().map_err(|e| e.to_string())?;
    if item_count < 12 {
        return Err(format!("EIP-1559 tx expects 12 fields, got {}", item_count));
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
            "baseFeePerGas": ["0x3B9ACA00"],
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
                "timestamp": format!("0x{:x}", block.header.timestamp),
                "miner": format!("0x{}", hex::encode(block.header.validator.as_slice())),
                "transactions": txs_value,
                "gasLimit": format!("0x{:x}", block.header.gas_limit),
                "baseFeePerGas": format!("0x{:x}", block.header.base_fee_per_gas),
                "sha3Uncles": "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
                "mixHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
                "nonce": "0x0000000000000000",
                "logsBloom": format!("0x{}", "00".repeat(256)),
                "difficulty": "0x0",
                "totalDifficulty": "0x0",
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
                "timestamp": format!("0x{:x}", block.header.timestamp),
                "miner": format!("0x{}", hex::encode(block.header.validator.as_slice())),
                "transactions": txs_value,
                "gasLimit": format!("0x{:x}", block.header.gas_limit),
                "baseFeePerGas": format!("0x{:x}", block.header.base_fee_per_gas),
                "sha3Uncles": "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
                "mixHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
                "nonce": "0x0000000000000000",
                "logsBloom": format!("0x{}", "00".repeat(256)),
                "difficulty": "0x0",
                "totalDifficulty": "0x0",
                "uncles": []
            })
        }
        "quyn_getTransactionByHash" => {
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
                    let tx_json = state.chain.get_block(&block_hash).ok().flatten()
                        .and_then(|b| b.body.transactions.get(index as usize).cloned())
                        .map(|tx| tx_to_json(&tx))
                        .unwrap_or(Value::Null);
                    serde_json::json!({
                        "transaction": tx_json,
                        "blockHash": format!("0x{}", hex::encode(block_hash.as_slice())),
                        "blockNumber": format!("0x{:x}", block_number),
                        "transactionIndex": format!("0x{:x}", index),
                    })
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
