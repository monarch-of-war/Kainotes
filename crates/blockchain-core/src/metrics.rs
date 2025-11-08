// blockchain-core/src/metrics.rs

use crate::{Block, BlockNumber, Amount, Gas};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Chain metrics and statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainMetrics {
    /// Current block height
    pub height: BlockNumber,
    /// Total transactions processed
    pub total_transactions: u64,
    /// Total gas used
    pub total_gas_used: u64,
    /// Average block time (seconds)
    pub avg_block_time: f64,
    /// Average gas price
    pub avg_gas_price: f64,
    /// Transactions per second (TPS)
    pub tps: f64,
    /// Average transactions per block
    pub avg_tx_per_block: f64,
    /// Block size statistics
    pub block_size_stats: SizeStats,
    /// Gas usage statistics
    pub gas_stats: GasStats,
}

impl ChainMetrics {
    /// Create new metrics
    pub fn new() -> Self {
        Self {
            height: 0,
            total_transactions: 0,
            total_gas_used: 0,
            avg_block_time: 0.0,
            avg_gas_price: 0.0,
            tps: 0.0,
            avg_tx_per_block: 0.0,
            block_size_stats: SizeStats::default(),
            gas_stats: GasStats::default(),
        }
    }
}

impl Default for ChainMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Size statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SizeStats {
    pub min: usize,
    pub max: usize,
    pub avg: f64,
}

/// Gas usage statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GasStats {
    pub total_used: u64,
    pub total_limit: u64,
    pub avg_usage: f64,
    pub utilization_rate: f64,
}

/// Metrics calculator
pub struct MetricsCalculator {
    /// Recent blocks for rolling statistics
    recent_blocks: VecDeque<BlockStats>,
    /// Window size for rolling averages
    window_size: usize,
    /// Cumulative metrics
    cumulative: ChainMetrics,
}

impl MetricsCalculator {
    /// Create new metrics calculator
    pub fn new(window_size: usize) -> Self {
        Self {
            recent_blocks: VecDeque::with_capacity(window_size),
            window_size,
            cumulative: ChainMetrics::new(),
        }
    }

    /// Update metrics with new block
    pub fn update(&mut self, block: &Block, prev_timestamp: u64) {
        let block_stats = BlockStats::from_block(block, prev_timestamp);
        
        // Update cumulative metrics
        self.cumulative.height = block.number();
        self.cumulative.total_transactions += block.transactions.len() as u64;
        self.cumulative.total_gas_used += block.header.gas_used;

        // Add to recent blocks
        self.recent_blocks.push_back(block_stats);
        if self.recent_blocks.len() > self.window_size {
            self.recent_blocks.pop_front();
        }

        // Recalculate rolling averages
        self.calculate_averages();
    }

    /// Calculate rolling averages
    fn calculate_averages(&mut self) {
        if self.recent_blocks.is_empty() {
            return;
        }

        let count = self.recent_blocks.len() as f64;

        // Average block time
        let total_block_time: f64 = self.recent_blocks.iter()
            .map(|b| b.block_time)
            .sum();
        self.cumulative.avg_block_time = total_block_time / count;

        // Average gas price
        let total_gas_price: u64 = self.recent_blocks.iter()
            .map(|b| b.avg_gas_price)
            .sum();
        self.cumulative.avg_gas_price = total_gas_price as f64 / count;

        // Average transactions per block
        let total_txs: usize = self.recent_blocks.iter()
            .map(|b| b.tx_count)
            .sum();
        self.cumulative.avg_tx_per_block = total_txs as f64 / count;

        // TPS calculation
        if self.cumulative.avg_block_time > 0.0 {
            self.cumulative.tps = self.cumulative.avg_tx_per_block / self.cumulative.avg_block_time;
        }

        // Block size stats
        let sizes: Vec<usize> = self.recent_blocks.iter()
            .map(|b| b.size)
            .collect();
        if !sizes.is_empty() {
            self.cumulative.block_size_stats = SizeStats {
                min: *sizes.iter().min().unwrap(),
                max: *sizes.iter().max().unwrap(),
                avg: sizes.iter().sum::<usize>() as f64 / sizes.len() as f64,
            };
        }

        // Gas stats
        let total_gas_used: u64 = self.recent_blocks.iter()
            .map(|b| b.gas_used)
            .sum();
        let total_gas_limit: u64 = self.recent_blocks.iter()
            .map(|b| b.gas_limit)
            .sum();
        
        self.cumulative.gas_stats = GasStats {
            total_used: total_gas_used,
            total_limit: total_gas_limit,
            avg_usage: total_gas_used as f64 / count,
            utilization_rate: if total_gas_limit > 0 {
                (total_gas_used as f64 / total_gas_limit as f64) * 100.0
            } else {
                0.0
            },
        };
    }

    /// Get current metrics
    pub fn metrics(&self) -> &ChainMetrics {
        &self.cumulative
    }

    /// Reset metrics
    pub fn reset(&mut self) {
        self.recent_blocks.clear();
        self.cumulative = ChainMetrics::new();
    }
}

/// Statistics for a single block
#[derive(Debug, Clone)]
struct BlockStats {
    number: BlockNumber,
    timestamp: u64,
    block_time: f64,
    tx_count: usize,
    size: usize,
    gas_used: Gas,
    gas_limit: Gas,
    avg_gas_price: u64,
}

impl BlockStats {
    fn from_block(block: &Block, prev_timestamp: u64) -> Self {
        let block_time = if prev_timestamp > 0 {
            block.header.timestamp.saturating_sub(prev_timestamp) as f64
        } else {
            0.0
        };

        let avg_gas_price = if !block.transactions.is_empty() {
            let total: u64 = block.transactions.iter()
                .map(|tx| tx.gas_price)
                .sum();
            total / block.transactions.len() as u64
        } else {
            0
        };

        let size = bincode::serialize(block)
            .map(|b| b.len())
            .unwrap_or(0);

        Self {
            number: block.number(),
            timestamp: block.header.timestamp,
            block_time,
            tx_count: block.transactions.len(),
            size,
            gas_used: block.header.gas_used,
            gas_limit: block.header.gas_limit,
            avg_gas_price,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blockchain_crypto::{Address, Hash};

    #[test]
    fn test_metrics_calculator() {
        let mut calc = MetricsCalculator::new(10);
        
        let genesis = Block::genesis(Hash::zero());
        calc.update(&genesis, 0);

        let metrics = calc.metrics();
        assert_eq!(metrics.height, 0);
        assert_eq!(metrics.total_transactions, 0);
    }

    #[test]
    fn test_metrics_averages() {
        let mut calc = MetricsCalculator::new(5);
        
        for i in 0..5 {
            let block = Block::new(
                i + 1,
                Hash::zero(),
                Hash::zero(),
                Address::zero(),
                vec![],
                10_000_000,
            ).unwrap();
            
            calc.update(&block, i * 3);
        }

        let metrics = calc.metrics();
        assert_eq!(metrics.height, 5);
        assert!(metrics.avg_block_time > 0.0);
    }
}