// liquidity/src/risk.rs

use crate::{pool::PoolInfo, LiquidityError, LiquidityResult};
use blockchain_core::Amount;
use serde::{Deserialize, Serialize};

/// Risk assessment for a liquidity deployment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAssessment {
    /// Overall risk score (0-10000, higher = riskier)
    pub risk_score: u16,
    /// Volatility component
    pub volatility_risk: u16,
    /// Smart contract risk
    pub contract_risk: u16,
    /// Liquidity risk (impermanent loss potential)
    pub liquidity_risk: u16,
    /// Counterparty risk
    pub counterparty_risk: u16,
    /// Risk category
    pub risk_category: RiskCategory,
}

impl RiskAssessment {
    /// Calculate aggregate risk score
    pub fn calculate_aggregate(&self) -> u16 {
        // Weighted average of risk components
        let weighted = (self.volatility_risk as u32 * 30
            + self.contract_risk as u32 * 25
            + self.liquidity_risk as u32 * 25
            + self.counterparty_risk as u32 * 20) / 100;

        (weighted as u16).min(10000)
    }

    /// Get risk category
    pub fn categorize(&self) -> RiskCategory {
        let score = self.calculate_aggregate();
        RiskCategory::from_score(score)
    }
}

/// Risk categories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskCategory {
    VeryLow,    // 0-2000
    Low,        // 2000-4000
    Medium,     // 4000-6000
    High,       // 6000-8000
    VeryHigh,   // 8000-10000
}

impl RiskCategory {
    /// Create category from risk score
    pub fn from_score(score: u16) -> Self {
        match score {
            0..=2000 => RiskCategory::VeryLow,
            2001..=4000 => RiskCategory::Low,
            4001..=6000 => RiskCategory::Medium,
            6001..=8000 => RiskCategory::High,
            _ => RiskCategory::VeryHigh,
        }
    }

    /// Get maximum allocation percentage for this risk category
    pub fn max_allocation_percentage(&self) -> f64 {
        match self {
            RiskCategory::VeryLow => 100.0,
            RiskCategory::Low => 50.0,
            RiskCategory::Medium => 30.0,
            RiskCategory::High => 15.0,
            RiskCategory::VeryHigh => 5.0,
        }
    }
}

/// Risk profile for a validator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskProfile {
    /// Maximum acceptable risk score
    pub max_risk_score: u16,
    /// Maximum percentage in high-risk pools
    pub max_high_risk_percentage: f64,
    /// Diversification requirements (min number of pools)
    pub min_pool_diversification: usize,
    /// Maximum allocation to single pool
    pub max_single_pool_percentage: f64,
}

impl Default for RiskProfile {
    fn default() -> Self {
        Self {
            max_risk_score: 5000,           // Medium risk tolerance
            max_high_risk_percentage: 20.0, // Max 20% in high-risk
            min_pool_diversification: 3,     // At least 3 pools
            max_single_pool_percentage: 40.0, // Max 40% in one pool
        }
    }
}

impl RiskProfile {
    /// Conservative risk profile
    pub fn conservative() -> Self {
        Self {
            max_risk_score: 3000,
            max_high_risk_percentage: 10.0,
            min_pool_diversification: 4,
            max_single_pool_percentage: 30.0,
        }
    }

    /// Aggressive risk profile
    pub fn aggressive() -> Self {
        Self {
            max_risk_score: 7000,
            max_high_risk_percentage: 40.0,
            min_pool_diversification: 2,
            max_single_pool_percentage: 60.0,
        }
    }

    /// Check if pool meets risk tolerance
    pub fn accepts_pool(&self, pool: &PoolInfo) -> bool {
        pool.risk_score <= self.max_risk_score
    }
}

/// Risk calculator for liquidity deployments
pub struct RiskCalculator {
    /// Historical volatility data (simplified)
    volatility_cache: std::collections::HashMap<u64, f64>,
}

impl RiskCalculator {
    /// Create new risk calculator
    pub fn new() -> Self {
        Self {
            volatility_cache: std::collections::HashMap::new(),
        }
    }

    /// Calculate risk assessment for a pool
    pub fn assess_pool(&self, pool: &PoolInfo) -> RiskAssessment {
        let volatility_risk = self.calculate_volatility_risk(pool);
        let contract_risk = self.calculate_contract_risk(pool);
        let liquidity_risk = self.calculate_liquidity_risk(pool);
        let counterparty_risk = self.calculate_counterparty_risk(pool);

        let assessment = RiskAssessment {
            risk_score: pool.risk_score,
            volatility_risk,
            contract_risk,
            liquidity_risk,
            counterparty_risk,
            risk_category: RiskCategory::from_score(pool.risk_score),
        };

        assessment
    }

    /// Calculate volatility risk component
    fn calculate_volatility_risk(&self, pool: &PoolInfo) -> u16 {
        // Get cached volatility or use default
        let volatility = self.volatility_cache.get(&pool.id)
            .copied()
            .unwrap_or(0.15); // Default 15% volatility

        // Higher volatility = higher risk
        // Scale to 0-10000
        ((volatility * 10000.0).min(10000.0)) as u16
    }

    /// Calculate smart contract risk
    fn calculate_contract_risk(&self, pool: &PoolInfo) -> u16 {
        // Factors:
        // - Time deployed (older = safer)
        // - Audit status (would be stored in pool metadata)
        // - Code complexity
        
        let age_seconds = current_timestamp().saturating_sub(pool.created_at);
        let age_days = age_seconds / (24 * 3600);

        // Risk decreases with age, plateaus after 365 days
        let age_factor = if age_days == 0 {
            1.0
        } else {
            1.0 / (1.0 + (age_days as f64 / 365.0).min(1.0))
        };

        let base_risk = 3000u16; // 30% base contract risk
        ((base_risk as f64 * age_factor) as u16).min(10000)
    }

    /// Calculate liquidity risk (impermanent loss potential, slippage)
    fn calculate_liquidity_risk(&self, pool: &PoolInfo) -> u16 {
        // Higher TVL = lower liquidity risk
        let tvl_val = pool.tvl.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(1) as f64;

        // Risk inversely proportional to TVL
        // Assume $10M TVL = very low risk (500), $100K TVL = high risk (5000)
        let tvl_millions = tvl_val / 1_000_000.0;
        let risk = if tvl_millions < 0.1 {
            8000 // Very high risk for low TVL
        } else if tvl_millions < 1.0 {
            5000 // High risk
        } else if tvl_millions < 10.0 {
            2000 // Medium risk
        } else {
            500 // Low risk
        };

        risk.min(10000)
    }

    /// Calculate counterparty risk
    fn calculate_counterparty_risk(&self, _pool: &PoolInfo) -> u16 {
        // Factors:
        // - Pool operator reputation
        // - Insurance/guarantees
        // - Decentralization level
        
        // Simplified: Use pool type as proxy
        // Treasury and StabilityReserve = low risk (protocol-owned)
        // AMM and Lending = higher risk (external protocols)
        
        match _pool.pool_type {
            crate::pool::PoolType::Treasury => 500,
            crate::pool::PoolType::StabilityReserve => 1000,
            crate::pool::PoolType::Lending => 3000,
            crate::pool::PoolType::AMM => 4000,
        }
    }

    /// Calculate risk-adjusted return (RAR)
    /// RAR = (Protocol_Reward + DeFi_Yield) / Risk_Score
    pub fn calculate_rar(
        &self,
        pool: &PoolInfo,
        protocol_reward: &Amount,
        defi_yield: &Amount,
    ) -> f64 {
        let assessment = self.assess_pool(pool);
        let risk_score = assessment.calculate_aggregate() as f64;

        if risk_score == 0.0 {
            return 0.0;
        }

        let protocol_val = protocol_reward.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0) as f64;

        let yield_val = defi_yield.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0) as f64;

        (protocol_val + yield_val) / (risk_score / 100.0)
    }

    /// Calculate optimal allocation based on risk tolerance
    pub fn calculate_optimal_allocation(
        &self,
        pools: &[&PoolInfo],
        total_amount: &Amount,
        risk_profile: &RiskProfile,
    ) -> LiquidityResult<Vec<(u64, Amount)>> {
        // Filter pools by risk tolerance
        let acceptable_pools: Vec<_> = pools.iter()
            .filter(|p| risk_profile.accepts_pool(p))
            .collect();

        if acceptable_pools.is_empty() {
            return Err(LiquidityError::RiskError(
                "No pools match risk tolerance".into()
            ));
        }

        // Calculate risk-adjusted returns for each pool
        let mut pool_scores: Vec<(u64, f64)> = acceptable_pools.iter()
            .map(|p| {
                let assessment = self.assess_pool(p);
                let rar = p.risk_adjusted_return();
                (p.id, rar)
            })
            .collect();

        // Sort by RAR descending
        pool_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        // Allocate proportionally to RAR, respecting constraints
        let total_rar: f64 = pool_scores.iter().map(|(_, rar)| rar).sum();
        let mut allocations = Vec::new();

        let total_val = total_amount.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0) as f64;

        for (pool_id, rar) in pool_scores {
            let percentage = (rar / total_rar) * 100.0;
            let capped_percentage = percentage.min(risk_profile.max_single_pool_percentage);
            
            let amount_val = (total_val * capped_percentage) / 100.0;
            let amount = Amount::from_u64(amount_val as u64);

            if !amount.is_zero() {
                allocations.push((pool_id, amount));
            }
        }

        Ok(allocations)
    }

    /// Update volatility data
    pub fn update_volatility(&mut self, pool_id: u64, volatility: f64) {
        self.volatility_cache.insert(pool_id, volatility);
    }
}

impl Default for RiskCalculator {
    fn default() -> Self {
        Self::new()
    }
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
    use blockchain_crypto::Address;

    fn create_test_pool(risk_score: u16) -> PoolInfo {
        PoolInfo::new(
            1,
            crate::pool::PoolType::AMM,
            "Test Pool".into(),
            Address::zero(),
            1000,
            risk_score,
        )
    }

    #[test]
    fn test_risk_category() {
        assert_eq!(RiskCategory::from_score(1000), RiskCategory::VeryLow);
        assert_eq!(RiskCategory::from_score(3000), RiskCategory::Low);
        assert_eq!(RiskCategory::from_score(5000), RiskCategory::Medium);
        assert_eq!(RiskCategory::from_score(7000), RiskCategory::High);
        assert_eq!(RiskCategory::from_score(9000), RiskCategory::VeryHigh);
    }

    #[test]
    fn test_risk_assessment() {
        let calculator = RiskCalculator::new();
        let pool = create_test_pool(5000);
        
        let assessment = calculator.assess_pool(&pool);
        assert_eq!(assessment.risk_category, RiskCategory::Medium);
    }

    #[test]
    fn test_risk_profile() {
        let profile = RiskProfile::conservative();
        let low_risk_pool = create_test_pool(2000);
        let high_risk_pool = create_test_pool(8000);

        assert!(profile.accepts_pool(&low_risk_pool));
        assert!(!profile.accepts_pool(&high_risk_pool));
    }

    #[test]
    fn test_rar_calculation() {
        let calculator = RiskCalculator::new();
        let pool = create_test_pool(5000);
        
        let protocol_reward = Amount::from_u64(1000);
        let defi_yield = Amount::from_u64(500);
        
        let rar = calculator.calculate_rar(&pool, &protocol_reward, &defi_yield);
        assert!(rar > 0.0);
    }

    #[test]
    fn test_optimal_allocation() {
        let calculator = RiskCalculator::new();
        let pools = vec![
            create_test_pool(2000),
            create_test_pool(4000),
            create_test_pool(8000),
        ];
        let pool_refs: Vec<_> = pools.iter().collect();

        let total = Amount::from_u64(10000);
        let profile = RiskProfile::default();

        let allocations = calculator.calculate_optimal_allocation(
            &pool_refs,
            &total,
            &profile,
        ).unwrap();

        assert!(!allocations.is_empty());
    }
}