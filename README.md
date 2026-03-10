> ⚠️ PROPRIETARY SOURCE CODE  
> This repository is source-available 
> for transparency purposes only.  
> Copying, forking, or commercial use 
> is strictly prohibited without written 
> permission from Quyn Technologies Ltd.  
> See LICENSE for full terms.

# Quyn (QYN)

Decentralized payment network with smart 
contracts. Fast, cheap global transfers 
with EVM compatibility.

- **Ticker**: QYN  
- **Chain ID**: 7777 (mainnet), 7779 (testnet)  
- **Block time**: 3s  
- **Supply**: 1 billion QYN (18 decimals)  
- **Consensus**: Proof of Stake
- **Website**: https://getquyn.com
- **Testnet RPC**: https://rpc.getquyn.com
- **Discord**: https://discord.gg/CmBbbtG434
- **Twitter**: https://twitter.com/getquyn

## Build

cargo build

## Run a full node

cargo run -p quyn -- full 
  --data-dir ./data 
  --rpc-addr 127.0.0.1:8545

Apply genesis before first run 
(see genesis.json and core/genesis.rs).

## Wallet (CLI)

cargo run -p quyn-wallet -- new

cargo run -p quyn-wallet -- import 
  --mnemonic "word1 word2 ..." 
  --index 0

## SDKs

JavaScript: quyn/sdk/js-sdk
npm install && npm run build

Python: quyn/sdk/py-sdk
pip install -e .

## Docs

Architecture: docs/architecture.md
Security: docs/SECURITY.md
Whitepaper: https://getquyn.com/whitepaper

## Security

Security audit score: 92/100
Full report: https://getquyn.com/security
For vulnerability disclosures contact:
security@getquyn.com

## License

Copyright (c) 2026 Quyn Technologies Ltd.
All rights reserved.

This source code is made available for 
transparency and security review purposes 
only. You may read and audit the code.

You may NOT:
- Copy, fork, or clone this codebase 
  to create a competing product
- Use this code commercially without 
  written permission from 
  Quyn Technologies Ltd
- Redistribute this code or any 
  derivative works
- Use the Quyn or QYN name, branding, 
  or identity in any project

For licensing enquiries contact:
security@getquyn.com

Quyn, QYN, and the Quyn logo are 
trademarks of Quyn Technologies Ltd.
