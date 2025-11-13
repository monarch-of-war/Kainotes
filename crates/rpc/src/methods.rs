// rpc/src/methods.rs
use crate::{RpcError, RpcResult, BlockId};
use blockchain_core::{Blockchain, Block, Transaction, Amount};
use blockchain_crypto::{Address, Hash};
use storage::Database;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct RpcMethods {
    blockchain: Arc<RwLock<Blockchain>>,
    database: Arc<Database>,
}

impl RpcMethods {
    pub fn new(blockchain: Arc<RwLock<Blockchain>>, database: Arc<Database>) -> Self {
        Self { blockchain, database }
    }

    pub async fn handle(&self, method: &str, params: serde_json::Value) -> RpcResult<serde_json::Value> {
        match method {
            // Ethereum-compatible methods
            "blockNumber" => self.kai_block_number().await,
            "kai_getBalance" => self.kai_get_balance(params).await,
            "kai_getBlockByNumber" => self.kai_get_block_by_number(params).await,
            "kai_getBlockByHash" => self.kai_get_block_by_hash(params).await,
            "kai_getTransactionByHash" => self.kai_get_transaction_by_hash(params).await,
            "kai_getTransactionReceipt" => self.kai_get_transaction_receipt(params).await,
            "kai_sendRawTransaction" => self.kai_send_raw_transaction(params).await,
            "kai_call" => self.kai_call(params).await,
            "kai_estimateGas" => self.kai_estimate_gas(params).await,
            "kai_gasPrice" => self.kai_gas_price().await,
            "kai_chainId" => self.kai_chain_id().await,
            "net_version" => self.net_version().await,
            "net_peerCount" => self.net_peer_count().await,
            "net_listening" => self.net_listening().await,
            "web3_clientVersion" => self.web3_client_version().await,
            
            // Mempool endpoints
            "kai_pendingTransactions" => self.kai_pending_transactions(params).await,
            "kai_txpoolStatus" => self.kai_txpool_status().await,
            "kai_txpoolContent" => self.kai_txpool_content(params).await,
            "kai_txpoolInspect" => self.kai_txpool_inspect().await,

            // Metrics endpoints
            "kai_metrics" => self.kai_metrics().await,
            "kai_metricsHistory" => self.kai_metrics_history(params).await,
            "kai_tps" => self.kai_tps(params).await,

            // Fork endpoints
            "kai_forkHistory" => self.kai_fork_history(params).await,
            "kai_forkChoice" => self.kai_fork_choice().await,

            // Block range
            "kai_getBlockRange" => self.kai_get_block_range(params).await,
            "kai_getTransactionRange" => self.kai_get_transaction_range(params).await,

            // Custom methods
            "utility_getIndex" => self.utility_get_index().await,
            "utility_getPhase" => self.utility_get_phase().await,
            "validator_getInfo" => self.validator_get_info(params).await,
            "validator_list" => self.validator_list().await,
            "liquidity_getPools" => self.liquidity_get_pools().await,
            "liquidity_getDeployment" => self.liquidity_get_deployment(params).await,
            
            _ => Err(RpcError::MethodNotFound(method.to_string())),
        }
    }

    // ==================== ETHEREUM-COMPATIBLE METHODS ====================

    async fn kai_block_number(&self) -> RpcResult<serde_json::Value> {
        let chain = self.blockchain.read().await;
        let number = chain.height();
        Ok(serde_json::json!(format!("0x{:x}", number)))
    }

    async fn kai_get_balance(&self, params: serde_json::Value) -> RpcResult<serde_json::Value> {
        let params: Vec<serde_json::Value> = serde_json::from_value(params)
            .map_err(|_| RpcError::InvalidParams("Expected array".into()))?;
        
        if params.len() < 2 {
            return Err(RpcError::InvalidParams("Expected address and block".into()));
        }

        let address_str = params[0].as_str()
            .ok_or_else(|| RpcError::InvalidParams("Invalid address".into()))?;
        let address = Address::from_hex(address_str)
            .map_err(|_| RpcError::InvalidParams("Invalid address format".into()))?;

        let chain = self.blockchain.read().await;
        let balance = chain.state().get_balance(&address);
        
        // Convert to hex string (wei)
        let balance_hex = format!("0x{:x}", balance.inner().to_u64_digits().first().copied().unwrap_or(0));
        Ok(serde_json::json!(balance_hex))
    }

    async fn kai_get_block_by_number(&self, params: serde_json::Value) -> RpcResult<serde_json::Value> {
        let params: Vec<serde_json::Value> = serde_json::from_value(params)
            .map_err(|_| RpcError::InvalidParams("Expected array".into()))?;
        
        if params.is_empty() {
            return Err(RpcError::InvalidParams("Expected block number".into()));
        }

        let block_str = params[0].as_str()
            .ok_or_else(|| RpcError::InvalidParams("Invalid block number".into()))?;

        let number = if block_str == "latest" {
            self.blockchain.read().await.height()
        } else if block_str.starts_with("0x") {
            u64::from_str_radix(&block_str[2..], 16)
                .map_err(|_| RpcError::InvalidParams("Invalid hex number".into()))?
        } else {
            block_str.parse()
                .map_err(|_| RpcError::InvalidParams("Invalid number".into()))?
        };

        match self.database.get_block_by_number(number).map_err(|e| RpcError::InternalError(e.to_string()))? {
            Some(block) => Ok(serde_json::to_value(block).unwrap()),
            None => Ok(serde_json::Value::Null),
        }
    }

    async fn kai_get_block_by_hash(&self, params: serde_json::Value) -> RpcResult<serde_json::Value> {
        let params: Vec<serde_json::Value> = serde_json::from_value(params)
            .map_err(|_| RpcError::InvalidParams("Expected array".into()))?;
        
        if params.is_empty() {
            return Err(RpcError::InvalidParams("Expected block hash".into()));
        }

        let hash_str = params[0].as_str()
            .ok_or_else(|| RpcError::InvalidParams("Invalid hash".into()))?;
        let hash = Hash::from_hex(hash_str)
            .map_err(|_| RpcError::InvalidParams("Invalid hash format".into()))?;

        match self.database.get_block(&hash).map_err(|e| RpcError::InternalError(e.to_string()))? {
            Some(block) => Ok(serde_json::to_value(block).unwrap()),
            None => Ok(serde_json::Value::Null),
        }
    }

    async fn kai_get_transaction_by_hash(&self, params: serde_json::Value) -> RpcResult<serde_json::Value> {
        let params: Vec<String> = serde_json::from_value(params)
            .map_err(|_| RpcError::InvalidParams("Expected array of strings".into()))?;
        
        if params.is_empty() {
            return Err(RpcError::InvalidParams("Expected transaction hash".into()));
        }

        let hash = Hash::from_hex(&params[0])
            .map_err(|_| RpcError::InvalidParams("Invalid hash".into()))?;

        match self.database.get_transaction(&hash).map_err(|e| RpcError::InternalError(e.to_string()))? {
            Some(tx) => Ok(serde_json::to_value(tx).unwrap()),
            None => Ok(serde_json::Value::Null),
        }
    }

    async fn kai_get_transaction_receipt(&self, params: serde_json::Value) -> RpcResult<serde_json::Value> {
        let params: Vec<String> = serde_json::from_value(params)
            .map_err(|_| RpcError::InvalidParams("Expected array".into()))?;
        
        if params.is_empty() {
            return Err(RpcError::InvalidParams("Expected transaction hash".into()));
        }

        let hash = Hash::from_hex(&params[0])
            .map_err(|_| RpcError::InvalidParams("Invalid hash".into()))?;

        match self.database.get_receipt(&hash).map_err(|e| RpcError::InternalError(e.to_string()))? {
            Some(receipt) => Ok(serde_json::to_value(receipt).unwrap()),
            None => Ok(serde_json::Value::Null),
        }
    }

    async fn kai_send_raw_transaction(&self, _params: serde_json::Value) -> RpcResult<serde_json::Value> {
        // Would implement actual transaction submission
        Ok(serde_json::json!("0x0000000000000000000000000000000000000000000000000000000000000000"))
    }

    async fn kai_call(&self, _params: serde_json::Value) -> RpcResult<serde_json::Value> {
        // Would implement contract call
        Ok(serde_json::json!("0x"))
    }

    async fn kai_estimate_gas(&self, _params: serde_json::Value) -> RpcResult<serde_json::Value> {
        Ok(serde_json::json!("0x5208")) // 21000 in hex
    }

    async fn kai_gas_price(&self) -> RpcResult<serde_json::Value> {
        Ok(serde_json::json!("0x9184e72a000")) // 10 gwei in hex
    }

    async fn kai_chain_id(&self) -> RpcResult<serde_json::Value> {
        Ok(serde_json::json!("0x539")) // 1337 in hex (local testnet)
    }

    async fn net_version(&self) -> RpcResult<serde_json::Value> {
        Ok(serde_json::json!("1337"))
    }

    async fn net_peer_count(&self) -> RpcResult<serde_json::Value> {
        Ok(serde_json::json!("0x0"))
    }

    async fn net_listening(&self) -> RpcResult<serde_json::Value> {
        Ok(serde_json::json!(true))
    }

    async fn web3_client_version(&self) -> RpcResult<serde_json::Value> {
        Ok(serde_json::json!("utility-blockchain/1.0.0/rust"))
    }

    // ==================== CUSTOM METHODS ====================

    async fn utility_get_index(&self) -> RpcResult<serde_json::Value> {
        // Would get actual utility index
        Ok(serde_json::json!({"value": 1.0, "phase": "Bootstrap"}))
    }

    async fn utility_get_phase(&self) -> RpcResult<serde_json::Value> {
        Ok(serde_json::json!({"current": "Bootstrap", "transition_block": null}))
    }

    async fn validator_get_info(&self, _params: serde_json::Value) -> RpcResult<serde_json::Value> {
        Ok(serde_json::json!({
            "stake": "15000000000000000000000",
            "commission": 500,
            "status": "Active"
        }))
    }

    async fn validator_list(&self) -> RpcResult<serde_json::Value> {
        Ok(serde_json::json!([]))
    }

    async fn liquidity_get_pools(&self) -> RpcResult<serde_json::Value> {
        Ok(serde_json::json!([]))
    }

    async fn liquidity_get_deployment(&self, _params: serde_json::Value) -> RpcResult<serde_json::Value> {
        Ok(serde_json::json!({"total": "0", "pools": []}))
    }

    // ==================== MEMPOOL METHODS ====================

    async fn kai_pending_transactions(&self, params: serde_json::Value) -> RpcResult<serde_json::Value> {
        // params: [limit]
        let limit = if let Ok(v) = serde_json::from_value::<Vec<serde_json::Value>>(params) {
            if !v.is_empty() {
                v[0].as_u64().unwrap_or(100) as usize
            } else {
                100usize
            }
        } else {
            100usize
        };

        let txs = self.database.load_pending_transactions()
            .map_err(|e| RpcError::InternalError(e.to_string()))?;
        let limited: Vec<Transaction> = txs.into_iter().take(limit).collect();
        Ok(serde_json::to_value(limited).unwrap())
    }

    async fn kai_txpool_status(&self) -> RpcResult<serde_json::Value> {
        let txs = self.database.load_pending_transactions()
            .map_err(|e| RpcError::InternalError(e.to_string()))?;
        let pending_count = txs.len();
        let queued_count = 0usize; // live queued not tracked here
        let total_added = pending_count as u64;
        let total_removed = 0u64;

        Ok(serde_json::json!({
            "pending_count": pending_count,
            "queued_count": queued_count,
            "total_added": total_added,
            "total_removed": total_removed,
        }))
    }

    async fn kai_txpool_content(&self, params: serde_json::Value) -> RpcResult<serde_json::Value> {
        // params: [address?]
        let v: Vec<serde_json::Value> = serde_json::from_value(params)
            .map_err(|_| RpcError::InvalidParams("Expected array".into()))?;

        if !v.is_empty() && !v[0].is_null() {
            let addr_str = v[0].as_str().ok_or_else(|| RpcError::InvalidParams("Invalid address".into()))?;
            let addr = Address::from_hex(addr_str).map_err(|_| RpcError::InvalidParams("Invalid address".into()))?;
            let txs = self.database.get_transactions_by_address(&addr).map_err(|e| RpcError::InternalError(e.to_string()))?;
            return Ok(serde_json::json!({"pending": txs, "queued": []}));
        }

        let txs = self.database.load_pending_transactions().map_err(|e| RpcError::InternalError(e.to_string()))?;
        Ok(serde_json::json!({"pending": txs, "queued": []}))
    }

    async fn kai_txpool_inspect(&self) -> RpcResult<serde_json::Value> {
        let txs = self.database.load_pending_transactions().map_err(|e| RpcError::InternalError(e.to_string()))?;
        use std::collections::BTreeMap;
        let mut map: BTreeMap<String, usize> = BTreeMap::new();
        for tx in txs {
            let addr = tx.from.to_hex();
            *map.entry(addr).or_insert(0) += 1;
        }
        Ok(serde_json::to_value(map).unwrap())
    }

    // ==================== METRICS METHODS ====================

    async fn kai_metrics(&self) -> RpcResult<serde_json::Value> {
        match self.database.get_latest_metrics().map_err(|e| RpcError::InternalError(e.to_string()))? {
            Some(snapshot) => Ok(serde_json::to_value(snapshot.metrics).unwrap()),
            None => Ok(serde_json::Value::Null),
        }
    }

    async fn kai_metrics_history(&self, params: serde_json::Value) -> RpcResult<serde_json::Value> {
        // params: [start_block, end_block, granularity?]
        let v: Vec<serde_json::Value> = serde_json::from_value(params)
            .map_err(|_| RpcError::InvalidParams("Expected array".into()))?;
        if v.len() < 2 {
            return Err(RpcError::InvalidParams("Expected start and end block".into()));
        }
        let start = v[0].as_u64().ok_or_else(|| RpcError::InvalidParams("Invalid start".into()))?;
        let end = v[1].as_u64().ok_or_else(|| RpcError::InvalidParams("Invalid end".into()))?;

        let max_points = 1000usize;
        let count = (end.saturating_sub(start) + 1) as usize;
        if count > max_points {
            return Err(RpcError::InvalidParams(format!("Range too large: {} points (max {})", count, max_points)));
        }

        let snapshots = self.database.get_metrics_range(start, end).map_err(|e| RpcError::InternalError(e.to_string()))?;
        Ok(serde_json::to_value(snapshots).unwrap())
    }

    async fn kai_tps(&self, params: serde_json::Value) -> RpcResult<serde_json::Value> {
        // params: [window_blocks?]
        let window = if let Ok(v) = serde_json::from_value::<Vec<serde_json::Value>>(params) {
            if !v.is_empty() { v[0].as_u64().unwrap_or(10) as u64 } else { 10 }
        } else { 10 };

        let chain = self.blockchain.read().await;
        let height = chain.height();
        let start = height.saturating_sub(window.saturating_sub(1));
        let tx_count = chain.get_transaction_count(start, height);

        // Attempt to use avg_block_time from metrics to calculate seconds, else assume 1s
        let seconds = if let Ok(Some(snapshot)) = self.database.get_latest_metrics() { snapshot.metrics.avg_block_time * (window as f64) } else { window as f64 };
        let tps = if seconds > 0.0 { tx_count as f64 / seconds } else { tx_count as f64 };
        Ok(serde_json::json!(tps))
    }

    // ==================== FORK METHODS ====================

    async fn kai_fork_history(&self, params: serde_json::Value) -> RpcResult<serde_json::Value> {
        // params: [limit?, start_time_unix?]
        let v: Vec<serde_json::Value> = serde_json::from_value(params).unwrap_or_default();
        let limit = v.get(0).and_then(|x| x.as_u64()).unwrap_or(100) as usize;
        let start_time = v.get(1).and_then(|x| x.as_u64());

        let hours_back = if let Some(start) = start_time {
            let now = unix_timestamp();
            if now > start { Some((now - start) / 3600) } else { Some(0) }
        } else { None };

        let mut events = self.database.get_fork_history(hours_back).map_err(|e| RpcError::InternalError(e.to_string()))?;
        if events.len() > limit { events.truncate(limit); }
        Ok(serde_json::to_value(events).unwrap())
    }

    async fn kai_fork_choice(&self) -> RpcResult<serde_json::Value> {
        // We don't store live fork choice here; return a default
        Ok(serde_json::json!("LongestChain"))
    }

    // ==================== BLOCK RANGE METHODS ====================

    async fn kai_get_block_range(&self, params: serde_json::Value) -> RpcResult<serde_json::Value> {
        // params: [start, end, include_transactions?]
        let v: Vec<serde_json::Value> = serde_json::from_value(params)
            .map_err(|_| RpcError::InvalidParams("Expected array".into()))?;
        if v.len() < 2 { return Err(RpcError::InvalidParams("Expected start and end".into())); }
        let start = v[0].as_u64().ok_or_else(|| RpcError::InvalidParams("Invalid start".into()))?;
        let end = v[1].as_u64().ok_or_else(|| RpcError::InvalidParams("Invalid end".into()))?;
        let include_tx = v.get(2).and_then(|x| x.as_bool()).unwrap_or(true);

        if end < start { return Err(RpcError::InvalidParams("end < start".into())); }
        let count = end.saturating_sub(start) + 1;
        if count > 100 { return Err(RpcError::InvalidParams("Range too large (max 100)".into())); }

        let chain = self.blockchain.read().await;
        let blocks = chain.get_block_range(start, end);
        if include_tx {
            Ok(serde_json::to_value(blocks).unwrap())
        } else {
            // strip transactions
            let stripped: Vec<serde_json::Value> = blocks.into_iter().map(|mut b| {
                let mut v = serde_json::to_value(&b).unwrap();
                if let serde_json::Value::Object(ref mut map) = v { map.insert("transactions".to_string(), serde_json::Value::Array(vec![])); }
                v
            }).collect();
            Ok(serde_json::Value::Array(stripped))
        }
    }

    async fn kai_get_transaction_range(&self, params: serde_json::Value) -> RpcResult<serde_json::Value> {
        // params: [start, end]
        let v: Vec<serde_json::Value> = serde_json::from_value(params)
            .map_err(|_| RpcError::InvalidParams("Expected array".into()))?;
        if v.len() < 2 { return Err(RpcError::InvalidParams("Expected start and end".into())); }
        let start = v[0].as_u64().ok_or_else(|| RpcError::InvalidParams("Invalid start".into()))?;
        let end = v[1].as_u64().ok_or_else(|| RpcError::InvalidParams("Invalid end".into()))?;
        if end < start { return Err(RpcError::InvalidParams("end < start".into())); }

        let txs = self.blockchain.read().await.get_transactions_in_range(start, end);
        if txs.len() > 1000 { return Err(RpcError::InvalidParams("Too many transactions (max 1000)".into())); }
        Ok(serde_json::to_value(txs).unwrap())
    }

    // ==================== ENHANCEMENTS ====================

    async fn kai_send_raw_transaction(&self, params: serde_json::Value) -> RpcResult<serde_json::Value> {
        // Accept either hex string of bincode or a JSON transaction object
        let v: Vec<serde_json::Value> = serde_json::from_value(params).map_err(|_| RpcError::InvalidParams("Expected array".into()))?;
        if v.is_empty() { return Err(RpcError::InvalidParams("Expected raw transaction".into())); }

        let tx: Transaction = if let Some(s) = v[0].as_str() {
            // assume hex of bincode
            let bytes = hex::decode(s.trim_start_matches("0x")).map_err(|_| RpcError::InvalidParams("Invalid hex".into()))?;
            bincode::deserialize(&bytes).map_err(|_| RpcError::InvalidParams("Invalid transaction encoding".into()))?
        } else {
            serde_json::from_value(v[0].clone()).map_err(|_| RpcError::InvalidParams("Invalid transaction object".into()))?
        };

        // Validate basic properties (signature etc.)
        if let Err(e) = tx.validate_basic() {
            return Err(RpcError::InvalidParams(format!("Invalid transaction: {}", e)));
        }

        // Persist to pending transactions storage
        self.database.store_pending_transactions(vec![(tx.clone(), tx.gas_price)])
            .map_err(|e| RpcError::InternalError(e.to_string()))?;

        // Determine position by gas price
        let pending = self.database.load_pending_transactions().map_err(|e| RpcError::InternalError(e.to_string()))?;
        let pos = pending.iter().position(|t| t.hash() == tx.hash()).map(|i| i + 1).unwrap_or(0);

        Ok(serde_json::json!({"tx_hash": tx.hash().to_hex(), "position": pos}))
    }

    async fn kai_get_block_by_number(&self, params: serde_json::Value) -> RpcResult<serde_json::Value> {
        // Support second param include_txpool
        let params: Vec<serde_json::Value> = serde_json::from_value(params)
            .map_err(|_| RpcError::InvalidParams("Expected array".into()))?;
        
        if params.is_empty() {
            return Err(RpcError::InvalidParams("Expected block number".into()));
        }

        let block_str = params[0].as_str()
            .ok_or_else(|| RpcError::InvalidParams("Invalid block number".into()))?;

        let number = if block_str == "latest" {
            self.blockchain.read().await.height()
        } else if block_str.starts_with("0x") {
            u64::from_str_radix(&block_str[2..], 16)
                .map_err(|_| RpcError::InvalidParams("Invalid hex number".into()))?
        } else {
            block_str.parse()
                .map_err(|_| RpcError::InvalidParams("Invalid number".into()))?
        };

        let include_txpool = params.get(1).and_then(|v| v.as_bool()).unwrap_or(false);

        match self.database.get_block_by_number(number).map_err(|e| RpcError::InternalError(e.to_string()))? {
            Some(block) => {
                if include_txpool {
                    let txpool = self.kai_txpool_status().await?;
                    let mut obj = serde_json::Map::new();
                    obj.insert("block".to_string(), serde_json::to_value(block).unwrap());
                    obj.insert("txpool".to_string(), txpool);
                    Ok(serde_json::Value::Object(obj))
                } else {
                    Ok(serde_json::to_value(block).unwrap())
                }
            }
            None => Ok(serde_json::Value::Null),
        }
    }

    // ==================== UTIL HELPERS ====================

    fn unix_timestamp() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    }
}
