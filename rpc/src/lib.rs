//! Quyn RPC - JSON-RPC, REST, WebSocket (stub).

pub mod error;
pub mod server;

pub use error::RpcError;
pub use server::{serve, AppState};
