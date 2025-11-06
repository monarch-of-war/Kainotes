// networking/src/peer.rs

use crate::{NetworkError, NetworkResult};
use blockchain_core::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;

/// Peer identifier (using libp2p PeerId concept)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PeerId([u8; 32]);

impl PeerId {
    /// Create new peer ID from bytes
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Generate random peer ID
    pub fn random() -> Self {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let bytes: [u8; 32] = rng.gen();
        Self(bytes)
    }

    /// Get as bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

/// Peer status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PeerStatus {
    /// Connecting
    Connecting,
    /// Connected and handshake complete
    Connected,
    /// Temporarily disconnected
    Disconnected,
    /// Banned due to misbehavior
    Banned,
}

/// Peer information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    /// Peer identifier
    pub id: PeerId,
    /// Network address
    pub address: SocketAddr,
    /// Current status
    pub status: PeerStatus,
    /// Peer's latest block number
    pub best_block: u64,
    /// Peer's protocol version
    pub protocol_version: u32,
    /// Client version string
    pub client_version: String,
    /// Connection timestamp
    pub connected_at: Timestamp,
    /// Last message timestamp
    pub last_seen: Timestamp,
    /// Reputation score
    pub reputation: i32,
    /// Is this an outbound connection (we initiated)
    pub outbound: bool,
}

impl PeerInfo {
    /// Create new peer info
    pub fn new(
        id: PeerId,
        address: SocketAddr,
        protocol_version: u32,
        client_version: String,
        outbound: bool,
    ) -> Self {
        let now = current_timestamp();
        Self {
            id,
            address,
            status: PeerStatus::Connecting,
            best_block: 0,
            protocol_version,
            client_version,
            connected_at: now,
            last_seen: now,
            reputation: 0,
            outbound,
        }
    }

    /// Update last seen timestamp
    pub fn update_last_seen(&mut self) {
        self.last_seen = current_timestamp();
    }

    /// Check if peer is connected
    pub fn is_connected(&self) -> bool {
        self.status == PeerStatus::Connected
    }

    /// Check if peer is banned
    pub fn is_banned(&self) -> bool {
        self.status == PeerStatus::Banned
    }

    /// Increase reputation
    pub fn increase_reputation(&mut self, amount: i32) {
        self.reputation = self.reputation.saturating_add(amount).min(1000);
    }

    /// Decrease reputation
    pub fn decrease_reputation(&mut self, amount: i32) {
        self.reputation = self.reputation.saturating_sub(amount);
        if self.reputation < -100 {
            self.status = PeerStatus::Banned;
        }
    }
}

/// Peer manager
pub struct PeerManager {
    /// All known peers
    peers: HashMap<PeerId, PeerInfo>,
    /// Maximum peers to maintain
    max_peers: usize,
    /// Maximum inbound connections
    max_inbound: usize,
    /// Maximum outbound connections
    max_outbound: usize,
}

impl PeerManager {
    /// Create new peer manager
    pub fn new(max_peers: usize, max_inbound: usize, max_outbound: usize) -> Self {
        Self {
            peers: HashMap::new(),
            max_peers,
            max_inbound,
            max_outbound,
        }
    }

    /// Add a peer
    pub fn add_peer(&mut self, peer: PeerInfo) -> NetworkResult<()> {
        if self.peers.len() >= self.max_peers {
            return Err(NetworkError::PeerError("Maximum peers reached".into()));
        }

        // Check inbound/outbound limits
        let (inbound_count, outbound_count) = self.connection_counts();
        
        if peer.outbound && outbound_count >= self.max_outbound {
            return Err(NetworkError::PeerError("Maximum outbound connections reached".into()));
        }
        
        if !peer.outbound && inbound_count >= self.max_inbound {
            return Err(NetworkError::PeerError("Maximum inbound connections reached".into()));
        }

        self.peers.insert(peer.id, peer);
        Ok(())
    }

    /// Remove a peer
    pub fn remove_peer(&mut self, peer_id: &PeerId) -> Option<PeerInfo> {
        self.peers.remove(peer_id)
    }

    /// Get peer info
    pub fn get_peer(&self, peer_id: &PeerId) -> Option<&PeerInfo> {
        self.peers.get(peer_id)
    }

    /// Get mutable peer info
    pub fn get_peer_mut(&mut self, peer_id: &PeerId) -> Option<&mut PeerInfo> {
        self.peers.get_mut(peer_id)
    }

    /// Get all connected peers
    pub fn connected_peers(&self) -> Vec<&PeerInfo> {
        self.peers.values()
            .filter(|p| p.is_connected())
            .collect()
    }

    /// Get all peers
    pub fn all_peers(&self) -> Vec<&PeerInfo> {
        self.peers.values().collect()
    }

    /// Get peer count
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Get connected peer count
    pub fn connected_count(&self) -> usize {
        self.connected_peers().len()
    }

    /// Get connection counts (inbound, outbound)
    pub fn connection_counts(&self) -> (usize, usize) {
        let mut inbound = 0;
        let mut outbound = 0;
        
        for peer in self.peers.values() {
            if peer.is_connected() {
                if peer.outbound {
                    outbound += 1;
                } else {
                    inbound += 1;
                }
            }
        }
        
        (inbound, outbound)
    }

    /// Update peer status
    pub fn update_status(&mut self, peer_id: &PeerId, status: PeerStatus) -> NetworkResult<()> {
        let peer = self.peers.get_mut(peer_id)
            .ok_or_else(|| NetworkError::PeerError("Peer not found".into()))?;
        
        peer.status = status;
        Ok(())
    }

    /// Update peer best block
    pub fn update_best_block(&mut self, peer_id: &PeerId, block_number: u64) -> NetworkResult<()> {
        let peer = self.peers.get_mut(peer_id)
            .ok_or_else(|| NetworkError::PeerError("Peer not found".into()))?;
        
        peer.best_block = block_number;
        peer.update_last_seen();
        Ok(())
    }

    /// Ban a peer
    pub fn ban_peer(&mut self, peer_id: &PeerId, reason: &str) -> NetworkResult<()> {
        let peer = self.peers.get_mut(peer_id)
            .ok_or_else(|| NetworkError::PeerError("Peer not found".into()))?;
        
        peer.status = PeerStatus::Banned;
        tracing::warn!("Peer {} banned: {}", peer_id.to_hex(), reason);
        
        Ok(())
    }

    /// Get peers for sync (highest block number)
    pub fn get_sync_peers(&self, min_block: u64) -> Vec<&PeerInfo> {
        let mut peers: Vec<_> = self.peers.values()
            .filter(|p| p.is_connected() && p.best_block >= min_block)
            .collect();
        
        peers.sort_by(|a, b| b.best_block.cmp(&a.best_block));
        peers
    }

    /// Prune disconnected and low reputation peers
    pub fn prune_peers(&mut self, timeout_seconds: u64) {
        let now = current_timestamp();
        let timeout = timeout_seconds;
        
        self.peers.retain(|_, peer| {
            // Keep if connected
            if peer.is_connected() {
                return true;
            }
            
            // Keep if recently seen
            if now - peer.last_seen < timeout {
                return true;
            }
            
            // Keep if good reputation
            if peer.reputation > 50 {
                return true;
            }
            
            false
        });
    }

    /// Get best connected peer (by block number)
    pub fn best_peer(&self) -> Option<&PeerInfo> {
        self.connected_peers()
            .into_iter()
            .max_by_key(|p| p.best_block)
    }
}

/// Helper to get current timestamp
fn current_timestamp() -> Timestamp {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn create_test_peer(outbound: bool) -> PeerInfo {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        PeerInfo::new(
            PeerId::random(),
            addr,
            1,
            "test-client/1.0".into(),
            outbound,
        )
    }

    #[test]
    fn test_peer_manager() {
        let mut manager = PeerManager::new(100, 50, 50);
        
        let peer = create_test_peer(true);
        let peer_id = peer.id;
        
        manager.add_peer(peer).unwrap();
        assert_eq!(manager.peer_count(), 1);
        assert!(manager.get_peer(&peer_id).is_some());
    }

    #[test]
    fn test_max_peers_limit() {
        let mut manager = PeerManager::new(2, 1, 1);
        
        manager.add_peer(create_test_peer(true)).unwrap();
        manager.add_peer(create_test_peer(false)).unwrap();
        
        let result = manager.add_peer(create_test_peer(true));
        assert!(result.is_err());
    }

    #[test]
    fn test_reputation() {
        let mut peer = create_test_peer(true);
        
        peer.increase_reputation(50);
        assert_eq!(peer.reputation, 50);
        
        peer.decrease_reputation(100);
        assert_eq!(peer.reputation, -50);
        
        peer.decrease_reputation(100);
        assert!(peer.is_banned());
    }

    #[test]
    fn test_connected_peers() {
        let mut manager = PeerManager::new(100, 50, 50);
        
        let mut peer1 = create_test_peer(true);
        peer1.status = PeerStatus::Connected;
        let mut peer2 = create_test_peer(true);
        peer2.status = PeerStatus::Disconnected;
        
        manager.add_peer(peer1).unwrap();
        manager.add_peer(peer2).unwrap();
        
        assert_eq!(manager.connected_count(), 1);
    }

    #[test]
    fn test_sync_peers() {
        let mut manager = PeerManager::new(100, 50, 50);
        
        let mut peer1 = create_test_peer(true);
        peer1.status = PeerStatus::Connected;
        peer1.best_block = 100;
        
        let mut peer2 = create_test_peer(true);
        peer2.status = PeerStatus::Connected;
        peer2.best_block = 200;
        
        manager.add_peer(peer1).unwrap();
        manager.add_peer(peer2).unwrap();
        
        let sync_peers = manager.get_sync_peers(150);
        assert_eq!(sync_peers.len(), 1);
        assert_eq!(sync_peers[0].best_block, 200);
    }
}