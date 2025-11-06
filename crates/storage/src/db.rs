// storage/src/db.rs

use crate::{PruningMode, StorageError, StorageResult};
use blockchain_core::{Block, BlockNumber, Transaction, TransactionReceipt, WorldState};
use blockchain_crypto::{Address, Hash};
use smart_contracts::EVMState;
use consensus::validator::{ValidatorInfo, ValidatorSet};
use rocksdb::{DB, Options, WriteBatch, IteratorMode};
use std::path::Path;
use std::sync::Arc;

/// Column families for different data types
#[derive(Debug, Clone, Copy)]
pub enum ColumnFamily {
    Blocks,
    BlockHashes,
    BlockNumbers,
    Transactions,
    Receipts,
    State,
    ContractCode,
    ContractStorage,
    Validators,
    Meta,
}

impl ColumnFamily {
    fn as_str(&self) -> &'static str {
        match self {
            ColumnFamily::Blocks => "blocks",
            ColumnFamily::BlockHashes => "block_hashes",
            ColumnFamily::BlockNumbers => "block_numbers",
            ColumnFamily::Transactions => "transactions",
            ColumnFamily::Receipts => "receipts",
            ColumnFamily::State => "state",
            ColumnFamily::ContractCode => "contract_code",
            ColumnFamily::ContractStorage => "contract_storage",
            ColumnFamily::Validators => "validators",
            ColumnFamily::Meta => "meta",
        }
    }

    fn all() -> Vec<Self> {
        vec![
            Self::Blocks,
            Self::BlockHashes,
            Self::BlockNumbers,
            Self::Transactions,
            Self::Receipts,
            Self::State,
            Self::ContractCode,
            Self::ContractStorage,
            Self::Validators,
            Self::Meta,
        ]
    }
}

/// Database configuration
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub path: String,
    pub create_if_missing: bool,
    pub max_open_files: i32,
    pub cache_size: usize,
    pub write_buffer_size: usize,
    pub max_write_buffer_number: i32,
    pub pruning_mode: PruningMode,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: "./data".to_string(),
            create_if_missing: true,
            max_open_files: 1024,
            cache_size: 512 * 1024 * 1024, // 512 MB
            write_buffer_size: 64 * 1024 * 1024, // 64 MB
            max_write_buffer_number: 3,
            pruning_mode: PruningMode::Pruned { keep_blocks: 10000 },
        }
    }
}

/// Main database interface
pub struct Database {
    db: Arc<DB>,
    config: DatabaseConfig,
}

impl Database {
    /// Open or create database
    pub fn open(config: DatabaseConfig) -> StorageResult<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(config.create_if_missing);
        opts.create_missing_column_families(true);
        opts.set_max_open_files(config.max_open_files);
        opts.set_write_buffer_size(config.write_buffer_size);
        opts.set_max_write_buffer_number(config.max_write_buffer_number);
        opts.increase_parallelism(num_cpus::get() as i32);
        
        let cfs: Vec<_> = ColumnFamily::all().iter().map(|cf| cf.as_str()).collect();

        let db = DB::open_cf(&opts, &config.path, &cfs)
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        tracing::info!("Database opened at {}", config.path);

        Ok(Self {
            db: Arc::new(db),
            config,
        })
    }

    // ==================== BLOCK OPERATIONS ====================

    /// Store a block with all related data
    pub fn store_block(&self, block: &Block) -> StorageResult<()> {
        let block_hash = block.hash();
        let block_number = block.number();

        let block_bytes = bincode::serialize(block)
            .map_err(|e| StorageError::SerializationError(e.to_string()))?;

        let cf_blocks = self.cf(ColumnFamily::Blocks)?;
        let cf_hashes = self.cf(ColumnFamily::BlockHashes)?;
        let cf_numbers = self.cf(ColumnFamily::BlockNumbers)?;

        let mut batch = WriteBatch::default();
        
        // Store block by hash
        batch.put_cf(cf_blocks, block_hash.as_bytes(), &block_bytes);
        
        // Store number -> hash mapping
        batch.put_cf(cf_hashes, block_number.to_be_bytes(), block_hash.as_bytes());
        
        // Store hash -> number mapping
        batch.put_cf(cf_numbers, block_hash.as_bytes(), block_number.to_be_bytes());

        self.db.write(batch)
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        // Store transactions in this block
        for tx in &block.transactions {
            let receipt = blockchain_core::TransactionReceipt {
                tx_hash: tx.hash(),
                block_number,
                from: tx.from,
                to: tx.recipient(),
                gas_used: 21000,
                status: blockchain_core::transaction::ExecutionStatus::Success,
                contract_address: None,
                logs: vec![],
            };
            self.store_transaction(tx, &receipt)?;
        }

        tracing::debug!("Stored block #{} ({})", block_number, block_hash.to_hex());
        Ok(())
    }

    /// Get block by hash
    pub fn get_block(&self, hash: &Hash) -> StorageResult<Option<Block>> {
        let cf = self.cf(ColumnFamily::Blocks)?;

        match self.db.get_cf(cf, hash.as_bytes())
            .map_err(|e| StorageError::DatabaseError(e.to_string()))? {
            Some(bytes) => {
                let block = bincode::deserialize(&bytes)
                    .map_err(|e| StorageError::SerializationError(e.to_string()))?;
                Ok(Some(block))
            }
            None => Ok(None),
        }
    }

    /// Get block by number
    pub fn get_block_by_number(&self, number: BlockNumber) -> StorageResult<Option<Block>> {
        let cf_hashes = self.cf(ColumnFamily::BlockHashes)?;

        let hash_bytes = match self.db.get_cf(cf_hashes, number.to_be_bytes())
            .map_err(|e| StorageError::DatabaseError(e.to_string()))? {
            Some(bytes) => bytes,
            None => return Ok(None),
        };

        let hash = Hash::from_slice(&hash_bytes)
            .map_err(|_| StorageError::DatabaseError("Invalid hash".into()))?;

        self.get_block(&hash)
    }

    /// Get block number by hash
    pub fn get_block_number(&self, hash: &Hash) -> StorageResult<Option<BlockNumber>> {
        let cf = self.cf(ColumnFamily::BlockNumbers)?;

        match self.db.get_cf(cf, hash.as_bytes())
            .map_err(|e| StorageError::DatabaseError(e.to_string()))? {
            Some(bytes) => {
                let number = u64::from_be_bytes(bytes.try_into()
                    .map_err(|_| StorageError::Corruption("Invalid block number".into()))?);
                Ok(Some(number))
            }
            None => Ok(None),
        }
    }

    // ==================== TRANSACTION OPERATIONS ====================

    /// Store transaction with receipt
    pub fn store_transaction(&self, tx: &Transaction, receipt: &TransactionReceipt) -> StorageResult<()> {
        let tx_hash = tx.hash();

        let tx_bytes = bincode::serialize(tx)
            .map_err(|e| StorageError::SerializationError(e.to_string()))?;
        let receipt_bytes = bincode::serialize(receipt)
            .map_err(|e| StorageError::SerializationError(e.to_string()))?;

        let cf_tx = self.cf(ColumnFamily::Transactions)?;
        let cf_receipts = self.cf(ColumnFamily::Receipts)?;

        let mut batch = WriteBatch::default();
        batch.put_cf(cf_tx, tx_hash.as_bytes(), &tx_bytes);
        batch.put_cf(cf_receipts, tx_hash.as_bytes(), &receipt_bytes);

        self.db.write(batch)
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    /// Get transaction
    pub fn get_transaction(&self, hash: &Hash) -> StorageResult<Option<Transaction>> {
        let cf = self.cf(ColumnFamily::Transactions)?;

        match self.db.get_cf(cf, hash.as_bytes())
            .map_err(|e| StorageError::DatabaseError(e.to_string()))? {
            Some(bytes) => {
                let tx = bincode::deserialize(&bytes)
                    .map_err(|e| StorageError::SerializationError(e.to_string()))?;
                Ok(Some(tx))
            }
            None => Ok(None),
        }
    }

    /// Get transaction receipt
    pub fn get_receipt(&self, hash: &Hash) -> StorageResult<Option<TransactionReceipt>> {
        let cf = self.cf(ColumnFamily::Receipts)?;

        match self.db.get_cf(cf, hash.as_bytes())
            .map_err(|e| StorageError::DatabaseError(e.to_string()))? {
            Some(bytes) => {
                let receipt = bincode::deserialize(&bytes)
                    .map_err(|e| StorageError::SerializationError(e.to_string()))?;
                Ok(Some(receipt))
            }
            None => Ok(None),
        }
    }

    // ==================== STATE OPERATIONS ====================

    /// Store world state at specific block
    pub fn store_state(&self, block_number: BlockNumber, state: &WorldState) -> StorageResult<()> {
        let state_bytes = bincode::serialize(state)
            .map_err(|e| StorageError::SerializationError(e.to_string()))?;

        let cf = self.cf(ColumnFamily::State)?;
        self.db.put_cf(cf, block_number.to_be_bytes(), state_bytes)
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    /// Get world state at specific block
    pub fn get_state(&self, block_number: BlockNumber) -> StorageResult<Option<WorldState>> {
        let cf = self.cf(ColumnFamily::State)?;

        match self.db.get_cf(cf, block_number.to_be_bytes())
            .map_err(|e| StorageError::DatabaseError(e.to_string()))? {
            Some(bytes) => {
                let state = bincode::deserialize(&bytes)
                    .map_err(|e| StorageError::SerializationError(e.to_string()))?;
                Ok(Some(state))
            }
            None => Ok(None),
        }
    }

    // ==================== CONTRACT OPERATIONS ====================

    /// Store contract code
    pub fn store_contract_code(&self, address: &Address, code: &[u8]) -> StorageResult<()> {
        let cf = self.cf(ColumnFamily::ContractCode)?;
        self.db.put_cf(cf, address.as_bytes(), code)
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    /// Get contract code
    pub fn get_contract_code(&self, address: &Address) -> StorageResult<Option<Vec<u8>>> {
        let cf = self.cf(ColumnFamily::ContractCode)?;
        self.db.get_cf(cf, address.as_bytes())
            .map_err(|e| StorageError::DatabaseError(e.to_string()))
    }

    /// Store contract storage slot
    pub fn store_contract_storage(&self, address: &Address, slot: &[u8; 32], value: &[u8; 32]) -> StorageResult<()> {
        let mut key = Vec::with_capacity(52);
        key.extend_from_slice(address.as_bytes());
        key.extend_from_slice(slot);

        let cf = self.cf(ColumnFamily::ContractStorage)?;
        self.db.put_cf(cf, &key, value)
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    /// Get contract storage slot
    pub fn get_contract_storage(&self, address: &Address, slot: &[u8; 32]) -> StorageResult<[u8; 32]> {
        let mut key = Vec::with_capacity(52);
        key.extend_from_slice(address.as_bytes());
        key.extend_from_slice(slot);

        let cf = self.cf(ColumnFamily::ContractStorage)?;
        match self.db.get_cf(cf, &key)
            .map_err(|e| StorageError::DatabaseError(e.to_string()))? {
            Some(bytes) => {
                let mut result = [0u8; 32];
                result.copy_from_slice(&bytes);
                Ok(result)
            }
            None => Ok([0u8; 32]),
        }
    }

    // ==================== VALIDATOR OPERATIONS ====================

    /// Store validator set
    pub fn store_validator_set(&self, validators: &ValidatorSet) -> StorageResult<()> {
        let bytes = bincode::serialize(validators)
            .map_err(|e| StorageError::SerializationError(e.to_string()))?;

        let cf = self.cf(ColumnFamily::Validators)?;
        self.db.put_cf(cf, b"current", bytes)
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    /// Get validator set
    pub fn get_validator_set(&self) -> StorageResult<Option<ValidatorSet>> {
        let cf = self.cf(ColumnFamily::Validators)?;

        match self.db.get_cf(cf, b"current")
            .map_err(|e| StorageError::DatabaseError(e.to_string()))? {
            Some(bytes) => {
                let validators = bincode::deserialize(&bytes)
                    .map_err(|e| StorageError::SerializationError(e.to_string()))?;
                Ok(Some(validators))
            }
            None => Ok(None),
        }
    }

    // ==================== METADATA OPERATIONS ====================

    /// Store metadata
    pub fn store_meta(&self, key: &str, value: &[u8]) -> StorageResult<()> {
        let cf = self.cf(ColumnFamily::Meta)?;
        self.db.put_cf(cf, key.as_bytes(), value)
            .map_err(|e| StorageError::DatabaseError(e.to_string()))
    }

    /// Get metadata
    pub fn get_meta(&self, key: &str) -> StorageResult<Option<Vec<u8>>> {
        let cf = self.cf(ColumnFamily::Meta)?;
        self.db.get_cf(cf, key.as_bytes())
            .map_err(|e| StorageError::DatabaseError(e.to_string()))
    }

    /// Get latest block number
    pub fn get_latest_block_number(&self) -> StorageResult<Option<BlockNumber>> {
        match self.get_meta("latest_block_number")? {
            Some(bytes) => {
                let number = u64::from_be_bytes(bytes.try_into()
                    .map_err(|_| StorageError::Corruption("Invalid block number".into()))?);
                Ok(Some(number))
            }
            None => Ok(None),
        }
    }

    /// Update latest block number
    pub fn update_latest_block_number(&self, number: BlockNumber) -> StorageResult<()> {
        self.store_meta("latest_block_number", &number.to_be_bytes())
    }

    // ==================== PRUNING OPERATIONS ====================

    /// Prune old data based on configuration
    pub fn prune(&self, current_block: BlockNumber) -> StorageResult<u64> {
        let keep_from = match self.config.pruning_mode {
            PruningMode::Archive => {
                tracing::debug!("Archive mode: skipping pruning");
                return Ok(0);
            }
            PruningMode::Pruned { keep_blocks } => {
                current_block.saturating_sub(keep_blocks)
            }
        };

        tracing::info!("Pruning data before block #{}", keep_from);

        let mut pruned_count = 0u64;

        // Prune old state snapshots
        let cf_state = self.cf(ColumnFamily::State)?;
        for i in 0..keep_from {
            if self.db.delete_cf(cf_state, i.to_be_bytes()).is_ok() {
                pruned_count += 1;
            }
        }

        tracing::info!("Pruned {} state snapshots", pruned_count);
        Ok(pruned_count)
    }

    // ==================== UTILITY OPERATIONS ====================

    /// Compact database
    pub fn compact(&self) -> StorageResult<()> {
        tracing::info!("Compacting database...");
        
        for cf_type in ColumnFamily::all() {
            if let Ok(cf) = self.cf(cf_type) {
                self.db.compact_range_cf(cf, None::<&[u8]>, None::<&[u8]>);
            }
        }

        tracing::info!("Database compaction complete");
        Ok(())
    }

    /// Get database statistics
    pub fn stats(&self) -> StorageResult<DatabaseStats> {
        let latest_block = self.get_latest_block_number()?.unwrap_or(0);
        
        // Count transactions
        let cf_tx = self.cf(ColumnFamily::Transactions)?;
        let tx_count = self.db.iterator_cf(cf_tx, IteratorMode::Start).count();

        // Estimate size
        let mut total_size = 0u64;
        if let Ok(metadata) = std::fs::metadata(&self.config.path) {
            total_size = metadata.len();
        }

        Ok(DatabaseStats {
            latest_block,
            total_blocks: latest_block + 1,
            total_transactions: tx_count as u64,
            total_size_bytes: total_size,
            pruning_mode: self.config.pruning_mode,
        })
    }

    /// Get column family handle
    fn cf(&self, cf_type: ColumnFamily) -> StorageResult<&rocksdb::ColumnFamily> {
        self.db.cf_handle(cf_type.as_str())
            .ok_or_else(|| StorageError::DatabaseError(format!("{} CF not found", cf_type.as_str())))
    }
}

/// Database statistics
#[derive(Debug, Clone)]
pub struct DatabaseStats {
    pub latest_block: BlockNumber,
    pub total_blocks: u64,
    pub total_transactions: u64,
    pub total_size_bytes: u64,
    pub pruning_mode: PruningMode,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_db() -> (Database, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let config = DatabaseConfig {
            path: temp_dir.path().to_str().unwrap().to_string(),
            ..Default::default()
        };
        let db = Database::open(config).unwrap();
        (db, temp_dir)
    }

    #[test]
    fn test_store_retrieve_block() {
        let (db, _temp) = create_test_db();
        
        let block = Block::genesis(Hash::zero());
        db.store_block(&block).unwrap();

        let retrieved = db.get_block(&block.hash()).unwrap().unwrap();
        assert_eq!(retrieved.hash(), block.hash());
    }

    #[test]
    fn test_block_by_number() {
        let (db, _temp) = create_test_db();
        
        let block = Block::genesis(Hash::zero());
        db.store_block(&block).unwrap();

        let retrieved = db.get_block_by_number(0).unwrap().unwrap();
        assert_eq!(retrieved.number(), 0);
    }

    #[test]
    fn test_state_storage() {
        let (db, _temp) = create_test_db();
        
        let state = WorldState::new();
        db.store_state(100, &state).unwrap();

        let retrieved = db.get_state(100).unwrap();
        assert!(retrieved.is_some());
    }
}