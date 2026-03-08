# Quyn (QYN)

Decentralized payment network with smart contracts. Fast, cheap global transfers with EVM compatibility.

- **Ticker**: QYN  
- **Chain ID**: 7777 (mainnet), 7779 (testnet)  
- **Block time**: 3s  
- **Supply**: 1 billion QYN (18 decimals)  
- **Consensus**: Proof of Stake  

## Build

```bash
cd quyn
cargo build
```

## Run a full node

```bash
cargo run -p quyn -- full --data-dir ./data --rpc-addr 127.0.0.1:8545
```

Apply genesis before first run (see `genesis.json` and `core/genesis.rs`).

## Wallet (CLI)

```bash
cargo run -p quyn-wallet -- new    # generate mnemonic
cargo run -p quyn-wallet -- import --mnemonic "word1 word2 ..." --index 0
```

## SDKs

- **JavaScript**: `quyn/sdk/js-sdk` — `npm install && npm run build`
- **Python**: `quyn/sdk/py-sdk` — `pip install -e .`

## Docs

- [Architecture](docs/architecture.md)
- [Security](docs/SECURITY.md)

## License

MIT
