//! Quyn node binary - full, light, validator.

use clap::Parser;
use quyn::runner::FullNode;
use quyn_consensus::{select_proposer, slash_penalty_bps, SlashEvidence, SlashReason, ValidatorSet, MIN_STAKE};
use quyn_core::{
    accept_block, apply_genesis_alloc,
    block::{Block, BlockBody, BlockHeader},
    chain::ChainDB,
    error::CoreError,
    genesis::split_fees,
    types::{BLOCK_TIME_SECS, CHAIN_ID_MAINNET, CHAIN_ID_TESTNET},
    validation::{validate_tx_against_state, validate_tx_basic},
};
use quyn_vm::{block_env, execute_tx, StateDBAdapter};
use alloy_primitives::{Address, B256, U256};
use quyn_core::Transaction;
use quyn_wallet::{sign_transaction, KeyPair};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tracing_subscriber::EnvFilter;

/// Fixed devnet faucet secret (deterministic). Address is funded in genesis.
/// SECURITY: This key must NEVER be used for mainnet or any real funds. Devnet only.
fn devnet_faucet_keypair() -> KeyPair {
    let mut secret = [0u8; 32];
    secret[0] = 0xde;
    secret[1] = 0xad;
    secret[2] = 0xbe;
    secret[3] = 0xef;
    secret[4] = 0x11;
    KeyPair::from_secret(secret).expect("devnet faucet key")
}

#[derive(Parser, Debug)]
#[command(name = "quyn")]
#[command(about = "Quyn (QYN) node - full, light, or validator")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Run a full node
    Full {
        #[arg(long, default_value = "./data")]
        data_dir: PathBuf,
        #[arg(long, default_value = "127.0.0.1:8545")]
        rpc_addr: String,
    },
    /// Run a light node
    Light,
    /// Run a validator node
    Validator,
    /// Run local devnet (single node + RPC, genesis + block producer every 3s)
    Devnet {
        #[arg(long, default_value = "./devnet-data")]
        data_dir: PathBuf,
        #[arg(long, default_value = "127.0.0.1:8545")]
        rpc_addr: String,
    },
    /// Wallet: new, balance <addr>, send <to> <amount>
    Wallet {
        #[command(subcommand)]
        sub: WalletSub,
    },
    /// Devnet faucet: send QYN from the faucet account to any address (requires running devnet)
    Faucet {
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: String,
    },
}

#[derive(clap::Subcommand, Debug)]
enum WalletSub {
    /// Generate new mnemonic and address
    New,
    /// Show balance for address (requires running node at 127.0.0.1:8545)
    Balance { address: String },
    /// Send QYN (requires --mnemonic and running node)
    Send {
        to: String,
        amount: String,
        #[arg(long)]
        mnemonic: Option<String>,
        #[arg(long, default_value = "0")]
        index: u32,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let args = Args::parse();
    match args.command {
        Command::Full { data_dir, rpc_addr } => {
            let node = FullNode::open(&data_dir)?;
            tracing::info!("Full node started. Chain and state opened. RPC will listen on {}", rpc_addr);
            quyn_rpc::serve(node.chain, node.state, node.mempool, CHAIN_ID_MAINNET, rpc_addr).await?;
        }
        Command::Light => println!("Light node not yet implemented."),
        Command::Validator => println!("Validator node not yet implemented."),
        Command::Devnet { data_dir, rpc_addr } => run_devnet(data_dir, rpc_addr).await?,
        Command::Wallet { sub } => run_wallet(sub).await?,
        Command::Faucet { to, amount } => run_faucet(to, amount).await?,
    }
    Ok(())
}

/// Devnet: create genesis if needed, start block producer every 3s, serve RPC.
async fn run_devnet(data_dir: PathBuf, rpc_addr: String) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let node = FullNode::open(&data_dir)?;
    let chain = node.chain.clone();
    let state = node.state.clone();
    let mempool = node.mempool.clone();

    if chain.get_head()?.is_none() {
        let mut devnet_validator_bytes = [0u8; 20];
        devnet_validator_bytes[19] = 1;
        let devnet_validator = Address::from_slice(&devnet_validator_bytes);
        let faucet_addr = devnet_faucet_keypair().address();
        let one_qyn = 10_u128.pow(18);
        let mut alloc = HashMap::new();
        alloc.insert(
            format!("0x{}", hex::encode(devnet_validator.as_slice())),
            format!("0x{:x}", 1_000_000_000u128 * one_qyn),
        );
        alloc.insert(
            format!("0x{}", hex::encode(faucet_addr.as_slice())),
            format!("0x{:x}", 100_000_000u128 * one_qyn),
        );
        // Founder / team allocation: 200M QYN (20%) at block 0
        const FOUNDER_ADDR: &str = "0x034ADBD563043B1ba028691839Adc37d07C08909";
        alloc.insert(
            FOUNDER_ADDR.to_string(),
            format!("0x{:x}", 200_000_000u128 * one_qyn), // 200M QYN = 0x29a2241af62c000000000000
        );
        apply_genesis_alloc(&state, &alloc)?;
        let state_root = state.compute_state_root()?;
        let genesis = Block {
            header: BlockHeader {
                parent_hash: B256::ZERO,
                state_root,
                transactions_root: B256::ZERO,
                receipts_root: B256::ZERO,
                timestamp: 0,
                number: 0,
                validator: devnet_validator,
                signature: vec![],
                extra_data: vec![],
                gas_limit: 30_000_000,
                base_fee_per_gas: U256::ZERO,
            },
            body: BlockBody::default(),
        };
        chain.put_block(&genesis)?;
        chain.set_head(&genesis.hash())?;
        state.save_state_root(&genesis.hash(), genesis.header.state_root)?;
        let mut validator_set = ValidatorSet::new();
        validator_set
            .register(devnet_validator, U256::from(MIN_STAKE), 0)
            .map_err(|e| format!("validator set register: {}", e))?;
        chain.put_validator_set_bytes(&bincode::serialize(&validator_set).map_err(|e| e.to_string())?)?;
        tracing::info!("Genesis block created (number=0). Validator: 0x{}", hex::encode(devnet_validator.as_slice()));
        tracing::info!("Faucet address: 0x{} (use: quyn faucet --to <ADDR> --amount <AMOUNT>)", hex::encode(faucet_addr.as_slice()));
    }

    if chain.get_validator_set_bytes()?.is_none() {
        let mut validator_set = ValidatorSet::new();
        let mut devnet_validator_bytes = [0u8; 20];
        devnet_validator_bytes[19] = 1;
        let devnet_validator = Address::from_slice(&devnet_validator_bytes);
        validator_set
            .register(devnet_validator, U256::from(MIN_STAKE), 0)
            .map_err(|e| format!("validator set register: {}", e))?;
        chain.put_validator_set_bytes(&bincode::serialize(&validator_set).map_err(|e| e.to_string())?)?;
    }

    let chain_prod = chain.clone();
    let state_prod = state.clone();
    let mempool_prod = mempool.clone();
    let mut validator_bytes = [0u8; 20];
    validator_bytes[19] = 1;
    let validator_addr = Address::from_slice(&validator_bytes);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(BLOCK_TIME_SECS)).await;
            if let Err(e) = produce_block(&chain_prod, &state_prod, &mempool_prod, &validator_addr, CHAIN_ID_TESTNET) {
                tracing::warn!("Block production failed: {}", e);
            }
        }
    });

    tracing::info!("Devnet started (chain_id=7778). RPC on {}. Blocks every {}s.", rpc_addr, BLOCK_TIME_SECS);
    quyn_rpc::serve(chain, state, mempool, CHAIN_ID_TESTNET, rpc_addr).await?;
    Ok(())
}

fn produce_block(
    chain: &ChainDB,
    state: &quyn_core::StateDB,
    mempool: &quyn_core::Mempool,
    validator: &Address,
    chain_id: u64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let head = match chain.get_head()? {
        Some(h) => h,
        None => return Ok(()),
    };
    let parent = match chain.get_block(&head)? {
        Some(b) => b,
        None => return Ok(()),
    };
    let parent_number = parent.header.number;
    let parent_hash = parent.hash();
    let next_number = parent_number + 1;

    let proposer = chain
        .get_validator_set_bytes()?
        .and_then(|b| bincode::deserialize::<ValidatorSet>(&b).ok())
        .and_then(|set| {
            let active = set.active_validators();
            let hash_arr: [u8; 32] = parent_hash.0;
            select_proposer(&active, next_number, &hash_arr)
        });
    if let Some(proposer_addr) = proposer {
        if proposer_addr != *validator {
            return Ok(());
        }
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("system time before UNIX_EPOCH: {}", e))?
        .as_secs();

    let candidates = mempool.get_best(100)?;
    let mut to_apply: Vec<(quyn_core::SignedTransaction, u64)> = Vec::new();
    let block_env = block_env(
        next_number,
        now,
        30_000_000,
        U256::ZERO,
        *validator,
    );
    for tx in &candidates {
        if validate_tx_basic(tx, chain_id).is_err() || validate_tx_against_state(tx, state).is_err() {
            continue;
        }
        let mut adapter = StateDBAdapter::new(state);
        if let Ok(result) = execute_tx(&mut adapter, tx, &block_env, chain_id) {
            to_apply.push((tx.clone(), result.gas_used));
        }
    }
    // Apply 50% burn / 50% proposer: revm credited full gas to coinbase (validator); deduct burn.
    let total_gas_fees: U256 = to_apply
        .iter()
        .map(|(tx, gas_used)| tx.gas_price().saturating_mul(U256::from(*gas_used)))
        .fold(U256::ZERO, |a, b| a.saturating_add(b));
    let (burn, _proposer_portion) = split_fees(total_gas_fees);
    if !burn.is_zero() {
        let validator_bal = state.get_balance(validator)?;
        state.set_balance(validator, validator_bal.saturating_sub(burn))?;
    }
    let state_root = state.compute_state_root()?;
    let txs_only: Vec<quyn_core::SignedTransaction> = to_apply.iter().map(|(tx, _)| tx.clone()).collect();
    let block = Block::new(
        parent_hash,
        next_number,
        state_root,
        B256::ZERO,
        txs_only.clone(),
        *validator,
        vec![],
        30_000_000,
        U256::ZERO,
    )?;
    if let Err(e) = accept_block(chain, state, &block, now) {
        if let CoreError::DoubleSign { validator: v, height, second_block, .. } = &e {
            let evidence = SlashEvidence {
                validator: *v,
                reason: SlashReason::DoubleSign,
                block_number: *height,
                payload: second_block.as_slice().to_vec(),
            };
            let _ = chain.put_slash_evidence(v, *height, &bincode::serialize(&evidence).unwrap_or_default());
            let bps = slash_penalty_bps(&SlashReason::DoubleSign);
            if let Ok(bal) = state.get_balance(v) {
                let penalty = bal * U256::from(bps) / U256::from(10000u32);
                let _ = state.set_balance(v, bal.saturating_sub(penalty));
            }
        }
        return Err(e.into());
    }
    for (i, (tx, _)) in to_apply.iter().enumerate() {
        chain.put_tx_receipt_index(&tx.hash(), block.hash(), block.header.number, i as u32)?;
        let h = tx.hash();
        let arr: [u8; 32] = h.0;
        let _ = mempool.remove(&arr);
    }
    tracing::info!("Produced block {} ({})", block.header.number, hex::encode(block.hash().as_slice()));
    Ok(())
}

async fn fetch_chain_id(url: &str) -> u64 {
    let client = reqwest::Client::new();
    let body = serde_json::json!({ "jsonrpc": "2.0", "method": "eth_chainId", "params": [], "id": 1 });
    let chain_id_hex = match client.post(url).json(&body).send().await {
        Ok(r) => r.json::<serde_json::Value>().await.ok().and_then(|j| j.get("result").and_then(|r| r.as_str()).map(String::from)),
        Err(_) => None,
    }.unwrap_or_else(|| "0x1e61".into());
    u64::from_str_radix(chain_id_hex.trim_start_matches("0x"), 16).unwrap_or(7777)
}

async fn run_faucet(to: String, amount: String) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url = std::env::var("QYN_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8545".into());
    let url = if url.ends_with('/') { format!("{}rpc", url) } else if !url.contains("/rpc") { format!("{}/rpc", url) } else { url };
    let chain_id = fetch_chain_id(&url).await;
    let kp = devnet_faucet_keypair();
    let from_addr = format!("0x{}", hex::encode(kp.address().as_slice()));
    let client = reqwest::Client::new();
    let nonce_body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionCount",
        "params": [from_addr, "latest"],
        "id": 1
    });
    let nonce_res: serde_json::Value = client.post(&url).json(&nonce_body).send().await?.json().await?;
    let nonce_hex = nonce_res.get("result").and_then(|r| r.as_str()).unwrap_or("0x0");
    let nonce = u64::from_str_radix(nonce_hex.trim_start_matches("0x"), 16).unwrap_or(0);
    let value_wei: U256 = if amount.trim_start_matches("0x").chars().all(|c| c.is_ascii_hexdigit()) {
        U256::from_str_radix(amount.trim_start_matches("0x"), 16).map_err(|_| "invalid hex amount")?
    } else {
        U256::from(amount.parse::<u128>().map_err(|_| "invalid amount")?)
    };
    let to_hex = if to.starts_with("0x") { to.clone() } else { format!("0x{}", to) };
    let to_addr = quyn_wallet::address_from_str(&to_hex)?;
    let tx = Transaction {
        nonce,
        gas_price: U256::ZERO,
        gas_limit: 21_000,
        to: Some(to_addr),
        value: value_wei,
        data: vec![],
        chain_id,
    };
    let signed = sign_transaction(&tx, &kp)?;
    let raw = bincode::serialize(&signed).map_err(|e| e.to_string())?;
    let raw_hex = format!("0x{}", hex::encode(&raw));
    let send_body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_sendRawTransaction",
        "params": [raw_hex],
        "id": 1
    });
    let send_res: serde_json::Value = client.post(&url).json(&send_body).send().await?.json().await?;
    let tx_hash = send_res.get("result").and_then(|r| r.as_str()).unwrap_or_else(|| send_res.get("error").and_then(|e| e.get("message").and_then(|m| m.as_str())).unwrap_or("error"));
    println!("Faucet sent {} wei to {}. Tx hash: {}", value_wei, to, tx_hash);
    Ok(())
}

async fn run_wallet(sub: WalletSub) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match sub {
        WalletSub::New => {
            let out = quyn_wallet::run_new()?;
            print!("{}", out);
        }
        WalletSub::Balance { address } => {
            let url = std::env::var("QYN_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8545".into());
            let url = if url.ends_with('/') { format!("{}rpc", url) } else if !url.contains("/rpc") { format!("{}/rpc", url) } else { url };
            let body = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "eth_getBalance",
                "params": [address, "latest"],
                "id": 1
            });
            let client = reqwest::Client::new();
            let res = client.post(&url).json(&body).send().await?;
            let json: serde_json::Value = res.json().await?;
            let balance = json.get("result").and_then(|r| r.as_str()).unwrap_or("0x0");
            println!("Balance: {}", balance);
        }
        WalletSub::Send { to, amount, mnemonic, index } => {
            let mnemonic = mnemonic.ok_or("wallet send requires --mnemonic")?;
            let url = std::env::var("QYN_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8545".into());
            let url = if url.ends_with('/') { format!("{}rpc", url) } else if !url.contains("/rpc") { format!("{}/rpc", url) } else { url };
            let chain_id = fetch_chain_id(&url).await;
            let from_addr = quyn_wallet::address_for_mnemonic(&mnemonic, index)?;
            let nonce_body = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "eth_getTransactionCount",
                "params": [from_addr, "latest"],
                "id": 1
            });
            let client = reqwest::Client::new();
            let nonce_res: serde_json::Value = client.post(&url).json(&nonce_body).send().await?.json().await?;
            let nonce_hex = nonce_res.get("result").and_then(|r| r.as_str()).unwrap_or("0x0");
            let nonce = u64::from_str_radix(nonce_hex.trim_start_matches("0x"), 16).unwrap_or(0);
            let value_wei = if amount.trim_start_matches("0x").chars().all(|c| c.is_ascii_hexdigit()) {
                alloy_primitives::U256::from_str_radix(amount.trim_start_matches("0x"), 16).map_err(|_| "invalid hex value")?
            } else {
                let amt: u128 = amount.parse().map_err(|_| "invalid amount")?;
                alloy_primitives::U256::from(amt)
            };
            let signed_bytes = quyn_wallet::run_sign_tx(
                nonce,
                "0x0".to_string(),
                21_000,
                Some(to),
                format!("0x{:x}", value_wei),
                "".to_string(),
                chain_id,
                &mnemonic,
                index,
            )?;
            let raw_hex = format!("0x{}", hex::encode(&signed_bytes));
            let send_body = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "eth_sendRawTransaction",
                "params": [raw_hex],
                "id": 1
            });
            let send_res: serde_json::Value = client.post(&url).json(&send_body).send().await?.json().await?;
            let tx_hash = send_res.get("result").and_then(|r| r.as_str()).unwrap_or_else(|| send_res.get("error").map(|e| e.get("message").and_then(|m| m.as_str()).unwrap_or("error")).unwrap_or("unknown"));
            println!("Tx hash: {}", tx_hash);
        }
    }
    Ok(())
}
