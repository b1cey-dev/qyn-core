# QYN Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| Testnet | ✅ Fully supported |
| Mainnet | ⏳ Not yet launched |

## Reporting a Vulnerability

**Email:** security@getquyn.com

**Response time:** 48 hours

We take security seriously. If you discover a vulnerability, please report it responsibly. We will acknowledge receipt and work with you to understand and address the issue.

## Bug Bounty Program (Coming Q3 2026)

| Severity | Reward (QYN) |
|----------|--------------|
| Critical | Up to 50,000 |
| High     | Up to 20,000 |
| Medium   | Up to 5,000  |
| Low      | Up to 1,000  |

*Terms and conditions will be published when the program launches.*

## Known Limitations (Testnet)

The following are known limitations being worked on for mainnet readiness:

- **P2P block propagation:** Block sync between nodes is not yet implemented; devnet is single-node.
- **Light client proofs:** State root uses hash-based structure; Merkle Patricia Trie with proof support is planned.
- **cargo audit:** Requires Rust 1.88+ to run; run `cargo audit` when upgrading Rust to check dependencies for CVEs.

## Security Best Practices

- Never use the devnet faucet key for mainnet or any real funds.
- Keep mnemonic and private keys secure; never commit them to version control.
- Use `QYN_PRODUCTION=1` in production to enable restrictive CORS and security headers.
