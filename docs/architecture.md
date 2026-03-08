# Quyn Architecture

- **core**: Block, transaction, chain DB, state DB, mempool, validation, fork resolution, genesis.
- **consensus**: Proof of Stake validator set, proposer selection, rewards, slashing, delegation.
- **network**: libp2p swarm (TCP, Noise, Kademlia, mDNS, Identify); block/tx protocol types.
- **vm**: QVM (revm) for EVM-compatible execution; gas, deploy, execute, ABI helpers.
- **wallet**: HD (BIP39/BIP44), keys, signing, CLI.
- **node**: Full node runner (chain + state + mempool + RPC).
- **rpc**: JSON-RPC and REST server.
- **sdk**: JavaScript and Python clients for RPC.

Block time 3s; chainId 7777 (mainnet), 7778 (testnet); 1 billion QYN supply, 18 decimals; 50% of gas fees burned.
