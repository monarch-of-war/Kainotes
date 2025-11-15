// networking/src/p2p.rs
use crate::{peer::PeerManager, protocol::ProtocolMessage, NetworkResult};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::collections::{HashMap, HashSet};
use blockchain_core::{Transaction, TransactionPool, fork::ForkResolver};
use blockchain_crypto::Hash;
use crate::peer::PeerId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub listen_addr: SocketAddr,
    pub max_peers: usize,
    pub max_inbound: usize,
    pub max_outbound: usize,
    pub bootstrap_peers: Vec<SocketAddr>,
    // New configuration flags
    pub enable_tx_gossip: bool,
    pub mempool_sync_on_connect: bool,
    pub max_tx_propagate_peers: usize,
    pub fork_detection_enabled: bool,
}

#[derive(Debug, Clone)]
pub enum NetworkEvent {
    PeerConnected(crate::peer::PeerId),
    PeerDisconnected(crate::peer::PeerId),
    MessageReceived(crate::peer::PeerId, ProtocolMessage),
}

pub struct NetworkService {
    config: NetworkConfig,
    peer_manager: Arc<Mutex<PeerManager>>,
    /// Recently seen transactions (hash -> timestamp)
    seen_tx: Arc<Mutex<HashMap<Hash, u64>>>,
    /// Inflight requests tracking (simple string keys)
    inflight: Arc<Mutex<HashSet<String>>>,
    /// Outbox for messages (test/transport shim)
    outbox: Arc<Mutex<HashMap<PeerId, Vec<ProtocolMessage>>>>,
    /// Optional handle to the local mempool
    pub mempool: Option<Arc<Mutex<TransactionPool>>>,
    /// Optional fork resolver reference
    pub fork_resolver: Option<Arc<Mutex<ForkResolver>>>,
}

impl NetworkService {
    pub fn new(config: NetworkConfig) -> Self {
        Self {
            peer_manager: Arc::new(Mutex::new(PeerManager::new(
                config.max_peers,
                config.max_inbound,
                config.max_outbound,
            ))),
            config,
            seen_tx: Arc::new(Mutex::new(HashMap::new())),
            inflight: Arc::new(Mutex::new(HashSet::new())),
            outbox: Arc::new(Mutex::new(HashMap::new())),
            mempool: None,
            fork_resolver: None,
        }
    }

    /// Attach a mempool instance to the network service for direct writes
    pub fn set_mempool(&mut self, pool: Arc<Mutex<TransactionPool>>) {
        self.mempool = Some(pool);
    }

    /// Attach a fork resolver to the network service
    pub fn set_fork_resolver(&mut self, resolver: Arc<Mutex<ForkResolver>>) {
        self.fork_resolver = Some(resolver);
    }

    /// Send a protocol message to a specific peer (outbox shim)
    pub async fn send_to_peer(&self, peer_id: PeerId, msg: ProtocolMessage) -> NetworkResult<()> {
        let mut out = self.outbox.lock().await;
        out.entry(peer_id).or_insert_with(Vec::new).push(msg);
        Ok(())
    }

    /// Broadcast message to up-to `limit` peers (skips `exclude`)
    pub async fn broadcast(&self, msg: ProtocolMessage, limit: usize, exclude: Option<PeerId>) -> NetworkResult<()> {
        let pm = self.peer_manager.lock().await;
        let peers = pm.connected_peers();
        let mut sent = 0usize;
        for p in peers {
            if let Some(ex) = exclude { if p.id == ex { continue; } }
            self.send_to_peer(p.id, msg.clone()).await?;
            sent += 1;
            if sent >= limit { break; }
        }
        Ok(())
    }

    /// Handle an incoming protocol message from a peer
    pub async fn handle_incoming_message(&self, peer_id: crate::peer::PeerId, msg: ProtocolMessage) -> NetworkResult<()> {
        use crate::protocol::ProtocolMessage::*;

        match msg {
            NewPendingTransaction(tx_msg) => {
                self.handle_new_pending_transaction(peer_id, tx_msg).await
            }
            RequestMempoolSync(req) => {
                self.handle_mempool_sync_request(peer_id, req).await
            }
            ForkDetected(fmsg) => {
                self.handle_fork_detected(peer_id, fmsg).await
            }
            RequestChainSegment(req) => {
                self.handle_request_chain_segment(peer_id, req).await
            }
            // Fallbacks - other messages are handled elsewhere
            _ => Ok(()),
        }
    }

    async fn handle_new_pending_transaction(&self, peer_id: crate::peer::PeerId, msg: crate::protocol::NewPendingTransactionMessage) -> NetworkResult<()> {
        // Dedup by hash
        let tx_hash = msg.transaction.hash();
        let mut seen = self.seen_tx.lock().await;
        if seen.contains_key(&tx_hash) {
            // Already seen; ignore
            return Ok(());
        }

        // Validate basic
        if let Err(e) = msg.transaction.validate_basic() {
            tracing::warn!("Invalid transaction from peer {}: {:?}", peer_id.to_hex(), e);
            return Err(crate::NetworkError::InvalidMessage(format!("Invalid transaction: {:?}", e)));
        }

        // Insert into seen cache
        seen.insert(tx_hash, current_timestamp());
        drop(seen);

        // Add to mempool if available (best-effort)
        if let Some(pool) = &self.mempool {
            let mut pool = pool.lock().await;
            // best-effort current_nonce = 0 (caller should provide better info)
            if pool.add(msg.transaction.clone(), 0).is_ok() {
                // reward peer for valid tx
                let mut pm = self.peer_manager.lock().await;
                if let Some(peer) = pm.get_peer_mut(&peer_id) {
                    peer.increase_reputation(1);
                }
            } else {
                let mut pm = self.peer_manager.lock().await;
                if let Some(peer) = pm.get_peer_mut(&peer_id) {
                    peer.decrease_reputation(5);
                }
            }
        }

        // Propagate to other peers (simple broadcast respecting max peers)
        if self.config.enable_tx_gossip {
            let pm = self.peer_manager.lock().await;
            let peers = pm.connected_peers();
            let mut forwarded = 0usize;
            for p in peers {
                if p.id == peer_id { continue; }
                // send NewPendingTransaction to peer outbox
                let forward_msg = crate::protocol::ProtocolMessage::NewPendingTransaction(crate::protocol::NewPendingTransactionMessage {
                    transaction: msg.transaction.clone(),
                    gas_price: msg.gas_price,
                    timestamp: msg.timestamp,
                });
                let _ = self.send_to_peer(p.id, forward_msg).await;
                forwarded += 1;
                if forwarded >= self.config.max_tx_propagate_peers { break; }
            }
        }

        Ok(())
    }

    async fn handle_mempool_sync_request(&self, peer_id: crate::peer::PeerId, req: crate::protocol::RequestMempoolSyncMessage) -> NetworkResult<()> {
        // Rate limiting: simple inflight key
        let key = format!("mempool_sync:{}", peer_id.to_hex());
        {
            let mut inflight = self.inflight.lock().await;
            if inflight.contains(&key) {
                return Err(crate::NetworkError::Timeout);
            }
            inflight.insert(key.clone());
        }

        // Prepare response
        let mut txs = Vec::new();
        if let Some(pool) = &self.mempool {
            let pool = pool.lock().await;
            // Iterate pending transactions and filter by gas price
            for tx in pool.get_pending(u64::MAX, req.max_count) {
                if tx.gas_price >= req.min_gas_price {
                    txs.push(tx);
                }
                if txs.len() >= req.max_count { break; }
            }
        }

        // Send response via outbox
        let response = crate::protocol::ProtocolMessage::MempoolSyncResponse(crate::protocol::MempoolSyncResponseMessage { transactions: txs });
        let _ = self.send_to_peer(peer_id, response).await;
        tracing::debug!("Responding mempool sync to {}", peer_id.to_hex());

        // clear inflight
        let mut inflight = self.inflight.lock().await;
        inflight.remove(&key);

        Ok(())
    }

    async fn handle_fork_detected(&self, peer_id: crate::peer::PeerId, msg: crate::protocol::ForkDetectedMessage) -> NetworkResult<()> {
        tracing::info!("Received fork detected from {}: fork_point={}", peer_id.to_hex(), hex::encode(msg.fork_point_hash.as_bytes()));

        if !self.config.fork_detection_enabled { return Ok(()); }

        // Basic acceptance: if we have a fork_resolver, record
        if let Some(resolver) = &self.fork_resolver {
            let mut resolver = resolver.lock().await;
            // Create a ForkInfo placeholder if possible
            // Real validation would require fetching competing tips/blocks
            // We'll just record a lightweight info if possible
            // (fork_point -> use hash as placeholder number 0)
            // Note: we can't easily construct ForkInfo without block numbers here
            tracing::debug!("ForkResolver available; noting fork notification");
        }

        Ok(())
    }

    async fn handle_request_chain_segment(&self, peer_id: crate::peer::PeerId, req: crate::protocol::RequestChainSegmentMessage) -> NetworkResult<()> {
        tracing::debug!("Peer {} requested chain segment {}..{}", peer_id.to_hex(), req.start_block, req.end_block);
        // In a full implementation we'd fetch blocks and respond. Here we log.
        Ok(())
    }

    pub async fn start(&mut self) -> NetworkResult<()> {
        tracing::info!("Network service started on {}", self.config.listen_addr);
        Ok(())
    }

    pub fn peer_manager(&self) -> &PeerManager {
        // Convenience: return a locked reference is not possible here; callers should lock the Arc
        // Deprecated: this method kept for compatibility; prefer `peer_manager_arc()`
        panic!("Use peer_manager_arc() to access peer manager")
    }

    pub fn peer_manager_mut(&mut self) -> &mut PeerManager {
        panic!("peer_manager is now internally synchronized; use peer_manager_arc()");
    }

    /// Return the Arc<Mutex<PeerManager>> so callers can lock for access
    pub fn peer_manager_arc(&self) -> Arc<Mutex<PeerManager>> {
        self.peer_manager.clone()
    }

    /// Read the outbox for tests / transport verification
    pub async fn drain_outbox_for(&self, peer_id: PeerId) -> Vec<ProtocolMessage> {
        let mut out = self.outbox.lock().await;
        out.remove(&peer_id).unwrap_or_default()
    }
}

/// Helper to get current timestamp
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}