// blockchain-core/src/lib.rs

//! Core blockchain data structures and logic
//!
//! This crate provides:
//! - Block structure
//! - Transaction types
//! - Blockchain state management
//! - Chain validation logic

pub mod block;
pub mod chain;
pub mod transaction;
pub mod state;
pub mod types;

pub use block::{Block, BlockHeader};
pub use chain::Blockchain;
pub use transaction::{Transaction, TransactionType, TransactionReceipt};
pub use state::{Account, WorldState};
pub use types::*;

use blockchain_crypto::{Address, Hash};

/// Result type for blockchain operations
pub type BlockchainResult<T> = Result<T, BlockchainError>;

/// Errors that can occur in blockchain operations
#[derive(Debug, thiserror::Error)]
pub enum BlockchainError {
    #[error("Invalid block: {0}")]
    InvalidBlock(String),
    
    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),
    
    #[error("Invalid chain: {0}")]
    InvalidChain(String),
    
    #[error("State error: {0}")]
    StateError(String),
    
    #[error("Insufficient balance")]
    InsufficientBalance,
    
    #[error("Nonce mismatch")]
    NonceMismatch,
    
    #[error("Block not found: {0}")]
    BlockNotFound(Hash),
    
    #[error("Transaction not found: {0}")]
    TransactionNotFound(Hash),
    
    #[error("Cryptographic error: {0}")]
    CryptoError(#[from] blockchain_crypto::CryptoError),
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic_imports() {
        // Smoke test to ensure all modules compile
    }
}