// liquidity/src/lib.rs

//! Liquidity Management for Active Stake Deployment
//!
//! This crate implements the active liquidity deployment system where
//! validator stakes are not idle but actively deployed in:
//! - AMM Liquidity Pools
//! - Lending Protocols
//! - Network Treasury
//! - Stability Reserves

pub mod pool;
pub mod deployment;
pub mod risk;
pub mod amm;
pub mod lending;
pub mod treasury;

pub use pool::{LiquidityPool, PoolType, PoolInfo, PoolMetrics};
pub use deployment::{DeploymentManager, DeploymentStrategy, DeploymentAllocation};
pub use risk::{RiskAssessment, RiskCalculator, RiskProfile};
pub use amm::{AMMPool, TradingPair, SwapQuote};
pub use lending::{LendingPool, LoanPosition, InterestRate};
pub use treasury::{NetworkTreasury, TreasuryAllocation, Grant};

use blockchain_core::Amount;

/// Result type for liquidity operations
pub type LiquidityResult<T> = Result<T, LiquidityError>;

/// Errors that can occur in liquidity operations
#[derive(Debug, thiserror::Error)]
pub enum LiquidityError {
    #[error("Pool error: {0}")]
    PoolError(String),
    
    #[error("Deployment error: {0}")]
    DeploymentError(String),
    
    #[error("Risk error: {0}")]
    RiskError(String),
    
    #[error("Insufficient liquidity: required {required}, available {available}")]
    InsufficientLiquidity { required: Amount, available: Amount },
    
    #[error("Pool not found: {0}")]
    PoolNotFound(u64),
    
    #[error("Invalid allocation: {0}")]
    InvalidAllocation(String),
    
    #[error("Slippage too high: {0}%")]
    SlippageTooHigh(f64),
    
    #[error("Position not found: {0}")]
    PositionNotFound(String),
    
    #[error("Calculation error: {0}")]
    CalculationError(String),
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic_imports() {
        // Smoke test
    }
}