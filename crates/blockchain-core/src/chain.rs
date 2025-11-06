// blockchain-core/src/chain.rs

use crate::{
    block::{Block, BlockHeader},
    state::WorldState,
    transaction::{Transaction, TransactionReceipt},
    types::*,
    BlockchainError, BlockchainResult,
};
use blockchain_crypto::{Address, Hash};
use std::collections::HashMap;

/// Main blockchain structure
pub struct Blockchain {
    /// All blocks indexed by hash
    blocks: HashMap<Hash, Block>,
    /// Block hashes indexed by number
    block_by_number: HashMap<BlockNumber, Hash>,
    /// Current chain head
    head: Hash,
    /// Genesis block hash
    genesis: Hash,
    /// Current world state
    state: WorldState,
    /// Transaction receipts
    receipts: HashMap<Hash, TransactionReceipt>,
}

impl Blockchain {
    /// Create a new blockchain with genesis block
    pub fn new(genesis_block: Block) -> BlockchainResult<Self> {
        if !genesis_block.is_genesis() {
            return Err(BlockchainError::InvalidChain(
                "First block must be genesis".into()
            ));
        }

        let genesis_hash = genesis_block.hash();
        let mut blocks = HashMap::new();
        let mut block_by_number = HashMap::new();
        
        blocks.insert(genesis_hash, genesis_block.clone());
        block_by_number.insert(0, genesis_hash);

        let state = WorldState::new();

        Ok(Self {
            blocks,
            block_by_number,
            head: genesis_hash,
            genesis: genesis_hash,
            state,
            receipts: HashMap::new(),
        })
    }

    /// Get the genesis block
    pub fn genesis_block(&self) -> &Block {
        self.blocks.get(&self.genesis).unwrap()
    }

    /// Get the current head block
    pub fn head_block(&self) -> &Block {
        self.blocks.get(&self.head).unwrap()
    }

    /// Get current block height
    pub fn height(&self) -> BlockNumber {
        self.head_block().number()
    }

    /// Get block by hash
    pub fn get_block(&self, hash: &Hash) -> Option<&Block> {
        self.blocks.get(hash)
    }

    /// Get block by number
    pub fn get_block_by_number(&self, number: BlockNumber) -> Option<&Block> {
        self.block_by_number.get(&number)
            .and_then(|hash| self.blocks.get(hash))
    }

    /// Get transaction receipt
    pub fn get_receipt(&self, tx_hash: &Hash) -> Option<&TransactionReceipt> {
        self.receipts.get(tx_hash)
    }

    /// Get current state
    pub fn state(&self) -> &WorldState {
        &self.state
    }

    /// Get mutable state reference
    pub fn state_mut(&mut self) -> &mut WorldState {
        &mut self.state
    }

    /// Add a new block to the chain
    pub fn add_block(&mut self, block: Block) -> BlockchainResult<()> {
        // Get parent block
        let parent = self.get_block(&block.header.parent_hash)
            .ok_or_else(|| BlockchainError::BlockNotFound(block.header.parent_hash))?
            .clone();

        // Validate block
        block.validate(&parent)?;

        // Verify state root matches
        if block.header.state_root != self.state.state_root() {
            return Err(BlockchainError::InvalidBlock(
                "State root mismatch".into()
            ));
        }

        // Add block to chain
        let block_hash = block.hash();
        let block_number = block.number();
        
        self.blocks.insert(block_hash, block);
        self.block_by_number.insert(block_number, block_hash);
        self.head = block_hash;

        Ok(())
    }

    /// Execute a transaction
    pub fn execute_transaction(
        &mut self,
        tx: &Transaction,
    ) -> BlockchainResult<TransactionReceipt> {
        // Validate transaction
        tx.validate_basic()?;

        // Check nonce
        let current_nonce = self.state.get_nonce(&tx.from);
        if tx.nonce != current_nonce {
            return Err(BlockchainError::NonceMismatch);
        }

        // Check balance for gas
        let max_gas_cost = Amount::from_u64(tx.gas_limit * tx.gas_price);
        let sender_balance = self.state.get_balance(&tx.from);
        if sender_balance.inner() < max_gas_cost.inner() {
            return Err(BlockchainError::InsufficientBalance);
        }

        // Execute transaction based on type
        self.state.checkpoint();
        
        let result = self.execute_transaction_type(tx);
        
        match result {
            Ok(receipt) => {
                self.state.commit();
                Ok(receipt)
            }
            Err(e) => {
                self.state.rollback();
                Err(e)
            }
        }
    }

    fn execute_transaction_type(
        &mut self,
        tx: &Transaction,
    ) -> BlockchainResult<TransactionReceipt> {
        use crate::transaction::TransactionType;

        // Increment nonce
        self.state.get_account_mut(&tx.from).increment_nonce();

        let status = match &tx.tx_type {
            TransactionType::Transfer { to, amount } => {
                self.state.transfer(&tx.from, to, amount)?;
                crate::transaction::ExecutionStatus::Success
            }
            TransactionType::Stake { amount } => {
                self.state.get_account_mut(&tx.from).stake(amount)?;
                crate::transaction::ExecutionStatus::Success
            }
            TransactionType::Unstake { amount } => {
                self.state.get_account_mut(&tx.from).unstake(amount)?;
                crate::transaction::ExecutionStatus::Success
            }
            TransactionType::DeployLiquidity { amount, .. } => {
                self.state.get_account_mut(&tx.from).deploy_liquidity(amount)?;
                crate::transaction::ExecutionStatus::Success
            }
            TransactionType::WithdrawLiquidity { amount, .. } => {
                self.state.get_account_mut(&tx.from).withdraw_liquidity(amount)?;
                crate::transaction::ExecutionStatus::Success
            }
            _ => {
                // Contract operations would be implemented here
                crate::transaction::ExecutionStatus::Success
            }
        };

        // Deduct gas fee
        let gas_used = 21000; // Simplified, would calculate actual usage
        let gas_fee = tx.calculate_fee(gas_used);
        self.state.get_account_mut(&tx.from).sub_balance(&gas_fee)?;

        // Create receipt
        let receipt = TransactionReceipt {
            tx_hash: tx.hash(),
            block_number: self.height() + 1, // Will be in next block
            from: tx.from,
            to: tx.recipient(),
            gas_used,
            status,
            contract_address: None,
            logs: Vec::new(),
        };

        // Store receipt
        self.receipts.insert(tx.hash(), receipt.clone());

        Ok(receipt)
    }

    /// Verify the entire chain
    pub fn verify_chain(&self) -> BlockchainResult<()> {
        let mut current = self.genesis_block().clone();
        let height = self.height();

        for i in 1..=height {
            let next = self.get_block_by_number(i)
                .ok_or_else(|| BlockchainError::InvalidChain(
                    format!("Missing block at height {}", i)
                ))?;
            
            next.validate(&current)?;
            current = next.clone();
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blockchain_creation() {
        let genesis = Block::genesis(Hash::zero());
        let chain = Blockchain::new(genesis).unwrap();
        
        assert_eq!(chain.height(), 0);
        assert!(chain.genesis_block().is_genesis());
    }

    #[test]
    fn test_add_block() {
        let genesis = Block::genesis(Hash::zero());
        let mut chain = Blockchain::new(genesis.clone()).unwrap();
        
        let block1 = Block::new(
            1,
            genesis.hash(),
            chain.state().state_root(),
            Address::zero(),
            vec![],
            10_000_000,
        ).unwrap();
        
        chain.add_block(block1).unwrap();
        assert_eq!(chain.height(), 1);
    }

    #[test]
    fn test_get_block_by_number() {
        let genesis = Block::genesis(Hash::zero());
        let chain = Blockchain::new(genesis).unwrap();
        
        let block = chain.get_block_by_number(0);
        assert!(block.is_some());
        assert_eq!(block.unwrap().number(), 0);
    }
}