// consensus/src/poas.rs

use crate::{
    selection::ValidatorSelector,
    slashing::SlashingManager,
    validator::{ValidatorInfo, ValidatorSet},
    ConsensusError, ConsensusResult,
};
use blockchain_core::{Block, BlockNumber, StakeAmount, Timestamp};
use blockchain_crypto::Address;
use serde::{Deserialize, Serialize};

/// Configuration for PoAS consensus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusConfig {
    /// Block time target in seconds
    pub block_time: u64,
    /// Minimum stake required to be a validator (e.g., 10,000 tokens)
    pub min_stake: StakeAmount,
    /// Unbonding period in seconds (e.g., 14 days)
    pub unbonding_period: u64,
    /// Required uptime percentage (basis points, 9500 = 95%)
    pub required_uptime: u16,
    /// Blocks before downtime slashing
    pub max_downtime_blocks: u64,
    /// Number of blocks for finality
    pub finality_blocks: u64,
    /// Target validator count
    pub target_validator_count: usize,
    /// Maximum validator count
    pub max_validator_count: usize,
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            block_time: 3,                          // 3 seconds
            min_stake: StakeAmount::from_u64(10000), // 10,000 tokens
            unbonding_period: 14 * 24 * 3600,       // 14 days
            required_uptime: 9500,                  // 95%
            max_downtime_blocks: 28800,             // ~1 day at 3s blocks
            finality_blocks: 2,                     // 6 seconds
            target_validator_count: 100,
            max_validator_count: 1000,
        }
    }
}

/// Main PoAS consensus engine
pub struct PoASConsensus {
    /// Configuration
    config: ConsensusConfig,
    /// Validator set
    validator_set: ValidatorSet,
    /// Validator selector
    selector: ValidatorSelector,
    /// Slashing manager
    slashing: SlashingManager,
    /// Current epoch number
    current_epoch: u64,
    /// Blocks per epoch
    blocks_per_epoch: u64,
}

impl PoASConsensus {
    /// Create a new PoAS consensus engine
    pub fn new(config: ConsensusConfig) -> Self {
        let validator_set = ValidatorSet::new(
            config.min_stake.clone(),
            config.unbonding_period,
        );
        
        Self {
            config,
            validator_set,
            selector: ValidatorSelector::new_random(),
            slashing: SlashingManager::new(),
            current_epoch: 0,
            blocks_per_epoch: 28800, // ~1 day at 3s blocks
        }
    }

    /// Get consensus configuration
    pub fn config(&self) -> &ConsensusConfig {
        &self.config
    }

    /// Get validator set
    pub fn validator_set(&self) -> &ValidatorSet {
        &self.validator_set
    }

    /// Get mutable validator set
    pub fn validator_set_mut(&mut self) -> &mut ValidatorSet {
        &mut self.validator_set
    }

    /// Get slashing manager
    pub fn slashing_manager(&self) -> &SlashingManager {
        &self.slashing
    }

    /// Get mutable slashing manager
    pub fn slashing_manager_mut(&mut self) -> &mut SlashingManager {
        &mut self.slashing
    }

    /// Select the next block proposer
    pub fn select_proposer(&mut self, slot: u64) -> ConsensusResult<Address> {
        let active_validators = self.validator_set.active_validators();
        
        if active_validators.is_empty() {
            return Err(ConsensusError::SelectionError(
                "No active validators available".into()
            ));
        }

        // Use deterministic selection for predictability
        self.selector.select_for_slot(&active_validators, slot)
    }

    /// Validate a proposed block
    pub fn validate_block(&self, block: &Block, parent: &Block) -> ConsensusResult<()> {
        // Basic block validation
        block.validate(parent)
            .map_err(|e| ConsensusError::ValidationError(e.to_string()))?;

        // Verify proposer is a valid validator
        let proposer = block.header.proposer;
        let validator = self.validator_set.get(&proposer)
            .ok_or_else(|| ConsensusError::ValidatorNotFound(proposer.to_hex()))?;

        // Check validator can produce blocks
        if !validator.can_produce_blocks() {
            return Err(ConsensusError::ValidationError(
                "Proposer cannot produce blocks".into()
            ));
        }

        // Verify block time
        let expected_time = parent.header.timestamp + self.config.block_time;
        let time_diff = if block.header.timestamp > expected_time {
            block.header.timestamp - expected_time
        } else {
            expected_time - block.header.timestamp
        };

        // Allow some tolerance (±2 seconds)
        if time_diff > 2 {
            return Err(ConsensusError::ValidationError(
                format!("Block time outside acceptable range: {}s difference", time_diff)
            ));
        }

        Ok(())
    }

    /// Process a finalized block
    pub fn finalize_block(&mut self, block: &Block) -> ConsensusResult<()> {
        let proposer = block.header.proposer;
        
        // Update validator statistics
        if let Some(validator) = self.validator_set.get_mut(&proposer) {
            validator.update_uptime(true);
            
            // Check for downtime slashing
            self.slashing.check_downtime_slashing(
                validator,
                self.config.max_downtime_blocks,
            )?;
        }

        // Update epoch if needed
        if block.header.number % self.blocks_per_epoch == 0 {
            self.process_epoch_transition(block.header.number)?;
        }

        Ok(())
    }

    /// Process epoch transition
    fn process_epoch_transition(&mut self, block_number: BlockNumber) -> ConsensusResult<()> {
        self.current_epoch += 1;

        // Process unbonding completions
        let current_time = current_timestamp();
        let completed = self.validator_set.process_unbonding(current_time);

        tracing::info!(
            "Epoch {} transition at block {}, {} validators completed unbonding",
            self.current_epoch,
            block_number,
            completed.len()
        );

        // Check validator performance
        for validator in self.validator_set.all_validators() {
            if validator.uptime < self.config.required_uptime {
                tracing::warn!(
                    "Validator {} below required uptime: {}%",
                    validator.address.to_hex(),
                    validator.uptime as f64 / 100.0
                );
            }
        }

        Ok(())
    }

    /// Check if a block is finalized
    pub fn is_finalized(&self, block_number: BlockNumber, head: BlockNumber) -> bool {
        head >= block_number + self.config.finality_blocks
    }

    /// Register a new validator
    pub fn register_validator(
        &mut self,
        address: Address,
        stake: StakeAmount,
        commission_rate: u16,
    ) -> ConsensusResult<()> {
        // Check maximum validator count
        if self.validator_set.count() >= self.config.max_validator_count {
            return Err(ConsensusError::ValidatorError(
                "Maximum validator count reached".into()
            ));
        }

        self.validator_set.register(address, stake.clone(), commission_rate)?;

        tracing::info!(
            "Validator {} registered with stake {} and commission {}%",
            address.to_hex(),
            stake,
            commission_rate as f64 / 100.0
        );

        Ok(())
    }

    /// Unregister a validator
    pub fn unregister_validator(&mut self, address: &Address) -> ConsensusResult<ValidatorInfo> {
        let validator = self.validator_set.unregister(address)?;

        tracing::info!(
            "Validator {} unregistered",
            address.to_hex()
        );

        Ok(validator)
    }

    /// Get current epoch
    pub fn current_epoch(&self) -> u64 {
        self.current_epoch
    }

    /// Calculate network security metrics
    pub fn calculate_security_metrics(&self) -> SecurityMetrics {
        let validators = self.validator_set.active_validators();
        
        let nakamoto_coefficient = crate::selection::SelectionProbability::nakamoto_coefficient(&validators);
        let gini_coefficient = crate::selection::SelectionProbability::gini_coefficient(&validators);
        
        let total_stake = self.validator_set.total_stake();
        let total_liquidity = self.validator_set.total_liquidity_deployed();
        
        // Calculate attack cost (33% of stake)
        let attack_cost = StakeAmount::new(
            (total_stake.inner() * 33u64) / 100u64
        );

        SecurityMetrics {
            nakamoto_coefficient,
            gini_coefficient,
            total_stake,
            total_liquidity,
            active_validators: validators.len(),
            attack_cost,
        }
    }

    /// Calculate validator rewards (placeholder - actual implementation in tokenomics)
    pub fn calculate_validator_reward(
        &self,
        validator: &ValidatorInfo,
        base_reward: &StakeAmount,
    ) -> StakeAmount {
        // Simplified calculation
        let total_stake = self.validator_set.total_stake();
        
        if total_stake.is_zero() {
            return StakeAmount::zero();
        }

        // Reward proportional to stake and utility
        let stake_ratio = validator.stake.inner().clone() * 10000u64 / total_stake.inner().clone();
        let utility_factor = 10000 + validator.utility_score.value(); // Base 1.0 + utility
        
        let reward_amount = (base_reward.inner() * stake_ratio * utility_factor) / (10000u64 * 10000u64);
        
        StakeAmount::new(reward_amount)
    }
}

/// Network security metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityMetrics {
    /// Minimum validators needed to control 33% of weight
    pub nakamoto_coefficient: usize,
    /// Stake distribution inequality (0-1)
    pub gini_coefficient: f64,
    /// Total staked amount
    pub total_stake: StakeAmount,
    /// Total liquidity deployed
    pub total_liquidity: StakeAmount,
    /// Number of active validators
    pub active_validators: usize,
    /// Cost to attack network (33% stake)
    pub attack_cost: StakeAmount,
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
    use blockchain_crypto::{KeyPair, SignatureScheme};

    #[test]
    fn test_consensus_creation() {
        let config = ConsensusConfig::default();
        let consensus = PoASConsensus::new(config);
        
        assert_eq!(consensus.current_epoch(), 0);
        assert_eq!(consensus.validator_set().count(), 0);
    }

    #[test]
    fn test_validator_registration() {
        let config = ConsensusConfig::default();
        let mut consensus = PoASConsensus::new(config);
        
        let keypair = KeyPair::generate(SignatureScheme::Ed25519).unwrap();
        let address = keypair.public_key().to_address();
        
        consensus.register_validator(
            address,
            StakeAmount::from_u64(20000),
            500,
        ).unwrap();
        
        assert_eq!(consensus.validator_set().count(), 1);
        assert_eq!(consensus.validator_set().active_count(), 1);
    }

    #[test]
    fn test_proposer_selection() {
        let config = ConsensusConfig::default();
        let mut consensus = PoASConsensus::new(config);
        
        // Register multiple validators
        for i in 0..5 {
            let keypair = KeyPair::generate(SignatureScheme::Ed25519).unwrap();
            let address = keypair.public_key().to_address();
            consensus.register_validator(
                address,
                StakeAmount::from_u64(10000 + i * 1000),
                100,
            ).unwrap();
        }
        
        let proposer = consensus.select_proposer(1).unwrap();
        assert!(consensus.validator_set().get(&proposer).is_some());
    }

    #[test]
    fn test_security_metrics() {
        let config = ConsensusConfig::default();
        let mut consensus = PoASConsensus::new(config);
        
        // Register validators
        for i in 0..10 {
            let keypair = KeyPair::generate(SignatureScheme::Ed25519).unwrap();
            let address = keypair.public_key().to_address();
            consensus.register_validator(
                address,
                StakeAmount::from_u64(10000 + i * 5000),
                100,
            ).unwrap();
        }
        
        let metrics = consensus.calculate_security_metrics();
        assert_eq!(metrics.active_validators, 10);
        assert!(metrics.nakamoto_coefficient > 0);
        assert!(metrics.gini_coefficient >= 0.0 && metrics.gini_coefficient <= 1.0);
    }
}