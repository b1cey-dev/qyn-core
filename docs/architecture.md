# Quyn Architecture

## State Root Design (M3)

The state root is computed from a hash of all balance and nonce entries (MVP implementation). This is **not** a full Merkle Patricia Trie like Ethereum. Design decisions:

- **Current:** SHA-256 over sorted `balance:*` and `nonce:*` keys from RocksDB. Fast, deterministic, suitable for single-node devnet.
- **Light clients:** Merkle proofs are not available; light clients cannot verify state without full sync.
- **Future:** A Merkle Patricia Trie implementation is planned for mainnet to enable light client support and Ethereum-compatible state proofs.

---

## Components

- **core**: Block, transaction, chain DB, state DB, mempool, validation, fork resolution, genesis.
- **consensus**: Proof of Stake validator set, proposer selection, rewards, slashing, delegation.
- **network**: libp2p swarm (TCP, Noise, Kademlia, mDNS, Identify); block/tx protocol types.
- **vm**: QVM (revm) for EVM-compatible execution; gas, deploy, execute, ABI helpers.
- **wallet**: HD (BIP39/BIP44), keys, signing, CLI.
- **node**: Full node runner (chain + state + mempool + RPC).
- **rpc**: JSON-RPC and REST server.
- **sdk**: JavaScript and Python clients for RPC.

Block time 3s; chainId 7777 (mainnet), 7779 (testnet); 1 billion QYN supply, 18 decimals; 50% of gas fees burned.
