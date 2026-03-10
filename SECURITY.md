# Security Policy — Quyn Technologies Ltd

## Supported Versions

| Version | Status |
| ------- | ------ |
| Testnet (Chain ID 7779) | Actively supported |
| Mainnet (Chain ID 7777) | Not yet launched — Q4 2026 |

---

## Reporting a Vulnerability

If you discover a security vulnerability in the QYN blockchain, RPC, or any 
associated infrastructure, please report it responsibly through our 
coordinated disclosure process.

**Contact:** security@getquyn.com  
**Response time:** Within 48 hours of receipt  
**Resolution target:** Critical issues within 7 days

Please include in your report:
- A clear description of the vulnerability
- Steps to reproduce the issue
- The potential impact if exploited
- Any suggested remediation if known

We ask that you do not publicly disclose any vulnerability until we have had 
reasonable time to investigate and address it. We will acknowledge receipt of 
your report, keep you informed of our progress, and credit your contribution 
if you wish to be recognised.

We will not take legal action against researchers who report vulnerabilities 
in good faith and in accordance with this policy.

---

## Bug Bounty Program

The QYN bug bounty program is scheduled to launch in Q3 2026 ahead of 
mainnet. Rewards will be paid in QYN tokens upon mainnet launch.

| Severity | Description | Reward |
|----------|-------------|--------|
| Critical | Remote code execution, consensus bypass, double spend, unauthorised fund access | Up to 50,000 QYN |
| High | RPC exploits, validator manipulation, significant data exposure | Up to 20,000 QYN |
| Medium | Denial of service, mempool manipulation, non-critical data leakage | Up to 5,000 QYN |
| Low | Minor bugs, documentation issues, low impact findings | Up to 1,000 QYN |

Full terms and conditions will be published at getquyn.com/security 
when the programme launches. Reports submitted before the programme 
launches will be reviewed and may be eligible for retroactive rewards 
at our discretion.

---

## Security Audit

The QYN codebase has undergone an internal security audit scoring 92 out of 
100. The full audit report including all findings, fixes applied, and attack 
simulation results is available at:

https://getquyn.com/security

An external third party audit is planned for Q3 2026 prior to mainnet launch.

---

## Known Limitations — Testnet

The following are known limitations currently being addressed ahead of 
mainnet readiness:

**P2P block propagation:**
Block synchronisation between multiple nodes is not yet fully implemented. 
The current testnet operates as a single-node devnet. Multi-node P2P sync 
is planned for Q3 2026.

**Light client proofs:**
The current state root uses a hash-based structure. Merkle Patricia Trie 
with full proof support is planned prior to mainnet.

**Dependency auditing:**
Running cargo audit requires Rust 1.88 or later. Run cargo audit when 
upgrading Rust to check all dependencies for known CVEs.

**EIP-1559 transactions:**
Full EIP-1559 type 2 transaction support is in progress. Legacy transactions 
are fully supported. Use the legacy flag when testing with Foundry or cast.

---

## Security Best Practices

The following practices are required for anyone running or developing on 
the QYN network:

- Never use the devnet faucet key for mainnet or any wallet holding real funds.
- Never commit private keys, mnemonics, or seed phrases to version control.
- Always store sensitive credentials in environment variables, never hardcoded.
- Set QYN_PRODUCTION=1 in production environments to enable restrictive 
  CORS policies and security headers.
- Use HTTPS exclusively when connecting to the RPC endpoint. 
  The production RPC is available at https://rpc.getquyn.com only.
- Regularly run cargo audit to check for dependency vulnerabilities.

---

## Contact

For all security related enquiries contact:

**Email:** security@getquyn.com  
**Website:** https://getquyn.com/security  
**Company:** Quyn Technologies Ltd, Leicester, United Kingdom
