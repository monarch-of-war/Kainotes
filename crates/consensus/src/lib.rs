// consensus/src/lib.rs

//! Proof-of-Active-Stake (PoAS) Consensus Mechanism
//!
//! This crate implements the PoAS consensus where:
//! - Validators stake tokens to participate
//! - Staked tokens are actively deployed as liquidity
//! - Selection weight based on: Stake × Utility_Score × Uptime
//! - Validators earn both protocol rewards and DeFi yields

pub mod poas;
pub mod validator;
pub mod selection;
pub mod slashing;

pub use poas::{PoASConsensus, ConsensusConfig};
pub use validator::{Validator, ValidatorSet, ValidatorInfo, ValidatorStatus};
pub use selection::{ValidatorSelector, SelectionWeight};
pub use slashing::{SlashingManager, SlashingCondition, SlashingPenalty};

use blockchain_core::BlockchainError;

/// Result type for consensus operations
pub type ConsensusResult<T> = Result<T, ConsensusError>;

/// Errors that can occur during consensus operations
#[derive(Debug, thiserror::Error)]
pub enum ConsensusError {
    #[error("Validator error: {0}")]
    ValidatorError(String),
    
    #[error("Insufficient stake: required {required}, provided {provided}")]
    InsufficientStake { required: u64, provided: u64 },
    
    #[error("Validator not found: {0}")]
    ValidatorNotFound(String),
    
    #[error("Validator already exists: {0}")]
    ValidatorAlreadyExists(String),
    
    #[error("Invalid validator status: {0}")]
    InvalidValidatorStatus(String),
    
    #[error("Block production error: {0}")]
    BlockProductionError(String),
    
    #[error("Validation error: {0}")]
    ValidationError(String),
    
    #[error("Slashing error: {0}")]
    SlashingError(String),
    
    #[error("Selection error: {0}")]
    SelectionError(String),
    
    #[error("Blockchain error: {0}")]
    BlockchainError(#[from] BlockchainError),
    
    #[error("Crypto error: {0}")]
    CryptoError(#[from] blockchain_crypto::CryptoError),
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic_imports() {
        // Smoke test
    }
}