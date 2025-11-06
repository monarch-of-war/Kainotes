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
}
