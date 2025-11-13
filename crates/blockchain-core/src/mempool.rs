// blockchain-core/src/mempool.rs

use crate::{transaction::Transaction, BlockchainError, BlockchainResult, Gas};
use blockchain_crypto::{Address, Hash};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, BTreeMap};

/// Transaction pool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    /// Maximum number of transactions in pool
    pub max_size: usize,
    /// Maximum transactions per account
    pub max_per_account: usize,
    /// Minimum gas price to accept
    pub min_gas_price: u64,
    /// Maximum transaction age in seconds
    pub max_age: u64,
    /// Enable replacement by fee
    pub enable_replacement: bool,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_size: 10_000,
            max_per_account: 100,
            min_gas_price: 1,
            max_age: 3_600, // 1 hour
            enable_replacement: true,
        }
    }
}

/// Pool metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PoolMetrics {
    pub total_transactions: usize,
    pub pending_count: usize,
    pub queued_count: usize,
    pub total_added: u64,
    pub total_removed: u64,
    pub total_replaced: u64,
}

/// Transaction status in pool
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxStatus {
    /// Ready to be included in next block
    Pending,
    /// Waiting for nonce gap to be filled
    Queued,
    /// Invalid or rejected
    Rejected,
}

/// Transaction pool entry
#[derive(Debug, Clone)]
struct PoolEntry {
    transaction: Transaction,
    added_at: crate::Timestamp,
    status: TxStatus,
}

/// Transaction pool (mempool)
pub struct TransactionPool {
    config: PoolConfig,
    /// Pending transactions (ready for inclusion)
    pending: BTreeMap<u64, HashMap<Hash, PoolEntry>>, // gas_price -> tx
    /// Queued transactions (nonce gaps)
    queued: HashMap<Address, BTreeMap<u64, PoolEntry>>, // account -> nonce -> tx
    /// All transactions by hash
    by_hash: HashMap<Hash, Transaction>,
    /// Transaction count by sender
    by_sender: HashMap<Address, usize>,
    /// Metrics
    metrics: PoolMetrics,
}

impl TransactionPool {
    /// Create new transaction pool
    pub fn new(config: PoolConfig) -> Self {
        Self {
            config,
            pending: BTreeMap::new(),
            queued: HashMap::new(),
            by_hash: HashMap::new(),
            by_sender: HashMap::new(),
            metrics: PoolMetrics::default(),
        }
    }

    /// Add transaction to pool
    pub fn add(&mut self, tx: Transaction, current_nonce: u64) -> BlockchainResult<()> {
        // Validate transaction
        tx.validate_basic()?;

        // Check if already exists
        let tx_hash = tx.hash();
        if self.by_hash.contains_key(&tx_hash) {
            return Err(BlockchainError::DuplicateTransaction(tx_hash));
        }

        // Check pool size limit
        if self.by_hash.len() >= self.config.max_size {
            // Try to evict lowest gas price pending transaction
            if !self.try_evict_lowest_gas() {
                return Err(BlockchainError::PoolFull);
            }
        }

        // Check per-account limit
        let sender_count = self.by_sender.get(&tx.from).copied().unwrap_or(0);
        if sender_count >= self.config.max_per_account {
            return Err(BlockchainError::InvalidTransaction(
                "Too many pending transactions from sender".into()
            ));
        }

        // Check minimum gas price
        if tx.gas_price < self.config.min_gas_price {
            return Err(BlockchainError::InvalidTransaction(
                format!("Gas price {} below minimum {}", tx.gas_price, self.config.min_gas_price)
            ));
        }

        // Determine status
        let status = if tx.nonce == current_nonce {
            TxStatus::Pending
        } else if tx.nonce > current_nonce {
            TxStatus::Queued
        } else {
            return Err(BlockchainError::NonceMismatch);
        };

        let entry = PoolEntry {
            transaction: tx.clone(),
            added_at: current_timestamp(),
            status,
        };

        // Add to appropriate collection
        match status {
            TxStatus::Pending => {
                self.pending.entry(tx.gas_price)
                    .or_insert_with(HashMap::new)
                    .insert(tx_hash, entry);
                self.metrics.pending_count += 1;
            }
            TxStatus::Queued => {
                self.queued.entry(tx.from)
                    .or_insert_with(BTreeMap::new)
                    .insert(tx.nonce, entry);
                self.metrics.queued_count += 1;
            }
            TxStatus::Rejected => unreachable!(),
        }

        // Update tracking
        self.by_hash.insert(tx_hash, tx.clone());
        *self.by_sender.entry(tx.from).or_insert(0) += 1;
        self.metrics.total_added += 1;
        self.metrics.total_transactions = self.by_hash.len();

        Ok(())
    }

    /// Get pending transactions for block production
    pub fn get_pending(&self, max_gas: Gas, max_count: usize) -> Vec<Transaction> {
        let mut transactions = Vec::new();
        let mut total_gas = 0u64;

        // Iterate from highest to lowest gas price
        for (_, tx_map) in self.pending.iter().rev() {
            for entry in tx_map.values() {
                if total_gas + entry.transaction.gas_limit > max_gas {
                    continue;
                }

                if transactions.len() >= max_count {
                    return transactions;
                }

                total_gas += entry.transaction.gas_limit;
                transactions.push(entry.transaction.clone());
            }
        }

        transactions
    }

    /// Remove transaction from pool
    pub fn remove(&mut self, tx_hash: &Hash) -> Option<Transaction> {
        let tx = self.by_hash.remove(tx_hash)?;
        
        // Update sender count
        if let Some(count) = self.by_sender.get_mut(&tx.from) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.by_sender.remove(&tx.from);
            }
        }

        // Remove from pending or queued
        self.remove_from_pending(tx_hash, tx.gas_price);
        self.remove_from_queued(&tx.from, tx.nonce);

        self.metrics.total_removed += 1;
        self.metrics.total_transactions = self.by_hash.len();

        Some(tx)
    }

    /// Remove transactions that were included in a block
    pub fn remove_included(&mut self, transactions: &[Transaction]) {
        for tx in transactions {
            self.remove(&tx.hash());
        }

        // Try to promote queued transactions
        self.promote_queued();
    }

    /// Promote queued transactions when nonce gaps are filled
    fn promote_queued(&mut self) {
        let mut to_promote = Vec::new();

        for (sender, nonce_map) in &self.queued {
            if let Some((nonce, entry)) = nonce_map.iter().next() {
                // Check if this can be promoted (would need current nonce from state)
                to_promote.push((*sender, *nonce));
            }
        }

        for (sender, nonce) in to_promote {
            if let Some(nonce_map) = self.queued.get_mut(&sender) {
                if let Some(entry) = nonce_map.remove(&nonce) {
                    let tx_hash = entry.transaction.hash();
                    let gas_price = entry.transaction.gas_price;

                    self.pending.entry(gas_price)
                        .or_insert_with(HashMap::new)
                        .insert(tx_hash, PoolEntry {
                            transaction: entry.transaction,
                            added_at: entry.added_at,
                            status: TxStatus::Pending,
                        });

                    self.metrics.pending_count += 1;
                    self.metrics.queued_count = self.metrics.queued_count.saturating_sub(1);
                }
            }
        }
    }

    /// Prune old transactions
    pub fn prune(&mut self) {
        let now = current_timestamp();
        let max_age = self.config.max_age;
        let mut to_remove = Vec::new();

        // Find old pending transactions
        for tx_map in self.pending.values() {
            for (hash, entry) in tx_map {
                if now.saturating_sub(entry.added_at) > max_age {
                    to_remove.push(*hash);
                }
            }
        }

        // Find old queued transactions
        for nonce_map in self.queued.values() {
            for entry in nonce_map.values() {
                if now.saturating_sub(entry.added_at) > max_age {
                    to_remove.push(entry.transaction.hash());
                }
            }
        }

        // Remove old transactions
        for hash in to_remove {
            self.remove(&hash);
        }
    }

    /// Get transaction by hash
    pub fn get(&self, hash: &Hash) -> Option<&Transaction> {
        self.by_hash.get(hash)
    }

    /// Get all transactions from sender
    pub fn get_by_sender(&self, sender: &Address) -> Vec<Transaction> {
        self.by_hash.values()
            .filter(|tx| tx.from == *sender)
            .cloned()
            .collect()
    }

    /// Get pool metrics
    pub fn metrics(&self) -> &PoolMetrics {
        &self.metrics
    }

    /// Get pending count
    pub fn pending_count(&self) -> usize {
        self.metrics.pending_count
    }

    /// Get queued count
    pub fn queued_count(&self) -> usize {
        self.metrics.queued_count
    }

    /// Clear all transactions
    pub fn clear(&mut self) {
        self.pending.clear();
        self.queued.clear();
        self.by_hash.clear();
        self.by_sender.clear();
        self.metrics = PoolMetrics::default();
    }

    // Helper methods

    fn remove_from_pending(&mut self, tx_hash: &Hash, gas_price: u64) {
        if let Some(tx_map) = self.pending.get_mut(&gas_price) {
            if tx_map.remove(tx_hash).is_some() {
                self.metrics.pending_count = self.metrics.pending_count.saturating_sub(1);
            }
            if tx_map.is_empty() {
                self.pending.remove(&gas_price);
            }
        }
    }

    fn remove_from_queued(&mut self, sender: &Address, nonce: u64) {
        if let Some(nonce_map) = self.queued.get_mut(sender) {
            if nonce_map.remove(&nonce).is_some() {
                self.metrics.queued_count = self.metrics.queued_count.saturating_sub(1);
            }
            if nonce_map.is_empty() {
                self.queued.remove(sender);
            }
        }
    }

    fn try_evict_lowest_gas(&mut self) -> bool {
        if let Some((&lowest_price, _)) = self.pending.iter().next() {
            if let Some(tx_map) = self.pending.get(&lowest_price) {
                if let Some(hash) = tx_map.keys().next().copied() {
                    self.remove(&hash);
                    return true;
                }
            }
        }
        false
    }
}

fn current_timestamp() -> crate::Timestamp {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use blockchain_crypto::{KeyPair, SignatureScheme};

    fn create_test_tx(nonce: u64, gas_price: u64) -> Transaction {
        let keypair = KeyPair::generate(SignatureScheme::Ed25519).unwrap();
        Transaction::new(
            keypair.public_key().to_address(),
            nonce,
            crate::TransactionType::Transfer {
                to: Address::zero(),
                amount: crate::Amount::from_u64(100),
            },
            gas_price,
            21000,
        )
    }

    #[test]
    fn test_pool_add_pending() {
        let mut pool = TransactionPool::new(PoolConfig::default());
        let tx = create_test_tx(0, 10);

        pool.add(tx, 0).unwrap();
        assert_eq!(pool.pending_count(), 1);
        assert_eq!(pool.queued_count(), 0);
    }

    #[test]
    fn test_pool_add_queued() {
        let mut pool = TransactionPool::new(PoolConfig::default());
        let tx = create_test_tx(5, 10);

        pool.add(tx, 0).unwrap();
        assert_eq!(pool.pending_count(), 0);
        assert_eq!(pool.queued_count(), 1);
    }

    #[test]
    fn test_get_pending_by_gas_price() {
        let mut pool = TransactionPool::new(PoolConfig::default());
        
        let tx1 = create_test_tx(0, 5);
        let tx2 = create_test_tx(1, 10);
        let tx3 = create_test_tx(2, 15);

        pool.add(tx1, 0).unwrap();
        pool.add(tx2, 1).unwrap();
        pool.add(tx3, 2).unwrap();

        let pending = pool.get_pending(100000, 10);
        
        // Should be ordered by gas price (highest first)
        assert_eq!(pending.len(), 3);
        assert!(pending[0].gas_price >= pending[1].gas_price);
        assert!(pending[1].gas_price >= pending[2].gas_price);
    }
}