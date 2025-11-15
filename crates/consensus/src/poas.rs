// consensus/src/poas.rs

use crate::{
    selection::ValidatorSelector,
    slashing::SlashingManager,
    validator::{ValidatorInfo, ValidatorSet},
    ConsensusError, ConsensusResult,
};
use blockchain_core::{Block, BlockNumber, StakeAmount, Timestamp, fork::{ForkChoice, ForkResolver, ForkInfo, ReorgPath}, mempool::TransactionPool, Gas};
use blockchain_crypto::Hash;
use blockchain_crypto::Address;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    /// Fork choice rule for chain selection
    pub fork_choice: ForkChoice,
    /// Maximum allowed reorg depth
    pub max_reorg_depth: u64,
    /// Enable fork detection and resolution
    pub enable_fork_detection: bool,
    /// Slash validators for producing blocks on wrong fork
    pub slash_for_wrong_fork: bool,
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
            fork_choice: ForkChoice::LatestJustified,
            max_reorg_depth: 100,
            enable_fork_detection: true,
            slash_for_wrong_fork: true,
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
    /// Fork resolver for chain reorganizations
    fork_resolver: ForkResolver,
    /// Finality checkpoints: block_number -> is_justified
    finality_checkpoints: HashMap<BlockNumber, bool>,
    /// Fork events for metrics
    fork_events: Vec<(BlockNumber, String)>,
    /// Highest justified checkpoint (block number)
    highest_justified: Option<BlockNumber>,
    /// Metrics: total forks observed
    fork_frequency: u64,
    /// Metrics: total reorg depth observed
    total_reorg_depth: u64,
    /// Metrics: maximum reorg depth observed
    max_reorg_depth_observed: u64,
    /// Finality time records: block_number -> timestamp when justified
    finality_times: Vec<(BlockNumber, u64)>,
}

impl PoASConsensus {
    /// Create a new PoAS consensus engine
    pub fn new(config: ConsensusConfig) -> Self {
        let validator_set = ValidatorSet::new(
            config.min_stake.clone(),
            config.unbonding_period,
        );
        
        let fork_resolver = ForkResolver::new(config.fork_choice, config.max_reorg_depth);
        
        Self {
            config,
            validator_set,
            selector: ValidatorSelector::new_random(),
            slashing: SlashingManager::new(),
            current_epoch: 0,
            blocks_per_epoch: 28800, // ~1 day at 3s blocks
            fork_resolver,
            finality_checkpoints: HashMap::new(),
            fork_events: Vec::new(),
            highest_justified: None,
            fork_frequency: 0,
            total_reorg_depth: 0,
            max_reorg_depth_observed: 0,
            finality_times: Vec::new(),
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

        // Fork detection
        if self.config.enable_fork_detection {
            if let Some(fork_info) = self.fork_resolver.detect_fork(parent, block) {
                // Log fork detection for observability; deeper handling happens during block application
                tracing::warn!("Fork detected at {}: main_tip={}, fork_tip={}",
                    fork_info.fork_point, fork_info.main_tip, fork_info.fork_tip);

                // If the fork is deeper than allowed, reject the block
                if fork_info.fork_length > self.config.max_reorg_depth {
                    return Err(ConsensusError::BlockchainError(
                        blockchain_core::BlockchainError::ReorgTooDeep { depth: fork_info.fork_length }
                    ));
                }
            }
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
        // If we have a justified checkpoint, enforce it as final
        if let Some(j) = self.highest_justified {
            return block_number <= j;
        }

        head >= block_number + self.config.finality_blocks
    }

    /// Mark a block as justified (justified checkpoint)
    pub fn update_justified_checkpoint(&mut self, block_number: BlockNumber) {
        let entry = self.finality_checkpoints.entry(block_number).or_insert(true);
        *entry = true;

        self.highest_justified = Some(std::cmp::max(self.highest_justified.unwrap_or(0), block_number));
        // Record finality timestamp
        self.finality_times.push((block_number, current_timestamp()));
        tracing::info!("Justified checkpoint updated: {}", block_number);
    }

    /// Verify before block production that head is canonical and safe to build on
    pub fn verify_before_produce(&self, head: &Hash) -> ConsensusResult<()> {
        if !self.verify_chain_canonical(head) {
            return Err(ConsensusError::BlockProductionError("Head is not canonical according to fork history".into()));
        }

        // Do not produce if head would violate a justified checkpoint
        if let Some(j) = self.highest_justified {
            // Here we would check head block number against justified checkpoint; simplified: ensure not empty
            // In a full node this would translate head hash to block number via storage and compare.
        }

        Ok(())
    }

    /// Get basic fork metrics: (frequency, total_reorg_depth, max_reorg_depth)
    pub fn fork_metrics(&self) -> (u64, u64, u64) {
        (self.fork_frequency, self.total_reorg_depth, self.max_reorg_depth_observed)
    }

    /// Get finality time records
    pub fn finality_records(&self) -> &Vec<(BlockNumber, u64)> {
        &self.finality_times
    }

    /// Produce a new block: select transactions from mempool, validate head canonicality,
    /// and return the constructed block. Removes included transactions from the pool.
    pub fn produce_block(&mut self, parent: &Block, proposer: Address, pool: &mut TransactionPool) -> ConsensusResult<Block> {
        // Pre-production checks
        self.verify_before_produce(&parent.hash())?;

        // Choose transactions by gas price up to gas limit
        let max_gas = parent.header.gas_limit;
        let txs = pool.get_pending(max_gas, 1000);

        // Filter transactions by basic validity
        let mut valid_txs = Vec::new();
        for tx in txs {
            if tx.validate_basic().is_ok() {
                valid_txs.push(tx);
            }
        }

        // Create block
        let number = parent.number() + 1;
        let state_root = Hash::zero(); // state root computed during execution in full node
        let block = Block::new(number, parent.hash(), state_root, proposer, valid_txs.clone(), max_gas)
            .map_err(|e| ConsensusError::BlockchainError(e))?;

        // Remove included transactions from pool
        pool.remove_included(&valid_txs);

        // Update proposer stats
        if let Some(v) = self.validator_set.get_mut(&proposer) {
            v.blocks_produced += 1;
            v.update_uptime(true);
        }

        Ok(block)
    }

    /// Get validator participation rates and missed blocks map
    pub fn validator_participation(&self) -> Vec<(Address, u16, u64, u64)> {
        // Returns (address, uptime, produced, missed)
        self.validator_set.all_validators().into_iter().map(|v| {
            (v.address, v.uptime, v.blocks_produced, v.blocks_missed)
        }).collect()
    }

    /// Apply a reorganization path. This records fork events, enforces justified checkpoints,
    /// and performs basic slashing for double-signing evidence.
    ///
    /// An optional persistence callback may be provided to persist fork events. The callback
    /// receives `(&ForkInfo, reorg_depth, resolution_str)` and should return `ConsensusResult<()>`.
    pub fn apply_reorg(&mut self, reorg: ReorgPath, mut persist: Option<&mut dyn FnMut(&ForkInfo, u64, &str) -> ConsensusResult<()>>) -> ConsensusResult<()> {
        // Determine common ancestor block number from revert/apply blocks
        let common_ancestor_number = if !reorg.revert_blocks.is_empty() {
            reorg.revert_blocks.iter().map(|b| b.header.number).min().unwrap_or(0).saturating_sub(1)
        } else if !reorg.apply_blocks.is_empty() {
            reorg.apply_blocks[0].header.number.saturating_sub(1)
        } else {
            return Err(ConsensusError::ValidationError("Empty reorg path".into()));
        };

        // Prevent reorgs that revert past justified checkpoints
        if let Some(j) = self.highest_justified {
            if common_ancestor_number < j {
                return Err(ConsensusError::BlockchainError(
                    blockchain_core::BlockchainError::ReorgTooDeep { depth: reorg.depth }
                ));
            }
        }

        // Detect double-signing: same proposer produced blocks at same height on both sides
        for a in &reorg.apply_blocks {
            for r in &reorg.revert_blocks {
                if a.header.number == r.header.number && a.header.proposer == r.header.proposer {
                    tracing::warn!("Double-sign detected: validator={} at height {}", a.header.proposer.to_hex(), a.header.number);
                    if self.config.slash_for_wrong_fork {
                        if let Some(validator) = self.validator_set.get_mut(&a.header.proposer) {
                            let _penalty = self.slashing.slash_validator(
                                validator,
                                crate::slashing::SlashingCondition::DoubleSigning,
                                Some(a.hash()),
                            )?;
                        }
                    }
                }
            }
        }

        // Record fork info and metrics
        let main_tip = reorg.revert_blocks.get(0).map(|b| b.hash()).unwrap_or_else(|| reorg.common_ancestor);
        let fork_tip = reorg.apply_blocks.last().map(|b| b.hash()).unwrap_or_else(|| reorg.common_ancestor);
        let fork_info = ForkInfo {
            fork_point: common_ancestor_number,
            fork_hash: reorg.common_ancestor,
            main_tip,
            fork_tip,
            main_length: reorg.revert_blocks.len() as u64,
            fork_length: reorg.apply_blocks.len() as u64,
        };

        self.fork_resolver.record_fork(fork_info.clone());
        self.fork_events.push((common_ancestor_number, "resolved".into()));

        // Update metrics
        self.fork_frequency += 1;
        self.total_reorg_depth += reorg.depth;
        if reorg.depth > self.max_reorg_depth_observed {
            self.max_reorg_depth_observed = reorg.depth;
        }

        // Use fork_resolver to choose which chain to keep (simplified)
        let choice = self.fork_resolver.choose_chain(&reorg.revert_blocks, &reorg.apply_blocks)?;
        let resolution = if choice { "fork_chain" } else { "main_chain" };

        // Persist fork event if a persistence callback was provided
        if let Some(cb) = persist.as_mut() {
            cb(&fork_info, reorg.depth, resolution)?;
        }

        if choice {
            tracing::info!("Reorg chosen: switch to fork chain (depth={})", reorg.depth);
        } else {
            tracing::info!("Reorg chosen: keep main chain (depth={})", reorg.depth);
        }

        Ok(())
    }

    /// Verify that the given head hash is canonical according to fork resolver/history.
    /// Note: Full verification requires chain storage; this is a best-effort check against recent fork history.
    pub fn verify_chain_canonical(&self, head: &Hash) -> bool {
        // If our fork history contains this head as a fork tip that lost, it's not canonical
        for info in self.fork_resolver.fork_history() {
            if info.fork_tip == *head && info.fork_length <= info.main_length {
                return false;
            }
        }

        true
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
    fn test_apply_reorg_and_slash() {
        let config = ConsensusConfig::default();
        let mut consensus = PoASConsensus::new(config);

        // Create a validator and register
        let kp = KeyPair::generate(SignatureScheme::Ed25519).unwrap();
        let addr = kp.public_key().to_address();
        consensus.register_validator(addr, StakeAmount::from_u64(100000), 100).unwrap();

        // Create two conflicting blocks at same height with same proposer to simulate double-sign
        let genesis = Block::genesis(Hash::zero());
        let mut block_main = Block::new(1, genesis.hash(), Hash::zero(), addr, vec![], 10_000_000).unwrap();
        let mut block_fork = Block::new(1, genesis.hash(), Hash::zero(), addr, vec![], 10_000_000).unwrap();

        // Build a reorg path: revert main (one block) and apply fork (one block)
        let reorg = ReorgPath {
            common_ancestor: genesis.hash(),
            revert_blocks: vec![block_main.clone()],
            apply_blocks: vec![block_fork.clone()],
            depth: 1,
        };

        // Apply reorg without persistence (None)
        consensus.apply_reorg(reorg, None).unwrap();

        // Validator should have been slashed for double-sign
        let v = consensus.validator_set().get(&addr).unwrap();
        assert!(v.stake.inner() < StakeAmount::from_u64(100000).inner());
    }

    #[test]
    fn test_verify_before_produce_and_metrics() {
        let config = ConsensusConfig::default();
        let mut consensus = PoASConsensus::new(config);

        // No forks yet — head should be considered canonical
        let head = Hash::zero();
        assert!(consensus.verify_before_produce(&head).is_ok());

        // Create a fake fork info and record it via fork_resolver
        let fork_info = ForkInfo {
            fork_point: 0,
            fork_hash: Hash::zero(),
            main_tip: Hash::zero(),
            fork_tip: Hash::new([1u8; 32]),
            main_length: 2,
            fork_length: 1,
        };
        consensus.fork_resolver.record_fork(fork_info);

        // Now fork_tip that lost should be non-canonical
        let non_canonical = Hash::new([1u8; 32]);
        assert!(!consensus.verify_chain_canonical(&non_canonical));

        // Metrics getters
        let (freq, total_depth, max_depth) = consensus.fork_metrics();
        assert_eq!(freq, 0);
        assert_eq!(total_depth, 0);
        assert_eq!(max_depth, 0);
    }

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