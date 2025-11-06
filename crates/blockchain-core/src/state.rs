// blockchain-core/src/state.rs

use crate::{types::*, BlockchainError, BlockchainResult};
use blockchain_crypto::{hash::Hashable, Address, Hash};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Account state
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Account {
    /// Account nonce (transaction counter)
    pub nonce: Nonce,
    /// Account balance
    pub balance: Amount,
    /// Staked amount (for validators)
    pub staked: StakeAmount,
    /// Liquidity deployed by this validator
    pub liquidity_deployed: Amount,
    /// Utility contribution score
    pub utility_score: UtilityScore,
    /// Contract code hash (if this is a contract account)
    pub code_hash: Option<Hash>,
    /// Storage root (for contract storage)
    pub storage_root: Option<Hash>,
}

impl Account {
    /// Create a new empty account
    pub fn new() -> Self {
        Self {
            nonce: 0,
            balance: Amount::zero(),
            staked: StakeAmount::zero(),
            liquidity_deployed: Amount::zero(),
            utility_score: UtilityScore::zero(),
            code_hash: None,
            storage_root: None,
        }
    }

    /// Create account with initial balance
    pub fn with_balance(balance: Amount) -> Self {
        Self {
            balance,
            ..Self::new()
        }
    }

    /// Check if account is a validator
    pub fn is_validator(&self) -> bool {
        !self.staked.is_zero()
    }

    /// Check if account is a contract
    pub fn is_contract(&self) -> bool {
        self.code_hash.is_some()
    }

    /// Increment nonce
    pub fn increment_nonce(&mut self) {
        self.nonce += 1;
    }

    /// Add to balance
    pub fn add_balance(&mut self, amount: &Amount) -> BlockchainResult<()> {
        self.balance = self.balance.checked_add(amount)
            .ok_or(BlockchainError::StateError("Balance overflow".into()))?;
        Ok(())
    }

    /// Subtract from balance
    pub fn sub_balance(&mut self, amount: &Amount) -> BlockchainResult<()> {
        self.balance = self.balance.checked_sub(amount)
            .ok_or(BlockchainError::InsufficientBalance)?;
        Ok(())
    }

    /// Stake tokens
    pub fn stake(&mut self, amount: &StakeAmount) -> BlockchainResult<()> {
        self.sub_balance(amount)?;
        self.staked = self.staked.checked_add(amount)
            .ok_or(BlockchainError::StateError("Stake overflow".into()))?;
        Ok(())
    }

    /// Unstake tokens
    pub fn unstake(&mut self, amount: &StakeAmount) -> BlockchainResult<()> {
        if self.staked.inner() < amount.inner() {
            return Err(BlockchainError::StateError("Insufficient stake".into()));
        }
        self.staked = self.staked.checked_sub(amount).unwrap();
        self.add_balance(amount)?;
        Ok(())
    }

    /// Deploy liquidity
    pub fn deploy_liquidity(&mut self, amount: &Amount) -> BlockchainResult<()> {
        if self.staked.inner() < amount.inner() {
            return Err(BlockchainError::StateError(
                "Cannot deploy more liquidity than staked".into()
            ));
        }
        self.liquidity_deployed = self.liquidity_deployed.checked_add(amount)
            .ok_or(BlockchainError::StateError("Liquidity deployment overflow".into()))?;
        Ok(())
    }

    /// Withdraw liquidity
    pub fn withdraw_liquidity(&mut self, amount: &Amount) -> BlockchainResult<()> {
        self.liquidity_deployed = self.liquidity_deployed.checked_sub(amount)
            .ok_or(BlockchainError::StateError("Insufficient liquidity deployed".into()))?;
        Ok(())
    }
}

impl Default for Account {
    fn default() -> Self {
        Self::new()
    }
}

/// World state managing all accounts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldState {
    /// Accounts mapping
    accounts: HashMap<Address, Account>,
    /// State modifications (for efficient rollback)
    modifications: Vec<StateModification>,
}

impl WorldState {
    /// Create new empty world state
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            modifications: Vec::new(),
        }
    }

    /// Get account (creates empty account if not exists)
    pub fn get_account(&self, address: &Address) -> Account {
        self.accounts.get(address).cloned().unwrap_or_default()
    }

    /// Get mutable account reference
    pub fn get_account_mut(&mut self, address: &Address) -> &mut Account {
        // Record the original account state for rollback support if not already recorded
        self.record_account_modification(*address);

        self.accounts.entry(*address).or_insert_with(Account::new)
    }

    /// Set account
    pub fn set_account(&mut self, address: Address, account: Account) {
        // Record the original account state for rollback support if not already recorded
        self.record_account_modification(address);

        self.accounts.insert(address, account);
    }

    /// Get account balance
    pub fn get_balance(&self, address: &Address) -> Amount {
        self.accounts.get(address)
            .map(|acc| acc.balance.clone())
            .unwrap_or_else(Amount::zero)
    }

    /// Get account nonce
    pub fn get_nonce(&self, address: &Address) -> Nonce {
        self.accounts.get(address).map(|acc| acc.nonce).unwrap_or(0)
    }

    /// Transfer tokens between accounts
    pub fn transfer(
        &mut self,
        from: &Address,
        to: &Address,
        amount: &Amount,
    ) -> BlockchainResult<()> {
        // Get accounts
        let from_account = self.get_account(from);
        let to_account = self.get_account(to);

        // Check sufficient balance
        if from_account.balance.inner() < amount.inner() {
            return Err(BlockchainError::InsufficientBalance);
        }

        // Perform transfer
        let mut new_from = from_account;
        let mut new_to = to_account;
        
        new_from.sub_balance(amount)?;
        new_to.add_balance(amount)?;

        // Update state
        self.set_account(*from, new_from);
        self.set_account(*to, new_to);

        Ok(())
    }

    /// Calculate state root hash
    pub fn state_root(&self) -> Hash {
        // Sort accounts by address for deterministic hashing
        let mut sorted_accounts: Vec<_> = self.accounts.iter().collect();
        sorted_accounts.sort_by_key(|(addr, _)| *addr);

        // Serialize and hash
        let mut combined = Vec::new();
        for (addr, account) in sorted_accounts {
            combined.extend_from_slice(addr.as_bytes());
            combined.extend_from_slice(&bincode::serialize(account).unwrap());
        }

        if combined.is_empty() {
            Hash::zero()
        } else {
            combined.hash()
        }
    }

    /// Begin transaction (checkpoint)
    pub fn checkpoint(&mut self) {
        self.modifications.push(StateModification::Checkpoint);
    }

    /// Commit transaction
    pub fn commit(&mut self) {
        // Remove modifications up to last checkpoint
        while let Some(mod_type) = self.modifications.pop() {
            if matches!(mod_type, StateModification::Checkpoint) {
                break;
            }
        }
    }

    /// Rollback transaction
    pub fn rollback(&mut self) {
        // Reverse modifications up to last checkpoint
        while let Some(mod_type) = self.modifications.pop() {
            match mod_type {
                StateModification::Checkpoint => break,
                StateModification::AccountSet { address, old_account } => {
                    if let Some(old) = old_account {
                        self.accounts.insert(address, old);
                    } else {
                        self.accounts.remove(&address);
                    }
                }
            }
        }
    }
}

impl WorldState {
    /// Record an AccountSet modification for `address` unless one has already been
    /// recorded since the last checkpoint. This ensures rollback restores the
    /// pre-checkpoint account state.
    fn record_account_modification(&mut self, address: Address) {
        // Search backwards through modifications until the last Checkpoint (or start)
        // to see if this address already has a recorded original value.
        for mod_entry in self.modifications.iter().rev() {
            match mod_entry {
                StateModification::Checkpoint => break,
                StateModification::AccountSet { address: a, .. } if *a == address => {
                    // Already recorded for this checkpoint; nothing to do
                    return;
                }
                _ => {}
            }
        }

        // Not recorded yet: push the current value (if any)
        let old = self.accounts.get(&address).cloned();
        self.modifications.push(StateModification::AccountSet {
            address,
            old_account: old,
        });
    }
}

impl Default for WorldState {
    fn default() -> Self {
        Self::new()
    }
}

/// State modification for rollback support
#[derive(Debug, Clone, Serialize, Deserialize)]
enum StateModification {
    Checkpoint,
    AccountSet {
        address: Address,
        old_account: Option<Account>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_creation() {
        let account = Account::new();
        assert_eq!(account.nonce, 0);
        assert!(account.balance.is_zero());
        assert!(!account.is_validator());
    }

    #[test]
    fn test_account_balance() {
        let mut account = Account::new();
        account.add_balance(&Amount::from_u64(100)).unwrap();
        assert_eq!(account.balance, Amount::from_u64(100));
        
        account.sub_balance(&Amount::from_u64(50)).unwrap();
        assert_eq!(account.balance, Amount::from_u64(50));
    }

    #[test]
    fn test_account_staking() {
        let mut account = Account::with_balance(Amount::from_u64(1000));
        account.stake(&StakeAmount::from_u64(500)).unwrap();
        
        assert!(account.is_validator());
        assert_eq!(account.staked, StakeAmount::from_u64(500));
        assert_eq!(account.balance, Amount::from_u64(500));
    }

    #[test]
    fn test_world_state_transfer() {
        let mut state = WorldState::new();
        let addr1 = Address::zero();
        let mut addr2_bytes = [0u8; 20];
        addr2_bytes[0] = 1;
        let addr2 = Address::new(addr2_bytes);
        
        // Setup initial balances
        let mut acc1 = Account::new();
        acc1.add_balance(&Amount::from_u64(1000)).unwrap();
        state.set_account(addr1, acc1);
        
        // Transfer
        state.transfer(&addr1, &addr2, &Amount::from_u64(300)).unwrap();
        
        assert_eq!(state.get_balance(&addr1), Amount::from_u64(700));
        assert_eq!(state.get_balance(&addr2), Amount::from_u64(300));
    }

    #[test]
    fn test_state_root() {
        let mut state = WorldState::new();
        let root1 = state.state_root();
        
        // Add account
        state.set_account(Address::zero(), Account::with_balance(Amount::from_u64(100)));
        let root2 = state.state_root();
        
        assert_ne!(root1, root2);
    }
}