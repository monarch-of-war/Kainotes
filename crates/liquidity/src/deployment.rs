// liquidity/src/deployment.rs

use crate::{
    pool::{LiquidityPool, PoolType},
    risk::RiskCalculator,
    LiquidityError, LiquidityResult,
};
use blockchain_core::{Amount, StakeAmount};
use blockchain_crypto::Address;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Deployment strategy for validator liquidity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeploymentStrategy {
    /// Conservative: Prioritize safety, lower yields
    Conservative,
    /// Balanced: Mix of safety and yield
    Balanced,
    /// Aggressive: Prioritize yield, higher risk tolerance
    Aggressive,
    /// Custom: User-defined allocation
    Custom(Vec<DeploymentAllocation>),
}

impl DeploymentStrategy {
    /// Get default allocations for strategy
    pub fn default_allocations(&self) -> Vec<DeploymentAllocation> {
        match self {
            DeploymentStrategy::Conservative => vec![
                DeploymentAllocation {
                    pool_type: PoolType::Treasury,
                    percentage: 40.0,
                    min_amount: None,
                    max_amount: None,
                },
                DeploymentAllocation {
                    pool_type: PoolType::StabilityReserve,
                    percentage: 35.0,
                    min_amount: None,
                    max_amount: None,
                },
                DeploymentAllocation {
                    pool_type: PoolType::Lending,
                    percentage: 20.0,
                    min_amount: None,
                    max_amount: None,
                },
                DeploymentAllocation {
                    pool_type: PoolType::AMM,
                    percentage: 5.0,
                    min_amount: None,
                    max_amount: None,
                },
            ],
            DeploymentStrategy::Balanced => vec![
                DeploymentAllocation {
                    pool_type: PoolType::Lending,
                    percentage: 35.0,
                    min_amount: None,
                    max_amount: None,
                },
                DeploymentAllocation {
                    pool_type: PoolType::AMM,
                    percentage: 30.0,
                    min_amount: None,
                    max_amount: None,
                },
                DeploymentAllocation {
                    pool_type: PoolType::StabilityReserve,
                    percentage: 20.0,
                    min_amount: None,
                    max_amount: None,
                },
                DeploymentAllocation {
                    pool_type: PoolType::Treasury,
                    percentage: 15.0,
                    min_amount: None,
                    max_amount: None,
                },
            ],
            DeploymentStrategy::Aggressive => vec![
                DeploymentAllocation {
                    pool_type: PoolType::AMM,
                    percentage: 50.0,
                    min_amount: None,
                    max_amount: None,
                },
                DeploymentAllocation {
                    pool_type: PoolType::Lending,
                    percentage: 40.0,
                    min_amount: None,
                    max_amount: None,
                },
                DeploymentAllocation {
                    pool_type: PoolType::StabilityReserve,
                    percentage: 10.0,
                    min_amount: None,
                    max_amount: None,
                },
            ],
            DeploymentStrategy::Custom(allocations) => allocations.clone(),
        }
    }

    /// Validate that allocations sum to ~100%
    pub fn validate(&self) -> LiquidityResult<()> {
        let allocations = self.default_allocations();
        let total: f64 = allocations.iter().map(|a| a.percentage).sum();
        
        if (total - 100.0).abs() > 0.1 {
            return Err(LiquidityError::InvalidAllocation(
                format!("Allocations sum to {}%, must be 100%", total)
            ));
        }

        Ok(())
    }
}

/// Deployment allocation specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentAllocation {
    /// Pool type to allocate to
    pub pool_type: PoolType,
    /// Percentage of total to allocate (0-100)
    pub percentage: f64,
    /// Minimum amount to allocate
    pub min_amount: Option<Amount>,
    /// Maximum amount to allocate
    pub max_amount: Option<Amount>,
}

/// Deployment manager coordinating liquidity across pools
pub struct DeploymentManager {
    /// All managed pools (pool_id -> pool)
    pools: HashMap<u64, LiquidityPool>,
    /// Validator deployments (validator -> strategy)
    strategies: HashMap<Address, DeploymentStrategy>,
    /// Risk calculator
    risk_calculator: RiskCalculator,
    /// Minimum deployment per pool
    min_deployment_per_pool: Amount,
}

impl DeploymentManager {
    /// Create new deployment manager
    pub fn new(min_deployment_per_pool: Amount) -> Self {
        Self {
            pools: HashMap::new(),
            strategies: HashMap::new(),
            risk_calculator: RiskCalculator::new(),
            min_deployment_per_pool,
        }
    }

    /// Register a new pool
    pub fn register_pool(&mut self, pool: LiquidityPool) -> LiquidityResult<()> {
        let id = pool.id();
        if self.pools.contains_key(&id) {
            return Err(LiquidityError::PoolError(
                format!("Pool {} already exists", id)
            ));
        }

        self.pools.insert(id, pool);
        Ok(())
    }

    /// Get a pool
    pub fn get_pool(&self, pool_id: u64) -> LiquidityResult<&LiquidityPool> {
        self.pools.get(&pool_id)
            .ok_or(LiquidityError::PoolNotFound(pool_id))
    }

    /// Get mutable pool reference
    pub fn get_pool_mut(&mut self, pool_id: u64) -> LiquidityResult<&mut LiquidityPool> {
        self.pools.get_mut(&pool_id)
            .ok_or(LiquidityError::PoolNotFound(pool_id))
    }

    /// Set validator deployment strategy
    pub fn set_strategy(
        &mut self,
        validator: Address,
        strategy: DeploymentStrategy,
    ) -> LiquidityResult<()> {
        strategy.validate()?;
        self.strategies.insert(validator, strategy);
        Ok(())
    }

    /// Get validator strategy
    pub fn get_strategy(&self, validator: &Address) -> DeploymentStrategy {
        self.strategies.get(validator)
            .cloned()
            .unwrap_or(DeploymentStrategy::Balanced)
    }

    /// Deploy liquidity for a validator according to their strategy
    pub fn deploy_liquidity(
        &mut self,
        validator: Address,
        total_amount: StakeAmount,
    ) -> LiquidityResult<DeploymentReport> {
        let strategy = self.get_strategy(&validator);
        let allocations = strategy.default_allocations();

        let mut report = DeploymentReport {
            validator,
            total_deployed: Amount::zero(),
            deployments: Vec::new(),
            failed_deployments: Vec::new(),
        };

        // Calculate amounts for each allocation
        for allocation in allocations {
            let amount = self.calculate_allocation_amount(&total_amount, &allocation)?;

            // Skip if below minimum
            if amount.inner() < self.min_deployment_per_pool.inner() {
                continue;
            }

            // Find best pool for this type
            let pool_id = self.find_best_pool(allocation.pool_type)?;
            
            // Deploy to pool
            match self.deploy_to_pool(pool_id, validator, amount.clone()) {
                Ok(()) => {
                    report.deployments.push(PoolDeployment {
                        pool_id,
                        pool_type: allocation.pool_type,
                        amount: amount.clone(),
                    });

                    report.total_deployed = report.total_deployed.checked_add(&amount)
                        .ok_or_else(|| LiquidityError::CalculationError(
                            "Total deployed overflow".into()
                        ))?;
                }
                Err(e) => {
                    report.failed_deployments.push(FailedDeployment {
                        pool_type: allocation.pool_type,
                        amount,
                        error: e.to_string(),
                    });
                }
            }
        }

        Ok(report)
    }

    /// Calculate allocation amount based on percentage
    fn calculate_allocation_amount(
        &self,
        total: &StakeAmount,
        allocation: &DeploymentAllocation,
    ) -> LiquidityResult<Amount> {
        let total_val = total.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0) as f64;

        let mut amount_val = (total_val * allocation.percentage) / 100.0;

        // Apply min/max constraints
        if let Some(min) = &allocation.min_amount {
            let min_val = min.inner().to_u64_digits().first().copied().unwrap_or(0) as f64;
            amount_val = amount_val.max(min_val);
        }

        if let Some(max) = &allocation.max_amount {
            let max_val = max.inner().to_u64_digits().first().copied().unwrap_or(u64::MAX) as f64;
            amount_val = amount_val.min(max_val);
        }

        Ok(Amount::from_u64(amount_val as u64))
    }

    /// Find best pool for a given type based on risk-adjusted return
    fn find_best_pool(&self, pool_type: PoolType) -> LiquidityResult<u64> {
        let mut best_pool: Option<(u64, f64)> = None;

        for (id, pool) in &self.pools {
            if pool.pool_type() == pool_type && pool.is_active() {
                let rar = pool.info.risk_adjusted_return();
                
                if let Some((_, best_rar)) = best_pool {
                    if rar > best_rar {
                        best_pool = Some((*id, rar));
                    }
                } else {
                    best_pool = Some((*id, rar));
                }
            }
        }

        best_pool.map(|(id, _)| id)
            .ok_or_else(|| LiquidityError::PoolError(
                format!("No active pool found for type {:?}", pool_type)
            ))
    }

    /// Deploy to a specific pool
    fn deploy_to_pool(
        &mut self,
        pool_id: u64,
        validator: Address,
        amount: Amount,
    ) -> LiquidityResult<()> {
        let pool = self.get_pool_mut(pool_id)?;
        pool.deposit(validator, amount)
    }

    /// Withdraw liquidity from a pool
    pub fn withdraw_liquidity(
        &mut self,
        pool_id: u64,
        validator: Address,
        amount: Amount,
        unlock_time: blockchain_core::Timestamp,
    ) -> LiquidityResult<()> {
        let pool = self.get_pool_mut(pool_id)?;
        pool.request_withdrawal(validator, amount, unlock_time)
    }

    /// Rebalance validator's liquidity deployment
    pub fn rebalance(
        &mut self,
        validator: Address,
    ) -> LiquidityResult<RebalanceReport> {
        // Get current deployments
        let current_deployments = self.get_validator_deployments(&validator);
        let total: Amount = current_deployments.iter()
            .map(|d| d.amount.clone())
            .fold(Amount::zero(), |acc, a| acc.checked_add(&a).unwrap_or(acc));

        // Withdraw all
        for deployment in &current_deployments {
            let unlock_time = blockchain_core::Timestamp::MAX; // Immediate for rebalance
            self.withdraw_liquidity(deployment.pool_id, validator, deployment.amount.clone(), unlock_time)?;
        }

        // Redeploy according to strategy
        let total_stake = StakeAmount::new(total.inner().clone());
        let deploy_report = self.deploy_liquidity(validator, total_stake)?;

        Ok(RebalanceReport {
            validator,
            withdrawn: current_deployments,
            redeployed: deploy_report.deployments,
        })
    }

    /// Get validator's current deployments
    pub fn get_validator_deployments(&self, validator: &Address) -> Vec<PoolDeployment> {
        let mut deployments = Vec::new();

        for (pool_id, pool) in &self.pools {
            let balance = pool.get_balance(validator);
            if !balance.is_zero() {
                deployments.push(PoolDeployment {
                    pool_id: *pool_id,
                    pool_type: pool.pool_type(),
                    amount: balance,
                });
            }
        }

        deployments
    }

    /// Calculate total liquidity deployed by validator
    pub fn get_total_deployed(&self, validator: &Address) -> Amount {
        self.get_validator_deployments(validator)
            .iter()
            .fold(Amount::zero(), |acc, d| {
                acc.checked_add(&d.amount).unwrap_or(acc)
            })
    }

    /// Get all pools
    pub fn pools(&self) -> &HashMap<u64, LiquidityPool> {
        &self.pools
    }

    /// Process pending withdrawals for all pools
    pub fn process_pending_withdrawals(&mut self, current_time: blockchain_core::Timestamp) -> Vec<(u64, Address, Amount)> {
        let mut completed = Vec::new();

        for (pool_id, pool) in &mut self.pools {
            let pool_completed = pool.process_withdrawals(current_time);
            for (validator, amount) in pool_completed {
                completed.push((*pool_id, validator, amount));
            }
        }

        completed
    }
}

/// Deployment report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentReport {
    pub validator: Address,
    pub total_deployed: Amount,
    pub deployments: Vec<PoolDeployment>,
    pub failed_deployments: Vec<FailedDeployment>,
}

/// Individual pool deployment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolDeployment {
    pub pool_id: u64,
    pub pool_type: PoolType,
    pub amount: Amount,
}

/// Failed deployment attempt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedDeployment {
    pub pool_type: PoolType,
    pub amount: Amount,
    pub error: String,
}

/// Rebalance report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebalanceReport {
    pub validator: Address,
    pub withdrawn: Vec<PoolDeployment>,
    pub redeployed: Vec<PoolDeployment>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::PoolInfo;

    #[test]
    fn test_strategy_validation() {
        let strategy = DeploymentStrategy::Balanced;
        assert!(strategy.validate().is_ok());

        let custom = DeploymentStrategy::Custom(vec![
            DeploymentAllocation {
                pool_type: PoolType::AMM,
                percentage: 60.0,
                min_amount: None,
                max_amount: None,
            },
        ]);
        assert!(custom.validate().is_err()); // Doesn't sum to 100%
    }

    #[test]
    fn test_deployment_manager() {
        let mut manager = DeploymentManager::new(Amount::from_u64(100));
        
        let pool_info = PoolInfo::new(
            1,
            PoolType::AMM,
            "Test Pool".into(),
            Address::zero(),
            1000,
            500,
        );
        let pool = LiquidityPool::new(pool_info);
        
        manager.register_pool(pool).unwrap();
        assert!(manager.get_pool(1).is_ok());
    }

    #[test]
    fn test_liquidity_deployment() {
        let mut manager = DeploymentManager::new(Amount::from_u64(10));
        
        // Register pools for each type
        for (id, pool_type) in [(1, PoolType::AMM), (2, PoolType::Lending), (3, PoolType::Treasury)].iter() {
            let pool_info = PoolInfo::new(
                *id,
                *pool_type,
                format!("Pool {}", id),
                Address::zero(),
                1000,
                500,
            );
            manager.register_pool(LiquidityPool::new(pool_info)).unwrap();
        }

        let validator = Address::zero();
        let stake = StakeAmount::from_u64(10000);
        
        manager.set_strategy(validator, DeploymentStrategy::Balanced).unwrap();
        let report = manager.deploy_liquidity(validator, stake).unwrap();

        assert!(report.total_deployed.inner() > &Amount::zero().inner());
        assert!(!report.deployments.is_empty());
    }
}