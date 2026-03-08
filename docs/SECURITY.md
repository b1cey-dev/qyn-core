# Quyn Security

## Attack Vectors and Mitigations

### Sybil resistance
- **PoS minimum stake**: 10,000 QYN required to register as validator; economic cost to create many identities.
- No single-identity requirement; stake-weighted selection limits impact of small validators.

### DDoS
- **Mempool limits**: Configurable max pool size (default 100,000); eviction by gas price.
- **RPC**: Rate limiting and connection limits (implement at deployment).
- **P2P**: Peer scoring and connection limits per peer.

### Reentrancy (smart contracts)
- Use Solidity 0.8+ and follow checks-effects-interactions; no VM-level reentrancy guard in QVM.

### Integer overflow
- revm and Solidity 0.8+ use checked arithmetic.

### Eclipse
- Multiple bootstrap peers and DHT (Kademlia); random peer selection; document bootstrap list.

### 51% / consensus
- PoS design: attacking requires acquiring >50% of staked QYN.
- Slashing for double-sign and liveness; see consensus/slashing.

### Formal verification
- Critical invariants documented: single canonical chain (longest chain), balance conservation (state transition).
- Consensus state machine and fork choice suitable for TLA+ or Coq specs (future work).

## Audit checklist
- [ ] Consensus: validator set update, proposer selection, slashing application.
- [ ] Core: block validation, state transition, reorg handling.
- [ ] Network: message validation, sync from genesis.
- [ ] RPC: input validation, no unbounded allocations.
