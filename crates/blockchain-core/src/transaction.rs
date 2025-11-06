// blockchain-core/src/transaction.rs

use crate::{types::*, BlockchainError, BlockchainResult};
use blockchain_crypto::{hash::Hashable, Address, Hash, PublicKey, Signature};
use serde::{Deserialize, Serialize};

/// Transaction types supported by the protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionType {
    /// Standard token transfer
    Transfer {
        to: Address,
        amount: Amount,
    },
    /// Validator staking
    Stake {
        amount: StakeAmount,
    },
    /// Unstake validator tokens
    Unstake {
        amount: StakeAmount,
    },
    /// Deploy liquidity to utility pools
    DeployLiquidity {
        pool_id: u64,
        amount: Amount,
    },
    /// Withdraw liquidity from pools
    WithdrawLiquidity {
        pool_id: u64,
        amount: Amount,
    },
    /// Smart contract deployment
    ContractDeployment {
        bytecode: Vec<u8>,
        constructor_args: Vec<u8>,
    },
    /// Smart contract call
    ContractCall {
        contract: Address,
        data: Vec<u8>,
    },
}

/// Complete transaction structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    /// Sender's address
    pub from: Address,
    /// Transaction nonce (prevents replay attacks)
    pub nonce: Nonce,
    /// Type of transaction
    pub tx_type: TransactionType,
    /// Gas price willing to pay
    pub gas_price: GasPrice,
    /// Maximum gas to consume
    pub gas_limit: Gas,
    /// Transaction timestamp
    pub timestamp: Timestamp,
    /// Digital signature
    pub signature: Option<Signature>,
}

impl Transaction {
    /// Create a new unsigned transaction
    pub fn new(
        from: Address,
        nonce: Nonce,
        tx_type: TransactionType,
        gas_price: GasPrice,
        gas_limit: Gas,
    ) -> Self {
        Self {
            from,
            nonce,
            tx_type,
            gas_price,
            gas_limit,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            signature: None,
        }
    }

    /// Sign the transaction
    pub fn sign(&mut self, keypair: &blockchain_crypto::KeyPair) -> BlockchainResult<()> {
        let hash = self.hash_for_signing();
        let signature = keypair.sign(hash.as_bytes())?;
        self.signature = Some(signature);
        Ok(())
    }

    /// Verify transaction signature
    pub fn verify_signature(&self, public_key: &PublicKey) -> BlockchainResult<bool> {
        let signature = self.signature.as_ref()
            .ok_or(BlockchainError::InvalidTransaction("Missing signature".into()))?;
        
        let hash = self.hash_for_signing();
        Ok(public_key.verify(hash.as_bytes(), signature)?)
    }

    /// Calculate transaction hash
    pub fn hash(&self) -> Hash {
        let bytes = bincode::serialize(self).unwrap();
        bytes.hash()
    }

    /// Hash used for signing (excludes signature field)
    fn hash_for_signing(&self) -> Hash {
        let mut tx_copy = self.clone();
        tx_copy.signature = None;
        let bytes = bincode::serialize(&tx_copy).unwrap();
        bytes.hash()
    }

    /// Calculate transaction fee
    pub fn calculate_fee(&self, gas_used: Gas) -> Amount {
        Amount::from_u64(gas_used * self.gas_price)
    }

    /// Validate basic transaction properties
    pub fn validate_basic(&self) -> BlockchainResult<()> {
        // Check signature exists
        if self.signature.is_none() {
            return Err(BlockchainError::InvalidTransaction("Missing signature".into()));
        }

        // Check gas limit is reasonable
        if self.gas_limit == 0 {
            return Err(BlockchainError::InvalidTransaction("Gas limit cannot be zero".into()));
        }

        // Check gas price is reasonable
        if self.gas_price == 0 {
            return Err(BlockchainError::InvalidTransaction("Gas price cannot be zero".into()));
        }

        // Validate transaction type specifics
        match &self.tx_type {
            TransactionType::Transfer { amount, .. } => {
                if amount.is_zero() {
                    return Err(BlockchainError::InvalidTransaction("Transfer amount cannot be zero".into()));
                }
            }
            TransactionType::Stake { amount } => {
                if amount.is_zero() {
                    return Err(BlockchainError::InvalidTransaction("Stake amount cannot be zero".into()));
                }
            }
            TransactionType::Unstake { amount } => {
                if amount.is_zero() {
                    return Err(BlockchainError::InvalidTransaction("Unstake amount cannot be zero".into()));
                }
            }
            TransactionType::DeployLiquidity { amount, .. } => {
                if amount.is_zero() {
                    return Err(BlockchainError::InvalidTransaction("Liquidity amount cannot be zero".into()));
                }
            }
            TransactionType::WithdrawLiquidity { amount, .. } => {
                if amount.is_zero() {
                    return Err(BlockchainError::InvalidTransaction("Withdrawal amount cannot be zero".into()));
                }
            }
            TransactionType::ContractDeployment { bytecode, .. } => {
                if bytecode.is_empty() {
                    return Err(BlockchainError::InvalidTransaction("Contract bytecode cannot be empty".into()));
                }
            }
            TransactionType::ContractCall { data, .. } => {
                if data.is_empty() {
                    return Err(BlockchainError::InvalidTransaction("Contract call data cannot be empty".into()));
                }
            }
        }

        Ok(())
    }

    /// Get the recipient address (if applicable)
    pub fn recipient(&self) -> Option<Address> {
        match &self.tx_type {
            TransactionType::Transfer { to, .. } => Some(*to),
            TransactionType::ContractCall { contract, .. } => Some(*contract),
            _ => None,
        }
    }

    /// Get transaction value (if applicable)
    pub fn value(&self) -> Amount {
        match &self.tx_type {
            TransactionType::Transfer { amount, .. } => amount.clone(),
            TransactionType::Stake { amount } => amount.clone(),
            TransactionType::Unstake { amount } => amount.clone(),
            TransactionType::DeployLiquidity { amount, .. } => amount.clone(),
            TransactionType::WithdrawLiquidity { amount, .. } => amount.clone(),
            _ => Amount::zero(),
        }
    }
}

/// Transaction receipt after execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionReceipt {
    /// Transaction hash
    pub tx_hash: Hash,
    /// Block number where included
    pub block_number: BlockNumber,
    /// Sender address
    pub from: Address,
    /// Recipient address (if applicable)
    pub to: Option<Address>,
    /// Gas used
    pub gas_used: Gas,
    /// Execution status
    pub status: ExecutionStatus,
    /// Contract address (if deployment)
    pub contract_address: Option<Address>,
    /// Logs generated
    pub logs: Vec<Log>,
}

/// Execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionStatus {
    Success,
    Failed,
    Reverted,
}

/// Event log emitted during execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Log {
    pub address: Address,
    pub topics: Vec<Hash>,
    pub data: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use blockchain_crypto::{KeyPair, SignatureScheme};

    #[test]
    fn test_transaction_creation() {
        let from = Address::zero();
        let to = Address::zero();
        let tx = Transaction::new(
            from,
            1,
            TransactionType::Transfer {
                to,
                amount: Amount::from_u64(100),
            },
            10,
            21000,
        );
        
        assert_eq!(tx.nonce, 1);
        assert_eq!(tx.gas_limit, 21000);
    }

    #[test]
    fn test_transaction_signing() {
        let keypair = KeyPair::generate(SignatureScheme::Ed25519).unwrap();
        let from = keypair.public_key().to_address();
        
        let mut tx = Transaction::new(
            from,
            1,
            TransactionType::Transfer {
                to: Address::zero(),
                amount: Amount::from_u64(100),
            },
            10,
            21000,
        );
        
        tx.sign(&keypair).unwrap();
        assert!(tx.signature.is_some());
        assert!(tx.verify_signature(keypair.public_key()).unwrap());
    }

    #[test]
    fn test_transaction_validation() {
        let keypair = KeyPair::generate(SignatureScheme::Ed25519).unwrap();
        let from = keypair.public_key().to_address();
        
        let mut tx = Transaction::new(
            from,
            1,
            TransactionType::Transfer {
                to: Address::zero(),
                amount: Amount::from_u64(100),
            },
            10,
            21000,
        );
        
        // Should fail without signature
        assert!(tx.validate_basic().is_err());
        
        // Should pass with signature
        tx.sign(&keypair).unwrap();
        assert!(tx.validate_basic().is_ok());
    }
}