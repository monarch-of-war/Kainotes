// liquidity/src/pool.rs

use crate::{LiquidityError, LiquidityResult};
use blockchain_core::{Amount, Timestamp};
use blockchain_crypto::Address;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Types of liquidity pools
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PoolType {
    /// Automated Market Maker (AMM) for trading
    AMM,
    /// Lending and borrowing protocol
    Lending,
    /// Network treasury for grants and development
    Treasury,
    /// Stability reserves for stablecoins
    StabilityReserve,
}

/// Pool information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolInfo {
    /// Unique pool identifier
    pub id: u64,
    /// Pool type
    pub pool_type: PoolType,
    /// Pool name
    pub name: String,
    /// Total value locked
    pub tvl: Amount,
    /// Creation timestamp
    pub created_at: Timestamp,
    /// Pool owner/controller
    pub owner: Address,
    /// Is pool active
    pub active: bool,
    /// Annual percentage yield (basis points)
    pub apy: u16,
    /// Risk score (0-10000, higher = riskier)
    pub risk_score: u16,
}

impl PoolInfo {
    /// Create new pool info
    pub fn new(
        id: u64,
        pool_type: PoolType,
        name: String,
        owner: Address,
        apy: u16,
        risk_score: u16,
    ) -> Self {
        Self {
            id,
            pool_type,
            name,
            tvl: Amount::zero(),
            created_at: current_timestamp(),
            owner,
            active: true,
            apy,
            risk_score,
        }
    }

    /// Calculate expected annual yield for an amount
    pub fn calculate_yield(&self, amount: &Amount) -> Amount {
        let amount_val = amount.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0) as f64;

        let yield_val = (amount_val * self.apy as f64) / 10000.0;
        Amount::from_u64(yield_val as u64)
    }

    /// Calculate risk-adjusted return
    pub fn risk_adjusted_return(&self) -> f64 {
        if self.risk_score == 0 {
            return self.apy as f64;
        }
        
        (self.apy as f64) / (self.risk_score as f64 / 100.0)
    }
}

/// Pool metrics and statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolMetrics {
    /// Total deposits
    pub total_deposits: Amount,
    /// Total withdrawals
    pub total_withdrawals: Amount,
    /// Total yield generated
    pub total_yield: Amount,
    /// Number of depositors
    pub depositor_count: usize,
    /// Average deposit size
    pub avg_deposit: Amount,
    /// Utilization rate (for lending pools)
    pub utilization_rate: f64,
    /// Last updated timestamp
    pub updated_at: Timestamp,
}

impl PoolMetrics {
    /// Create new metrics
    pub fn new() -> Self {
        Self {
            total_deposits: Amount::zero(),
            total_withdrawals: Amount::zero(),
            total_yield: Amount::zero(),
            depositor_count: 0,
            avg_deposit: Amount::zero(),
            utilization_rate: 0.0,
            updated_at: current_timestamp(),
        }
    }

    /// Update average deposit
    pub fn update_avg_deposit(&mut self) {
        if self.depositor_count > 0 {
            let total = self.total_deposits.inner()
                .to_u64_digits()
                .first()
                .copied()
                .unwrap_or(0);
            self.avg_deposit = Amount::from_u64(total / self.depositor_count as u64);
        }
    }
}

impl Default for PoolMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Liquidity pool base structure
pub struct LiquidityPool {
    /// Pool information
    pub info: PoolInfo,
    /// Pool metrics
    pub metrics: PoolMetrics,
    /// Validator deposits (validator address -> deposited amount)
    deposits: HashMap<Address, Amount>,
    /// Pending withdrawals
    pending_withdrawals: HashMap<Address, PendingWithdrawal>,
}

impl LiquidityPool {
    /// Create a new liquidity pool
    pub fn new(info: PoolInfo) -> Self {
        Self {
            info,
            metrics: PoolMetrics::new(),
            deposits: HashMap::new(),
            pending_withdrawals: HashMap::new(),
        }
    }

    /// Get pool ID
    pub fn id(&self) -> u64 {
        self.info.id
    }

    /// Get pool type
    pub fn pool_type(&self) -> PoolType {
        self.info.pool_type
    }

    /// Check if pool is active
    pub fn is_active(&self) -> bool {
        self.info.active
    }

    /// Deposit liquidity into the pool
    pub fn deposit(&mut self, validator: Address, amount: Amount) -> LiquidityResult<()> {
        if !self.info.active {
            return Err(LiquidityError::PoolError("Pool is not active".into()));
        }

        if amount.is_zero() {
            return Err(LiquidityError::DeploymentError("Cannot deposit zero amount".into()));
        }

        // Update deposits
        let current = self.deposits.entry(validator).or_insert_with(Amount::zero);
        *current = current.checked_add(&amount)
            .ok_or_else(|| LiquidityError::CalculationError("Deposit overflow".into()))?;

        // Update TVL
        self.info.tvl = self.info.tvl.checked_add(&amount)
            .ok_or_else(|| LiquidityError::CalculationError("TVL overflow".into()))?;

        // Update metrics
        self.metrics.total_deposits = self.metrics.total_deposits.checked_add(&amount)
            .ok_or_else(|| LiquidityError::CalculationError("Total deposits overflow".into()))?;

        if !self.deposits.contains_key(&validator) {
            self.metrics.depositor_count += 1;
        }
        self.metrics.update_avg_deposit();
        self.metrics.updated_at = current_timestamp();

        Ok(())
    }

    /// Request withdrawal (may have unbonding period)
    pub fn request_withdrawal(
        &mut self,
        validator: Address,
        amount: Amount,
        unlock_time: Timestamp,
    ) -> LiquidityResult<()> {
        // Check balance
        let balance = self.get_balance(&validator);
        if balance.inner() < amount.inner() {
            return Err(LiquidityError::InsufficientLiquidity {
                required: amount,
                available: balance,
            });
        }

        // Create pending withdrawal
        let withdrawal = PendingWithdrawal {
            validator,
            amount: amount.clone(),
            request_time: current_timestamp(),
            unlock_time,
        };

        self.pending_withdrawals.insert(validator, withdrawal);

        Ok(())
    }

    /// Process completed withdrawals
    pub fn process_withdrawals(&mut self, current_time: Timestamp) -> Vec<(Address, Amount)> {
        let mut completed = Vec::new();

        // Find completed withdrawals
        let ready: Vec<_> = self.pending_withdrawals.iter()
            .filter(|(_, w)| current_time >= w.unlock_time)
            .map(|(addr, w)| (*addr, w.amount.clone()))
            .collect();

        for (validator, amount) in ready {
            if let Some(balance) = self.deposits.get_mut(&validator) {
                if let Some(new_balance) = balance.checked_sub(&amount) {
                    *balance = new_balance;

                    // Update TVL
                    if let Some(new_tvl) = self.info.tvl.checked_sub(&amount) {
                        self.info.tvl = new_tvl;
                    }

                    // Update metrics
                    if let Some(new_total) = self.metrics.total_withdrawals.checked_add(&amount) {
                        self.metrics.total_withdrawals = new_total;
                    }

                    self.pending_withdrawals.remove(&validator);
                    completed.push((validator, amount));
                }
            }
        }

        if !completed.is_empty() {
            self.metrics.updated_at = current_time;
        }

        completed
    }

    /// Get validator balance in pool
    pub fn get_balance(&self, validator: &Address) -> Amount {
        self.deposits.get(validator).cloned().unwrap_or_else(Amount::zero)
    }

    /// Get all deposits
    pub fn deposits(&self) -> &HashMap<Address, Amount> {
        &self.deposits
    }

    /// Get pending withdrawals
    pub fn pending_withdrawals(&self) -> &HashMap<Address, PendingWithdrawal> {
        &self.pending_withdrawals
    }

    /// Calculate total yield generated
    pub fn calculate_yield(&mut self, elapsed_seconds: u64) -> Amount {
        // Simplified yield calculation: (TVL * APY * time) / (365 days)
        let tvl_val = self.info.tvl.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0) as f64;

        let apy_rate = self.info.apy as f64 / 10000.0;
        let time_fraction = elapsed_seconds as f64 / (365.0 * 24.0 * 3600.0);
        
        let yield_val = tvl_val * apy_rate * time_fraction;
        let yield_amount = Amount::from_u64(yield_val as u64);

        self.metrics.total_yield = self.metrics.total_yield.checked_add(&yield_amount)
            .unwrap_or_else(|| self.metrics.total_yield.clone());

        yield_amount
    }

    /// Update pool APY
    pub fn update_apy(&mut self, new_apy: u16) {
        self.info.apy = new_apy;
    }

    /// Update risk score
    pub fn update_risk_score(&mut self, new_score: u16) {
        self.info.risk_score = new_score.min(10000);
    }

    /// Deactivate pool
    pub fn deactivate(&mut self) {
        self.info.active = false;
    }

    /// Reactivate pool
    pub fn activate(&mut self) {
        self.info.active = true;
    }
}

/// Pending withdrawal information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingWithdrawal {
    pub validator: Address,
    pub amount: Amount,
    pub request_time: Timestamp,
    pub unlock_time: Timestamp,
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

    fn create_test_pool() -> LiquidityPool {
        let info = PoolInfo::new(
            1,
            PoolType::AMM,
            "Test Pool".into(),
            Address::zero(),
            1000, // 10% APY
            500,  // 5% risk
        );
        LiquidityPool::new(info)
    }

    #[test]
    fn test_pool_creation() {
        let pool = create_test_pool();
        assert_eq!(pool.id(), 1);
        assert_eq!(pool.pool_type(), PoolType::AMM);
        assert!(pool.is_active());
    }

    #[test]
    fn test_deposit() {
        let mut pool = create_test_pool();
        let validator = Address::zero();
        let amount = Amount::from_u64(1000);

        pool.deposit(validator, amount.clone()).unwrap();
        assert_eq!(pool.get_balance(&validator), amount);
        assert_eq!(pool.info.tvl, amount);
    }

    #[test]
    fn test_withdrawal_request() {
        let mut pool = create_test_pool();
        let validator = Address::zero();
        let amount = Amount::from_u64(1000);

        pool.deposit(validator, amount.clone()).unwrap();
        
        let unlock_time = current_timestamp() + 3600;
        pool.request_withdrawal(validator, Amount::from_u64(500), unlock_time).unwrap();

        assert!(pool.pending_withdrawals().contains_key(&validator));
    }

    #[test]
    fn test_process_withdrawals() {
        let mut pool = create_test_pool();
        let validator = Address::zero();
        let amount = Amount::from_u64(1000);

        pool.deposit(validator, amount).unwrap();
        
        let unlock_time = current_timestamp() + 10;
        pool.request_withdrawal(validator, Amount::from_u64(500), unlock_time).unwrap();

        // Process before unlock time
        let completed1 = pool.process_withdrawals(current_timestamp());
        assert_eq!(completed1.len(), 0);

        // Process after unlock time
        let completed2 = pool.process_withdrawals(unlock_time + 1);
        assert_eq!(completed2.len(), 1);
        assert_eq!(pool.get_balance(&validator), Amount::from_u64(500));
    }

    #[test]
    fn test_yield_calculation() {
        let info = PoolInfo::new(
            1,
            PoolType::AMM,
            "Test".into(),
            Address::zero(),
            1000, // 10% APY
            500,
        );
        
        let amount = Amount::from_u64(10000);
        let expected_yield = info.calculate_yield(&amount);
        
        assert_eq!(expected_yield, Amount::from_u64(1000)); // 10% of 10000
    }

    #[test]
    fn test_risk_adjusted_return() {
        let info = PoolInfo::new(
            1,
            PoolType::AMM,
            "Test".into(),
            Address::zero(),
            1000, // 10% APY
            500,  // 5% risk
        );

        let rar = info.risk_adjusted_return();
        assert!(rar > 0.0);
    }
}