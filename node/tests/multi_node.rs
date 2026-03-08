//! Multi-node sync integration tests.
//!
//! NOTE: P2P block propagation is not yet implemented in the node. The network module
//! has protocol types and swarm for discovery, but blocks are not propagated between
//! nodes. These tests document the expected behavior and provide a skeleton for when
//! P2P sync is implemented.
//!
//! Run with: cargo test -p quyn --test multi_node

/// Test 1: Three node sync - PLACEHOLDER
/// When P2P is implemented: start 3 nodes, connect via P2P, produce blocks on node 1,
/// verify nodes 2 and 3 sync and reach same block hash.
#[test]
fn multi_node_three_node_sync() {
    // P2P block propagation not implemented. Node runs single-node devnet only.
    // Placeholder: implement when network sync is added.
}

/// Test 2: Node crash recovery - PLACEHOLDER
/// When P2P is implemented: start 3 nodes synced, kill node 2, produce 10 blocks on
/// node 1, restart node 2, verify it catches up.
#[test]
fn multi_node_crash_recovery() {
    // Placeholder: implement when network sync is added.
}

/// Test 3: Network partition - PLACEHOLDER
/// When P2P is implemented: start 3 nodes, disconnect node 3, produce blocks on 1 and 2,
/// reconnect node 3, verify canonical chain resolution.
#[test]
fn multi_node_network_partition() {
    // Placeholder: implement when network sync is added.
}
