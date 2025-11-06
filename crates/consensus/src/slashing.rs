// consensus/src/slashing.rs

use crate::{validator::ValidatorInfo, ConsensusError, ConsensusResult};
use blockchain_core::{StakeAmount, Timestamp};
use blockchain_crypto::Address;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Slashing conditions as defined in the whitepaper
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlashingCondition {
    /// Double-signing blocks: 5% stake slash
    DoubleSigning,
    /// Extended downtime: 0.1% slash per day
    ExtendedDowntime { days: u64 },
    /// Liquidity mismanagement: 10% slash
    LiquidityMismanagement,
    /// Governance attack attempts: 100% slash
    GovernanceAttack,
}

impl SlashingCondition {
    /// Get the base penalty rate (in basis points, 0-10000)
    pub fn base_penalty_rate(&self) -> u16 {
        match self {
            SlashingCondition::DoubleSigning => 500,        // 5%
            SlashingCondition::ExtendedDowntime { days } => {
                // 0.1% per day, max 100%
                (10 * days).min(10000) as u16
            }
            SlashingCondition::LiquidityMismanagement => 1000, // 10%
            SlashingCondition::GovernanceAttack => 10000,   // 100%
        }
    }

    /// Get severity multiplier based on previous offenses
    pub fn severity_multiplier(&self, previous_offenses: u32) -> f64 {
        let base = 1.0 + (previous_offenses as f64 * 0.5);
        base.min(3.0) // Max 3x multiplier
    }

    /// Check if this is a capital offense (requires immediate exit)
    pub fn is_capital_offense(&self) -> bool {
        matches!(self, SlashingCondition::GovernanceAttack)
    }
}

/// Slashing penalty details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashingPenalty {
    /// Validator being slashed
    pub validator: Address,
    /// Reason for slashing
    pub condition: SlashingCondition,
    /// Amount slashed
    pub amount: StakeAmount,
    /// Timestamp of slashing
    pub timestamp: Timestamp,
    /// Previous offense count at time of slashing
    pub previous_offenses: u32,
    /// Evidence hash (proof of misbehavior)
    pub evidence_hash: Option<blockchain_crypto::Hash>,
}

/// Fund distribution for slashed tokens
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashingDistribution {
    /// Amount to burn (50%)
    pub burn: StakeAmount,
    /// Amount to insurance fund (30%)
    pub insurance: StakeAmount,
    /// Amount to whistleblower reward (20%)
    pub whistleblower: StakeAmount,
}

impl SlashingDistribution {
    /// Calculate distribution from slashed amount
    pub fn from_slashed_amount(amount: &StakeAmount) -> Self {
        let total = amount.inner();
        
        // Calculate percentages
        let burn_amount = (total * 50u64) / 100u64;
        let insurance_amount = (total * 30u64) / 100u64;
        let whistleblower_amount = (total * 20u64) / 100u64;

        Self {
            burn: StakeAmount::new(burn_amount),
            insurance: StakeAmount::new(insurance_amount),
            whistleblower: StakeAmount::new(whistleblower_amount),
        }
    }
}

/// Slashing manager
pub struct SlashingManager {
    /// Slashing events history
    slashing_history: Vec<SlashingPenalty>,
    /// Offense count per validator
    offense_count: HashMap<Address, u32>,
    /// Insurance fund balance
    insurance_fund: StakeAmount,
    /// Total slashed amount
    total_slashed: StakeAmount,
    /// Total burned
    total_burned: StakeAmount,
}

impl SlashingManager {
    /// Create a new slashing manager
    pub fn new() -> Self {
        Self {
            slashing_history: Vec::new(),
            offense_count: HashMap::new(),
            insurance_fund: StakeAmount::zero(),
            total_slashed: StakeAmount::zero(),
            total_burned: StakeAmount::zero(),
        }
    }

    /// Calculate slash amount for a validator
    pub fn calculate_slash_amount(
        &self,
        validator: &ValidatorInfo,
        condition: SlashingCondition,
    ) -> StakeAmount {
        let base_rate = condition.base_penalty_rate();
        let previous_offenses = self.get_offense_count(&validator.address);
        let multiplier = condition.severity_multiplier(previous_offenses);

        // Calculate: stake × (base_rate / 10000) × multiplier
        let stake_u64 = validator.stake.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0);

        let base_slash = (stake_u64 as f64 * base_rate as f64 / 10000.0) as u64;
        let final_slash = (base_slash as f64 * multiplier) as u64;

        // Cap at total stake
        StakeAmount::from_u64(final_slash.min(stake_u64))
    }

    /// Execute slashing on a validator
    pub fn slash_validator(
        &mut self,
        validator: &mut ValidatorInfo,
        condition: SlashingCondition,
        evidence_hash: Option<blockchain_crypto::Hash>,
    ) -> ConsensusResult<SlashingPenalty> {
        // Calculate slash amount
        let slash_amount = self.calculate_slash_amount(validator, condition);

        // Verify sufficient stake
        if validator.stake.inner() < slash_amount.inner() {
            return Err(ConsensusError::SlashingError(
                "Insufficient stake to slash".into()
            ));
        }

        // Get offense count
        let previous_offenses = self.get_offense_count(&validator.address);

        // Create penalty record
        let penalty = SlashingPenalty {
            validator: validator.address,
            condition,
            amount: slash_amount.clone(),
            timestamp: current_timestamp(),
            previous_offenses,
            evidence_hash,
        };

        // Remove stake from validator
        validator.stake = validator.stake.checked_sub(&slash_amount)
            .ok_or_else(|| ConsensusError::SlashingError("Stake underflow".into()))?;

        // Distribute slashed funds
        let distribution = SlashingDistribution::from_slashed_amount(&slash_amount);
        
        self.insurance_fund = self.insurance_fund.checked_add(&distribution.insurance)
            .ok_or_else(|| ConsensusError::SlashingError("Insurance fund overflow".into()))?;

        self.total_slashed = self.total_slashed.checked_add(&slash_amount)
            .ok_or_else(|| ConsensusError::SlashingError("Total slashed overflow".into()))?;

        self.total_burned = self.total_burned.checked_add(&distribution.burn)
            .ok_or_else(|| ConsensusError::SlashingError("Total burned overflow".into()))?;

        // Update offense count
        *self.offense_count.entry(validator.address).or_insert(0) += 1;

        // Add to history
        self.slashing_history.push(penalty.clone());

        // Handle capital offenses
        if condition.is_capital_offense() {
            validator.status = crate::validator::ValidatorStatus::Slashed;
        }

        Ok(penalty)
    }

    /// Get offense count for a validator
    pub fn get_offense_count(&self, address: &Address) -> u32 {
        self.offense_count.get(address).copied().unwrap_or(0)
    }

    /// Get slashing history for a validator
    pub fn get_validator_history(&self, address: &Address) -> Vec<&SlashingPenalty> {
        self.slashing_history.iter()
            .filter(|p| p.validator == *address)
            .collect()
    }

    /// Get total slashed amount
    pub fn total_slashed(&self) -> &StakeAmount {
        &self.total_slashed
    }

    /// Get insurance fund balance
    pub fn insurance_fund(&self) -> &StakeAmount {
        &self.insurance_fund
    }

    /// Get total burned amount
    pub fn total_burned(&self) -> &StakeAmount {
        &self.total_burned
    }

    /// Get all slashing events
    pub fn all_slashing_events(&self) -> &[SlashingPenalty] {
        &self.slashing_history
    }

    /// Check if validator should be auto-slashed for downtime
    pub fn check_downtime_slashing(
        &mut self,
        validator: &mut ValidatorInfo,
        max_downtime_blocks: u64,
    ) -> ConsensusResult<Option<SlashingPenalty>> {
        // Calculate consecutive misses
        let total_blocks = validator.blocks_produced + validator.blocks_missed;
        if total_blocks == 0 {
            return Ok(None);
        }

        // Check if downtime threshold exceeded
        if validator.blocks_missed >= max_downtime_blocks {
            let days_down = validator.blocks_missed / (28800); // Assuming 3s blocks, 28800 blocks/day
            
            if days_down > 0 {
                let condition = SlashingCondition::ExtendedDowntime { days: days_down };
                let penalty = self.slash_validator(validator, condition, None)?;
                return Ok(Some(penalty));
            }
        }

        Ok(None)
    }

    /// Withdraw from insurance fund (governance action)
    pub fn withdraw_insurance(
        &mut self,
        amount: &StakeAmount,
    ) -> ConsensusResult<()> {
        self.insurance_fund = self.insurance_fund.checked_sub(amount)
            .ok_or_else(|| ConsensusError::SlashingError(
                "Insufficient insurance fund balance".into()
            ))?;
        Ok(())
    }
}

impl Default for SlashingManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to get current timestamp
fn current_timestamp() -> Timestamp {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_validator(stake: u64) -> ValidatorInfo {
        ValidatorInfo::new(
            Address::zero(),
            StakeAmount::from_u64(stake),
            100,
        )
    }

    #[test]
    fn test_slashing_conditions() {
        assert_eq!(SlashingCondition::DoubleSigning.base_penalty_rate(), 500);
        assert_eq!(SlashingCondition::LiquidityMismanagement.base_penalty_rate(), 1000);
        assert_eq!(SlashingCondition::GovernanceAttack.base_penalty_rate(), 10000);
    }

    #[test]
    fn test_severity_multiplier() {
        let condition = SlashingCondition::DoubleSigning;
        assert_eq!(condition.severity_multiplier(0), 1.0);
        assert_eq!(condition.severity_multiplier(1), 1.5);
        assert_eq!(condition.severity_multiplier(2), 2.0);
        assert_eq!(condition.severity_multiplier(10), 3.0); // Capped at 3.0
    }

    #[test]
    fn test_slash_calculation() {
        let manager = SlashingManager::new();
        let validator = create_test_validator(100000);

        let slash = manager.calculate_slash_amount(&validator, SlashingCondition::DoubleSigning);
        // 5% of 100000 = 5000
        assert_eq!(slash, StakeAmount::from_u64(5000));
    }

    #[test]
    fn test_slash_execution() {
        let mut manager = SlashingManager::new();
        let mut validator = create_test_validator(100000);

        let penalty = manager.slash_validator(
            &mut validator,
            SlashingCondition::DoubleSigning,
            None,
        ).unwrap();

        assert_eq!(penalty.amount, StakeAmount::from_u64(5000));
        assert_eq!(validator.stake, StakeAmount::from_u64(95000));
        assert_eq!(manager.get_offense_count(&validator.address), 1);
    }

    #[test]
    fn test_distribution() {
        let amount = StakeAmount::from_u64(10000);
        let dist = SlashingDistribution::from_slashed_amount(&amount);

        assert_eq!(dist.burn, StakeAmount::from_u64(5000));
        assert_eq!(dist.insurance, StakeAmount::from_u64(3000));
        assert_eq!(dist.whistleblower, StakeAmount::from_u64(2000));
    }

    #[test]
    fn test_repeat_offender() {
        let mut manager = SlashingManager::new();
        let mut validator = create_test_validator(100000);

        // First offense
        manager.slash_validator(&mut validator, SlashingCondition::DoubleSigning, None).unwrap();
        let stake_after_first = validator.stake.clone();

        // Second offense (should have multiplier)
        manager.slash_validator(&mut validator, SlashingCondition::DoubleSigning, None).unwrap();
        
        assert_eq!(manager.get_offense_count(&validator.address), 2);
        assert!(validator.stake.inner() < stake_after_first.inner());
    }

    #[test]
    fn test_capital_offense() {
        let mut manager = SlashingManager::new();
        let mut validator = create_test_validator(100000);

        manager.slash_validator(
            &mut validator,
            SlashingCondition::GovernanceAttack,
            None,
        ).unwrap();

        // Should be completely slashed and marked as slashed
        assert_eq!(validator.stake, StakeAmount::from_u64(0));
        assert_eq!(validator.status, crate::validator::ValidatorStatus::Slashed);
    }
}