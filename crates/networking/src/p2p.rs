// networking/src/p2p.rs
use crate::{peer::PeerManager, protocol::ProtocolMessage, NetworkResult};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub listen_addr: SocketAddr,
    pub max_peers: usize,
    pub max_inbound: usize,
    pub max_outbound: usize,
    pub bootstrap_peers: Vec<SocketAddr>,
}

#[derive(Debug, Clone)]
pub enum NetworkEvent {
    PeerConnected(crate::peer::PeerId),
    PeerDisconnected(crate::peer::PeerId),
    MessageReceived(crate::peer::PeerId, ProtocolMessage),
}

pub struct NetworkService {
    config: NetworkConfig,
    peer_manager: PeerManager,
}

impl NetworkService {
    pub fn new(config: NetworkConfig) -> Self {
        Self {
            peer_manager: PeerManager::new(
                config.max_peers,
                config.max_inbound,
                config.max_outbound,
            ),
            config,
        }
    }

    pub async fn start(&mut self) -> NetworkResult<()> {
        tracing::info!("Network service started on {}", self.config.listen_addr);
        Ok(())
    }

    pub fn peer_manager(&self) -> &PeerManager {
        &self.peer_manager
    }

    pub fn peer_manager_mut(&mut self) -> &mut PeerManager {
        &mut self.peer_manager
    }
}