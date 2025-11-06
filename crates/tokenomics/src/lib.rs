// tokenomics/src/lib.rs

//! Dual-Phase Tokenomics Implementation
//!
//! This crate implements the dual-phase economic model:
//! - Phase 1: Bootstrap (Adoption-Driven Minting)
//! - Phase 2: Utility-Driven Minting
//!
//! Phase transition occurs when Initial Volume Threshold (IVT) is reached

pub mod minting;
pub mod phase_manager;
pub mod utility_index;
pub mod rewards;
pub mod burning;

pub use minting::{MintingController, MintingPhase, MintingConfig};
pub use phase_manager::{PhaseManager, IVTConfig, PhaseTransition};
pub use utility_index::{UtilityIndex, UtilityMetrics, MetricWeights};
pub use rewards::{RewardDistributor, RewardCalculation};
pub use burning::{BurningMechanism, BurnConfig};

/// Result type for tokenomics operations
pub type TokenomicsResult<T> = Result<T, TokenomicsError>;

/// Errors that can occur in tokenomics operations
#[derive(Debug, thiserror::Error)]
pub enum TokenomicsError {
    #[error("Minting error: {0}")]
    MintingError(String),
    
    #[error("Phase transition error: {0}")]
    PhaseTransitionError(String),
    
    #[error("Utility calculation error: {0}")]
    UtilityCalculationError(String),
    
    #[error("Reward distribution error: {0}")]
    RewardDistributionError(String),
    
    #[error("Burning error: {0}")]
    BurningError(String),
    
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),
    
    #[error("Overflow error: {0}")]
    OverflowError(String),
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic_imports() {
        // Smoke test
    }
}