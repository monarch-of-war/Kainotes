// smart-contracts/src/vm.rs

use crate::{
    gas::{GasCalculator, GasMeter},
    precompiles::PrecompileRegistry,
    state::EVMState,
    ContractError, ContractResult,
};
use blockchain_core::{Amount, Gas};
use blockchain_crypto::{hash::Hashable, Address, Hash};
use serde::{Deserialize, Serialize};

/// EVM execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Success status
    pub success: bool,
    /// Gas used
    pub gas_used: Gas,
    /// Return data
    pub output: Vec<u8>,
    /// Logs emitted
    pub logs: Vec<Log>,
    /// Contract address (if deployment)
    pub contract_address: Option<Address>,
    /// Error message (if failed)
    pub error: Option<String>,
}

/// Event log
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Log {
    /// Contract address that emitted the log
    pub address: Address,
    /// Topics (indexed parameters)
    pub topics: Vec<Hash>,
    /// Data (non-indexed parameters)
    pub data: Vec<u8>,
}

/// Contract call parameters
#[derive(Debug, Clone)]
pub struct ContractCall {
    /// Caller address
    pub from: Address,
    /// Target contract address (None for deployment)
    pub to: Option<Address>,
    /// Call data (function selector + args)
    pub data: Vec<u8>,
    /// Value to transfer
    pub value: Amount,
    /// Gas limit
    pub gas_limit: Gas,
}

impl ContractCall {
    /// Create a new contract call
    pub fn new(from: Address, to: Address, data: Vec<u8>, value: Amount, gas_limit: Gas) -> Self {
        Self {
            from,
            to: Some(to),
            data,
            value,
            gas_limit,
        }
    }

    /// Create a contract deployment
    pub fn deploy(from: Address, bytecode: Vec<u8>, value: Amount, gas_limit: Gas) -> Self {
        Self {
            from,
            to: None,
            data: bytecode,
            value,
            gas_limit,
        }
    }

    /// Check if this is a contract deployment
    pub fn is_deployment(&self) -> bool {
        self.to.is_none()
    }
}

/// EVM executor
pub struct EVMExecutor {
    /// EVM state
    state: EVMState,
    /// Gas calculator
    gas_calculator: GasCalculator,
    /// Precompile registry
    precompiles: PrecompileRegistry,
    /// Block number
    block_number: u64,
    /// Block timestamp
    block_timestamp: u64,
}

impl EVMExecutor {
    /// Create new EVM executor
    pub fn new(block_number: u64, block_timestamp: u64) -> Self {
        Self {
            state: EVMState::new(),
            gas_calculator: GasCalculator::mainnet(),
            precompiles: PrecompileRegistry::new(),
            block_number,
            block_timestamp,
        }
    }

    /// Execute a contract call
    pub fn execute(&mut self, call: ContractCall) -> ContractResult<ExecutionResult> {
        // Create gas meter
        let mut gas_meter = GasMeter::new(call.gas_limit);

        // Calculate base transaction gas
        let base_gas = self.gas_calculator.calculate_base_tx_gas(
            call.is_deployment(),
            &call.data,
        );
        gas_meter.consume(base_gas)?;

        // Handle value transfer
        if !call.value.is_zero() {
            self.state.transfer(&call.from, &call.to.unwrap_or(call.from), &call.value)?;
        }

        // Execute based on call type
        let result = if call.is_deployment() {
            self.execute_deployment(call, &mut gas_meter)
        } else {
            self.execute_call(call, &mut gas_meter)
        };

        result
    }

    /// Execute contract deployment
    fn execute_deployment(
        &mut self,
        call: ContractCall,
        gas_meter: &mut GasMeter,
    ) -> ContractResult<ExecutionResult> {
        // Calculate deployment address
        let nonce = self.state.get_nonce(&call.from);
        let contract_address = self.state.calculate_create_address(&call.from, nonce);

        // Consume creation gas
        let create_gas = self.gas_calculator.calculate_create_gas(call.data.len());
        gas_meter.consume(create_gas)?;

        // Deploy contract
        self.state.deploy_contract(contract_address, call.data.clone(), call.value)?;
        self.state.increment_nonce(&call.from);

        Ok(ExecutionResult {
            success: true,
            gas_used: gas_meter.finalize(),
            output: contract_address.as_bytes().to_vec(),
            logs: vec![],
            contract_address: Some(contract_address),
            error: None,
        })
    }

    /// Execute contract call
    fn execute_call(
        &mut self,
        call: ContractCall,
        gas_meter: &mut GasMeter,
    ) -> ContractResult<ExecutionResult> {
        let target = call.to.ok_or_else(|| 
            ContractError::ExecutionError("No target address for call".into())
        )?;

        // Check if target is a precompile
        if self.precompiles.is_precompile(&target) {
            return self.execute_precompile(target, &call.data, gas_meter);
        }

        // Check if target is a contract
        if !self.state.is_contract(&target) {
            // Simple transfer to EOA
            return Ok(ExecutionResult {
                success: true,
                gas_used: gas_meter.finalize(),
                output: vec![],
                logs: vec![],
                contract_address: None,
                error: None,
            });
        }

        // Execute contract code
        self.execute_contract_code(target, &call.data, gas_meter)
    }

    /// Execute precompile
    fn execute_precompile(
        &self,
        address: Address,
        input: &[u8],
        gas_meter: &mut GasMeter,
    ) -> ContractResult<ExecutionResult> {
        let result = self.precompiles.execute(&address, input)?;
        
        gas_meter.consume(result.gas_used)?;

        Ok(ExecutionResult {
            success: result.success,
            gas_used: gas_meter.finalize(),
            output: result.output,
            logs: vec![],
            contract_address: None,
            error: if result.success { None } else { Some("Precompile failed".into()) },
        })
    }

    /// Execute contract code (simplified - would use revm in production)
    fn execute_contract_code(
        &mut self,
        _address: Address,
        input: &[u8],
        gas_meter: &mut GasMeter,
    ) -> ContractResult<ExecutionResult> {
        // In production, this would use revm to execute the actual bytecode
        // For now, simplified execution
        
        // Parse function selector (first 4 bytes)
        if input.len() < 4 {
            return Err(ContractError::ExecutionError("Invalid input".into()));
        }

        let selector = &input[0..4];
        let _args = &input[4..];

        // Consume some gas for execution
        gas_meter.consume(10000)?;

        // Simplified: return success with empty output
        Ok(ExecutionResult {
            success: true,
            gas_used: gas_meter.finalize(),
            output: vec![],
            logs: vec![],
            contract_address: None,
            error: None,
        })
    }

    /// Static call (read-only, no state changes)
    pub fn static_call(
        &self,
        target: Address,
        data: Vec<u8>,
        gas_limit: Gas,
    ) -> ContractResult<ExecutionResult> {
        let mut gas_meter = GasMeter::new(gas_limit);

        // Check if precompile
        if self.precompiles.is_precompile(&target) {
            let result = self.precompiles.execute(&target, &data)?;
            gas_meter.consume(result.gas_used)?;

            return Ok(ExecutionResult {
                success: result.success,
                gas_used: gas_meter.finalize(),
                output: result.output,
                logs: vec![],
                contract_address: None,
                error: None,
            });
        }

        // Read-only contract call
        // Would execute without state modifications
        Ok(ExecutionResult {
            success: true,
            gas_used: gas_meter.finalize(),
            output: vec![],
            logs: vec![],
            contract_address: None,
            error: None,
        })
    }

    /// Get EVM state
    pub fn state(&self) -> &EVMState {
        &self.state
    }

    /// Get mutable EVM state
    pub fn state_mut(&mut self) -> &mut EVMState {
        &mut self.state
    }

    /// Get gas calculator
    pub fn gas_calculator(&self) -> &GasCalculator {
        &self.gas_calculator
    }

    /// Estimate gas for a call
    pub fn estimate_gas(&mut self, call: ContractCall) -> ContractResult<Gas> {
        // Clone current state
        let original_state = self.state.clone();

        // Execute with high gas limit
        let mut estimation_call = call.clone();
        estimation_call.gas_limit = 10_000_000;

        let result = self.execute(estimation_call)?;

        // Restore original state
        self.state = original_state;

        if result.success {
            // Add 10% buffer to ensure execution succeeds
            Ok((result.gas_used as f64 * 1.1) as Gas)
        } else {
            Err(ContractError::ExecutionError(
                result.error.unwrap_or_else(|| "Execution failed".into())
            ))
        }
    }

    /// Update block context
    pub fn update_block_context(&mut self, block_number: u64, block_timestamp: u64) {
        self.block_number = block_number;
        self.block_timestamp = block_timestamp;
    }

    /// Get block number
    pub fn block_number(&self) -> u64 {
        self.block_number
    }

    /// Get block timestamp
    pub fn block_timestamp(&self) -> u64 {
        self.block_timestamp
    }
}

impl EVMState {
    // Add clone implementation for estimation
    fn clone(&self) -> Self {
        Self {
            contracts: self.contracts.clone(),
            storage: self.storage.clone(),
            balances: self.balances.clone(),
            nonces: self.nonces.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_deployment() {
        let mut executor = EVMExecutor::new(1, 1000);
        
        let deployer = Address::zero();
        executor.state_mut().set_balance(deployer, Amount::from_u64(100000));
        
        let bytecode = vec![0x60, 0x80, 0x60, 0x40, 0x52]; // Simple contract
        let call = ContractCall::deploy(deployer, bytecode, Amount::zero(), 1000000);
        
        let result = executor.execute(call).unwrap();
        
        assert!(result.success);
        assert!(result.contract_address.is_some());
    }

    #[test]
    fn test_precompile_call() {
        let mut executor = EVMExecutor::new(1, 1000);
        
        let caller = Address::zero();
        executor.state_mut().set_balance(caller, Amount::from_u64(100000));
        
        // Call SHA-256 precompile (address 0x02)
        let mut target_bytes = [0u8; 20];
        target_bytes[19] = 2;
        let target = Address::new(target_bytes);
        
        let data = b"hello".to_vec();
        let call = ContractCall::new(caller, target, data, Amount::zero(), 100000);
        
        let result = executor.execute(call).unwrap();
        
        assert!(result.success);
        assert_eq!(result.output.len(), 32); // SHA-256 output
    }

    #[test]
    fn test_value_transfer() {
        let mut executor = EVMExecutor::new(1, 1000);
        
        let from = Address::zero();
        let mut to_bytes = [0u8; 20];
        to_bytes[0] = 1;
        let to = Address::new(to_bytes);
        
        executor.state_mut().set_balance(from, Amount::from_u64(1000));
        
        let call = ContractCall::new(from, to, vec![], Amount::from_u64(500), 100000);
        let result = executor.execute(call).unwrap();
        
        assert!(result.success);
        assert_eq!(executor.state().get_balance(&from), Amount::from_u64(500));
        assert_eq!(executor.state().get_balance(&to), Amount::from_u64(500));
    }

    #[test]
    fn test_out_of_gas() {
        let mut executor = EVMExecutor::new(1, 1000);
        
        let deployer = Address::zero();
        let bytecode = vec![0x60, 0x80, 0x60, 0x40, 0x52];
        let call = ContractCall::deploy(deployer, bytecode, Amount::zero(), 1000); // Low gas
        
        let result = executor.execute(call);
        assert!(result.is_err());
    }

    #[test]
    fn test_gas_estimation() {
        let mut executor = EVMExecutor::new(1, 1000);
        
        let deployer = Address::zero();
        executor.state_mut().set_balance(deployer, Amount::from_u64(100000));
        
        let bytecode = vec![0x60, 0x80, 0x60, 0x40, 0x52];
        let call = ContractCall::deploy(deployer, bytecode, Amount::zero(), 1000000);
        
        let estimated = executor.estimate_gas(call).unwrap();
        assert!(estimated > 0);
    }
}