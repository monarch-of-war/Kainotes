// node/src/main.rs
use clap::{Parser, Subcommand};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(name = "utility-node")]
#[command(about = "Utility-Backed Blockchain Node", version, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    
    /// Enable debug logging
    #[arg(short, long, global = true)]
    debug: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the blockchain node
    Start {
        /// Configuration file path
        #[arg(short, long, default_value = "./config.toml")]
        config: String,
        
        /// Override data directory
        #[arg(short, long)]
        data_dir: Option<String>,
    },
    
    /// Initialize a new node
    Init {
        /// Data directory
        #[arg(short, long, default_value = "./data")]
        data_dir: String,
        
        /// Create genesis block
        #[arg(short, long)]
        genesis: bool,
    },
    
    /// Validator operations
    Validator {
        #[command(subcommand)]
        command: ValidatorCommands,
    },
    
    /// Database operations
    Db {
        #[command(subcommand)]
        command: DbCommands,
    },
    
    /// Show node status
    Status,
}

#[derive(Subcommand)]
enum ValidatorCommands {
    /// Register as validator
    Register {
        /// Stake amount
        #[arg(short, long)]
        stake: u64,
        
        /// Commission rate (basis points)
        #[arg(short, long, default_value = "500")]
        commission: u16,
    },
    
    /// Show validator info
    Info {
        /// Validator address
        #[arg(short, long)]
        address: Option<String>,
    },
    
    /// Unregister validator
    Unregister,
    
    /// Generate validator keys
    Keygen {
        /// Output path
        #[arg(short, long)]
        output: String,
    },
}

#[derive(Subcommand)]
enum DbCommands {
    /// Show database statistics
    Stats,
    
    /// Compact database
    Compact,
    
    /// Verify database integrity
    Verify,
    
    /// Prune old data
    Prune {
        /// Keep blocks from this number
        #[arg(short, long)]
        keep_from: u64,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    
    // Initialize logging
    let log_level = if cli.debug { "debug" } else { "info" };
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("{}={},hyper=warn,h2=warn", env!("CARGO_PKG_NAME"), log_level).into())
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
    
    match cli.command {
        Commands::Start { config, data_dir } => {
            start_node(&config, data_dir).await?;
        }
        Commands::Init { data_dir, genesis } => {
            init_node(&data_dir, genesis)?;
        }
        Commands::Validator { command } => {
            handle_validator_command(command).await?;
        }
        Commands::Db { command } => {
            handle_db_command(command).await?;
        }
        Commands::Status => {
            show_status().await?;
        }
    }
    
    Ok(())
}

async fn start_node(config_path: &str, data_dir_override: Option<String>) -> anyhow::Result<()> {
    use node::{Node, NodeConfig};
    use std::sync::Arc;
    
    tracing::info!("Loading configuration from {}", config_path);
    let mut config = NodeConfig::from_file(config_path)?;
    
    if let Some(data_dir) = data_dir_override {
        config.data_dir = data_dir;
    }
    
    tracing::info!("Starting node with data directory: {}", config.data_dir);
    
    let node = Arc::new(Node::new(config)?);
    node.clone().start().await?;
    
    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    tracing::info!("Received shutdown signal");
    
    node.stop().await?;
    tracing::info!("Node stopped gracefully");
    
    Ok(())
}

fn init_node(data_dir: &str, create_genesis: bool) -> anyhow::Result<()> {
    tracing::info!("Initializing node at {}", data_dir);
    
    // Create directories
    std::fs::create_dir_all(data_dir)?;
    std::fs::create_dir_all(format!("{}/db", data_dir))?;
    std::fs::create_dir_all(format!("{}/keys", data_dir))?;
    
    if create_genesis {
        tracing::info!("Creating genesis block");
        use blockchain_core::Block;
        use blockchain_crypto::Hash;
        
        let genesis = Block::genesis(Hash::zero());
        let genesis_json = serde_json::to_string_pretty(&genesis)?;
        std::fs::write(format!("{}/genesis.json", data_dir), genesis_json)?;
        
        tracing::info!("Genesis block created");
    }
    
    // Create default config
    let config = node::NodeConfig::default();
    let config_toml = toml::to_string_pretty(&config)?;
    std::fs::write(format!("{}/config.toml", data_dir), config_toml)?;
    
    tracing::info!("Node initialized successfully at {}", data_dir);
    tracing::info!("Edit {}/config.toml to configure your node", data_dir);
    
    Ok(())
}

async fn handle_validator_command(command: ValidatorCommands) -> anyhow::Result<()> {
    match command {
        ValidatorCommands::Register { stake, commission } => {
            tracing::info!("Registering validator with stake: {} and commission: {}%", stake, commission as f64 / 100.0);
            tracing::info!("Not yet implemented - requires running node");
        }
        ValidatorCommands::Info { address } => {
            tracing::info!("Getting validator info for: {:?}", address);
            tracing::info!("Not yet implemented - requires running node");
        }
        ValidatorCommands::Unregister => {
            tracing::info!("Unregistering validator");
            tracing::info!("Not yet implemented - requires running node");
        }
        ValidatorCommands::Keygen { output } => {
            use blockchain_crypto::{KeyPair, SignatureScheme};
            
            tracing::info!("Generating validator keypair");
            let keypair = KeyPair::generate(SignatureScheme::Ed25519)?;
            
            let key_json = serde_json::json!({
                "public_key": keypair.public_key().to_hex(),
                "secret_key": keypair.secret_key().to_hex(),
                "address": keypair.public_key().to_address().to_hex(),
            });
            
            std::fs::write(&output, serde_json::to_string_pretty(&key_json)?)?;
            tracing::info!("Keypair saved to {}", output);
            tracing::warn!("Keep this file secure!");
        }
    }
    
    Ok(())
}

async fn handle_db_command(command: DbCommands) -> anyhow::Result<()> {
    match command {
        DbCommands::Stats => {
            tracing::info!("Database statistics:");
            tracing::info!("Not yet implemented - requires running node");
        }
        DbCommands::Compact => {
            tracing::info!("Compacting database...");
            tracing::info!("Not yet implemented - requires running node");
        }
        DbCommands::Verify => {
            tracing::info!("Verifying database integrity...");
            tracing::info!("Not yet implemented - requires running node");
        }
        DbCommands::Prune { keep_from } => {
            tracing::info!("Pruning data before block {}", keep_from);
            tracing::info!("Not yet implemented - requires running node");
        }
    }
    
    Ok(())
}

async fn show_status() -> anyhow::Result<()> {
    tracing::info!("Node Status:");
    tracing::info!("Not yet implemented - requires running node");
    Ok(())
}