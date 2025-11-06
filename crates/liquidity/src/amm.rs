// liquidity/src/amm.rs

use crate::{pool::LiquidityPool, LiquidityError, LiquidityResult};
use blockchain_core::Amount;
use blockchain_crypto::Address;
use serde::{Deserialize, Serialize};

/// Trading pair for AMM
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TradingPair {
    /// Token A address
    pub token_a: Address,
    /// Token B address
    pub token_b: Address,
}

impl TradingPair {
    /// Create a new trading pair
    pub fn new(token_a: Address, token_b: Address) -> Self {
        Self { token_a, token_b }
    }

    /// Get canonical representation (sorted)
    pub fn canonical(&self) -> TradingPair {
        if self.token_a.as_bytes() < self.token_b.as_bytes() {
            self.clone()
        } else {
            TradingPair {
                token_a: self.token_b,
                token_b: self.token_a,
            }
        }
    }
}

/// AMM liquidity pool (constant product formula: x * y = k)
pub struct AMMPool {
    /// Base liquidity pool
    base: LiquidityPool,
    /// Trading pair
    pair: TradingPair,
    /// Reserve of token A
    reserve_a: Amount,
    /// Reserve of token B
    reserve_b: Amount,
    /// Total liquidity provider shares
    total_shares: Amount,
    /// Fee rate (basis points, e.g., 30 = 0.3%)
    fee_rate: u16,
}

impl AMMPool {
    /// Create new AMM pool
    pub fn new(base: LiquidityPool, pair: TradingPair, fee_rate: u16) -> Self {
        Self {
            base,
            pair: pair.canonical(),
            reserve_a: Amount::zero(),
            reserve_b: Amount::zero(),
            total_shares: Amount::zero(),
            fee_rate: fee_rate.min(10000),
        }
    }

    /// Add liquidity to the pool
    pub fn add_liquidity(
        &mut self,
        provider: Address,
        amount_a: Amount,
        amount_b: Amount,
    ) -> LiquidityResult<Amount> {
        if amount_a.is_zero() || amount_b.is_zero() {
            return Err(LiquidityError::DeploymentError(
                "Cannot add zero liquidity".into()
            ));
        }

        let shares = if self.total_shares.is_zero() {
            // Initial liquidity: shares = sqrt(amount_a * amount_b)
            let a_val = amount_a.inner().to_u64_digits().first().copied().unwrap_or(0) as f64;
            let b_val = amount_b.inner().to_u64_digits().first().copied().unwrap_or(0) as f64;
            let shares_val = (a_val * b_val).sqrt();
            Amount::from_u64(shares_val as u64)
        } else {
            // Proportional shares: shares = min(amount_a / reserve_a, amount_b / reserve_b) * total_shares
            let a_ratio = self.calculate_ratio(&amount_a, &self.reserve_a);
            let b_ratio = self.calculate_ratio(&amount_b, &self.reserve_b);
            let ratio = a_ratio.min(b_ratio);
            
            let total_val = self.total_shares.inner().to_u64_digits().first().copied().unwrap_or(0) as f64;
            let shares_val = total_val * ratio;
            Amount::from_u64(shares_val as u64)
        };

        // Update reserves
        self.reserve_a = self.reserve_a.checked_add(&amount_a)
            .ok_or_else(|| LiquidityError::CalculationError("Reserve A overflow".into()))?;
        self.reserve_b = self.reserve_b.checked_add(&amount_b)
            .ok_or_else(|| LiquidityError::CalculationError("Reserve B overflow".into()))?;

        // Update total shares
        self.total_shares = self.total_shares.checked_add(&shares)
            .ok_or_else(|| LiquidityError::CalculationError("Total shares overflow".into()))?;

        // Deposit to base pool
        let total_deposited = amount_a.checked_add(&amount_b)
            .ok_or_else(|| LiquidityError::CalculationError("Total deposit overflow".into()))?;
        self.base.deposit(provider, total_deposited)?;

        Ok(shares)
    }

    /// Remove liquidity from the pool
    pub fn remove_liquidity(
        &mut self,
        provider: Address,
        shares: Amount,
    ) -> LiquidityResult<(Amount, Amount)> {
        if shares.is_zero() {
            return Err(LiquidityError::DeploymentError(
                "Cannot remove zero shares".into()
            ));
        }

        if shares.inner() > self.total_shares.inner() {
            return Err(LiquidityError::InsufficientLiquidity {
                required: shares,
                available: self.total_shares.clone(),
            });
        }

        // Calculate amounts: amount = (shares / total_shares) * reserve
        let share_ratio = self.calculate_ratio(&shares, &self.total_shares);
        
        let reserve_a_val = self.reserve_a.inner().to_u64_digits().first().copied().unwrap_or(0) as f64;
        let reserve_b_val = self.reserve_b.inner().to_u64_digits().first().copied().unwrap_or(0) as f64;
        
        let amount_a = Amount::from_u64((reserve_a_val * share_ratio) as u64);
        let amount_b = Amount::from_u64((reserve_b_val * share_ratio) as u64);

        // Update reserves
        self.reserve_a = self.reserve_a.checked_sub(&amount_a)
            .ok_or_else(|| LiquidityError::CalculationError("Reserve A underflow".into()))?;
        self.reserve_b = self.reserve_b.checked_sub(&amount_b)
            .ok_or_else(|| LiquidityError::CalculationError("Reserve B underflow".into()))?;

        // Update total shares
        self.total_shares = self.total_shares.checked_sub(&shares)
            .ok_or_else(|| LiquidityError::CalculationError("Total shares underflow".into()))?;

        // Request withdrawal from base pool
        let total_withdrawn = amount_a.checked_add(&amount_b)
            .ok_or_else(|| LiquidityError::CalculationError("Total withdrawal overflow".into()))?;
        
        let unlock_time = current_timestamp() + 7 * 24 * 3600; // 7 days
        self.base.request_withdrawal(provider, total_withdrawn, unlock_time)?;

        Ok((amount_a, amount_b))
    }

    /// Get swap quote (how much output for given input)
    pub fn get_swap_quote(
        &self,
        token_in: Address,
        amount_in: Amount,
    ) -> LiquidityResult<SwapQuote> {
        if amount_in.is_zero() {
            return Err(LiquidityError::CalculationError("Cannot swap zero amount".into()));
        }

        let (reserve_in, reserve_out) = if token_in == self.pair.token_a {
            (&self.reserve_a, &self.reserve_b)
        } else if token_in == self.pair.token_b {
            (&self.reserve_b, &self.reserve_a)
        } else {
            return Err(LiquidityError::PoolError("Token not in pair".into()));
        };

        // Calculate output using constant product formula: (x + dx) * (y - dy) = x * y
        // dy = (y * dx * (1 - fee)) / (x + dx * (1 - fee))
        
        let amount_in_val = amount_in.inner().to_u64_digits().first().copied().unwrap_or(0) as f64;
        let reserve_in_val = reserve_in.inner().to_u64_digits().first().copied().unwrap_or(1) as f64;
        let reserve_out_val = reserve_out.inner().to_u64_digits().first().copied().unwrap_or(0) as f64;

        // Apply fee
        let fee_multiplier = 1.0 - (self.fee_rate as f64 / 10000.0);
        let amount_in_with_fee = amount_in_val * fee_multiplier;

        // Calculate output
        let amount_out_val = (reserve_out_val * amount_in_with_fee) / (reserve_in_val + amount_in_with_fee);
        let amount_out = Amount::from_u64(amount_out_val as u64);

        // Calculate price impact
        let price_before = reserve_out_val / reserve_in_val;
        let new_reserve_in = reserve_in_val + amount_in_val;
        let new_reserve_out = reserve_out_val - amount_out_val;
        let price_after = new_reserve_out / new_reserve_in;
        let price_impact = ((price_after - price_before) / price_before).abs() * 100.0;

        // Calculate effective price
        let effective_price = amount_out_val / amount_in_val;

        Ok(SwapQuote {
            amount_in: amount_in.clone(),
            amount_out,
            price_impact,
            effective_price,
            fee_amount: Amount::from_u64((amount_in_val * (self.fee_rate as f64 / 10000.0)) as u64),
        })
    }

    /// Execute swap
    pub fn swap(
        &mut self,
        token_in: Address,
        amount_in: Amount,
        min_amount_out: Amount,
    ) -> LiquidityResult<Amount> {
        let quote = self.get_swap_quote(token_in, amount_in.clone())?;

        // Check slippage
        if quote.amount_out.inner() < min_amount_out.inner() {
            let slippage = ((min_amount_out.inner().to_u64_digits().first().copied().unwrap_or(0) as f64
                - quote.amount_out.inner().to_u64_digits().first().copied().unwrap_or(0) as f64)
                / min_amount_out.inner().to_u64_digits().first().copied().unwrap_or(1) as f64) * 100.0;
            return Err(LiquidityError::SlippageTooHigh(slippage));
        }

        // Update reserves
        if token_in == self.pair.token_a {
            self.reserve_a = self.reserve_a.checked_add(&amount_in).ok_or_else(|| LiquidityError::CalculationError("Reserve A overflow".into()))?;
            self.reserve_b = self.reserve_b.checked_sub(&quote.amount_out).ok_or_else(|| LiquidityError::CalculationError("Reserve B underflow".into()))?;
        } else {
            self.reserve_b = self.reserve_b.checked_add(&amount_in).ok_or_else(|| LiquidityError::CalculationError("Reserve B overflow".into()))?;
            self.reserve_a = self.reserve_a.checked_sub(&quote.amount_out).ok_or_else(|| LiquidityError::CalculationError("Reserve A underflow".into()))?;
        }

        Ok(quote.amount_out)
    }

    /// Get current reserves
    pub fn reserves(&self) -> (Amount, Amount) {
        (self.reserve_a.clone(), self.reserve_b.clone())
    }

    /// Get trading pair
    pub fn pair(&self) -> &TradingPair {
        &self.pair
    }

    /// Calculate ratio between two amounts
    fn calculate_ratio(&self, numerator: &Amount, denominator: &Amount) -> f64 {
        if denominator.is_zero() {
            return 0.0;
        }

        let num = numerator.inner().to_u64_digits().first().copied().unwrap_or(0) as f64;
        let den = denominator.inner().to_u64_digits().first().copied().unwrap_or(1) as f64;
        num / den
    }
}

/// Swap quote information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapQuote {
    /// Input amount
    pub amount_in: Amount,
    /// Output amount
    pub amount_out: Amount,
    /// Price impact percentage
    pub price_impact: f64,
    /// Effective price (output/input)
    pub effective_price: f64,
    /// Fee amount
    pub fee_amount: Amount,
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
    use crate::pool::{PoolInfo, PoolType};

    fn create_test_amm() -> AMMPool {
        let pool_info = PoolInfo::new(
            1,
            PoolType::AMM,
            "Test AMM".into(),
            Address::zero(),
            1000,
            500,
        );
        let base = LiquidityPool::new(pool_info);
        let pair = TradingPair::new(Address::zero(), Address::zero());
        
        AMMPool::new(base, pair, 30) // 0.3% fee
    }

    #[test]
    fn test_add_liquidity() {
        let mut amm = create_test_amm();
        let provider = Address::zero();
        
        let shares = amm.add_liquidity(
            provider,
            Amount::from_u64(1000),
            Amount::from_u64(1000),
        ).unwrap();

        assert!(shares.inner() > &Amount::zero().inner());
        assert_eq!(amm.total_shares, shares);
    }

    #[test]
    fn test_swap_quote() {
        let mut amm = create_test_amm();
        let provider = Address::zero();
        
        // Add initial liquidity
        amm.add_liquidity(provider, Amount::from_u64(10000), Amount::from_u64(10000)).unwrap();

        // Get swap quote
        let quote = amm.get_swap_quote(amm.pair.token_a, Amount::from_u64(100)).unwrap();
        
        assert!(quote.amount_out.inner() > &Amount::zero().inner());
        assert!(quote.price_impact < 5.0); // Small trade should have low impact
    }

    #[test]
    fn test_swap() {
        let mut amm = create_test_amm();
        let provider = Address::zero();
        
        amm.add_liquidity(provider, Amount::from_u64(10000), Amount::from_u64(10000)).unwrap();

        let amount_out = amm.swap(
            amm.pair.token_a,
            Amount::from_u64(100),
            Amount::from_u64(90),
        ).unwrap();

        assert!(amount_out.inner() >= &Amount::from_u64(90).inner());
    }
}