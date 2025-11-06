// tokenomics/src/utility_index.rs

use crate::{TokenomicsError, TokenomicsResult};
use blockchain_core::{Amount, BlockNumber};
use serde::{Deserialize, Serialize};

/// Utility metrics tracked by the network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtilityMetrics {
    /// Total transaction volume (in base units)
    pub tx_volume: Amount,
    /// Total Value Locked in ecosystem dApps
    pub total_value_locked: Amount,
    /// Number of unique active addresses
    pub unique_addresses: u64,
    /// Total smart contract interactions
    pub contract_interactions: u64,
    /// Cross-chain bridging volume
    pub bridge_volume: Amount,
    /// Block number when measured
    pub block_number: BlockNumber,
}

impl UtilityMetrics {
    /// Create new metrics
    pub fn new(block_number: BlockNumber) -> Self {
        Self {
            tx_volume: Amount::zero(),
            total_value_locked: Amount::zero(),
            unique_addresses: 0,
            contract_interactions: 0,
            bridge_volume: Amount::zero(),
            block_number,
        }
    }

    /// Create metrics with values
    pub fn with_values(
        block_number: BlockNumber,
        tx_volume: Amount,
        tvl: Amount,
        unique_addresses: u64,
        contract_interactions: u64,
        bridge_volume: Amount,
    ) -> Self {
        Self {
            tx_volume,
            total_value_locked: tvl,
            unique_addresses,
            contract_interactions,
            bridge_volume,
            block_number,
        }
    }
}

/// Weights for different utility metrics (must sum to 1.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricWeights {
    /// Transaction volume weight (default: 0.30)
    pub tx_volume: f64,
    /// TVL weight (default: 0.25)
    pub tvl: f64,
    /// Active users weight (default: 0.20)
    pub active_users: f64,
    /// Contract interactions weight (default: 0.15)
    pub contract_interactions: f64,
    /// Bridge volume weight (default: 0.10)
    pub bridge_volume: f64,
}

impl MetricWeights {
    /// Create default weights as per whitepaper
    pub fn default_weights() -> Self {
        Self {
            tx_volume: 0.30,
            tvl: 0.25,
            active_users: 0.20,
            contract_interactions: 0.15,
            bridge_volume: 0.10,
        }
    }

    /// Validate that weights sum to 1.0
    pub fn validate(&self) -> TokenomicsResult<()> {
        let sum = self.tx_volume + self.tvl + self.active_users 
                + self.contract_interactions + self.bridge_volume;
        
        // Allow small floating point error
        if (sum - 1.0).abs() > 0.0001 {
            return Err(TokenomicsError::InvalidConfiguration(
                format!("Metric weights must sum to 1.0, got {}", sum)
            ));
        }

        Ok(())
    }
}

/// Utility Index calculator
pub struct UtilityIndex {
    /// Baseline metrics (established at Phase 1 → Phase 2 transition)
    baseline: UtilityMetrics,
    /// Metric weights
    weights: MetricWeights,
    /// Current metrics
    current: UtilityMetrics,
}

impl UtilityIndex {
    /// Create new utility index with baseline
    pub fn new(_baseline: UtilityMetrics, weights: MetricWeights) -> TokenomicsResult<()> {
        weights.validate()?;
        Ok(())
    }

    /// Create with default weights
    pub fn with_baseline(baseline: UtilityMetrics) -> Self {
        Self {
            baseline,
            weights: MetricWeights::default_weights(),
            current: UtilityMetrics::new(0),
        }
    }

    /// Update current metrics
    pub fn update_metrics(&mut self, metrics: UtilityMetrics) {
        self.current = metrics;
    }

    /// Calculate utility index value
    /// UI(t) = Σ(w_k × [M_k(t) / M_k(baseline)])
    pub fn calculate(&self) -> f64 {
        let mut index = 0.0;

        // Transaction volume contribution
        if !self.baseline.tx_volume.is_zero() {
            let ratio = self.calculate_ratio(
                &self.current.tx_volume,
                &self.baseline.tx_volume,
            );
            index += self.weights.tx_volume * ratio;
        }

        // TVL contribution
        if !self.baseline.total_value_locked.is_zero() {
            let ratio = self.calculate_ratio(
                &self.current.total_value_locked,
                &self.baseline.total_value_locked,
            );
            index += self.weights.tvl * ratio;
        }

        // Active users contribution
        if self.baseline.unique_addresses > 0 {
            let ratio = self.current.unique_addresses as f64 / self.baseline.unique_addresses as f64;
            index += self.weights.active_users * ratio;
        }

        // Contract interactions contribution
        if self.baseline.contract_interactions > 0 {
            let ratio = self.current.contract_interactions as f64 / self.baseline.contract_interactions as f64;
            index += self.weights.contract_interactions * ratio;
        }

        // Bridge volume contribution
        if !self.baseline.bridge_volume.is_zero() {
            let ratio = self.calculate_ratio(
                &self.current.bridge_volume,
                &self.baseline.bridge_volume,
            );
            index += self.weights.bridge_volume * ratio;
        }

        index
    }

    /// Calculate ratio between two amounts
    fn calculate_ratio(&self, current: &Amount, baseline: &Amount) -> f64 {
        if baseline.is_zero() {
            return 1.0;
        }

        // Convert to f64 for calculation (simplified)
        let current_val = current.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0) as f64;
        let baseline_val = baseline.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(1) as f64;

        current_val / baseline_val
    }

    /// Get baseline metrics
    pub fn baseline(&self) -> &UtilityMetrics {
        &self.baseline
    }

    /// Get current metrics
    pub fn current(&self) -> &UtilityMetrics {
        &self.current
    }

    /// Get weights
    pub fn weights(&self) -> &MetricWeights {
        &self.weights
    }

    /// Update weights (governance action)
    pub fn update_weights(&mut self, weights: MetricWeights) -> TokenomicsResult<()> {
        weights.validate()?;
        self.weights = weights;
        Ok(())
    }

    /// Check if utility is above baseline
    pub fn is_above_baseline(&self) -> bool {
        self.calculate() > 1.0
    }

    /// Get utility category
    pub fn utility_category(&self) -> UtilityCategory {
        let index = self.calculate();
        
        if index < 0.5 {
            UtilityCategory::VeryLow
        } else if index < 1.0 {
            UtilityCategory::Low
        } else if index < 1.5 {
            UtilityCategory::Normal
        } else if index < 2.0 {
            UtilityCategory::High
        } else {
            UtilityCategory::VeryHigh
        }
    }
}

/// Utility level categories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UtilityCategory {
    VeryLow,   // < 0.5x baseline
    Low,       // 0.5-1.0x baseline
    Normal,    // 1.0-1.5x baseline
    High,      // 1.5-2.0x baseline
    VeryHigh,  // > 2.0x baseline
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_baseline() -> UtilityMetrics {
        UtilityMetrics::with_values(
            100000,
            Amount::from_u64(1000000),  // 1M tx volume
            Amount::from_u64(5000000),  // 5M TVL
            10000,                       // 10k users
            50000,                       // 50k interactions
            Amount::from_u64(500000),   // 500k bridge volume
        )
    }

    #[test]
    fn test_weights_validation() {
        let weights = MetricWeights::default_weights();
        assert!(weights.validate().is_ok());

        let invalid_weights = MetricWeights {
            tx_volume: 0.5,
            tvl: 0.5,
            active_users: 0.5,
            contract_interactions: 0.0,
            bridge_volume: 0.0,
        };
        assert!(invalid_weights.validate().is_err());
    }

    #[test]
    fn test_utility_index_baseline() {
        let baseline = create_baseline();
        let index = UtilityIndex::with_baseline(baseline.clone());
        
        // At baseline, current = baseline, index should be ~1.0
        let mut index_mut = index;
        index_mut.update_metrics(baseline);
        
        let value = index_mut.calculate();
        assert!((value - 1.0).abs() < 0.1);
    }

    #[test]
    fn test_utility_index_above_baseline() {
        let baseline = create_baseline();
        let mut index = UtilityIndex::with_baseline(baseline);
        
        // Double all metrics
        let current = UtilityMetrics::with_values(
            200000,
            Amount::from_u64(2000000),
            Amount::from_u64(10000000),
            20000,
            100000,
            Amount::from_u64(1000000),
        );
        
        index.update_metrics(current);
        let value = index.calculate();
        
        assert!(value > 1.9); // Should be close to 2.0
        assert!(index.is_above_baseline());
    }

    #[test]
    fn test_utility_categories() {
        let baseline = create_baseline();
        let mut index = UtilityIndex::with_baseline(baseline);
        
        // Low utility
        let low_metrics = UtilityMetrics::with_values(
            150000,
            Amount::from_u64(600000),
            Amount::from_u64(3000000),
            6000,
            30000,
            Amount::from_u64(300000),
        );
        index.update_metrics(low_metrics);
        assert_eq!(index.utility_category(), UtilityCategory::Low);
        
        // High utility
        let high_metrics = UtilityMetrics::with_values(
            200000,
            Amount::from_u64(1800000),
            Amount::from_u64(9000000),
            18000,
            90000,
            Amount::from_u64(900000),
        );
        index.update_metrics(high_metrics);
        assert_eq!(index.utility_category(), UtilityCategory::High);
    }
}