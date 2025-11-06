// node/src/config.rs
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub data_dir: String,
    pub network: NetworkConfig,
    pub rpc: RpcConfig,
    pub consensus: ConsensusConfig,
    pub storage: StorageConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validator: Option<ValidatorConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub listen_addr: SocketAddr,
    pub max_peers: usize,
    pub bootstrap_peers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcConfig {
    pub enabled: bool,
    pub listen_addr: SocketAddr,
    pub cors_origins: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusConfig {
    pub min_stake: u64,
    pub block_time_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub cache_size_mb: usize,
    pub max_open_files: i32,
    pub pruning: String, // "archive" or "pruned"
    pub keep_blocks: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorConfig {
    pub key_path: String,
    pub commission_rate: u16,
    pub auto_stake: bool,
    pub initial_stake: u64,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            data_dir: "./data".into(),
            network: NetworkConfig {
                listen_addr: "0.0.0.0:30303".parse().unwrap(),
                max_peers: 50,
                bootstrap_peers: vec![],
            },
            rpc: RpcConfig {
                enabled: true,
                listen_addr: "127.0.0.1:8545".parse().unwrap(),
                cors_origins: vec!["*".into()],
            },
            consensus: ConsensusConfig {
                min_stake: 10_000,
                block_time_seconds: 3,
            },
            storage: StorageConfig {
                cache_size_mb: 512,
                max_open_files: 1024,
                pruning: "pruned".into(),
                keep_blocks: 10000,
            },
            validator: None,
        }
    }
}

impl NodeConfig {
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let config = toml::from_str(&contents)?;
        Ok(config)
    }

    pub fn to_file(&self, path: &str) -> anyhow::Result<()> {
        let contents = toml::to_string_pretty(self)?;
        std::fs::write(path, contents)?;
        Ok(())
    }
}
