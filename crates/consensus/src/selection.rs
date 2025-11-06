// consensus/src/selection.rs

use crate::{validator::ValidatorInfo, ConsensusError, ConsensusResult};
use blockchain_crypto::Address;
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};

/// Selection weight for a validator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionWeight {
    pub address: Address,
    pub weight: u64,
}

/// Validator selector implementing the PoAS selection algorithm
pub struct ValidatorSelector {
    /// Random number generator
    rng: StdRng,
}

impl ValidatorSelector {
    /// Create a new validator selector with a seed
    pub fn new(seed: u64) -> Self {
        Self {
            rng: StdRng::seed_from_u64(seed),
        }
    }

    /// Create with a random seed
    pub fn new_random() -> Self {
        Self {
            rng: StdRng::from_entropy(),
        }
    }

    /// Calculate selection weight for a validator
    /// Weight = Staked_Amount × Utility_Score × Reliability_Factor
    pub fn calculate_weight(&self, validator: &ValidatorInfo) -> u64 {
        if !validator.can_produce_blocks() {
            return 0;
        }

        // Get stake amount (as u64 for simplicity, in production would handle BigUint properly)
        let stake = validator.stake.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0);

        // Get utility score (scaled 0-10000)
        let utility = validator.utility_score.value() as f64 / 1000.0; // Convert to 0-10 range

        // Get reliability factor (0.0-1.0)
        let reliability = validator.reliability_factor();

        // Get efficiency score (0.0-1.0)
        let efficiency = validator.efficiency_score();

        // Calculate weight with formula: stake × (1 + utility/10) × reliability × (1 + efficiency)
        let weight = (stake as f64) 
            * (1.0 + utility / 10.0) 
            * reliability 
            * (1.0 + efficiency);

        weight as u64
    }

    /// Calculate weights for all validators
    pub fn calculate_weights(&self, validators: &[&ValidatorInfo]) -> Vec<SelectionWeight> {
        validators.iter()
            .map(|v| SelectionWeight {
                address: v.address,
                weight: self.calculate_weight(v),
            })
            .filter(|w| w.weight > 0)
            .collect()
    }

    /// Select a validator using weighted random selection
    pub fn select_validator(&mut self, validators: &[&ValidatorInfo]) -> ConsensusResult<Address> {
        if validators.is_empty() {
            return Err(ConsensusError::SelectionError("No validators available".into()));
        }

        let weights = self.calculate_weights(validators);
        if weights.is_empty() {
            return Err(ConsensusError::SelectionError("No eligible validators".into()));
        }

        // Calculate total weight
        let total_weight: u64 = weights.iter().map(|w| w.weight).sum();
        if total_weight == 0 {
            return Err(ConsensusError::SelectionError("Total weight is zero".into()));
        }

        // Weighted random selection
        let mut selection = self.rng.gen_range(0..total_weight);
        
        for weight in &weights {
            if selection < weight.weight {
                return Ok(weight.address);
            }
            selection -= weight.weight;
        }

        // Fallback (should never reach here)
        Ok(weights[0].address)
    }

    /// Select multiple validators (for committee)
    pub fn select_committee(
        &mut self,
        validators: &[&ValidatorInfo],
        count: usize,
    ) -> ConsensusResult<Vec<Address>> {
        if validators.is_empty() {
            return Err(ConsensusError::SelectionError("No validators available".into()));
        }

        let mut selected = Vec::new();
        let mut remaining = validators.to_vec();

        for _ in 0..count.min(validators.len()) {
            if remaining.is_empty() {
                break;
            }

            let address = self.select_validator(&remaining)?;
            selected.push(address);

            // Remove selected validator from remaining pool
            remaining.retain(|v| v.address != address);
        }

        Ok(selected)
    }

    /// Deterministically select validator for a specific slot
    /// Used for round-robin or time-based selection
    pub fn select_for_slot(
        &self,
        validators: &[&ValidatorInfo],
        slot: u64,
    ) -> ConsensusResult<Address> {
        if validators.is_empty() {
            return Err(ConsensusError::SelectionError("No validators available".into()));
        }

        let weights = self.calculate_weights(validators);
        if weights.is_empty() {
            return Err(ConsensusError::SelectionError("No eligible validators".into()));
        }

        // Weighted deterministic selection based on slot
        let total_weight: u64 = weights.iter().map(|w| w.weight).sum();
        if total_weight == 0 {
            return Err(ConsensusError::SelectionError("Total weight is zero".into()));
        }

        let selection = slot % total_weight;
        let mut accumulated = 0u64;

        for weight in &weights {
            accumulated += weight.weight;
            if selection < accumulated {
                return Ok(weight.address);
            }
        }

        // Fallback
        Ok(weights[0].address)
    }
}

/// Probability calculator for validator selection
pub struct SelectionProbability;

impl SelectionProbability {
    /// Calculate probability of being selected for a single block
    pub fn calculate_probability(validator: &ValidatorInfo, total_weight: u64) -> f64 {
        let selector = ValidatorSelector::new(0);
        let validator_weight = selector.calculate_weight(validator);
        
        if total_weight == 0 {
            return 0.0;
        }

        validator_weight as f64 / total_weight as f64
    }

    /// Calculate expected blocks per epoch
    pub fn expected_blocks(
        validator: &ValidatorInfo,
        total_weight: u64,
        epoch_blocks: u64,
    ) -> f64 {
        let prob = Self::calculate_probability(validator, total_weight);
        prob * epoch_blocks as f64
    }

    /// Calculate Nakamoto coefficient (minimum validators to control 33% of weight)
    pub fn nakamoto_coefficient(validators: &[&ValidatorInfo]) -> usize {
        let selector = ValidatorSelector::new(0);
        let mut weights: Vec<_> = validators.iter()
            .map(|v| selector.calculate_weight(v))
            .collect();

        // Sort descending
        weights.sort_by(|a, b| b.cmp(a));

        let total_weight: u64 = weights.iter().sum();
        let threshold = total_weight / 3;

        let mut accumulated = 0u64;
        let mut count = 0;

        for weight in weights {
            accumulated += weight;
            count += 1;
            if accumulated >= threshold {
                break;
            }
        }

        count
    }

    /// Calculate Gini coefficient (measure of stake distribution inequality)
    pub fn gini_coefficient(validators: &[&ValidatorInfo]) -> f64 {
        if validators.is_empty() {
            return 0.0;
        }

        let selector = ValidatorSelector::new(0);
        let mut stakes: Vec<_> = validators.iter()
            .map(|v| selector.calculate_weight(v) as f64)
            .collect();

        stakes.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let n = stakes.len() as f64;
        let sum: f64 = stakes.iter().sum();

        if sum == 0.0 {
            return 0.0;
        }

        let mut numerator = 0.0;
        for (i, stake) in stakes.iter().enumerate() {
            numerator += (2.0 * (i as f64 + 1.0) - n - 1.0) * stake;
        }

        numerator / (n * sum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blockchain_core::{StakeAmount, UtilityScore};

    fn create_test_validator(stake: u64, utility: u64, uptime: u16) -> ValidatorInfo {
        let mut validator = ValidatorInfo::new(
            Address::zero(),
            StakeAmount::from_u64(stake),
            100,
        );
        validator.utility_score = UtilityScore::new(utility);
        validator.uptime = uptime;
        validator
    }

    #[test]
    fn test_weight_calculation() {
        let selector = ValidatorSelector::new(42);
        let validator = create_test_validator(10000, 5000, 9500);

        let weight = selector.calculate_weight(&validator);
        assert!(weight > 0);
    }

    #[test]
    fn test_validator_selection() {
        let mut selector = ValidatorSelector::new(42);
        
        let v1 = create_test_validator(10000, 5000, 9500);
        let v2 = create_test_validator(5000, 3000, 9000);
        let v3 = create_test_validator(15000, 7000, 9800);

        let validators = vec![&v1, &v2, &v3];
        let selected = selector.select_validator(&validators).unwrap();

        assert!(validators.iter().any(|v| v.address == selected));
    }

    #[test]
    fn test_committee_selection() {
        let mut selector = ValidatorSelector::new(42);
        
        let validators: Vec<_> = (0..10)
            .map(|i| create_test_validator(10000 + i * 1000, 5000, 9500))
            .collect();
        let validator_refs: Vec<_> = validators.iter().collect();

        let committee = selector.select_committee(&validator_refs, 5).unwrap();
        assert_eq!(committee.len(), 5);

        // Check no duplicates
        let unique: std::collections::HashSet<_> = committee.iter().collect();
        assert_eq!(unique.len(), 5);
    }

    #[test]
    fn test_deterministic_selection() {
        let selector = ValidatorSelector::new(42);
        
        let validators: Vec<_> = (0..5)
            .map(|i| create_test_validator(10000, 5000 + i * 100, 9500))
            .collect();
        let validator_refs: Vec<_> = validators.iter().collect();

        let selected1 = selector.select_for_slot(&validator_refs, 100).unwrap();
        let selected2 = selector.select_for_slot(&validator_refs, 100).unwrap();

        assert_eq!(selected1, selected2);
    }

    #[test]
    fn test_nakamoto_coefficient() {
        let validators: Vec<_> = vec![
            create_test_validator(50000, 5000, 9500),
            create_test_validator(20000, 5000, 9500),
            create_test_validator(15000, 5000, 9500),
            create_test_validator(10000, 5000, 9500),
            create_test_validator(5000, 5000, 9500),
        ];
        let validator_refs: Vec<_> = validators.iter().collect();

        let nakamoto = SelectionProbability::nakamoto_coefficient(&validator_refs);
        assert!(nakamoto > 0);
        assert!(nakamoto <= validators.len());
    }

    #[test]
    fn test_gini_coefficient() {
        let validators: Vec<_> = (0..10)
            .map(|i| create_test_validator(10000 + i * 1000, 5000, 9500))
            .collect();
        let validator_refs: Vec<_> = validators.iter().collect();

        let gini = SelectionProbability::gini_coefficient(&validator_refs);
        assert!(gini >= 0.0 && gini <= 1.0);
    }
}