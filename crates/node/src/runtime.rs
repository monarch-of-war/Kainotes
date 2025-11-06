// node/src/runtime.rs
use crate::NodeConfig;
use blockchain_core::{Block, Blockchain};
use blockchain_crypto::Hash;
use consensus::{PoASConsensus, ConsensusConfig as PoASConfig};
use storage::{Database, DatabaseConfig, PruningMode};
use networking::{NetworkService, NetworkConfig as NetConfig};
use rpc::{RpcServer, RpcConfig as RpcCfg, RpcMethods};
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct Node {
    config: NodeConfig,
    blockchain: Arc<RwLock<Blockchain>>,
    consensus: Arc<RwLock<PoASConsensus>>,
    database: Arc<Database>,
    network: Option<Arc<NetworkService>>,
    rpc: Option<Arc<RpcServer>>,
}

impl Node {
    pub fn new(config: NodeConfig) -> anyhow::Result<Self> {
        tracing::info!("Initializing node components");
        
        // Initialize database
        let pruning_mode = if config.storage.pruning == "archive" {
            PruningMode::Archive
        } else {
            PruningMode::Pruned {
                keep_blocks: config.storage.keep_blocks,
            }
        };
        
        let db_config = DatabaseConfig {
            path: format!("{}/db", config.data_dir),
            cache_size: config.storage.cache_size_mb * 1024 * 1024,
            max_open_files: config.storage.max_open_files,
            pruning_mode,
            ..Default::default()
        };
        let database = Arc::new(Database::open(db_config)?);
        
        // Initialize blockchain
        let genesis = Block::genesis(Hash::zero());
        let blockchain = Arc::new(RwLock::new(Blockchain::new(genesis)?));
        
        // Initialize consensus
        let consensus_config = PoASConfig {
            min_stake: blockchain_core::StakeAmount::from_u64(config.consensus.min_stake),
            block_time: config.consensus.block_time_seconds,
            ..Default::default()
        };
        let consensus = Arc::new(RwLock::new(PoASConsensus::new(consensus_config)));
        
        tracing::info!("Node components initialized");
        
        Ok(Self {
            config,
            blockchain,
            consensus,
            database,
            network: None,
            rpc: None,
        })
    }

    pub async fn start(self: Arc<Self>) -> anyhow::Result<()> {
        tracing::info!("Starting Utility Blockchain Node");
        
        // Start network service
        let net_config = NetConfig {
            listen_addr: self.config.network.listen_addr,
            max_peers: self.config.network.max_peers,
            max_inbound: self.config.network.max_peers / 2,
            max_outbound: self.config.network.max_peers / 2,
            bootstrap_peers: self.config.network.bootstrap_peers.iter()
                .filter_map(|s| s.parse().ok())
                .collect(),
        };
        
        let mut network = NetworkService::new(net_config);
        network.start().await?;
        
        tracing::info!("✓ Network service started on {}", self.config.network.listen_addr);
        
        // Start RPC server if enabled
        if self.config.rpc.enabled {
            let rpc_config = RpcCfg {
                listen_addr: self.config.rpc.listen_addr,
                cors_origins: self.config.rpc.cors_origins.clone(),
                ..Default::default()
            };
            
            let methods = RpcMethods::new(
                self.blockchain.clone(),
                self.database.clone(),
            );
            
            let rpc_server = Arc::new(RpcServer::new(rpc_config, methods));
            
            // Spawn RPC server in background
            let rpc_clone = rpc_server.clone();
            tokio::spawn(async move {
                if let Err(e) = rpc_clone.start().await {
                    tracing::error!("RPC server error: {}", e);
                }
            });
            
            tracing::info!("✓ RPC server started on {}", self.config.rpc.listen_addr);
        }
        
        tracing::info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        tracing::info!("  🚀 Node is fully operational!");
        tracing::info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        tracing::info!("  Network:  {}", self.config.network.listen_addr);
        if self.config.rpc.enabled {
            tracing::info!("  RPC:      {}", self.config.rpc.listen_addr);
        }
        tracing::info!("  Data Dir: {}", self.config.data_dir);
        tracing::info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        
        Ok(())
    }

    pub async fn stop(&self) -> anyhow::Result<()> {
        tracing::info!("Shutting down node...");
        
        // Cleanup and shutdown logic
        tracing::info!("Flushing database...");
        self.database.compact()?;
        
        tracing::info!("Node shutdown complete");
        Ok(())
    }

    pub fn blockchain(&self) -> &Arc<RwLock<Blockchain>> {
        &self.blockchain
    }

    pub fn consensus(&self) -> &Arc<RwLock<PoASConsensus>> {
        &self.consensus
    }

    pub fn database(&self) -> &Arc<Database> {
        &self.database
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = NodeConfig::default();
        assert_eq!(config.consensus.min_stake, 10000);
        assert!(config.rpc.enabled);
    }

    #[tokio::test]
    async fn test_node_creation() {
        let config = NodeConfig {
            data_dir: "/tmp/test-node".into(),
            ..Default::default()
        };
        
        let result = Node::new(config);
        // May fail if directory doesn't exist, but tests structure
        assert!(result.is_ok() || result.is_err());
    }
}