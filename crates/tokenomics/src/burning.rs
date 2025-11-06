// tokenomics/src/burning.rs

use crate::{TokenomicsError, TokenomicsResult};
use blockchain_core::{Amount, Transaction};
use serde::{Deserialize, Serialize};

/// Burning mechanism configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurnConfig {
    /// Fee burn rate (basis points, 0-10000)
    /// Default: 3000 = 30% of transaction fees
    pub fee_burn_rate: u16,
    /// Utility index target for excess burning
    /// Default: 1.5 (50% above baseline)
    pub utility_target: f64,
    /// Enable excess utility burning
    pub enable_excess_burn: bool,
}

impl Default for BurnConfig {
    fn default() -> Self {
        Self {
            fee_burn_rate: 3000,  // 30%
            utility_target: 1.5,
            enable_excess_burn: true,
        }
    }
}

/// Types of token burning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BurnType {
    /// Transaction fee burning
    TransactionFee {
        tx_hash: blockchain_crypto::Hash,
        amount: Amount,
    },
    /// Excess utility burning
    ExcessUtility {
        utility_index: f64,
        amount: Amount,
    },
    /// Slashing burn
    Slashing {
        validator: blockchain_crypto::Address,
        amount: Amount,
    },
    /// Buy-back and burn
    BuyBack {
        amount: Amount,
    },
}

/// Burn record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurnRecord {
    /// Burn type
    pub burn_type: BurnType,
    /// Block number
    pub block_number: blockchain_core::BlockNumber,
    /// Timestamp
    pub timestamp: blockchain_core::Timestamp,
}

/// Burning mechanism manager
pub struct BurningMechanism {
    /// Configuration
    config: BurnConfig,
    /// Total burned amount
    total_burned: Amount,
    /// Burn history
    history: Vec<BurnRecord>,
    /// Fee burns
    fee_burns: Amount,
    /// Excess utility burns
    excess_burns: Amount,
    /// Slashing burns
    slashing_burns: Amount,
    /// Buy-back burns
    buyback_burns: Amount,
}

impl BurningMechanism {
    /// Create new burning mechanism
    pub fn new(config: BurnConfig) -> Self {
        Self {
            config,
            total_burned: Amount::zero(),
            history: Vec::new(),
            fee_burns: Amount::zero(),
            excess_burns: Amount::zero(),
            slashing_burns: Amount::zero(),
            buyback_burns: Amount::zero(),
        }
    }

    /// Calculate burn amount from transaction fees
    pub fn calculate_fee_burn(&self, total_fees: &Amount) -> Amount {
        let fees_val = total_fees.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0) as f64;

        let burn_amount = (fees_val * self.config.fee_burn_rate as f64) / 10000.0;
        Amount::from_u64(burn_amount as u64)
    }

    /// Burn transaction fees
    pub fn burn_transaction_fees(
        &mut self,
        transactions: &[Transaction],
        block_number: blockchain_core::BlockNumber,
    ) -> TokenomicsResult<Amount> {
        let mut total_fee_burn = Amount::zero();

        for tx in transactions {
            // Calculate fee for this transaction
            let gas_used = 21000; // Simplified
            let fee = tx.calculate_fee(gas_used);
            let burn_amount = self.calculate_fee_burn(&fee);

            // Record burn
            let record = BurnRecord {
                burn_type: BurnType::TransactionFee {
                    tx_hash: tx.hash(),
                    amount: burn_amount.clone(),
                },
                block_number,
                timestamp: current_timestamp(),
            };

            self.history.push(record);
            
            total_fee_burn = total_fee_burn.checked_add(&burn_amount)
                .ok_or_else(|| TokenomicsError::OverflowError("Fee burn overflow".into()))?;
        }

        self.fee_burns = self.fee_burns.checked_add(&total_fee_burn)
            .ok_or_else(|| TokenomicsError::OverflowError("Total fee burns overflow".into()))?;

        self.total_burned = self.total_burned.checked_add(&total_fee_burn)
            .ok_or_else(|| TokenomicsError::OverflowError("Total burned overflow".into()))?;

        Ok(total_fee_burn)
    }

    /// Burn excess utility (when network utility exceeds target)
    pub fn burn_excess_utility(
        &mut self,
        utility_index: f64,
        treasury_fees: &Amount,
        block_number: blockchain_core::BlockNumber,
    ) -> TokenomicsResult<Amount> {
        if !self.config.enable_excess_burn {
            return Ok(Amount::zero());
        }

        if utility_index <= self.config.utility_target {
            return Ok(Amount::zero());
        }

        // Calculate excess: (UI - target) / target
        let excess_ratio = (utility_index - self.config.utility_target) / self.config.utility_target;
        
        let treasury_val = treasury_fees.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0) as f64;

        let burn_amount_val = (treasury_val * excess_ratio).min(treasury_val);
        let burn_amount = Amount::from_u64(burn_amount_val as u64);

        // Record burn
        let record = BurnRecord {
            burn_type: BurnType::ExcessUtility {
                utility_index,
                amount: burn_amount.clone(),
            },
            block_number,
            timestamp: current_timestamp(),
        };

        self.history.push(record);
        
        self.excess_burns = self.excess_burns.checked_add(&burn_amount)
            .ok_or_else(|| TokenomicsError::OverflowError("Excess burns overflow".into()))?;

        self.total_burned = self.total_burned.checked_add(&burn_amount)
            .ok_or_else(|| TokenomicsError::OverflowError("Total burned overflow".into()))?;

        Ok(burn_amount)
    }

    /// Burn slashed tokens
    pub fn burn_slashed(
        &mut self,
        validator: blockchain_crypto::Address,
        amount: &Amount,
        block_number: blockchain_core::BlockNumber,
    ) -> TokenomicsResult<()> {
        let record = BurnRecord {
            burn_type: BurnType::Slashing {
                validator,
                amount: amount.clone(),
            },
            block_number,
            timestamp: current_timestamp(),
        };

        self.history.push(record);
        
        self.slashing_burns = self.slashing_burns.checked_add(amount)
            .ok_or_else(|| TokenomicsError::OverflowError("Slashing burns overflow".into()))?;

        self.total_burned = self.total_burned.checked_add(amount)
            .ok_or_else(|| TokenomicsError::OverflowError("Total burned overflow".into()))?;

        Ok(())
    }

    /// Execute buy-back and burn
    pub fn buyback_and_burn(
        &mut self,
        amount: &Amount,
        block_number: blockchain_core::BlockNumber,
    ) -> TokenomicsResult<()> {
        let record = BurnRecord {
            burn_type: BurnType::BuyBack {
                amount: amount.clone(),
            },
            block_number,
            timestamp: current_timestamp(),
        };

        self.history.push(record);
        
        self.buyback_burns = self.buyback_burns.checked_add(amount)
            .ok_or_else(|| TokenomicsError::OverflowError("Buyback burns overflow".into()))?;

        self.total_burned = self.total_burned.checked_add(amount)
            .ok_or_else(|| TokenomicsError::OverflowError("Total burned overflow".into()))?;

        Ok(())
    }

    /// Get total burned amount
    pub fn total_burned(&self) -> &Amount {
        &self.total_burned
    }

    /// Get burn breakdown
    pub fn burn_breakdown(&self) -> BurnBreakdown {
        BurnBreakdown {
            total: self.total_burned.clone(),
            fee_burns: self.fee_burns.clone(),
            excess_burns: self.excess_burns.clone(),
            slashing_burns: self.slashing_burns.clone(),
            buyback_burns: self.buyback_burns.clone(),
        }
    }

    /// Get burn history
    pub fn burn_history(&self) -> &[BurnRecord] {
        &self.history
    }

    /// Calculate burn rate (burned / minted) over a period
    pub fn calculate_burn_rate(&self, total_minted: &Amount) -> f64 {
        if total_minted.is_zero() {
            return 0.0;
        }

        let burned = self.total_burned.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0) as f64;
        let minted = total_minted.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(1) as f64;

        (burned / minted) * 100.0
    }

    /// Update configuration
    pub fn update_config(&mut self, config: BurnConfig) {
        self.config = config;
    }

    /// Get configuration
    pub fn config(&self) -> &BurnConfig {
        &self.config
    }
}

/// Burn amount breakdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurnBreakdown {
    pub total: Amount,
    pub fee_burns: Amount,
    pub excess_burns: Amount,
    pub slashing_burns: Amount,
    pub buyback_burns: Amount,
}

/// Helper to get current timestamp
fn current_timestamp() -> blockchain_core::Timestamp {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use blockchain_core::transaction::TransactionType;
    use blockchain_crypto::{KeyPair, SignatureScheme};

    fn create_test_transaction() -> Transaction {
        let keypair = KeyPair::generate(SignatureScheme::Ed25519).unwrap();
        let from = keypair.public_key().to_address();
        
        Transaction::new(
            from,
            1,
            TransactionType::Transfer {
                to: blockchain_crypto::Address::zero(),
                amount: Amount::from_u64(100),
            },
            10,
            21000,
        )
    }

    #[test]
    fn test_fee_burn_calculation() {
        let config = BurnConfig::default();
        let mechanism = BurningMechanism::new(config);

        let fees = Amount::from_u64(1000);
        let burn = mechanism.calculate_fee_burn(&fees);

        // 30% of 1000 = 300
        assert_eq!(burn, Amount::from_u64(300));
    }

    #[test]
    fn test_transaction_fee_burning() {
        let config = BurnConfig::default();
        let mut mechanism = BurningMechanism::new(config);

        let tx = create_test_transaction();
        let burned = mechanism.burn_transaction_fees(&[tx], 100).unwrap();

        assert!(burned.inner() > &Amount::zero().inner());
        assert!(mechanism.total_burned().inner() > &Amount::zero().inner());
    }

    #[test]
    fn test_excess_utility_burn() {
        let config = BurnConfig::default();
        let mut mechanism = BurningMechanism::new(config);

        let treasury = Amount::from_u64(10000);
        
        // No burn when below target
        let burn1 = mechanism.burn_excess_utility(1.2, &treasury, 100).unwrap();
        assert_eq!(burn1, Amount::zero());

        // Burn when above target
        let burn2 = mechanism.burn_excess_utility(2.0, &treasury, 101).unwrap();
        assert!(burn2.inner() > &Amount::zero().inner());
    }

    #[test]
    fn test_slashing_burn() {
        let config = BurnConfig::default();
        let mut mechanism = BurningMechanism::new(config);

        let validator = blockchain_crypto::Address::zero();
        let amount = Amount::from_u64(5000);

        mechanism.burn_slashed(validator, &amount, 100).unwrap();
        
        assert_eq!(mechanism.burn_breakdown().slashing_burns, amount);
    }

    #[test]
    fn test_burn_breakdown() {
        let config = BurnConfig::default();
        let mut mechanism = BurningMechanism::new(config);

        // Add various burns
        let tx = create_test_transaction();
        mechanism.burn_transaction_fees(&[tx], 100).unwrap();
        
        let validator = blockchain_crypto::Address::zero();
        mechanism.burn_slashed(validator, &Amount::from_u64(1000), 101).unwrap();

        let breakdown = mechanism.burn_breakdown();
        assert!(breakdown.total.inner() > &Amount::zero().inner());
        assert!(breakdown.fee_burns.inner() > &Amount::zero().inner());
        assert!(breakdown.slashing_burns.inner() > &Amount::zero().inner());
    }

    #[test]
    fn test_burn_rate_calculation() {
        let config = BurnConfig::default();
        let mut mechanism = BurningMechanism::new(config);

        let tx = create_test_transaction();
        mechanism.burn_transaction_fees(&[tx], 100).unwrap();

        let total_minted = Amount::from_u64(100000);
        let burn_rate = mechanism.calculate_burn_rate(&total_minted);

        assert!(burn_rate >= 0.0);
        assert!(burn_rate <= 100.0);
    }
}