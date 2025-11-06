// smart-contracts/src/lib.rs

//! EVM-Compatible Smart Contract Execution
//!
//! This crate provides EVM compatibility using revm, enabling:
//! - Contract deployment and execution
//! - Gas metering
//! - State management
//! - Precompiled contracts
//! - Event emission

pub mod vm;
pub mod precompiles;
pub mod state;
pub mod gas;

pub use vm::{EVMExecutor, ExecutionResult, ContractCall};
pub use precompiles::{PrecompileRegistry, PrecompileResult};
pub use state::{EVMState, ContractAccount};
pub use gas::{GasCalculator, GasConfig};

use blockchain_core::Amount;

/// Result type for smart contract operations
pub type ContractResult<T> = Result<T, ContractError>;

/// Errors that can occur during contract execution
#[derive(Debug, thiserror::Error)]
pub enum ContractError {
    #[error("Execution error: {0}")]
    ExecutionError(String),
    
    #[error("Out of gas")]
    OutOfGas,
    
    #[error("Contract not found: {0}")]
    ContractNotFound(String),
    
    #[error("Invalid bytecode: {0}")]
    InvalidBytecode(String),
    
    #[error("Deployment failed: {0}")]
    DeploymentFailed(String),
    
    #[error("Revert: {0}")]
    Revert(String),
    
    #[error("State error: {0}")]
    StateError(String),
    
    #[error("Gas calculation error: {0}")]
    GasError(String),
    
    #[error("Precompile error: {0}")]
    PrecompileError(String),
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic_imports() {
        // Smoke test
    }
}