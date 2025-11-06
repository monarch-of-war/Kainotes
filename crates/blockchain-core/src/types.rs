// blockchain-core/src/types.rs

use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use std::ops::{Add, Sub};

/// Block number/height
pub type BlockNumber = u64;

/// Transaction nonce
pub type Nonce = u64;

/// Gas price
pub type GasPrice = u64;

/// Gas limit/used
pub type Gas = u64;

/// Timestamp in Unix epoch seconds
pub type Timestamp = u64;

/// Token amount (using BigUint for arbitrary precision)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Amount(BigUint);

impl Amount {
    pub fn new(value: BigUint) -> Self {
        Self(value)
    }

    pub fn zero() -> Self {
        Self(BigUint::from(0u64))
    }

    pub fn from_u64(value: u64) -> Self {
        Self(BigUint::from(value))
    }

    pub fn from_tokens(tokens: u64) -> Self {
        // 1 token = 10^18 base units (similar to ETH/wei)
        Self(BigUint::from(tokens) * BigUint::from(10u64).pow(18))
    }

    pub fn inner(&self) -> &BigUint {
        &self.0
    }

    pub fn is_zero(&self) -> bool {
        self.0 == BigUint::from(0u64)
    }

    pub fn checked_add(&self, other: &Amount) -> Option<Amount> {
        Some(Amount(&self.0 + &other.0))
    }

    pub fn checked_sub(&self, other: &Amount) -> Option<Amount> {
        if &self.0 < &other.0 {
            None
        } else {
            Some(Amount(&self.0 - &other.0))
        }
    }
}

impl Add for Amount {
    type Output = Amount;

    fn add(self, other: Amount) -> Amount {
        Amount(&self.0 + &other.0)
    }
}

impl Sub for Amount {
    type Output = Amount;

    fn sub(self, other: Amount) -> Amount {
        Amount(&self.0 - &other.0)
    }
}

impl std::fmt::Display for Amount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Validator stake amount
pub type StakeAmount = Amount;

/// Utility score (scaled by 1000 for precision)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UtilityScore(u64);

impl UtilityScore {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn zero() -> Self {
        Self(0)
    }

    pub fn value(&self) -> u64 {
        self.0
    }

    pub fn from_percentage(percent: f64) -> Self {
        Self((percent * 1000.0) as u64)
    }

    pub fn to_percentage(&self) -> f64 {
        self.0 as f64 / 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_amount_arithmetic() {
        let a = Amount::from_u64(100);
        let b = Amount::from_u64(50);
        
        let sum = a.checked_add(&b).unwrap();
        assert_eq!(sum, Amount::from_u64(150));
        
        let diff = sum.checked_sub(&b).unwrap();
        assert_eq!(diff, Amount::from_u64(100));
    }

    #[test]
    fn test_amount_underflow() {
        let a = Amount::from_u64(50);
        let b = Amount::from_u64(100);
        
        assert!(a.checked_sub(&b).is_none());
    }

    #[test]
    fn test_utility_score() {
        let score = UtilityScore::from_percentage(85.5);
        assert_eq!(score.to_percentage(), 85.5);
    }
}