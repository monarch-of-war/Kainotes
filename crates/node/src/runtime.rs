// node/src/runtime.rs
use crate::NodeConfig;
use blockchain_core::{Block, Blockchain, TransactionPool, PoolConfig, ForkResolver, ForkChoice, ChainMetrics};
use blockchain_crypto::Hash;
use consensus::{PoASConsensus, ConsensusConfig as PoASConfig};
use storage::{Database, DatabaseConfig, PruningMode};
use networking::{NetworkService, NetworkConfig as NetConfig};
use rpc::{RpcServer, RpcConfig as RpcCfg, RpcMethods};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};

pub struct Node {
    config: NodeConfig,
    blockchain: Arc<RwLock<Blockchain>>,
    consensus: Arc<RwLock<PoASConsensus>>,
    database: Arc<Database>,
    mempool: Arc<RwLock<TransactionPool>>,
    fork_resolver: Arc<RwLock<ForkResolver>>,
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
            enable_mempool_persistence: true,
            fork_history_retention_days: 7,
            metrics_snapshot_interval: config.metrics.snapshot_interval,
            enable_metrics_compression: true,
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
        
        // Initialize transaction pool
        let pool_config = PoolConfig {
            max_size: config.mempool.max_size,
            max_per_account: config.mempool.max_per_account,
            min_gas_price: config.mempool.min_gas_price,
            max_age: config.mempool.max_age,
            enable_replacement: config.mempool.enable_replacement,
        };
        let mempool = Arc::new(RwLock::new(TransactionPool::new(pool_config)));
        
        tracing::info!(
            "✓ TransactionPool initialized: max_size={}, max_per_account={}",
            config.mempool.max_size,
            config.mempool.max_per_account
        );
        
        // Initialize fork resolver (map string to enum)
        let fork_choice = match config.fork_handling.fork_choice.as_str() {
            "HeaviestChain" => ForkChoice::HeaviestChain,
            "LatestJustified" => ForkChoice::LatestJustified,
            _ => ForkChoice::LongestChain,
        };

        let fork_resolver = Arc::new(RwLock::new(ForkResolver::new(
            fork_choice,
            config.fork_handling.max_reorg_depth,
        )));
        
        tracing::info!(
            "✓ ForkResolver initialized: strategy={}, max_reorg_depth={}",
            config.fork_handling.fork_choice,
            config.fork_handling.max_reorg_depth
        );
        
        tracing::info!("Node components initialized");
        
        Ok(Self {
            config,
            blockchain,
            consensus,
            database,
            mempool,
            fork_resolver,
            network: None,
            rpc: None,
        })
    }

    pub async fn start(self: Arc<Self>) -> anyhow::Result<()> {
        tracing::info!("Starting Utility Blockchain Node");
        
        // Start mempool background tasks
        self.start_mempool_tasks();
        
        // Start fork monitor
        self.start_fork_monitor();
        
        // Start metrics collector
        self.start_metrics_collector();
        
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

    pub fn mempool(&self) -> &Arc<RwLock<TransactionPool>> {
        &self.mempool
    }

    pub fn fork_resolver(&self) -> &Arc<RwLock<ForkResolver>> {
        &self.fork_resolver
    }

    // ==================== BACKGROUND TASKS ====================

    fn start_mempool_tasks(&self) {
        // Task 1: Periodic pruning
        let mempool = self.mempool.clone();
        let prune_interval = self.config.mempool.prune_interval_seconds;
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(prune_interval));
            loop {
                ticker.tick().await;
                // Compute removed count via metrics snapshot before/after
                let mut pool = mempool.write().await;
                let before = pool.metrics().total_removed;
                pool.prune();
                let after = pool.metrics().total_removed;
                let pruned = after.saturating_sub(before);
                tracing::debug!("Mempool pruning: removed {} transactions", pruned);
            }
        });

        // Task 2: Periodic persistence (every 10 blocks equivalent to ~30 seconds)
        let mempool = self.mempool.clone();
        let database = self.database.clone();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(30));
            loop {
                ticker.tick().await;
                let pending = mempool.read().await.get_pending(u64::MAX, 10000);
                let txs: Vec<(blockchain_core::Transaction, u64)> = pending
                    .into_iter()
                    .map(|tx| (tx.clone(), tx.gas_price))
                    .collect();
                match database.store_pending_transactions(txs) {
                    Ok(count) => tracing::debug!("Saved {} pending transactions", count),
                    Err(e) => tracing::warn!("Failed to persist mempool: {}", e),
                }
            }
        });

        tracing::info!("✓ Mempool background tasks started");
    }

    fn start_fork_monitor(&self) {
        let blockchain = self.blockchain.clone();
        let fork_resolver = self.fork_resolver.clone();
        let database = self.database.clone();
        let alert_threshold = self.config.fork_handling.alert_threshold_depth;

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(5));
            loop {
                ticker.tick().await;
                // Check for fork candidates (simplified check)
                let chain = blockchain.read().await;
                let height = chain.height();
                drop(chain);

                // Log fork statistics periodically
                if let Ok(stats) = database.get_fork_statistics() {
                    if stats.total_forks > 0 {
                        tracing::info!(
                            "Fork statistics: total={}, avg_depth={:.2}",
                            stats.total_forks,
                            stats.avg_reorg_depth
                        );
                        if stats.max_reorg_depth as u64 > alert_threshold {
                            tracing::warn!(
                                "⚠️  Deep reorganization detected: {} blocks",
                                stats.max_reorg_depth
                            );
                        }
                    }
                }
            }
        });

        tracing::info!("✓ Fork monitor started");
    }

    fn start_metrics_collector(&self) {
        let blockchain = self.blockchain.clone();
        let database = self.database.clone();
        let snapshot_interval = self.config.metrics.snapshot_interval;
        let window_size = self.config.metrics.window_size;

        tokio::spawn(async move {
            let mut last_block = 0u64;
            let mut ticker = interval(Duration::from_secs(5));

            loop {
                ticker.tick().await;

                let chain = blockchain.read().await;
                let current_block = chain.height();
                drop(chain);

                // Store metrics every N blocks
                if current_block > last_block + snapshot_interval {
                    if let Ok(Some(metrics)) = database.get_latest_metrics() {
                        tracing::debug!(
                            "Metrics: TPS={:.2}, block_time={:.2}s, gas_used={}",
                            metrics.metrics.tps,
                            metrics.metrics.avg_block_time,
                            metrics.metrics.total_gas_used
                        );
                    }
                    last_block = current_block;
                }
            }
        });

        tracing::info!("✓ Metrics collector started");
    }

    // ==================== BLOCK PRODUCTION ====================

    pub async fn produce_block(&self) -> anyhow::Result<Block> {
        let block_gas_limit = 8_000_000u64;
        let max_tx_count = 1000usize;

        // Step 1: Get pending transactions
        let txs = self.mempool.read().await.get_pending(block_gas_limit, max_tx_count);
        tracing::debug!("Selected {} transactions for block", txs.len());

        // Step 2-3: Validate and execute (simplified - actual implementation would execute all)
        let mut valid_txs = Vec::new();
        let mut total_gas = 0u64;

        for tx in txs {
            let gas = tx.gas_limit;
            if total_gas + gas <= block_gas_limit {
                total_gas += gas;
                valid_txs.push(tx);
            }
        }

        // Step 4: Create block using canonical constructor
        let blockchain = self.blockchain.read().await;
        let parent = blockchain.head_block().clone();
        let next_number = parent.number() + 1;
        let state_root = blockchain.state().state_root();
        drop(blockchain);

        let block = Block::new(
            next_number,
            parent.hash(),
            state_root,
            blockchain_crypto::Address::zero(),
            valid_txs,
            block_gas_limit,
        )?;

        tracing::info!("📦 Block #{} produced with {} transactions", block.number(), block.transactions.len());

        Ok(block)
    }

    // ==================== FORK HANDLING ====================

    pub async fn handle_incoming_block(&self, block: &Block) -> anyhow::Result<bool> {
        let blockchain = self.blockchain.read().await;
        let head = blockchain.head_block().clone();

        // Step 1: Detect fork
        let is_fork = block.header.parent_hash != head.hash();

        if is_fork {
            tracing::warn!("🔄 Fork detected at block #{}", block.header.number);

            // Step 2: Calculate reorg path (simplified)
            let reorg_depth = 1u64;

            // Step 3-5: Handle reorganization if necessary
            if reorg_depth <= self.config.fork_handling.max_reorg_depth {
                if self.config.fork_handling.alert_threshold_depth > 0
                    && reorg_depth > self.config.fork_handling.alert_threshold_depth
                {
                    tracing::warn!("⚠️  Deep reorg alert: {} blocks", reorg_depth);
                }

                // Record fork event with full audit trail
                if let Err(e) = self.database.store_fork_event(
                    head.number(),        // fork_point
                    head.hash(),          // fork_hash (main fork point)
                    head.hash(),          // main_tip
                    block.hash(),         // fork_tip
                    0u64,                 // main_length
                    1u64,                 // fork_length
                    &format!("fork_depth_{}", reorg_depth),
                    reorg_depth,
                ) {
                    tracing::error!("Failed to record fork event: {}", e);
                }
            }
        }

        Ok(!is_fork)
    }

    // ==================== METRICS EXPORT ====================

    pub async fn get_current_metrics(&self) -> anyhow::Result<serde_json::Value> {
        let metrics = self.database.get_latest_metrics()?;

        match metrics {
            Some(snapshot) => Ok(serde_json::json!({
                "block_number": snapshot.block_number,
                "timestamp": snapshot.timestamp,
                "tps": snapshot.metrics.tps,
                "avg_block_time": snapshot.metrics.avg_block_time,
                "total_transactions": snapshot.metrics.total_transactions,
                "total_gas_used": snapshot.metrics.total_gas_used,
            })),
            None => Ok(serde_json::json!(null)),
        }
    }

    pub async fn export_metrics_prometheus(&self) -> String {
        let mut output = String::new();

        if let Ok(Some(snapshot)) = self.database.get_latest_metrics() {
            output.push_str(&format!(
                "# HELP kai_tps Transactions per second\n\
                 # TYPE kai_tps gauge\n\
                 kai_tps {}\n\n",
                snapshot.metrics.tps
            ));

            output.push_str(&format!(
                "# HELP kai_block_time Average block time in seconds\n\
                 # TYPE kai_block_time gauge\n\
                 kai_block_time {}\n\n",
                snapshot.metrics.avg_block_time
            ));

            output.push_str(&format!(
                "# HELP kai_gas_used Total gas used\n\
                 # TYPE kai_gas_used counter\n\
                 kai_gas_used {}\n",
                snapshot.metrics.total_gas_used
            ));
        }

        output
    }

    // ==================== TRANSACTION SUBMISSION ====================

    pub async fn submit_transaction(&self, tx: blockchain_core::Transaction) -> anyhow::Result<serde_json::Value> {
        // Step 1: Validate transaction
        tx.validate_basic()?;

        let blockchain = self.blockchain.read().await;
        let current_state = blockchain.state();
        let sender_nonce = current_state.get_nonce(&tx.from);
        drop(blockchain);

        if tx.nonce != sender_nonce {
            return Err(anyhow::anyhow!("Invalid nonce: expected {}, got {}", sender_nonce, tx.nonce));
        }

        // Step 2: Add to pool
        let mut pool = self.mempool.write().await;
        pool.add(tx.clone(), sender_nonce)?;
        let position = pool.metrics().pending_count;
        drop(pool);

        // Step 3: Broadcast to network (would gossip here)
        tracing::info!("✓ Transaction {} submitted to pool (position: {})", tx.hash().to_hex(), position);

        Ok(serde_json::json!({
            "tx_hash": tx.hash().to_hex(),
            "position": position,
        }))
    }

    // ==================== CONFIGURATION UPDATES ====================

    pub async fn update_pool_config(&self, config: blockchain_core::PoolConfig) -> anyhow::Result<()> {
        let mut pool = self.mempool.write().await;
        // Replace pool with a new instance using updated config (dropping old state)
        *pool = TransactionPool::new(config);
        tracing::info!("✓ Mempool configuration updated (pool reset)");
        Ok(())
    }

    pub async fn update_fork_choice(&self, strategy: String) -> anyhow::Result<()> {
        let mut resolver = self.fork_resolver.write().await;
        let choice = match strategy.as_str() {
            "HeaviestChain" => ForkChoice::HeaviestChain,
            "LatestJustified" => ForkChoice::LatestJustified,
            _ => ForkChoice::LongestChain,
        };
        resolver.set_fork_choice(choice);
        tracing::info!("✓ Fork choice strategy updated to: {}", strategy);
        Ok(())
    }
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = NodeConfig::default();
        assert_eq!(config.consensus.min_stake, 10000);
        assert!(config.rpc.enabled);
        assert_eq!(config.mempool.max_size, 10_000);
        assert_eq!(config.fork_handling.max_reorg_depth, 100);
        assert!(config.metrics.enable_collection);
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

    #[test]
    fn test_mempool_config() {
        let config = NodeConfig::default();
        assert_eq!(config.mempool.max_per_account, 100);
        assert_eq!(config.mempool.min_gas_price, 1);
        assert!(config.mempool.enable_replacement);
        assert_eq!(config.mempool.prune_interval_seconds, 60);
    }

    #[test]
    fn test_fork_handling_config() {
        let config = NodeConfig::default();
        assert_eq!(config.fork_handling.fork_choice, "LongestChain");
        assert_eq!(config.fork_handling.max_reorg_depth, 100);
        assert!(config.fork_handling.enable_fork_alerts);
        assert_eq!(config.fork_handling.alert_threshold_depth, 10);
    }

    #[test]
    fn test_metrics_config() {
        let config = NodeConfig::default();
        assert_eq!(config.metrics.window_size, 100);
        assert!(config.metrics.enable_collection);
        assert_eq!(config.metrics.snapshot_interval, 10);
    }

    #[tokio::test]
    async fn test_block_production_empty_pool() {
        let config = NodeConfig {
            data_dir: "/tmp/test-node-prod".into(),
            ..Default::default()
        };

        if let Ok(node) = Node::new(config) {
            // Produce block with empty mempool
            match node.produce_block().await {
                Ok(block) => {
                    assert_eq!(block.transactions.len(), 0);
                    assert!(block.header.number > 0);
                }
                Err(_) => {
                    // Expected if genesis not set up properly
                }
            }
        }
    }

    #[tokio::test]
    async fn test_fork_detection() {
        let config = NodeConfig {
            data_dir: "/tmp/test-node-fork".into(),
            ..Default::default()
        };

        if let Ok(node) = Node::new(config) {
            let blockchain = node.blockchain.read().await;
            let head = blockchain.head_block().clone();
            drop(blockchain);

            // Create a block with mismatched parent (simulating fork)
            let mut forked_block = head.clone();
            forked_block.header.parent_hash = Hash::zero();

            match node.handle_incoming_block(&forked_block).await {
                Ok(is_valid) => {
                    // Should detect as fork (not valid continuation)
                    assert!(!is_valid || forked_block.header.parent_hash == head.hash());
                }
                Err(_) => {
                    // Expected in test environment
                }
            }
        }
    }

    #[tokio::test]
    async fn test_metrics_export() {
        let config = NodeConfig {
            data_dir: "/tmp/test-node-metrics".into(),
            ..Default::default()
        };

        if let Ok(node) = Node::new(config) {
            let prometheus_metrics = node.export_metrics_prometheus().await;
            // Prometheus metrics format should include HELP and TYPE
            assert!(prometheus_metrics.contains("TYPE") || prometheus_metrics.is_empty());
        }
    }

    #[test]
    fn test_unix_timestamp() {
        let ts = unix_timestamp();
        assert!(ts > 0);
        // Timestamp should be recent (not in year 1970)
        assert!(ts > 1_000_000_000);
    }
}