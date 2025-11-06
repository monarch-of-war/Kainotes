// smart-contracts/src/state.rs

use crate::{ContractError, ContractResult};
use blockchain_core::Amount;
use blockchain_crypto::{hash::Hashable, Address, Hash};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Contract account with code and storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractAccount {
    /// Contract address
    pub address: Address,
    /// Contract bytecode
    pub code: Vec<u8>,
    /// Code hash for verification
    pub code_hash: Hash,
    /// Contract balance
    pub balance: Amount,
    /// Storage root
    pub storage_root: Hash,
    /// Nonce (for create2)
    pub nonce: u64,
}

impl ContractAccount {
    /// Create new contract account
    pub fn new(address: Address, code: Vec<u8>, balance: Amount) -> Self {
        let code_hash = code.as_slice().hash();
        Self {
            address,
            code,
            code_hash,
            balance,
            storage_root: Hash::zero(),
            nonce: 1,
        }
    }

    /// Check if account has code
    pub fn has_code(&self) -> bool {
        !self.code.is_empty()
    }

    /// Get code size
    pub fn code_size(&self) -> usize {
        self.code.len()
    }
}

/// EVM state manager
#[derive(Clone)]
pub struct EVMState {
    /// Contract accounts
    pub contracts: HashMap<Address, ContractAccount>,
    /// Contract storage (contract_address -> slot -> value)
    pub storage: HashMap<Address, HashMap<[u8; 32], [u8; 32]>>,
    /// Account balances
    pub balances: HashMap<Address, Amount>,
    /// Account nonces
    pub nonces: HashMap<Address, u64>,
}

impl EVMState {
    /// Create new EVM state
    pub fn new() -> Self {
        Self {
            contracts: HashMap::new(),
            storage: HashMap::new(),
            balances: HashMap::new(),
            nonces: HashMap::new(),
        }
    }

    /// Deploy a new contract
    pub fn deploy_contract(
        &mut self,
        address: Address,
        code: Vec<u8>,
        balance: Amount,
    ) -> ContractResult<()> {
        if self.contracts.contains_key(&address) {
            return Err(ContractError::DeploymentFailed(
                "Contract already exists at address".into()
            ));
        }

        let contract = ContractAccount::new(address, code, balance.clone());
        self.contracts.insert(address, contract);
        self.balances.insert(address, balance);
        self.storage.insert(address, HashMap::new());

        Ok(())
    }

    /// Get contract account
    pub fn get_contract(&self, address: &Address) -> Option<&ContractAccount> {
        self.contracts.get(address)
    }

    /// Get contract code
    pub fn get_code(&self, address: &Address) -> Option<&Vec<u8>> {
        self.contracts.get(address).map(|c| &c.code)
    }

    

    /// Get code hash
    pub fn get_code_hash(&self, address: &Address) -> Hash {
        self.contracts.get(address)
            .map(|c| c.code_hash)
            .unwrap_or_else(Hash::zero)
    }

    /// Get code size
    pub fn get_code_size(&self, address: &Address) -> usize {
        self.contracts.get(address)
            .map(|c| c.code_size())
            .unwrap_or(0)
    }

    /// Check if address is a contract
    pub fn is_contract(&self, address: &Address) -> bool {
        self.contracts.contains_key(address)
    }

    /// Get storage value
    pub fn get_storage(&self, address: &Address, slot: [u8; 32]) -> [u8; 32] {
        self.storage.get(address)
            .and_then(|s| s.get(&slot))
            .copied()
            .unwrap_or([0u8; 32])
    }

    /// Set storage value
    pub fn set_storage(&mut self, address: Address, slot: [u8; 32], value: [u8; 32]) {
        self.storage.entry(address)
            .or_insert_with(HashMap::new)
            .insert(slot, value);
    }

    /// Get balance
    pub fn get_balance(&self, address: &Address) -> Amount {
        self.balances.get(address)
            .cloned()
            .unwrap_or_else(Amount::zero)
    }

    /// Set balance
    pub fn set_balance(&mut self, address: Address, balance: Amount) {
        self.balances.insert(address, balance);
    }

    /// Transfer balance
    pub fn transfer(
        &mut self,
        from: &Address,
        to: &Address,
        amount: &Amount,
    ) -> ContractResult<()> {
        let from_balance = self.get_balance(from);
        if from_balance.inner() < amount.inner() {
            return Err(ContractError::ExecutionError(
                "Insufficient balance for transfer".into()
            ));
        }

        let new_from_balance = from_balance.checked_sub(amount)
            .ok_or_else(|| ContractError::StateError("Balance underflow".into()))?;
        
        let to_balance = self.get_balance(to);
        let new_to_balance = to_balance.checked_add(amount)
            .ok_or_else(|| ContractError::StateError("Balance overflow".into()))?;

        self.set_balance(*from, new_from_balance);
        self.set_balance(*to, new_to_balance);

        Ok(())
    }

    /// Get nonce
    pub fn get_nonce(&self, address: &Address) -> u64 {
        self.nonces.get(address).copied().unwrap_or(0)
    }

    /// Increment nonce
    pub fn increment_nonce(&mut self, address: &Address) {
        let nonce = self.get_nonce(address);
        self.nonces.insert(*address, nonce + 1);
    }

    /// Calculate contract address using CREATE
    pub fn calculate_create_address(&self, deployer: &Address, nonce: u64) -> Address {
        // Ethereum's CREATE address calculation: keccak256(rlp([sender, nonce]))
        // Simplified version
        let mut data = Vec::new();
        data.extend_from_slice(deployer.as_bytes());
        data.extend_from_slice(&nonce.to_le_bytes());
        
        let hash = data.as_slice().hash();
        let mut address_bytes = [0u8; 20];
        address_bytes.copy_from_slice(&hash.as_bytes()[12..32]);
        Address::new(address_bytes)
    }

    /// Calculate contract address using CREATE2
    pub fn calculate_create2_address(
        &self,
        deployer: &Address,
        salt: [u8; 32],
        init_code_hash: Hash,
    ) -> Address {
        // Ethereum's CREATE2: keccak256(0xff ++ sender ++ salt ++ keccak256(init_code))
        let mut data = Vec::new();
        data.push(0xff);
        data.extend_from_slice(deployer.as_bytes());
        data.extend_from_slice(&salt);
        data.extend_from_slice(init_code_hash.as_bytes());
        
        let hash = data.as_slice().hash();
        let mut address_bytes = [0u8; 20];
        address_bytes.copy_from_slice(&hash.as_bytes()[12..32]);
        Address::new(address_bytes)
    }

    /// Get all contracts
    pub fn contracts(&self) -> &HashMap<Address, ContractAccount> {
        &self.contracts
    }

    /// Clear all state (for testing)
    pub fn clear(&mut self) {
        self.contracts.clear();
        self.storage.clear();
        self.balances.clear();
        self.nonces.clear();
    }
}

impl Default for EVMState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contract_deployment() {
        let mut state = EVMState::new();
        let address = Address::zero();
        let code = vec![0x60, 0x00, 0x60, 0x00, 0xf3]; // Simple contract
        
        state.deploy_contract(address, code.clone(), Amount::zero()).unwrap();
        
        assert!(state.is_contract(&address));
        assert_eq!(state.get_code(&address).unwrap(), &code);
    }

    #[test]
    fn test_storage() {
        let mut state = EVMState::new();
        let address = Address::zero();
        let slot = [1u8; 32];
        let value = [2u8; 32];
        
        state.set_storage(address, slot, value);
        assert_eq!(state.get_storage(&address, slot), value);
    }

    #[test]
    fn test_balance_transfer() {
        let mut state = EVMState::new();
        let from = Address::zero();
        let mut to_bytes = [0u8; 20];
        to_bytes[0] = 1;
        let to = Address::new(to_bytes);
        
        state.set_balance(from, Amount::from_u64(1000));
        state.transfer(&from, &to, &Amount::from_u64(300)).unwrap();
        
        assert_eq!(state.get_balance(&from), Amount::from_u64(700));
        assert_eq!(state.get_balance(&to), Amount::from_u64(300));
    }

    #[test]
    fn test_create_address_calculation() {
        let state = EVMState::new();
        let deployer = Address::zero();
        
        let addr1 = state.calculate_create_address(&deployer, 0);
        let addr2 = state.calculate_create_address(&deployer, 1);
        
        assert_ne!(addr1, addr2);
    }
}