//! JSON-RPC and REST server.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use quyn_core::{
    ChainDB, Mempool, StateDB,
    validation::{validate_tx_basic, validate_tx_against_state},
};
use alloy_primitives::{Address, B256};
use serde_json::Value;
use std::sync::Arc;
use tower_http::cors::CorsLayer;

pub type SharedChain = Arc<ChainDB>;
pub type SharedState = Arc<StateDB>;
pub type SharedMempool = Arc<Mempool>;

#[derive(Clone)]
pub struct AppState {
    pub chain: SharedChain,
    pub state: SharedState,
    pub mempool: SharedMempool,
    pub chain_id: u64,
}

/// Serve RPC and REST until shutdown. Pass chain_id so devnet can use 7778 and mainnet 7777.
pub async fn serve(
    chain: SharedChain,
    state: SharedState,
    mempool: SharedMempool,
    chain_id: u64,
    addr: String,
) -> Result<(), crate::error::RpcError> {
    let app_state = AppState { chain, state, mempool, chain_id };
    let app = Router::new()
        .route("/", get(health).post(jsonrpc_handler))
        .route("/rpc", get(rpc_chain_id_get).post(jsonrpc_handler))
        .route("/health", get(health))
        .layer(CorsLayer::permissive())
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| crate::error::RpcError::Internal(e.to_string()))?;
    tracing::info!("RPC listening on {}", addr);
    axum::serve(listener, app)
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
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let method = body.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = body.get("params").cloned().unwrap_or(Value::Array(vec![]));
    let id = body.get("id").cloned();
    let result = dispatch(state, method, params).await;
    let response = if result.get("error").is_some() {
        serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": result["error"] })
    } else {
        serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result })
    };
    (StatusCode::OK, Json(response))
}

fn param_str(params: &Value, i: usize) -> Option<&str> {
    params.get(i).and_then(|p| p.as_str())
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
        "quyn_health" => Value::String("ok".to_string()),
        "eth_getBalance" => {
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
            let hex_raw = param_str(&params, 0).unwrap_or("");
            let bytes = match hex::decode(hex_raw.trim_start_matches("0x")) {
                Ok(b) => b,
                Err(e) => return error_value(e.to_string()),
            };
            let tx: quyn_core::SignedTransaction = match bincode::deserialize(&bytes) {
                Ok(t) => t,
                Err(e) => return error_value(e.to_string()),
            };
            let tx_hash = tx.hash();
            if let Err(e) = validate_tx_basic(&tx, state.chain_id) {
                return error_value(e.to_string());
            }
            if let Err(e) = validate_tx_against_state(&tx, &state.state) {
                return error_value(e.to_string());
            }
            match state.mempool.insert(tx) {
                Ok(_) => Value::String(format!("0x{}", hex::encode(tx_hash.as_slice()))),
                Err(e) => error_value(e.to_string()),
            }
        }
        "eth_getBlockByNumber" => {
            let tag = param_str(&params, 0).unwrap_or("latest");
            let full_tx = params.get(1).and_then(|p| p.as_bool()).unwrap_or(false);
            let block_number = if tag == "latest" || tag == "pending" {
                state.chain.get_head().ok().flatten()
                    .and_then(|h| state.chain.get_block(&h).ok().flatten())
                    .map(|b| b.header.number)
            } else {
                let n = u64::from_str_radix(tag.trim_start_matches("0x"), 16).ok();
                n
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
                Value::Array(
                    block.body.transactions.iter()
                        .map(|tx| tx_to_json(tx))
                        .collect()
                )
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
            })
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
        "eth_getTransactionReceipt" => {
            let hash_hex = param_str(&params, 0).unwrap_or("");
            let tx_hash = match parse_tx_hash(hash_hex) {
                Ok(h) => h,
                Err(e) => return error_value(e),
            };
            match state.chain.get_tx_receipt_index(&tx_hash) {
                Ok(Some((block_hash, block_number, index))) => serde_json::json!({
                    "transactionHash": format!("0x{}", hex::encode(tx_hash.as_slice())),
                    "blockHash": format!("0x{}", hex::encode(block_hash.as_slice())),
                    "blockNumber": format!("0x{:x}", block_number),
                    "transactionIndex": format!("0x{:x}", index),
                    "status": "0x1",
                }),
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
