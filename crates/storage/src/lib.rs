// storage/src/lib.rs

//! Persistent Storage Layer
//!
//! This crate provides persistent storage using RocksDB:
//! - Block storage and indexing
//! - Transaction storage and lookups
//! - State storage and snapshots
//! - Receipt storage
//! - Pruning and archiving

pub mod db;
pub mod cache;

pub use db::{Database, DatabaseConfig, ColumnFamily};
pub use cache::{BlockCache, StateCache, TransactionCache};

use blockchain_core::BlockNumber;

/// Result type for storage operations
pub type StorageResult<T> = Result<T, StorageError>;

/// Errors that can occur during storage operations
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Database error: {0}")]
    DatabaseError(String),
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    #[error("Not found: {0}")]
    NotFound(String),
    
    #[error("Corruption detected: {0}")]
    Corruption(String),
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    
    #[error("Cache error: {0}")]
    CacheError(String),
}

/// Pruning mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PruningMode {
    /// Keep all historical data (archive node)
    Archive,
    /// Prune old state, keep recent blocks
    Pruned { keep_blocks: BlockNumber },
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic_imports() {
        // Smoke test
    }
}