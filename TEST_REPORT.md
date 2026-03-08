# QYN Blockchain Security Audit Report

**Date:** March 2026  
**Scope:** Full codebase audit (consensus, cryptography, EVM/QVM, network, RPC, economic, stress tests, code quality, wallet security)

**Post-audit update:** All critical and high severity fixes applied (see "Post-Audit Fixes Applied" below).

---

## Executive Summary

| Metric | Value |
|--------|--------|
| **Overall security rating** | **8/10** (post-fix target) |
| **Critical issues** | 0 (all fixed) |
| **High severity issues** | 0 (all fixed) |
| **Medium severity issues** | 8 (M2, M6 addressed) |
| **Low severity issues** | 6 (L1, L3 addressed) |
| **Mainnet ready** | **Progress** (critical/high fixed; stress tests and audit recommended) |

The QYN codebase implements a functional devnet with PoS consensus types, EVM execution via revm, and JSON-RPC. **After the post-audit fixes:** gas fee burn (50%/50%) is applied in the execution path; double-sign detection and slashing are implemented; fork choice uses GHOST and checkpoint finality; RPC has rate limiting, body size limit, CORS, and timeouts; transaction hash uses Keccak256 + EIP-155; ValidatorSet is wired into block production and persisted; mempool eviction preserves nonce ordering. Remaining work: stress tests (e.g. 1k txs, TPS measurement), cargo audit, and optional medium/low items.

---

## Post-Audit Fixes Applied

- **C1 (Gas fee burn):** In `node/src/main.rs` `produce_block`, after VM execution we sum `gas_used × gas_price` per tx, call `genesis::split_fees`, and deduct the burn half from the proposer balance before computing state root. No double-credit.
- **C2 (Double-sign):** In `core/src/chain.rs`, we store `validator → height → block_hash` on each `put_block`. In `accept_block` we check for an existing signed block at the same height; on conflict we return `CoreError::DoubleSign`. The node records `SlashEvidence` and applies `slash_penalty_bps` to the validator balance. Evidence and signed-block mapping are persisted.
- **C3 (Fork choice):** In `core/src/fork.rs`, `canonical_head(chain, state)` implements GHOST (heaviest subtree by validator stake/balance). In `core/src/chain.rs`, `FINALITY_DEPTH` (100) is used: we persist finalized height/hash and reject reorgs that would go past finalized. Children index added for GHOST.
- **H1 (RPC hardening):** Rate limit 100 req/IP/sec, max body 1MB, request timeout 30s, CORS restricted to `https://getquyn.com` and `https://testnet.getquyn.com` when `QYN_PRODUCTION=1`, suspicious IPs logged on rate limit.
- **H2 (Tx hash):** `core/src/transaction.rs` uses Keccak256 and EIP-155 RLP for signing and for `SignedTransaction::hash`. Documented in `docs/SECURITY.md`.
- **H3 (Consensus integration):** ValidatorSet loaded from chain (or created at genesis), `select_proposer` called per block in `produce_block`; only the selected proposer produces. ValidatorSet persisted via `put_validator_set_bytes` / `get_validator_set_bytes`.
- **H4 (Mempool eviction):** Eviction is by sender: we evict the sender with lowest total fees and remove *all* their txs to preserve nonce ordering. Per-sender nonce gap > 10 is rejected.
- **L1:** `SystemTime::now().duration_since(UNIX_EPOCH)` now uses `map_err` and returns `Result`.
- **L3:** Testnet uses derivation path `m/44'/7778'/0'/0/index` via `derive_keypair_for_chain(_, _, 7778)`; `run_sign_tx` uses chain_id to pick path.
- **M2:** RPC methods validate params array and types (`require_param_count`, `require_param_string`) and return clear JSON-RPC errors.
- **M6:** Clippy warnings fixed (unused import, collapsible_else_if, needless_question_mark, useless_conversion, etc.); `cargo clippy --all-targets` clean (with allowed too_many_arguments where appropriate).

---

## Critical Issues (must fix before mainnet) — FIXED

### C1. Gas fee burn (50%) not applied in execution path
- **Location:** `node/src/main.rs` (`produce_block`), `vm/executor.rs`, `core/state.rs`
- **Description:** Tokenomics specify 50% of gas fees burned and 50% to block proposer. `genesis::split_fees` implements this, and `apply_simple_transfer_tx` uses it, but the node uses the VM (revm) for all transactions. Revm credits the full gas fee to the coinbase (proposer); no burn is applied. So effectively 0% burn, 100% to proposer.
- **Impact:** Inflation of effective supply (no burn), and tokenomics violation.
- **Fix required:** After VM execution, apply fee split: compute total gas used × gas price per tx, apply `split_fees`, credit proposer with half, and ensure the “burn” half is never credited (or deduct from a burn counter). Alternatively integrate fee split inside a custom revm post-execution hook or state wrapper.

### C2. No double-sign or equivocation detection in block acceptance
- **Location:** `core/chain.rs` (`accept_block`), `core/validation.rs`, `consensus/slashing.rs`
- **Description:** Slashing types exist (`SlashReason::DoubleSign`, `SlashEvidence`) and `slash_penalty_bps` is defined, but no code in the block acceptance or sync path records or checks for two blocks at the same height from the same validator. Validator set is not persisted or used in the devnet block producer.
- **Impact:** Nothing-at-stake and equivocation attacks are not detected or slashed; validators could sign multiple chains without penalty.
- **Fix required:** Persist signed block height per validator; in `accept_block` (or equivalent) reject or slash when two blocks at the same height from the same validator are observed. Wire slashing into state (stake reduction/deactivation).

### C3. Long-range attack and fork choice not stake-aware
- **Location:** `core/fork.rs` (`canonical_head`), `node/src/main.rs`
- **Description:** Fork choice is “longest chain” (current head). There is no stake-weighted or finality rule. A validator could create a long reorg from an old checkpoint if they had enough stake (or in devnet, the single validator can always reorg). No checkpoint/finality is enforced.
- **Impact:** Long-range attacks possible; no economic finality guarantee.
- **Fix required:** Implement stake-weighted fork choice (e.g. GHOST or similar) and/or finality gadget (e.g. attestations + finality threshold). Consider checkpoints for practical sync and reorg limits.

---

## High Severity Issues

### H1. RPC has no rate limiting or request size limits
- **Location:** `rpc/src/server.rs`
- **Description:** JSON-RPC handler accepts arbitrary-sized JSON and params. No per-IP or global rate limit, no max body size. CORS is permissive.
- **Impact:** DoS via large requests or request flooding; possible resource exhaustion.
- **Fix required:** Add rate limiting (e.g. tower middleware or per-connection limits), max request body size, and restrict CORS to known origins in production.

### H2. Transaction hash uses Sha256, not Keccak256
- **Location:** `core/src/transaction.rs` (`signing_hash`, `SignedTransaction::hash`)
- **Description:** Transaction signing hash and tx hash use Sha256. Ethereum uses Keccak256(RLP(tx)) and EIP-155 uses chain_id in RLP. Replay from Ethereum is not possible (different hash), but tooling and standards (e.g. Etherscan-style) expect Keccak256 tx hashes.
- **Impact:** Ecosystem compatibility; some wallets or indexers may assume Keccak256 tx hashes.
- **Fix required:** Document as intentional QYN difference, or migrate to Keccak256 + RLP for signing and hashing for compatibility.

### H3. Devnet uses fixed validator; consensus ValidatorSet not used
- **Location:** `node/src/main.rs` (`run_devnet`, `produce_block`)
- **Description:** Block producer uses a fixed address (e.g. `0x00...01`) and does not call `consensus::select_proposer` or maintain a live validator set from state. MIN_STAKE and validator rotation exist in `consensus` but are not integrated.
- **Impact:** Single point of control in devnet; no real PoS rotation or stake-based selection in production path.
- **Fix required:** For mainnet, maintain validator set (from state or contract), call `select_proposer` for each block, and enforce that only the selected proposer can produce the block for that slot.

### H4. Mempool eviction can drop same-sender nonce sequence
- **Location:** `core/src/mempool.rs` (`evict_lowest_fee`)
- **Description:** When over capacity, eviction is by lowest gas price globally. A sender’s later nonce (e.g. nonce 2) could be evicted while nonce 1 remains, breaking nonce ordering for that sender and potentially blocking their future txs until nonce 1 is included.
- **Impact:** User txs can become stuck or inconsistent; block builder may see incomplete nonce sequences.
- **Fix required:** Prefer evicting by sender (e.g. evict lowest-fee sender entirely) or enforce per-sender nonce ordering when evicting (evict highest nonce first per sender).

---

## Medium Severity Issues

### M1. Block timestamp tolerance may be too loose
- **Location:** `core/src/validation.rs` (`validate_block_header`)
- **Description:** Allows `header.timestamp > current_timestamp + BLOCK_TIME_SECS * 2` (6 seconds in future). This is reasonable but could be tightened for stricter sync.
- **Impact:** Minor; slightly more room for time manipulation.
- **Fix required:** Consider reducing to 1× block time or configurable bound; document the choice.

### M2. No RPC input validation on param length or count
- **Location:** `rpc/src/server.rs` (`dispatch`)
- **Description:** Params are taken by index; no check on array length or that required params are present before use (e.g. `param_str(&params, 0)` returns Option; missing params can yield empty string and then parse errors).
- **Impact:** Poor error messages; possible edge cases with malformed params.
- **Fix required:** Validate param array length and types per method; return clear JSON-RPC errors.

### M3. State root is hash of state, not Merkle trie
- **Location:** `core/src/state.rs` (`compute_state_root`)
- **Description:** State root is computed from a hash of stored state (implementation detail). Not a Merkle Patricia trie like Ethereum. Merkle proofs for light clients are not available.
- **Impact:** Light clients cannot verify state efficiently; different from Ethereum model.
- **Fix required:** Document as design choice; if light clients are required, consider introducing a trie-based state root.

### M4. P2P protocol has no explicit message size or rate limits
- **Location:** `network/src/protocol.rs`, `network/src/swarm.rs`
- **Description:** Block and tx messages are raw bytes; no explicit max size or rate limiting in the protocol types. Libp2p may have transport limits but not application-level.
- **Impact:** Large block/tx messages could stress memory or bandwidth.
- **Fix required:** Enforce max block size and max tx size in protocol; consider rate limits per peer.

### M5. `apply_simple_transfer_tx` unused; dead code path
- **Location:** `core/src/state.rs`
- **Description:** `apply_simple_transfer_tx` implements 50% burn / 50% proposer and is correct, but the node never calls it; all txs go through the VM. So fee-split logic is dead.
- **Impact:** Confusion; tokenomics code path unused (see C1).
- **Fix required:** Either remove it or use it (e.g. for simple transfers only); prefer unifying with VM path and applying split after VM (see C1).

### M6. Unused import and clippy warnings
- **Location:** `vm/src/abi.rs` (unused `VmError`), multiple files (see Section 7)
- **Description:** Clippy reports unused import, collapsible_else_if, too_many_arguments, useless_conversion, needless_borrows, redundant_closure.
- **Impact:** Code quality and maintainability; no direct security impact.
- **Fix required:** Run `cargo clippy --fix` and address remaining warnings.

### M7. Genesis alloc parsing accepts hex only; no decimal
- **Location:** `core/src/genesis.rs` (`parse_u256`)
- **Description:** Genesis balances must be hex. Misconfiguration (decimal) would fail parse.
- **Impact:** Operational mistake in genesis config; document format.
- **Fix required:** Document; optionally support decimal for operator convenience.

### M8. No explicit replay protection beyond chain_id and nonce
- **Location:** `core/src/transaction.rs`, `core/src/validation.rs`
- **Description:** Replay is prevented by chain_id in signing hash and nonce checks. No replay window or “recent nonce” rule beyond current nonce.
- **Impact:** Adequate for single-chain; document that multi-chain replay is prevented by chain_id.
- **Fix required:** None mandatory; document replay design.

---

## Low Severity Issues

### L1. `unwrap()` in `produce_block` on system time
- **Location:** `node/src/main.rs` (e.g. `SystemTime::now().duration_since(...).unwrap()`)
- **Description:** Panics if system time is before UNIX_EPOCH.
- **Impact:** Rare; could crash node on broken clock.
- **Fix required:** Prefer `map_err` and return Result or use a fallback timestamp.

### L2. Hardcoded devnet faucet key in binary
- **Location:** `node/src/main.rs` (`devnet_faucet_keypair`)
- **Description:** Faucet private key is deterministic and embedded. Anyone can derive it.
- **Impact:** Devnet only; expected. Must never be used for mainnet.
- **Fix required:** Document; ensure mainnet has no such key.

### L3. HD path uses chain type 7777 for testnet
- **Location:** `wallet/src/hd.rs` (`DERIVATION_PATH_PREFIX`)
- **Description:** Path is m/44'/7777'/0'/0/index. Testnet uses chain ID 7778; path could be 7778 for testnet wallets.
- **Impact:** Same path for mainnet and testnet; keys shared across chains if user reuses mnemonic.
- **Fix required:** Consider chain-specific path for testnet (e.g. 7778) or document key reuse implications.

### L4. Validator set in consensus is in-memory only
- **Location:** `consensus/src/validator_set.rs`
- **Description:** ValidatorSet is not persisted to chain state; restart loses registration.
- **Impact:** For mainnet, validator set must be in state or contract.
- **Fix required:** Persist validator set in state/contract and load on startup.

### L5. No block propagation or sync in devnet
- **Location:** `node/src/main.rs`
- **Description:** Devnet is single-node; no P2P block sync or propagation tested.
- **Impact:** Multi-node and sync security not exercised.
- **Fix required:** Add integration tests for multi-node sync and propagation.

### L6. cargo audit not run
- **Location:** N/A
- **Description:** `cargo audit` was not available in the audit environment. Dependencies were not scanned for known CVEs.
- **Impact:** Unknown vulnerabilities in dependencies may remain.
- **Fix required:** Install `cargo-audit` and run regularly; fix reported advisories.

---

## Test Results

| Category | Result | Notes |
|----------|--------|------|
| **Consensus tests** | Pass | Unit tests for validator set and proposer selection pass. ValidatorSet wired in devnet; select_proposer used. |
| **Cryptography tests** | Pass | Wallet HD and signing tests pass. Keccak256 + EIP-155 for tx hash; chain_id in signing. |
| **EVM tests** | Pass | revm (CANCUN) used; fee burn applied after execution in produce_block. |
| **Network tests** | Not run | P2P protocol exists; no automated network stress or eclipse tests. |
| **Economic tests** | Pass | Fee burn 50%/50% applied in execution path (C1 fixed). |
| **Stress tests** | Recommended | Run 1k txs rapidly and measure TPS; 24h run and cargo audit still recommended. |
| **Code quality** | Pass | `cargo clippy --all-targets` clean; unwrap() removed in produce_block (L1). |
| **Wallet security** | Pass | No private key or mnemonic in RPC or logs; BIP39/BIP44; testnet path 7778 (L3). |

---

## Recommendations for Mainnet

1. **Implement gas fee burn in execution path** (C1): Apply 50% burn / 50% proposer after VM execution; ensure no double-credit of fees.
2. **Implement double-sign detection and slashing** (C2): Record signed blocks per validator per height; reject or slash on equivocation; persist slashing in state.
3. **Implement stake-aware fork choice and/or finality** (C3): Replace “longest chain” with stake-weighted rule or finality gadget; consider checkpoints.
4. **Harden RPC** (H1): Rate limiting, max body size, and restrictive CORS.
5. **Decide on tx hash standard** (H2): Document Sha256 vs Keccak256; align tooling or migrate.
6. **Wire consensus into block production** (H3): Use ValidatorSet and select_proposer; enforce single proposer per slot.
7. **Fix mempool eviction** (H4): Evict by sender or by nonce order so same-sender ordering is preserved.
8. **Run and automate stress tests**: 1k/10k/100k txs, 24h run, memory/disk checks; document TPS and limits.
9. **Run cargo audit**: Install and run regularly; fix CVEs.
10. **Address clippy and unwrap()**: Reduce panics in production paths; fix all clippy warnings.

---

## Conclusion

**Is the blockchain ready for mainnet?** **Progress.** Critical and high severity issues from this audit are fixed.

**Done:**

1. **Critical:** Gas fee burn (C1), double-sign detection and slashing (C2), GHOST fork choice and checkpoint finality (C3).
2. **High:** RPC hardening (H1), Keccak256 + EIP-155 tx hash (H2), ValidatorSet and select_proposer in block production (H3), mempool eviction by sender (H4).
3. **Low/medium:** L1 unwrap() fixed, L3 testnet HD path, M2 RPC param validation, M6 clippy.

**Recommended before mainnet:**

1. Run **cargo audit** and fix any advisories.
2. Run **stress tests**: e.g. send 1000 transactions rapidly, verify inclusion and measure TPS; consider 24h run.
3. Address remaining medium/low items (M1, M3–M5, M7–M8, L2, L4–L6) as part of release readiness.

The codebase is suitable for **testnet and development** and has made clear progress toward mainnet readiness.
