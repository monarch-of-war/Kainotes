// blockchain-core/src/block.rs
use crate::{types::*, transaction::Transaction, BlockchainError, BlockchainResult};
use blockchain_crypto::{hash::Hashable, Address, Hash, MerkleTree};
use serde::{Deserialize, Serialize};

/// Block header containing metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    /// Block number/height
    pub number: BlockNumber,
    /// Hash of previous block
    pub parent_hash: Hash,
    /// Merkle root of transactions
    pub transactions_root: Hash,
    /// State root (world state hash)
    pub state_root: Hash,
    /// Block timestamp
    pub timestamp: Timestamp,
    /// Block proposer (validator)
    pub proposer: Address,
    /// Gas limit for this block
    pub gas_limit: Gas,
    /// Gas used by all transactions
    pub gas_used: Gas,
    /// Extra data (can include validator signatures, etc.)
    pub extra_data: Vec<u8>,
}

impl BlockHeader {
    /// Calculate header hash
    pub fn hash(&self) -> Hash {
        let bytes = bincode::serialize(self).unwrap();
        bytes.hash()
    }

    /// Validate header basic properties
    pub fn validate(&self, parent: &BlockHeader) -> BlockchainResult<()> {
        // Check block number is sequential
        if self.number != parent.number + 1 {
            return Err(BlockchainError::InvalidBlock(
                format!("Invalid block number: expected {}, got {}", 
                    parent.number + 1, self.number)
            ));
        }

        // Check parent hash matches
        if self.parent_hash != parent.hash() {
            return Err(BlockchainError::InvalidBlock(
                "Parent hash mismatch".into()
            ));
        }

        // Check timestamp is after parent
        if self.timestamp <= parent.timestamp {
            return Err(BlockchainError::InvalidBlock(
                "Block timestamp must be after parent".into()
            ));
        }

        // Check gas used doesn't exceed limit
        if self.gas_used > self.gas_limit {
            return Err(BlockchainError::InvalidBlock(
                "Gas used exceeds gas limit".into()
            ));
        }

        Ok(())
    }
}

/// Complete block structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// Block header
    pub header: BlockHeader,
    /// List of transactions
    pub transactions: Vec<Transaction>,
    /// Validator signatures (for PoAS consensus)
    pub validator_signatures: Vec<ValidatorSignature>,
}

impl Block {
    /// Create a new block
    pub fn new(
        number: BlockNumber,
        parent_hash: Hash,
        state_root: Hash,
        proposer: Address,
        transactions: Vec<Transaction>,
        gas_limit: Gas,
    ) -> BlockchainResult<Self> {
        // Calculate transactions root
        let tx_hashes: Vec<Hash> = transactions.iter().map(|tx| tx.hash()).collect();
        let transactions_root = if tx_hashes.is_empty() {
            Hash::zero()
        } else {
            MerkleTree::new(&tx_hashes.iter().map(|h| h.as_bytes()).collect::<Vec<_>>())?
                .root()
        };

        // Calculate total gas used
        let gas_used = transactions.iter()
            .map(|tx| tx.gas_limit) // In real implementation, this would be actual gas used
            .sum();

        let header = BlockHeader {
            number,
            parent_hash,
            transactions_root,
            state_root,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            proposer,
            gas_limit,
            gas_used,
            extra_data: Vec::new(),
        };

        Ok(Self {
            header,
            transactions,
            validator_signatures: Vec::new(),
        })
    }

    /// Get block hash
    pub fn hash(&self) -> Hash {
        self.header.hash()
    }

    /// Get block number
    pub fn number(&self) -> BlockNumber {
        self.header.number
    }

    /// Validate block structure and content
    pub fn validate(&self, parent: &Block) -> BlockchainResult<()> {
        // Validate header
        self.header.validate(&parent.header)?;

        // Validate transactions
        for tx in &self.transactions {
            tx.validate_basic()?;
        }

        // Verify transactions merkle root
        let tx_hashes: Vec<Hash> = self.transactions.iter().map(|tx| tx.hash()).collect();
        if !tx_hashes.is_empty() {
            let computed_root = MerkleTree::new(&tx_hashes.iter().map(|h| h.as_bytes()).collect::<Vec<_>>())?
                .root();
            if computed_root != self.header.transactions_root {
                return Err(BlockchainError::InvalidBlock(
                    "Transactions merkle root mismatch".into()
                ));
            }
        }

        // Verify gas calculations
        let total_gas: Gas = self.transactions.iter().map(|tx| tx.gas_limit).sum();
        if total_gas != self.header.gas_used {
            return Err(BlockchainError::InvalidBlock(
                "Gas used calculation mismatch".into()
            ));
        }

        Ok(())
    }

    /// Create genesis block
    pub fn genesis(genesis_state_root: Hash) -> Self {
        let header = BlockHeader {
            number: 0,
            parent_hash: Hash::zero(),
            transactions_root: Hash::zero(),
            state_root: genesis_state_root,
            timestamp: 0,
            proposer: Address::zero(),
            gas_limit: 10_000_000,
            gas_used: 0,
            extra_data: b"Genesis Block".to_vec(),
        };

        Self {
            header,
            transactions: Vec::new(),
            validator_signatures: Vec::new(),
        }
    }

    /// Check if this is the genesis block
    pub fn is_genesis(&self) -> bool {
        self.header.number == 0 && self.header.parent_hash == Hash::zero()
    }
}

/// Validator signature for consensus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorSignature {
    pub validator: Address,
    pub signature: blockchain_crypto::Signature,
    pub timestamp: Timestamp,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genesis_block() {
        let genesis = Block::genesis(Hash::zero());
        assert_eq!(genesis.number(), 0);
        assert!(genesis.is_genesis());
        assert_eq!(genesis.transactions.len(), 0);
    }

    #[test]
    fn test_block_creation() {
        let block = Block::new(
            1,
            Hash::zero(),
            Hash::zero(),
            Address::zero(),
            vec![],
            10_000_000,
        ).unwrap();
        
        assert_eq!(block.number(), 1);
        assert!(!block.is_genesis());
    }

    #[test]
    fn test_block_hash() {
        let block1 = Block::genesis(Hash::zero());
        let block2 = Block::genesis(Hash::zero());
        assert_eq!(block1.hash(), block2.hash());
    }

    #[test]
    fn test_header_validation() {
        let genesis = Block::genesis(Hash::zero());
        let block = Block::new(
            1,
            genesis.hash(),
            Hash::zero(),
            Address::zero(),
            vec![],
            10_000_000,
        ).unwrap();
        
        assert!(block.validate(&genesis).is_ok());
    }
}