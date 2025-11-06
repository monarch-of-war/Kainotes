// tokenomics/src/minting.rs

use crate::{
    phase_manager::PhaseManager,
    utility_index::UtilityIndex,
    TokenomicsError, TokenomicsResult,
};
use blockchain_core::{Amount, BlockNumber};
use serde::{Deserialize, Serialize};

/// Minting phases
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MintingPhase {
    /// Phase 1: Bootstrap/Adoption-driven minting
    Bootstrap,
    /// Phase 2: Utility-driven minting
    UtilityDriven,
}

/// Minting configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintingConfig {
    /// Base minting rate (tokens per block)
    pub base_rate: Amount,
    /// Bootstrap multiplier α (e.g., 2.0 for 3x initial rewards)
    pub bootstrap_multiplier: f64,
    /// Decay constant β (e.g., 0.0001 per block)
    pub decay_constant: f64,
    /// Minimum minting rate for Phase 2
    pub min_rate: Amount,
    /// Maximum minting rate for Phase 2
    pub max_rate: Amount,
    /// Sensitivity parameter k for sigmoid function
    pub sensitivity: f64,
}

impl Default for MintingConfig {
    fn default() -> Self {
        Self {
            base_rate: Amount::from_u64(1000),        // 1000 tokens/block
            bootstrap_multiplier: 2.0,                 // 3x initial (1 + 2.0)
            decay_constant: 0.0001,
            min_rate: Amount::from_u64(100),          // 100 tokens/block minimum
            max_rate: Amount::from_u64(2000),         // 2000 tokens/block maximum
            sensitivity: 2.0,
        }
    }
}

/// Minting controller implementing dual-phase minting
pub struct MintingController {
    /// Configuration
    config: MintingConfig,
    /// Phase manager
    phase_manager: PhaseManager,
    /// Utility index (for Phase 2)
    utility_index: Option<UtilityIndex>,
    /// Genesis block number
    genesis_block: BlockNumber,
    /// Total minted amount
    total_minted: Amount,
}

impl MintingController {
    /// Create new minting controller
    pub fn new(
        config: MintingConfig,
        phase_manager: PhaseManager,
        genesis_block: BlockNumber,
    ) -> Self {
        Self {
            config,
            phase_manager,
            utility_index: None,
            genesis_block,
            total_minted: Amount::zero(),
        }
    }

    /// Get current phase
    pub fn current_phase(&self) -> MintingPhase {
        self.phase_manager.current_phase()
    }

    /// Set utility index (for Phase 2)
    pub fn set_utility_index(&mut self, utility_index: UtilityIndex) {
        self.utility_index = Some(utility_index);
    }

    /// Calculate Phase 1 minting rate
    /// M₁(t) = M_base × (1 + α × e^(-βt))
    pub fn calculate_phase1_rate(&self, block_number: BlockNumber) -> Amount {
        let t = block_number.saturating_sub(self.genesis_block) as f64;
        let base = self.config.base_rate.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0) as f64;
        
        let decay = (-self.config.decay_constant * t).exp();
        let multiplier = 1.0 + self.config.bootstrap_multiplier * decay;
        let rate = base * multiplier;
        
        Amount::from_u64(rate as u64)
    }

    /// Calculate Phase 2 minting rate
    /// M₂(t) = M_min + (M_max - M_min) × sigmoid(UI(t) - 1)
    pub fn calculate_phase2_rate(&self) -> TokenomicsResult<Amount> {
        let utility_index = self.utility_index.as_ref()
            .ok_or_else(|| TokenomicsError::MintingError(
                "Utility index not set for Phase 2".into()
            ))?;

        let ui = utility_index.calculate();
        let sigmoid_input = ui - 1.0;
        let sigmoid_value = self.sigmoid(sigmoid_input);

        let min = self.config.min_rate.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0) as f64;
        let max = self.config.max_rate.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0) as f64;

        let rate = min + (max - min) * sigmoid_value;
        
        Ok(Amount::from_u64(rate as u64))
    }

    /// Sigmoid function: 1 / (1 + e^(-k×x))
    fn sigmoid(&self, x: f64) -> f64 {
        1.0 / (1.0 + (-self.config.sensitivity * x).exp())
    }

    /// Calculate blended minting rate (during transition)
    /// R(i,t) = λ(t) × R₁(i,t) + (1 - λ(t)) × R₂(i,t)
    pub fn calculate_blended_rate(
        & mut self,
        block_number: BlockNumber,
    ) -> TokenomicsResult<Amount> {
        let blend_factor = self.phase_manager.calculate_blend_factor(block_number);
        
        let phase1_rate = self.calculate_phase1_rate(block_number);
        let phase2_rate = self.calculate_phase2_rate()?;

        let phase1_val = phase1_rate.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0) as f64;
        let phase2_val = phase2_rate.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0) as f64;

        let blended = phase1_val * blend_factor + phase2_val * (1.0 - blend_factor);
        
        Ok(Amount::from_u64(blended as u64))
    }

    /// Get minting rate for current block
    pub fn get_minting_rate(&mut self, block_number: BlockNumber) -> TokenomicsResult<Amount> {
        if self.phase_manager.is_in_blend_period(block_number) {
            self.calculate_blended_rate(block_number)
        } else {
            match self.current_phase() {
                MintingPhase::Bootstrap => Ok(self.calculate_phase1_rate(block_number)),
                MintingPhase::UtilityDriven => self.calculate_phase2_rate(),
            }
        }
    }

    /// Mint tokens for a block
    pub fn mint_for_block(&mut self, block_number: BlockNumber) -> TokenomicsResult<Amount> {
        let amount = self.get_minting_rate(block_number)?;
        
        self.total_minted = self.total_minted.checked_add(&amount)
            .ok_or_else(|| TokenomicsError::OverflowError("Total minted overflow".into()))?;

        Ok(amount)
    }

    /// Get total minted amount
    pub fn total_minted(&self) -> &Amount {
        &self.total_minted
    }

    /// Get phase manager
    pub fn phase_manager(&self) -> &PhaseManager {
        &self.phase_manager
    }

    /// Get mutable phase manager
    pub fn phase_manager_mut(&mut self) -> &mut PhaseManager {
        &mut self.phase_manager
    }

    /// Get utility index
    pub fn utility_index(&self) -> Option<&UtilityIndex> {
        self.utility_index.as_ref()
    }

    /// Calculate projected annual inflation rate
    pub fn calculate_annual_inflation(&mut self, block_number: BlockNumber, total_supply: &Amount) -> TokenomicsResult<f64> {
        let rate_per_block = self.get_minting_rate(block_number)?;
        
        // Blocks per year (assuming 3s blocks)
        let blocks_per_year = 365 * 24 * 60 * 60 / 3;
        
        let annual_minting = rate_per_block.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0) as f64 * blocks_per_year as f64;

        let supply = total_supply.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(1) as f64;

        Ok((annual_minting / supply) * 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phase_manager::{IVTConfig, PhaseManager};
    use crate::utility_index::{UtilityIndex, UtilityMetrics};

    #[test]
    fn test_phase1_minting() {
        let config = MintingConfig::default();
        let phase_manager = PhaseManager::new(IVTConfig::default());
        let controller = MintingController::new(config, phase_manager, 0);

        // At genesis (block 0)
        let rate0 = controller.calculate_phase1_rate(0);
        
        // Later block
        let rate1000 = controller.calculate_phase1_rate(1000);
        
        // Rate should decrease over time due to decay
        assert!(rate0.inner() > rate1000.inner());
    }

    #[test]
    fn test_phase2_minting() {
        let config = MintingConfig::default();
        let phase_manager = PhaseManager::new(IVTConfig::default());
        let mut controller = MintingController::new(config, phase_manager, 0);

        // Set up utility index
        let baseline = UtilityMetrics::new(1000);
        let mut utility_index = UtilityIndex::with_baseline(baseline.clone());
        
        // At baseline (UI = 1.0)
        utility_index.update_metrics(baseline);
        controller.set_utility_index(utility_index);

        let rate = controller.calculate_phase2_rate().unwrap();
        
        // Should be between min and max
        assert!(rate.inner() >= &controller.config.min_rate.inner());
        assert!(rate.inner() <= &controller.config.max_rate.inner());
    }

    #[test]
    fn test_sigmoid_function() {
        let config = MintingConfig::default();
        let phase_manager = PhaseManager::new(IVTConfig::default());
        let controller = MintingController::new(config, phase_manager, 0);

        // Sigmoid(0) should be 0.5
        assert!((controller.sigmoid(0.0) - 0.5).abs() < 0.01);
        
        // Sigmoid of positive should be > 0.5
        assert!(controller.sigmoid(1.0) > 0.5);
        
        // Sigmoid of negative should be < 0.5
        assert!(controller.sigmoid(-1.0) < 0.5);
    }

    #[test]
    fn test_minting_accumulation() {
        let config = MintingConfig::default();
        let phase_manager = PhaseManager::new(IVTConfig::default());
        let mut controller = MintingController::new(config, phase_manager, 0);

        let initial = controller.total_minted().clone();
        controller.mint_for_block(1).unwrap();
        
        assert!(controller.total_minted().inner() > initial.inner());
    }

    #[test]
    fn test_annual_inflation() {
        let config = MintingConfig::default();
        let phase_manager = PhaseManager::new(IVTConfig::default());
        let mut controller = MintingController::new(config, phase_manager, 0);

        let total_supply = Amount::from_u64(100_000_000); // 100M tokens
        let inflation = controller.calculate_annual_inflation(0, &total_supply).unwrap();

        assert!(inflation > 0.0);
        assert!(inflation < 100.0); // Reasonable inflation rate
    }
}