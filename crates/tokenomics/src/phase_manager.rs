// tokenomics/src/phase_manager.rs

use crate::{
    minting::MintingPhase,
    utility_index::{UtilityIndex, UtilityMetrics},
    TokenomicsError, TokenomicsResult,
};
use blockchain_core::{BlockNumber, Timestamp};
use serde::{Deserialize, Serialize};

/// Initial Volume Threshold configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IVTConfig {
    /// Target cumulative transaction count
    pub target_transactions: u64,
    /// Target unique active addresses
    pub target_addresses: u64,
    /// Allow governance override
    pub allow_governance_override: bool,
}

impl Default for IVTConfig {
    fn default() -> Self {
        Self {
            target_transactions: 1_000_000,    // 1 million transactions
            target_addresses: 100_000,         // 100k unique addresses
            allow_governance_override: true,
        }
    }
}

impl IVTConfig {
    /// Check if IVT has been reached
    pub fn is_reached(&self, cumulative_tx: u64, unique_addresses: u64) -> bool {
        cumulative_tx >= self.target_transactions || unique_addresses >= self.target_addresses
    }
}

/// Phase transition details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseTransition {
    /// Block number when transition occurred
    pub block_number: BlockNumber,
    /// Timestamp of transition
    pub timestamp: Timestamp,
    /// Metrics at transition
    pub metrics: UtilityMetrics,
    /// Whether governance override was used
    pub governance_override: bool,
}

/// Phase manager controlling transition from Phase 1 to Phase 2
pub struct PhaseManager {
    /// Current phase
    current_phase: MintingPhase,
    /// IVT configuration
    ivt_config: IVTConfig,
    /// Cumulative transaction count
    cumulative_transactions: u64,
    /// Unique address count
    unique_addresses: u64,
    /// Transition details (if transitioned)
    transition: Option<PhaseTransition>,
    /// Transition notice period in blocks (7 days)
    notice_period_blocks: u64,
    /// Notice block number (when transition was announced)
    notice_block: Option<BlockNumber>,
    /// Transition blend period in blocks (30 days)
    blend_period_blocks: u64,
    /// Block when actual transition starts
    transition_start_block: Option<BlockNumber>,
}

impl PhaseManager {
    /// Create new phase manager
    pub fn new(ivt_config: IVTConfig) -> Self {
        Self {
            current_phase: MintingPhase::Bootstrap,
            ivt_config,
            cumulative_transactions: 0,
            unique_addresses: 0,
            transition: None,
            notice_period_blocks: 201_600,  // ~7 days at 3s blocks
            notice_block: None,
            blend_period_blocks: 864_000,   // ~30 days at 3s blocks
            transition_start_block: None,
        }
    }

    /// Get current phase
    pub fn current_phase(&self) -> MintingPhase {
        self.current_phase
    }

    /// Get IVT configuration
    pub fn ivt_config(&self) -> &IVTConfig {
        &self.ivt_config
    }

    /// Update cumulative statistics
    pub fn update_statistics(&mut self, transactions: u64, addresses: u64) {
        self.cumulative_transactions += transactions;
        self.unique_addresses = self.unique_addresses.max(addresses);
    }

    /// Check if IVT has been reached
    pub fn is_ivt_reached(&self) -> bool {
        self.ivt_config.is_reached(
            self.cumulative_transactions,
            self.unique_addresses,
        )
    }

    /// Announce phase transition (7-day notice)
    pub fn announce_transition(&mut self, current_block: BlockNumber) -> TokenomicsResult<()> {
        if self.current_phase != MintingPhase::Bootstrap {
            return Err(TokenomicsError::PhaseTransitionError(
                "Already transitioned or in progress".into()
            ));
        }

        if !self.is_ivt_reached() {
            return Err(TokenomicsError::PhaseTransitionError(
                "IVT not reached yet".into()
            ));
        }

        self.notice_block = Some(current_block);
        
        tracing::info!(
            "Phase transition announced at block {}. Transition will begin at block {}",
            current_block,
            current_block + self.notice_period_blocks
        );

        Ok(())
    }

    /// Execute phase transition (after notice period)
    pub fn execute_transition(
        &mut self,
        current_block: BlockNumber,
        metrics: UtilityMetrics,
    ) -> TokenomicsResult<UtilityIndex> {
        // Check notice period has passed
        if let Some(notice_block) = self.notice_block {
            if current_block < notice_block + self.notice_period_blocks {
                return Err(TokenomicsError::PhaseTransitionError(
                    format!("Notice period not complete. {} blocks remaining", 
                        notice_block + self.notice_period_blocks - current_block)
                ));
            }
        } else {
            return Err(TokenomicsError::PhaseTransitionError(
                "Transition not announced".into()
            ));
        }

        // Mark transition start
        self.transition_start_block = Some(current_block);
        
        // Create transition record
        let transition = PhaseTransition {
            block_number: current_block,
            timestamp: current_timestamp(),
            metrics: metrics.clone(),
            governance_override: false,
        };

        self.transition = Some(transition);

        // Create utility index with baseline
        let utility_index = UtilityIndex::with_baseline(metrics);

        tracing::info!(
            "Phase transition executed at block {}. Blend period: {} blocks",
            current_block,
            self.blend_period_blocks
        );

        Ok(utility_index)
    }

    /// Execute governance override transition
    pub fn execute_governance_override(
        &mut self,
        current_block: BlockNumber,
        metrics: UtilityMetrics,
    ) -> TokenomicsResult<UtilityIndex> {
        if !self.ivt_config.allow_governance_override {
            return Err(TokenomicsError::PhaseTransitionError(
                "Governance override not allowed".into()
            ));
        }

        if self.current_phase != MintingPhase::Bootstrap {
            return Err(TokenomicsError::PhaseTransitionError(
                "Already transitioned".into()
            ));
        }

        self.transition_start_block = Some(current_block);
        
        let transition = PhaseTransition {
            block_number: current_block,
            timestamp: current_timestamp(),
            metrics: metrics.clone(),
            governance_override: true,
        };

        self.transition = Some(transition);

        let utility_index = UtilityIndex::with_baseline(metrics);

        tracing::warn!(
            "Phase transition executed via governance override at block {}",
            current_block
        );

        Ok(utility_index)
    }

    /// Calculate blend factor λ(t) for transition period
    /// λ(t) = max(0, 1 - (t - t_transition) / T_transition)
    /// Returns value between 0 and 1:
    /// - 1.0 = fully Phase 1
    /// - 0.0 = fully Phase 2
    pub fn calculate_blend_factor(& mut self, current_block: BlockNumber) -> f64 {
        if let Some(start_block) = self.transition_start_block {
            if current_block < start_block {
                return 1.0; // Still in Phase 1
            }

            let blocks_since_transition = current_block - start_block;
            
            if blocks_since_transition >= self.blend_period_blocks {
                self.current_phase = MintingPhase::UtilityDriven;
                return 0.0; // Fully Phase 2
            }

            // Linear interpolation
            let progress = blocks_since_transition as f64 / self.blend_period_blocks as f64;
            1.0 - progress
        } else {
            1.0 // No transition started
        }
    }

    /// Check if currently in blend period
    pub fn is_in_blend_period(&self, current_block: BlockNumber) -> bool {
        if let Some(start_block) = self.transition_start_block {
            let blocks_since = current_block.saturating_sub(start_block);
            blocks_since < self.blend_period_blocks
        } else {
            false
        }
    }

    /// Get transition details
    pub fn transition_details(&self) -> Option<&PhaseTransition> {
        self.transition.as_ref()
    }

    /// Get progress statistics
    pub fn get_progress(&self) -> PhaseProgress {
        let tx_progress = if self.ivt_config.target_transactions > 0 {
            (self.cumulative_transactions as f64 / self.ivt_config.target_transactions as f64 * 100.0).min(100.0)
        } else {
            0.0
        };

        let address_progress = if self.ivt_config.target_addresses > 0 {
            (self.unique_addresses as f64 / self.ivt_config.target_addresses as f64 * 100.0).min(100.0)
        } else {
            0.0
        };

        PhaseProgress {
            current_phase: self.current_phase,
            cumulative_transactions: self.cumulative_transactions,
            target_transactions: self.ivt_config.target_transactions,
            unique_addresses: self.unique_addresses,
            target_addresses: self.ivt_config.target_addresses,
            tx_progress_percent: tx_progress,
            address_progress_percent: address_progress,
            is_ivt_reached: self.is_ivt_reached(),
            notice_block: self.notice_block,
            transition_start_block: self.transition_start_block,
        }
    }
}

/// Phase progress information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseProgress {
    pub current_phase: MintingPhase,
    pub cumulative_transactions: u64,
    pub target_transactions: u64,
    pub unique_addresses: u64,
    pub target_addresses: u64,
    pub tx_progress_percent: f64,
    pub address_progress_percent: f64,
    pub is_ivt_reached: bool,
    pub notice_block: Option<BlockNumber>,
    pub transition_start_block: Option<BlockNumber>,
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

    #[test]
    fn test_ivt_config() {
        let config = IVTConfig::default();
        
        assert!(!config.is_reached(500_000, 50_000));
        assert!(config.is_reached(1_000_000, 50_000));
        assert!(config.is_reached(500_000, 100_000));
    }

    #[test]
    fn test_phase_manager_creation() {
        let config = IVTConfig::default();
        let manager = PhaseManager::new(config);
        
        assert_eq!(manager.current_phase(), MintingPhase::Bootstrap);
        assert!(!manager.is_ivt_reached());
    }

    #[test]
    fn test_ivt_progress() {
        let config = IVTConfig::default();
        let mut manager = PhaseManager::new(config);
        
        manager.update_statistics(500_000, 50_000);
        assert!(!manager.is_ivt_reached());
        
        manager.update_statistics(500_000, 50_001);
        assert!(manager.is_ivt_reached());
    }

    #[test]
    fn test_transition_announcement() {
        let config = IVTConfig::default();
        let mut manager = PhaseManager::new(config);
        
        // Should fail - IVT not reached
        assert!(manager.announce_transition(1000).is_err());
        
        // Reach IVT
        manager.update_statistics(1_000_000, 0);
        assert!(manager.announce_transition(1000).is_ok());
    }

    #[test]
    fn test_blend_factor() {
        let config = IVTConfig::default();
        let mut manager = PhaseManager::new(config);
        manager.transition_start_block = Some(1000);
        
        // At start
        assert_eq!(manager.calculate_blend_factor(1000), 1.0);
        
        // Halfway through
        let halfway = 1000 + manager.blend_period_blocks / 2;
        assert!((manager.calculate_blend_factor(halfway) - 0.5).abs() < 0.01);
        
        // After completion
        let end = 1000 + manager.blend_period_blocks;
        assert_eq!(manager.calculate_blend_factor(end), 0.0);
    }

    #[test]
    fn test_governance_override() {
        let config = IVTConfig::default();
        let mut manager = PhaseManager::new(config);
        
        let metrics = UtilityMetrics::new(1000);
        let result = manager.execute_governance_override(1000, metrics);
        
        assert!(result.is_ok());
        assert!(manager.transition_details().is_some());
        assert!(manager.transition_details().unwrap().governance_override);
    }
}