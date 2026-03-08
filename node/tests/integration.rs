//! Integration tests for Quyn node. Run with: cargo test -p quyn --test integration

#[test]
fn full_node_opens_data_dir() {
    let dir = tempfile::tempdir().unwrap();
    let node = quyn::runner::FullNode::open(dir.path());
    assert!(node.is_ok());
}
