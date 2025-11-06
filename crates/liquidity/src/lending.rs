// liquidity/src/lending.rs

use crate::{pool::LiquidityPool, LiquidityError, LiquidityResult};
use blockchain_core::{Amount, Timestamp};
use blockchain_crypto::{Address, Hash};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Interest rate model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterestRate {
    /// Base rate (APY in basis points)
    pub base_rate: u16,
    /// Optimal utilization rate (basis points, e.g., 8000 = 80%)
    pub optimal_utilization: u16,
    /// Rate at optimal utilization
    pub rate_at_optimal: u16,
    /// Maximum rate (at 100% utilization)
    pub max_rate: u16,
}

impl InterestRate {
    /// Create standard interest rate model
    pub fn standard() -> Self {
        Self {
            base_rate: 200,           // 2% base
            optimal_utilization: 8000, // 80% optimal
            rate_at_optimal: 1000,     // 10% at optimal
            max_rate: 5000,            // 50% max
        }
    }

    /// Calculate current interest rate based on utilization
    pub fn calculate_rate(&self, utilization: f64) -> u16 {
        let util_bp = (utilization * 10000.0) as u16;

        if util_bp <= self.optimal_utilization {
            // Linear interpolation: base_rate -> rate_at_optimal
            let progress = util_bp as f64 / self.optimal_utilization as f64;
            let rate_diff = self.rate_at_optimal - self.base_rate;
            self.base_rate + (rate_diff as f64 * progress) as u16
        } else {
            // Linear interpolation: rate_at_optimal -> max_rate
            let excess = util_bp - self.optimal_utilization;
            let max_excess = 10000 - self.optimal_utilization;
            let progress = excess as f64 / max_excess as f64;
            let rate_diff = self.max_rate - self.rate_at_optimal;
            self.rate_at_optimal + (rate_diff as f64 * progress) as u16
        }
    }
}

/// Loan position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoanPosition {
    /// Unique loan ID
    pub id: Hash,
    /// Borrower address
    pub borrower: Address,
    /// Borrowed amount
    pub principal: Amount,
    /// Accrued interest
    pub interest: Amount,
    /// Collateral amount
    pub collateral: Amount,
    /// Interest rate at time of borrow (basis points)
    pub rate: u16,
    /// Borrow timestamp
    pub borrowed_at: Timestamp,
    /// Last interest accrual
    pub last_accrual: Timestamp,
    /// Loan status
    pub status: LoanStatus,
}

impl LoanPosition {
    /// Create new loan position
    pub fn new(
        borrower: Address,
        principal: Amount,
        collateral: Amount,
        rate: u16,
    ) -> Self {
        let now = current_timestamp();
        Self {
            id: Hash::zero(), // Would generate proper hash
            borrower,
            principal,
            interest: Amount::zero(),
            collateral,
            rate,
            borrowed_at: now,
            last_accrual: now,
            status: LoanStatus::Active,
        }
    }

    /// Calculate accrued interest
    pub fn calculate_interest(&self, current_time: Timestamp) -> Amount {
        let elapsed = current_time.saturating_sub(self.last_accrual);
        let elapsed_years = elapsed as f64 / (365.25 * 24.0 * 3600.0);

        let principal_val = self.principal.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0) as f64;

        let rate_per_year = self.rate as f64 / 10000.0;
        let interest_val = principal_val * rate_per_year * elapsed_years;

        Amount::from_u64(interest_val as u64)
    }

    /// Update accrued interest
    pub fn accrue_interest(&mut self, current_time: Timestamp) {
        let new_interest = self.calculate_interest(current_time);
        self.interest = self.interest.checked_add(&new_interest)
            .unwrap_or_else(|| self.interest.clone());
        self.last_accrual = current_time;
    }

    /// Get total debt (principal + interest)
    pub fn total_debt(&self) -> Amount {
        self.principal.checked_add(&self.interest)
            .unwrap_or_else(|| self.principal.clone())
    }

    /// Calculate health factor (collateral / debt)
    pub fn health_factor(&self) -> f64 {
        let debt = self.total_debt();
        if debt.is_zero() {
            return f64::INFINITY;
        }

        let collateral_val = self.collateral.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0) as f64;
        let debt_val = debt.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(1) as f64;

        collateral_val / debt_val
    }

    /// Check if loan is liquidatable
    pub fn is_liquidatable(&self, liquidation_threshold: f64) -> bool {
        self.status == LoanStatus::Active && self.health_factor() < liquidation_threshold
    }
}

/// Loan status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoanStatus {
    Active,
    Repaid,
    Liquidated,
    Defaulted,
}

/// Lending pool for borrowing/lending
pub struct LendingPool {
    /// Base liquidity pool
    base: LiquidityPool,
    /// Interest rate model
    interest_model: InterestRate,
    /// Total borrowed amount
    total_borrowed: Amount,
    /// Total supplied (available to borrow)
    total_supplied: Amount,
    /// Active loans
    loans: HashMap<Hash, LoanPosition>,
    /// Collateralization ratio (basis points, e.g., 15000 = 150%)
    collateral_ratio: u16,
    /// Liquidation threshold (basis points, e.g., 12000 = 120%)
    liquidation_threshold: u16,
}

impl LendingPool {
    /// Create new lending pool
    pub fn new(
        base: LiquidityPool,
        interest_model: InterestRate,
        collateral_ratio: u16,
        liquidation_threshold: u16,
    ) -> Self {
        Self {
            base,
            interest_model,
            total_borrowed: Amount::zero(),
            total_supplied: Amount::zero(),
            loans: HashMap::new(),
            collateral_ratio,
            liquidation_threshold,
        }
    }

    /// Supply liquidity for lending
    pub fn supply(&mut self, supplier: Address, amount: Amount) -> LiquidityResult<()> {
        self.base.deposit(supplier, amount.clone())?;
        
        self.total_supplied = self.total_supplied.checked_add(&amount)
            .ok_or_else(|| LiquidityError::CalculationError("Total supplied overflow".into()))?;

        Ok(())
    }

    /// Borrow against collateral
    pub fn borrow(
        &mut self,
        borrower: Address,
        borrow_amount: Amount,
        collateral: Amount,
    ) -> LiquidityResult<Hash> {
        // Check sufficient liquidity
        let available = self.available_liquidity();
        if available.inner() < borrow_amount.inner() {
            return Err(LiquidityError::InsufficientLiquidity {
                required: borrow_amount,
                available,
            });
        }

        // Check collateralization
        let required_collateral = self.calculate_required_collateral(&borrow_amount);
        if collateral.inner() < required_collateral.inner() {
            return Err(LiquidityError::DeploymentError(
                format!("Insufficient collateral: need {}, provided {}",
                    required_collateral, collateral)
            ));
        }

        // Get current interest rate
        let utilization = self.utilization_rate();
        let rate = self.interest_model.calculate_rate(utilization);

        // Create loan
        let mut loan = LoanPosition::new(borrower, borrow_amount.clone(), collateral, rate);
        loan.id = self.generate_loan_id(&loan);

        // Update totals
        self.total_borrowed = self.total_borrowed.checked_add(&borrow_amount)
            .ok_or_else(|| LiquidityError::CalculationError("Total borrowed overflow".into()))?;

        let loan_id = loan.id;
        self.loans.insert(loan_id, loan);

        Ok(loan_id)
    }

    /// Repay loan
    pub fn repay(
        &mut self,
        loan_id: Hash,
        amount: Amount,
    ) -> LiquidityResult<(Amount, Amount)> {
        let loan = self.loans.get_mut(&loan_id)
            .ok_or_else(|| LiquidityError::PositionNotFound(loan_id.to_hex()))?;

        if loan.status != LoanStatus::Active {
            return Err(LiquidityError::PoolError("Loan is not active".into()));
        }

        // Accrue interest
        loan.accrue_interest(current_timestamp());

        let total_debt = loan.total_debt();
        let repay_amount = amount.inner().min(total_debt.inner()).clone();
        let repay_amount = Amount::new(repay_amount);

        // Update loan
        let remaining_debt = total_debt.checked_sub(&repay_amount).ok_or_else(|| {
            LiquidityError::CalculationError("Repay amount exceeds total debt".into())
        })?;
        
        if remaining_debt.is_zero() {
            // Fully repaid
            loan.status = LoanStatus::Repaid;
            let collateral_returned = loan.collateral.clone();
            
            self.total_borrowed = self.total_borrowed.checked_sub(&loan.principal).ok_or_else(|| {
                LiquidityError::CalculationError("Total borrowed underflow".into())
            })?;
            
            return Ok((collateral_returned, Amount::zero()));
        } else {
            // Partial repayment
            // Apply to interest first, then principal
            if amount.inner() <= loan.interest.inner() {
                loan.interest = loan.interest.checked_sub(&amount).ok_or_else(|| {
                    LiquidityError::CalculationError("Interest underflow".into())
                })?;
            } else {
                let principal_payment = amount.checked_sub(&loan.interest).ok_or_else(|| {
                    LiquidityError::CalculationError("Calculation error in principal payment".into())
                })?;
                loan.interest = Amount::zero();
                loan.principal = loan.principal.checked_sub(&principal_payment).ok_or_else(|| {
                    LiquidityError::CalculationError("Principal underflow".into())
                })?;
                
                self.total_borrowed = self.total_borrowed.checked_sub(&principal_payment).ok_or_else(|| {
                    LiquidityError::CalculationError("Total borrowed underflow".into())
                })?;
            }
            
            return Ok((Amount::zero(), remaining_debt));
        }
    }

    /// Liquidate undercollateralized loan
    pub fn liquidate(&mut self, loan_id: Hash) -> LiquidityResult<(Address, Amount)> {
        let loan = self.loans.get_mut(&loan_id)
            .ok_or_else(|| LiquidityError::PositionNotFound(loan_id.to_hex()))?;

        // Check if liquidatable
        let threshold = self.liquidation_threshold as f64 / 10000.0;
        if !loan.is_liquidatable(threshold) {
            return Err(LiquidityError::PoolError(
                format!("Loan not liquidatable, health factor: {}", loan.health_factor())
            ));
        }

        // Mark as liquidated
        loan.status = LoanStatus::Liquidated;
        
        // Update totals
        self.total_borrowed = self.total_borrowed.checked_sub(&loan.principal).ok_or_else(|| {
            LiquidityError::CalculationError("Total borrowed underflow".into())
        })?;

        // Return borrower and collateral
        Ok((loan.borrower, loan.collateral.clone()))
    }

    /// Calculate utilization rate
    pub fn utilization_rate(&self) -> f64 {
        if self.total_supplied.is_zero() {
            return 0.0;
        }

        let borrowed = self.total_borrowed.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0) as f64;
        let supplied = self.total_supplied.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(1) as f64;

        borrowed / supplied
    }

    /// Get current borrow APY
    pub fn borrow_apy(&self) -> u16 {
        let utilization = self.utilization_rate();
        self.interest_model.calculate_rate(utilization)
    }

    /// Get current supply APY
    pub fn supply_apy(&self) -> u16 {
        let borrow_apy = self.borrow_apy();
        let utilization = self.utilization_rate();
        
        // Supply APY = Borrow APY × Utilization × (1 - Protocol Fee)
        // Simplified: no protocol fee
        ((borrow_apy as f64) * utilization) as u16
    }

    /// Calculate required collateral for borrow amount
    fn calculate_required_collateral(&self, borrow_amount: &Amount) -> Amount {
        let amount_val = borrow_amount.inner()
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0) as f64;

        let ratio = self.collateral_ratio as f64 / 10000.0;
        let required = amount_val * ratio;

        Amount::from_u64(required as u64)
    }

    /// Get available liquidity for borrowing
    fn available_liquidity(&self) -> Amount {
        self.total_supplied.checked_sub(&self.total_borrowed)
            .unwrap_or_else(Amount::zero)
    }

    /// Generate loan ID (simplified)
    fn generate_loan_id(&self, loan: &LoanPosition) -> Hash {
        let data = format!("{:?}{}", loan.borrower, loan.borrowed_at);
        blockchain_crypto::hash::Hashable::hash(data.as_bytes())
    }

    /// Get loan
    pub fn get_loan(&self, loan_id: &Hash) -> Option<&LoanPosition> {
        self.loans.get(loan_id)
    }

    /// Get all active loans
    pub fn active_loans(&self) -> Vec<&LoanPosition> {
        self.loans.values()
            .filter(|l| l.status == LoanStatus::Active)
            .collect()
    }
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
    use crate::pool::{PoolInfo, PoolType};

    fn create_test_lending_pool() -> LendingPool {
        let pool_info = PoolInfo::new(
            1,
            PoolType::Lending,
            "Test Lending".into(),
            Address::zero(),
            1000,
            500,
        );
        let base = LiquidityPool::new(pool_info);
        
        LendingPool::new(
            base,
            InterestRate::standard(),
            15000, // 150% collateral ratio
            12000, // 120% liquidation threshold
        )
    }

    #[test]
    fn test_supply() {
        let mut pool = create_test_lending_pool();
        let supplier = Address::zero();
        
        pool.supply(supplier, Amount::from_u64(10000)).unwrap();
        assert_eq!(pool.total_supplied, Amount::from_u64(10000));
    }

    #[test]
    fn test_borrow() {
        let mut pool = create_test_lending_pool();
        let supplier = Address::zero();
        let borrower = Address::zero();
        
        pool.supply(supplier, Amount::from_u64(10000)).unwrap();
        
        let loan_id = pool.borrow(
            borrower,
            Amount::from_u64(5000),
            Amount::from_u64(8000), // 150% collateral + buffer
        ).unwrap();
        
        assert!(pool.get_loan(&loan_id).is_some());
        assert_eq!(pool.total_borrowed, Amount::from_u64(5000));
    }

    #[test]
    fn test_interest_accrual() {
        let borrower = Address::zero();
        let mut loan = LoanPosition::new(
            borrower,
            Amount::from_u64(10000),
            Amount::from_u64(15000),
            1000, // 10% APY
        );

        // Fast forward 1 year (simplified)
        let future_time = current_timestamp() + 365 * 24 * 3600;
        loan.accrue_interest(future_time);

        assert!(loan.interest.inner() > &Amount::zero().inner());
    }

    #[test]
    fn test_liquidation_check() {
        let loan = LoanPosition {
            id: Hash::zero(),
            borrower: Address::zero(),
            principal: Amount::from_u64(10000),
            interest: Amount::zero(),
            collateral: Amount::from_u64(11000), // Only 110% collateralized
            rate: 1000,
            borrowed_at: current_timestamp(),
            last_accrual: current_timestamp(),
            status: LoanStatus::Active,
        };

        assert!(loan.is_liquidatable(1.2)); // 120% threshold
        assert!(!loan.is_liquidatable(1.0)); // 100% threshold
    }
}