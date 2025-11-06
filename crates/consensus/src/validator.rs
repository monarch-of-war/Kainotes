// consensus/src/validator.rs

use crate::{ConsensusError, ConsensusResult};
use blockchain_core::{Amount, StakeAmount, Timestamp, UtilityScore};
use blockchain_crypto::Address;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Validator status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidatorStatus {
    /// Active and participating in consensus
    Active,
    /// Temporarily inactive (offline or maintenance)
    Inactive,
    /// Unbonding period (after unstaking request)
    Unbonding { unlock_time: Timestamp },
    /// Slashed due to misbehavior
    Slashed,
    /// Exited from validator set
    Exited,
}

/// Complete validator information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorInfo {
    /// Validator address
    pub address: Address,
    /// Staked amount
    pub stake: StakeAmount,
    /// Liquidity deployed to utility pools
    pub liquidity_deployed: Amount,
    /// Utility contribution score
    pub utility_score: UtilityScore,
    /// Current status
    pub status: ValidatorStatus,
    /// Uptime percentage (0-10000 representing 0-100%)
    pub uptime: u16,
    /// Total blocks produced
    pub blocks_produced: u64,
    /// Total blocks missed
    pub blocks_missed: u64,
    /// Registration timestamp
    pub registered_at: Timestamp,
    /// Last active timestamp
    pub last_active: Timestamp,
    /// Commission rate (basis points, 0-10000)
    pub commission_rate: u16,
}

impl ValidatorInfo {
    /// Create a new validator
    pub fn new(address: Address, stake: StakeAmount, commission_rate: u16) -> Self {
        let now = current_timestamp();
        Self {
            address,
            stake,
            liquidity_deployed: Amount::zero(),
            utility_score: UtilityScore::zero(),
            status: ValidatorStatus::Active,
            uptime: 10000, // Start at 100%
            blocks_produced: 0,
            blocks_missed: 0,
            registered_at: now,
            last_active: now,
            commission_rate: commission_rate.min(10000),
        }
    }

    /// Check if validator is active
    pub fn is_active(&self) -> bool {
        matches!(self.status, ValidatorStatus::Active)
    }

    /// Check if validator can produce blocks
    pub fn can_produce_blocks(&self) -> bool {
        self.is_active() && !self.stake.is_zero()
    }

    /// Update uptime based on block production
    pub fn update_uptime(&mut self, produced: bool) {
        if produced {
            self.blocks_produced += 1;
        } else {
            self.blocks_missed += 1;
        }

        // Calculate new uptime (exponential moving average)
        let total = self.blocks_produced + self.blocks_missed;
        if total > 0 {
            self.uptime = ((self.blocks_produced as f64 / total as f64) * 10000.0) as u16;
        }

        self.last_active = current_timestamp();
    }

    /// Calculate reliability factor (0.0 to 1.0)
    pub fn reliability_factor(&self) -> f64 {
        self.uptime as f64 / 10000.0
    }

    /// Calculate efficiency score (yield generated per risk)
    pub fn efficiency_score(&self) -> f64 {
        if self.stake.is_zero() {
            return 0.0;
        }
        
        // Simplified: based on liquidity deployment ratio
        let deployment_ratio = if !self.stake.is_zero() {
            self.liquidity_deployed.inner().clone().min(self.stake.inner().clone());
            let deployed = self.liquidity_deployed.inner();
            let staked = self.stake.inner();
            
            if staked.bits() > 0 {
                (deployed.clone() * 10000u64) / staked.clone()
            } else {
                num_bigint::BigUint::from(0u64)
            }
        } else {
            num_bigint::BigUint::from(0u64)
        };

        deployment_ratio.to_u64_digits().first().copied().unwrap_or(0) as f64 / 10000.0
    }

    /// Add stake
    pub fn add_stake(&mut self, amount: &StakeAmount) -> ConsensusResult<()> {
        self.stake = self.stake.checked_add(amount)
            .ok_or_else(|| ConsensusError::ValidatorError("Stake overflow".into()))?;
        Ok(())
    }

    /// Remove stake (initiate unbonding)
    pub fn remove_stake(&mut self, amount: &StakeAmount, unbonding_period: u64) -> ConsensusResult<()> {
        if self.stake.inner() < amount.inner() {
            return Err(ConsensusError::InsufficientStake {
                required: amount.inner().to_u64_digits().first().copied().unwrap_or(0),
                provided: self.stake.inner().to_u64_digits().first().copied().unwrap_or(0),
            });
        }

        self.stake = self.stake.checked_sub(amount)
            .ok_or_else(|| ConsensusError::ValidatorError("Stake underflow".into()))?;

        // Set unbonding status
        let unlock_time = current_timestamp() + unbonding_period;
        self.status = ValidatorStatus::Unbonding { unlock_time };

        Ok(())
    }

    /// Deploy liquidity
    pub fn deploy_liquidity(&mut self, amount: &Amount) -> ConsensusResult<()> {
        if self.stake.inner() < amount.inner() {
            return Err(ConsensusError::ValidatorError(
                "Cannot deploy more liquidity than staked".into()
            ));
        }

        self.liquidity_deployed = self.liquidity_deployed.checked_add(amount)
            .ok_or_else(|| ConsensusError::ValidatorError("Liquidity overflow".into()))?;

        Ok(())
    }

    /// Withdraw liquidity
    pub fn withdraw_liquidity(&mut self, amount: &Amount) -> ConsensusResult<()> {
        self.liquidity_deployed = self.liquidity_deployed.checked_sub(amount)
            .ok_or_else(|| ConsensusError::ValidatorError("Insufficient liquidity deployed".into()))?;

        Ok(())
    }
}

/// Manages the validator set
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorSet {
    /// All validators indexed by address
    validators: HashMap<Address, ValidatorInfo>,
    /// Minimum stake required to be a validator
    min_stake: StakeAmount,
    /// Unbonding period in seconds (default: 14 days)
    unbonding_period: u64,
}

impl ValidatorSet {
    /// Create a new validator set
    pub fn new(min_stake: StakeAmount, unbonding_period: u64) -> Self {
        Self {
            validators: HashMap::new(),
            min_stake,
            unbonding_period,
        }
    }

    /// Get minimum stake requirement
    pub fn min_stake(&self) -> &StakeAmount {
        &self.min_stake
    }

    /// Get a validator by address
    pub fn get(&self, address: &Address) -> Option<&ValidatorInfo> {
        self.validators.get(address)
    }

    /// Get mutable validator reference
    pub fn get_mut(&mut self, address: &Address) -> Option<&mut ValidatorInfo> {
        self.validators.get_mut(address)
    }

    /// Register a new validator
    pub fn register(
        &mut self,
        address: Address,
        stake: StakeAmount,
        commission_rate: u16,
    ) -> ConsensusResult<()> {
        // Check if already exists
        if self.validators.contains_key(&address) {
            return Err(ConsensusError::ValidatorAlreadyExists(address.to_hex()));
        }

        // Check minimum stake
        if stake.inner() < self.min_stake.inner() {
            return Err(ConsensusError::InsufficientStake {
                required: self.min_stake.inner().to_u64_digits().first().copied().unwrap_or(0),
                provided: stake.inner().to_u64_digits().first().copied().unwrap_or(0),
            });
        }

        // Create and add validator
        let validator = ValidatorInfo::new(address, stake, commission_rate);
        self.validators.insert(address, validator);

        Ok(())
    }

    /// Unregister a validator (exit)
    pub fn unregister(&mut self, address: &Address) -> ConsensusResult<ValidatorInfo> {
        let mut validator = self.validators.remove(address)
            .ok_or_else(|| ConsensusError::ValidatorNotFound(address.to_hex()))?;

        validator.status = ValidatorStatus::Exited;
        Ok(validator)
    }

    /// Get all active validators
    pub fn active_validators(&self) -> Vec<&ValidatorInfo> {
        self.validators.values()
            .filter(|v| v.is_active())
            .collect()
    }

    /// Get all validators
    pub fn all_validators(&self) -> Vec<&ValidatorInfo> {
        self.validators.values().collect()
    }

    /// Get total staked amount across all validators
    pub fn total_stake(&self) -> StakeAmount {
        self.validators.values()
            .fold(StakeAmount::zero(), |acc, v| {
                acc.checked_add(&v.stake).unwrap_or(acc)
            })
    }

    /// Get total liquidity deployed
    pub fn total_liquidity_deployed(&self) -> Amount {
        self.validators.values()
            .fold(Amount::zero(), |acc, v| {
                acc.checked_add(&v.liquidity_deployed).unwrap_or(acc)
            })
    }

    /// Update validator status
    pub fn update_status(&mut self, address: &Address, status: ValidatorStatus) -> ConsensusResult<()> {
        let validator = self.validators.get_mut(address)
            .ok_or_else(|| ConsensusError::ValidatorNotFound(address.to_hex()))?;

        validator.status = status;
        Ok(())
    }

    /// Process unbonding completions
    pub fn process_unbonding(&mut self, current_time: Timestamp) -> Vec<Address> {
        let mut completed = Vec::new();

        for (address, validator) in &mut self.validators {
            if let ValidatorStatus::Unbonding { unlock_time } = validator.status {
                if current_time >= unlock_time {
                    validator.status = ValidatorStatus::Inactive;
                    completed.push(*address);
                }
            }
        }

        completed
    }

    /// Get validator count
    pub fn count(&self) -> usize {
        self.validators.len()
    }

    /// Get active validator count
    pub fn active_count(&self) -> usize {
        self.active_validators().len()
    }
}

/// Validator abstraction for block production
pub struct Validator {
    info: ValidatorInfo,
    keypair: blockchain_crypto::KeyPair,
}

impl Validator {
    /// Create a new validator instance
    pub fn new(info: ValidatorInfo, keypair: blockchain_crypto::KeyPair) -> Self {
        Self { info, keypair }
    }

    /// Get validator info
    pub fn info(&self) -> &ValidatorInfo {
        &self.info
    }

    /// Get mutable validator info
    pub fn info_mut(&mut self) -> &mut ValidatorInfo {
        &mut self.info
    }

    /// Get validator address
    pub fn address(&self) -> Address {
        self.info.address
    }

    /// Get keypair
    pub fn keypair(&self) -> &blockchain_crypto::KeyPair {
        &self.keypair
    }

    /// Sign data
    pub fn sign(&self, data: &[u8]) -> ConsensusResult<blockchain_crypto::Signature> {
        Ok(self.keypair.sign(data)?)
    }
}

/// Helper function to get current timestamp
fn current_timestamp() -> Timestamp {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use blockchain_crypto::{KeyPair, SignatureScheme};

    #[test]
    fn test_validator_creation() {
        let address = Address::zero();
        let stake = StakeAmount::from_u64(10000);
        let validator = ValidatorInfo::new(address, stake, 500);

        assert!(validator.is_active());
        assert!(validator.can_produce_blocks());
        assert_eq!(validator.commission_rate, 500);
    }

    #[test]
    fn test_validator_set() {
        let mut set = ValidatorSet::new(StakeAmount::from_u64(1000), 14 * 24 * 3600);
        let keypair = KeyPair::generate(SignatureScheme::Ed25519).unwrap();
        let address = keypair.public_key().to_address();

        set.register(address, StakeAmount::from_u64(5000), 100).unwrap();
        assert_eq!(set.count(), 1);
        assert_eq!(set.active_count(), 1);
    }

    #[test]
    fn test_insufficient_stake() {
        let mut set = ValidatorSet::new(StakeAmount::from_u64(10000), 14 * 24 * 3600);
        let address = Address::zero();

        let result = set.register(address, StakeAmount::from_u64(5000), 100);
        assert!(result.is_err());
    }

    #[test]
    fn test_liquidity_deployment() {
        let address = Address::zero();
        let mut validator = ValidatorInfo::new(address, StakeAmount::from_u64(10000), 100);

        validator.deploy_liquidity(&Amount::from_u64(5000)).unwrap();
        assert_eq!(validator.liquidity_deployed, Amount::from_u64(5000));

        validator.withdraw_liquidity(&Amount::from_u64(2000)).unwrap();
        assert_eq!(validator.liquidity_deployed, Amount::from_u64(3000));
    }
}