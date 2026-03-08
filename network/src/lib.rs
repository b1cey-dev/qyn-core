//! Quyn P2P networking with libp2p: discovery, block/tx propagation, sync.

pub mod error;
pub mod protocol;
pub mod swarm;

pub use error::NetworkError;
pub use protocol::{BlockRequest, BlockResponse, TxMessage, PROTOCOL_BLOCKS, PROTOCOL_TX};
pub use swarm::{build_swarm, QuynBehaviour};
