// tokenomics/src/rewards.rs

use crate::{
    minting::MintingPhase,
    utility_index::UtilityIndex,
    TokenomicsError, TokenomicsResult,
};
use blockchain_core::{Amount, StakeAmount};
use blockchain_crypto::Address;
use consensus::validator::ValidatorInfo;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Reward calculation for a validator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardCalculation {
    /// Validator address
    pub validator: Address,
    /// Base reward amount
    pub base_reward: Amount,
    /// Stake weight multiplier
    pub stake_weight: f64,
    /// Time factor multiplier
    pub time_factor: f64,
    /// Block production bonus
    pub block_bonus: f64,
    /// Utility contribution factor (Phase 2)
    pub utility_factor: Option<f64>,
    /// Final reward amount
    pub final_reward: Amount,
    /// Commission deducted
    pub commission: Amount,
    /// Net reward after commission
    pub net_reward: Amount,
}

/// Reward distributor
pub struct RewardDistributor {
    /// Current minting phase
    phase: MintingPhase,
    /// Utility index (for Phase 2)
    utility_index: Option<UtilityIndex>,
    /// Reward history
    history: Vec<RewardCalculation>,
}

impl RewardDistributor {
    /// Create new reward distributor
    pub fn new(phase: MintingPhase) -> Self {
        Self {
            phase,
            utility_index: None,
            history: Vec::new(),
        }
    }

    /// Set utility index for Phase 2
    pub fn set_utility_index(&mut self, utility_index: UtilityIndex) {
        self.utility_index = Some(utility_index);
    }

    /// Update current phase
    pub fn set_phase(&mut self, phase: MintingPhase) {
        self.phase = phase;
    }

    /// Calculate Phase 1 reward for a validator
    /// R₁(i,t) = M₁(t) × [S(i) / Σ S(j)] × T(i,t) × B(i)
    pub fn calculate_phase1_reward(
        &self,
        validator: &ValidatorInfo,
        network_mint: &Amount,
        total_stake: &StakeAmount,
        blocks_staked: u64,
        produced_block: bool,
    ) -> TokenomicsResult<RewardCalculation> {
        // Calculate stake weight
        let stake_weight = if !total_stake.is_zero() {
            let validator_stake = validator.stake.inner()
                .to_u64_digits()
                .first()
                .copied()
                .unwrap_or(0) as f64;
            let total = total_stake.inner()
                .to_u64_digits()
                .first()
                .copied()
                .unwrap_or(1) as f64;
            validator_stake / total
        } else {
            0.0
        };

        // Calculate time factor: min(1, blocks_staked / 100,000) T(i,t)
        let time_factor = (blocks_staked as f64 / 100_000.0).min(1.0);

        // Block production bonus: 1.2 if produced block, else 1.0
        let block_bonus = if produced_block { 1.2 } else { 1.0 };

        // Calculate base reward
        let network_mint_val = network_mint.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0) as f64;

        let base_reward_val = network_mint_val * stake_weight * time_factor * block_bonus;
        let base_reward = Amount::from_u64(base_reward_val as u64);

        // Calculate commission
        let commission_val = (base_reward_val * validator.commission_rate as f64) / 10000.0;
        let commission = Amount::from_u64(commission_val as u64);

        let net_reward = base_reward.checked_sub(&commission)
            .unwrap_or_else(|| base_reward.clone());

        Ok(RewardCalculation {
            validator: validator.address,
            base_reward: base_reward.clone(),
            stake_weight,
            time_factor,
            block_bonus,
            utility_factor: None,
            final_reward: base_reward.clone(),
            commission,
            net_reward,
        })
    }

    /// Calculate Phase 2 reward for a validator
    /// R₂(i,t) = M₂(t) × [UC(i,t) / Σ UC(j,t)] × B(i)
    pub fn calculate_phase2_reward(
        &self,
        validator: &ValidatorInfo,
        network_mint: &Amount,
        total_utility: f64,
        validator_utility: f64,
        produced_block: bool,
    ) -> TokenomicsResult<RewardCalculation> {
        // Calculate utility contribution weight
        let utility_weight = if total_utility > 0.0 {
            validator_utility / total_utility
        } else {
            0.0
        };

        // Block production bonus
        let block_bonus = if produced_block { 1.2 } else { 1.0 };

        // Calculate base reward
        let network_mint_val = network_mint.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0) as f64;

        let base_reward_val = network_mint_val * utility_weight * block_bonus;
        let base_reward = Amount::from_u64(base_reward_val as u64);

        // Calculate commission
        let commission_val = (base_reward_val * validator.commission_rate as f64) / 10000.0;
        let commission = Amount::from_u64(commission_val as u64);

        let net_reward = base_reward.checked_sub(&commission)
            .unwrap_or_else(|| base_reward.clone());

        Ok(RewardCalculation {
            validator: validator.address,
            base_reward: base_reward.clone(),
            stake_weight: utility_weight,
            time_factor: 1.0,
            block_bonus,
            utility_factor: Some(utility_weight),
            final_reward: base_reward.clone(),
            commission,
            net_reward,
        })
    }

    /// Calculate rewards for all validators
    pub fn distribute_rewards(
        &mut self,
        validators: &[&ValidatorInfo],
        network_mint: &Amount,
        total_stake: &StakeAmount,
        produced_by: Option<Address>,
    ) -> TokenomicsResult<HashMap<Address, RewardCalculation>> {
        let mut rewards = HashMap::new();

        match self.phase {
            MintingPhase::Bootstrap => {
                // Phase 1: Distribute based on stake
                for validator in validators {
                    let produced_block = produced_by == Some(validator.address);
                    let reward = self.calculate_phase1_reward(
                        validator,
                        network_mint,
                        total_stake,
                        100_000, // Simplified: assume fully vested
                        produced_block,
                    )?;

                    self.history.push(reward.clone());
                    rewards.insert(validator.address, reward);
                }
            }
            MintingPhase::UtilityDriven => {
                // Phase 2: Distribute based on utility contribution
                let utility_index = self.utility_index.as_ref()
                    .ok_or_else(|| TokenomicsError::RewardDistributionError(
                        "Utility index not set for Phase 2".into()
                    ))?;

                // Calculate total utility contribution
                let total_utility: f64 = validators.iter()
                    .map(|v| self.calculate_validator_utility(v, utility_index))
                    .sum();

                for validator in validators {
                    let produced_block = produced_by == Some(validator.address);
                    let validator_utility = self.calculate_validator_utility(validator, utility_index);
                    
                    let reward = self.calculate_phase2_reward(
                        validator,
                        network_mint,
                        total_utility,
                        validator_utility,
                        produced_block,
                    )?;

                    self.history.push(reward.clone());
                    rewards.insert(validator.address, reward);
                }
            }
        }

        Ok(rewards)
    }

    /// Calculate utility contribution for a validator (Phase 2)
    /// UC(i,t) = Σ(w_k × C_k(i,t))
    fn calculate_validator_utility(
        &self,
        validator: &ValidatorInfo,
        _utility_index: &UtilityIndex,
    ) -> f64 {
        // Simplified: based on validator's metrics
        // In production, this would calculate actual contribution to each metric

        let base_contribution = validator.utility_score.value() as f64;
        let liquidity_factor = if !validator.stake.is_zero() {
            let deployed = validator.liquidity_deployed.inner()
                .to_u64_digits()
                .first()
                .copied()
                .unwrap_or(0) as f64;
            let staked = validator.stake.inner()
                .to_u64_digits()
                .first()
                .copied()
                .unwrap_or(1) as f64;
            deployed / staked
        } else {
            0.0
        };

        let uptime_factor = validator.uptime as f64 / 10000.0;

        base_contribution * (1.0 + liquidity_factor) * uptime_factor
    }

    /// Get reward history
    pub fn reward_history(&self) -> &[RewardCalculation] {
        &self.history
    }

    /// Get total rewards distributed
    pub fn total_distributed(&self) -> Amount {
        self.history.iter()
            .fold(Amount::zero(), |acc, r| {
                acc.checked_add(&r.final_reward).unwrap_or(acc)
            })
    }

    /// Get validator total rewards
    pub fn validator_total_rewards(&self, address: &Address) -> Amount {
        self.history.iter()
            .filter(|r| r.validator == *address)
            .fold(Amount::zero(), |acc, r| {
                acc.checked_add(&r.final_reward).unwrap_or(acc)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blockchain_core::UtilityScore;

    fn create_test_validator(stake: u64, utility: u64, commission: u16) -> ValidatorInfo {
        let mut validator = ValidatorInfo::new(
            Address::zero(),
            StakeAmount::from_u64(stake),
            commission,
        );
        validator.utility_score = UtilityScore::new(utility);
        validator.uptime = 9500; // 95%
        validator
    }

    #[test]
    fn test_phase1_reward_calculation() {
        let distributor = RewardDistributor::new(MintingPhase::Bootstrap);
        let validator = create_test_validator(10000, 0, 500);
        let network_mint = Amount::from_u64(1000);
        let total_stake = StakeAmount::from_u64(100000);

        let reward = distributor.calculate_phase1_reward(
            &validator,
            &network_mint,
            &total_stake,
            100000,
            true,
        ).unwrap();

        assert!(reward.final_reward.inner() > &Amount::zero().inner());
        assert_eq!(reward.stake_weight, 0.1); // 10000/100000
        assert_eq!(reward.time_factor, 1.0);
        assert_eq!(reward.block_bonus, 1.2);
    }

    #[test]
    fn test_phase2_reward_calculation() {
        let distributor = RewardDistributor::new(MintingPhase::UtilityDriven);
        let validator = create_test_validator(10000, 5000, 500);
        let network_mint = Amount::from_u64(1000);

        let reward = distributor.calculate_phase2_reward(
            &validator,
            &network_mint,
            10000.0,
            5000.0,
            false,
        ).unwrap();

        assert!(reward.final_reward.inner() > &Amount::zero().inner());
        assert!(reward.utility_factor.is_some());
    }

    #[test]
    fn test_commission_deduction() {
        let distributor = RewardDistributor::new(MintingPhase::Bootstrap);
        let validator = create_test_validator(10000, 0, 1000); // 10% commission
        let network_mint = Amount::from_u64(1000);
        let total_stake = StakeAmount::from_u64(10000);

        let reward = distributor.calculate_phase1_reward(
            &validator,
            &network_mint,
            &total_stake,
            100000,
            false,
        ).unwrap();

        assert!(reward.commission.inner() > &Amount::zero().inner());
        assert!(reward.net_reward.inner() < reward.final_reward.inner());
    }
}