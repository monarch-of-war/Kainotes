// networking/src/lib.rs

//! P2P Networking Layer
//!
//! This crate implements the peer-to-peer networking layer using libp2p:
//! - Peer discovery and management
//! - Block propagation
//! - Transaction broadcasting
//! - State synchronization
//! - Gossip protocol

pub mod p2p;
pub mod sync;
pub mod protocol;
pub mod gossip;
pub mod peer;

pub use p2p::{NetworkService, NetworkConfig, NetworkEvent};
pub use peer::PeerId;
pub use sync::{SyncManager, SyncStatus, SyncStrategy};
pub use protocol::{ProtocolMessage, MessageType};
pub use gossip::{GossipService, GossipTopic};
pub use peer::{PeerInfo, PeerManager, PeerStatus};

/// Result type for networking operations
pub type NetworkResult<T> = Result<T, NetworkError>;

/// Errors that can occur during networking operations
#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    #[error("Connection error: {0}")]
    ConnectionError(String),
    
    #[error("Protocol error: {0}")]
    ProtocolError(String),
    
    #[error("Peer error: {0}")]
    PeerError(String),
    
    #[error("Sync error: {0}")]
    SyncError(String),
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    #[error("Timeout")]
    Timeout,
    
    #[error("Invalid message: {0}")]
    InvalidMessage(String),
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic_imports() {
        // Smoke test
    }
}