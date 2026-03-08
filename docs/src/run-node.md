# Run a Node

## Full node

```bash
cargo run -p quyn -- full --data-dir ./data --rpc-addr 127.0.0.1:8545
```

Ensure genesis has been applied to the state (see core/genesis and genesis.json). For a fresh chain, run once with genesis allocation applied to the state DB.

## Local devnet

Use the same command with an empty or devnet data dir. Configure genesis for a single validator and prefunded accounts for testing.
