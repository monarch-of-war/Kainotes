// smart-contracts/src/gas.rs

use crate::{ContractError, ContractResult};
use blockchain_core::Gas;
use serde::{Deserialize, Serialize};

/// Gas configuration (Ethereum-compatible)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasConfig {
    /// Gas per transaction
    pub tx_gas: Gas,
    /// Gas per byte of transaction data (zero bytes)
    pub tx_data_zero_gas: Gas,
    /// Gas per byte of transaction data (non-zero bytes)
    pub tx_data_non_zero_gas: Gas,
    /// Gas per contract creation
    pub tx_create_gas: Gas,
    /// Base gas for SSTORE operation
    pub sstore_set_gas: Gas,
    /// Gas for SSTORE when value doesn't change
    pub sstore_reset_gas: Gas,
    /// Refund for SSTORE when clearing storage
    pub sstore_clears_schedule: Gas,
    /// Gas per SLOAD operation
    pub sload_gas: Gas,
    /// Gas for CALL operation
    pub call_gas: Gas,
    /// Gas for contract creation
    pub create_gas: Gas,
    /// Gas per 32-byte word for memory expansion
    pub memory_gas: Gas,
    /// Gas for LOG operations
    pub log_gas: Gas,
    /// Gas per LOG topic
    pub log_topic_gas: Gas,
    /// Gas per byte of LOG data
    pub log_data_gas: Gas,
    /// Gas per KECCAK256 operation
    pub keccak256_gas: Gas,
    /// Gas per 32-byte word for KECCAK256
    pub keccak256_word_gas: Gas,
}

impl Default for GasConfig {
    fn default() -> Self {
        Self::mainnet()
    }
}

impl GasConfig {
    /// Ethereum mainnet gas configuration
    pub fn mainnet() -> Self {
        Self {
            tx_gas: 21000,
            tx_data_zero_gas: 4,
            tx_data_non_zero_gas: 16,
            tx_create_gas: 32000,
            sstore_set_gas: 20000,
            sstore_reset_gas: 5000,
            sstore_clears_schedule: 15000,
            sload_gas: 800, // Post-Berlin
            call_gas: 700,  // Post-Berlin
            create_gas: 32000,
            memory_gas: 3,
            log_gas: 375,
            log_topic_gas: 375,
            log_data_gas: 8,
            keccak256_gas: 30,
            keccak256_word_gas: 6,
        }
    }

    /// Lower gas configuration for testing
    pub fn test() -> Self {
        Self {
            tx_gas: 21000,
            tx_data_zero_gas: 4,
            tx_data_non_zero_gas: 16,
            tx_create_gas: 10000,
            sstore_set_gas: 5000,
            sstore_reset_gas: 2500,
            sstore_clears_schedule: 5000,
            sload_gas: 200,
            call_gas: 100,
            create_gas: 10000,
            memory_gas: 1,
            log_gas: 100,
            log_topic_gas: 100,
            log_data_gas: 2,
            keccak256_gas: 10,
            keccak256_word_gas: 2,
        }
    }
}

/// Gas calculator for operations
pub struct GasCalculator {
    config: GasConfig,
}

impl GasCalculator {
    /// Create new gas calculator with configuration
    pub fn new(config: GasConfig) -> Self {
        Self { config }
    }

    /// Create with mainnet configuration
    pub fn mainnet() -> Self {
        Self::new(GasConfig::mainnet())
    }

    /// Calculate gas for transaction data
    pub fn calculate_tx_data_gas(&self, data: &[u8]) -> Gas {
        let mut gas = 0u64;
        for byte in data {
            if *byte == 0 {
                gas += self.config.tx_data_zero_gas;
            } else {
                gas += self.config.tx_data_non_zero_gas;
            }
        }
        gas
    }

    /// Calculate base transaction gas
    pub fn calculate_base_tx_gas(&self, is_create: bool, data: &[u8]) -> Gas {
        let mut gas = self.config.tx_gas;
        
        if is_create {
            gas += self.config.tx_create_gas;
        }
        
        gas += self.calculate_tx_data_gas(data);
        
        gas
    }

    /// Calculate memory expansion gas
    pub fn calculate_memory_gas(&self, current_size: u64, new_size: u64) -> Gas {
        if new_size <= current_size {
            return 0;
        }

        let new_words = (new_size + 31) / 32;
        let current_words = (current_size + 31) / 32;
        let words_diff = new_words - current_words;

        // Memory cost: linear + quadratic
        let linear_cost = words_diff * self.config.memory_gas;
        let quadratic_cost = (new_words * new_words) / 512;
        let current_quadratic = (current_words * current_words) / 512;

        linear_cost + quadratic_cost - current_quadratic
    }

    /// Calculate storage gas (SSTORE)
    pub fn calculate_sstore_gas(
        &self,
        current_value: &[u8; 32],
        new_value: &[u8; 32],
    ) -> (Gas, Gas) {
        let is_zero = |v: &[u8; 32]| v.iter().all(|&b| b == 0);
        
        let current_is_zero = is_zero(current_value);
        let new_is_zero = is_zero(new_value);

        match (current_is_zero, new_is_zero) {
            (true, false) => {
                // Setting storage (was zero, now non-zero)
                (self.config.sstore_set_gas, 0)
            }
            (false, true) => {
                // Clearing storage (was non-zero, now zero)
                (self.config.sstore_reset_gas, self.config.sstore_clears_schedule)
            }
            (false, false) if current_value != new_value => {
                // Modifying storage (both non-zero, different)
                (self.config.sstore_reset_gas, 0)
            }
            _ => {
                // No change or both zero
                (self.config.sload_gas, 0)
            }
        }
    }

    /// Calculate LOG gas
    pub fn calculate_log_gas(&self, topics: usize, data_len: usize) -> Gas {
        let mut gas = self.config.log_gas;
        gas += (topics as Gas) * self.config.log_topic_gas;
        gas += (data_len as Gas) * self.config.log_data_gas;
        gas
    }

    /// Calculate KECCAK256 gas
    pub fn calculate_keccak256_gas(&self, data_len: usize) -> Gas {
        let words = ((data_len + 31) / 32) as Gas;
        self.config.keccak256_gas + words * self.config.keccak256_word_gas
    }

    /// Calculate CALL gas
    pub fn calculate_call_gas(&self, value_transfer: bool, account_exists: bool) -> Gas {
        let mut gas = self.config.call_gas;
        
        if value_transfer {
            gas += 9000; // Value transfer stipend
            
            if !account_exists {
                gas += 25000; // New account creation
            }
        }
        
        gas
    }

    /// Calculate CREATE gas
    pub fn calculate_create_gas(&self, code_size: usize) -> Gas {
        self.config.create_gas + ((code_size as Gas) * 200) // 200 gas per byte
    }

    /// Get gas configuration
    pub fn config(&self) -> &GasConfig {
        &self.config
    }
}

/// Gas meter for tracking usage during execution
pub struct GasMeter {
    limit: Gas,
    used: Gas,
    refunded: Gas,
}

impl GasMeter {
    /// Create new gas meter
    pub fn new(limit: Gas) -> Self {
        Self {
            limit,
            used: 0,
            refunded: 0,
        }
    }

    /// Consume gas
    pub fn consume(&mut self, amount: Gas) -> ContractResult<()> {
        if self.used + amount > self.limit {
            return Err(ContractError::OutOfGas);
        }
        self.used += amount;
        Ok(())
    }

    /// Refund gas
    pub fn refund(&mut self, amount: Gas) {
        self.refunded += amount;
    }

    /// Get remaining gas
    pub fn remaining(&self) -> Gas {
        self.limit.saturating_sub(self.used)
    }

    /// Get used gas
    pub fn used(&self) -> Gas {
        self.used
    }

    /// Get refunded gas
    pub fn refunded(&self) -> Gas {
        self.refunded
    }

    /// Get final gas used (after refunds, capped at 50% of used)
    pub fn finalize(&self) -> Gas {
        let max_refund = self.used / 2;
        let actual_refund = self.refunded.min(max_refund);
        self.used.saturating_sub(actual_refund)
    }

    /// Check if out of gas
    pub fn is_out_of_gas(&self) -> bool {
        self.used >= self.limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tx_data_gas() {
        let calculator = GasCalculator::mainnet();
        
        let data = vec![0, 0, 1, 2, 3];
        let gas = calculator.calculate_tx_data_gas(&data);
        
        // 2 zeros * 4 + 3 non-zeros * 16 = 8 + 48 = 56
        assert_eq!(gas, 56);
    }

    #[test]
    fn test_base_tx_gas() {
        let calculator = GasCalculator::mainnet();
        
        let gas = calculator.calculate_base_tx_gas(false, &[]);
        assert_eq!(gas, 21000);
        
        let gas_create = calculator.calculate_base_tx_gas(true, &[]);
        assert_eq!(gas_create, 21000 + 32000);
    }

    #[test]
    fn test_memory_expansion() {
        let calculator = GasCalculator::mainnet();
        
        let gas = calculator.calculate_memory_gas(0, 32);
        assert!(gas > 0);
        
        let gas_no_expansion = calculator.calculate_memory_gas(64, 64);
        assert_eq!(gas_no_expansion, 0);
    }

    #[test]
    fn test_sstore_gas() {
        let calculator = GasCalculator::mainnet();
        
        let zero = [0u8; 32];
        let non_zero = [1u8; 32];
        
        // Setting storage
        let (gas, refund) = calculator.calculate_sstore_gas(&zero, &non_zero);
        assert_eq!(gas, 20000);
        assert_eq!(refund, 0);
        
        // Clearing storage
        let (gas, refund) = calculator.calculate_sstore_gas(&non_zero, &zero);
        assert_eq!(gas, 5000);
        assert_eq!(refund, 15000);
    }

    #[test]
    fn test_gas_meter() {
        let mut meter = GasMeter::new(100000);
        
        meter.consume(21000).unwrap();
        assert_eq!(meter.used(), 21000);
        assert_eq!(meter.remaining(), 79000);
        
        meter.refund(5000);
        assert_eq!(meter.refunded(), 5000);
        
        let final_gas = meter.finalize();
        assert_eq!(final_gas, 21000 - 5000);
    }

    #[test]
    fn test_out_of_gas() {
        let mut meter = GasMeter::new(10000);
        
        assert!(meter.consume(15000).is_err());
        assert!(meter.is_out_of_gas());
    }

    #[test]
    fn test_log_gas() {
        let calculator = GasCalculator::mainnet();
        
        // LOG0: 0 topics
        let gas = calculator.calculate_log_gas(0, 32);
        assert_eq!(gas, 375 + 32 * 8);
        
        // LOG2: 2 topics
        let gas = calculator.calculate_log_gas(2, 64);
        assert_eq!(gas, 375 + 2 * 375 + 64 * 8);
    }
}