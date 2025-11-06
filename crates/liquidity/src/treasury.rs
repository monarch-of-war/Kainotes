// liquidity/src/treasury.rs

use crate::{pool::LiquidityPool, LiquidityError, LiquidityResult};
use blockchain_core::{Amount, Timestamp};
use blockchain_crypto::{Address, Hash};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Grant proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grant {
    /// Grant ID
    pub id: Hash,
    /// Recipient address
    pub recipient: Address,
    /// Grant amount
    pub amount: Amount,
    /// Purpose/description
    pub purpose: String,
    /// Milestone-based disbursement
    pub milestones: Vec<Milestone>,
    /// Current milestone
    pub current_milestone: usize,
    /// Total disbursed
    pub disbursed: Amount,
    /// Proposal timestamp
    pub proposed_at: Timestamp,
    /// Approval timestamp
    pub approved_at: Option<Timestamp>,
    /// Status
    pub status: GrantStatus,
}

impl Grant {
    /// Create new grant proposal
    pub fn new(recipient: Address, amount: Amount, purpose: String, milestones: Vec<Milestone>) -> Self {
        Self {
            id: Hash::zero(), // Would generate proper hash
            recipient,
            amount,
            purpose,
            milestones,
            current_milestone: 0,
            disbursed: Amount::zero(),
            proposed_at: current_timestamp(),
            approved_at: None,
            status: GrantStatus::Proposed,
        }
    }

    /// Calculate total milestone amounts
    pub fn total_milestone_amounts(&self) -> Amount {
        self.milestones.iter()
            .fold(Amount::zero(), |acc, m| {
                acc.checked_add(&m.amount).unwrap_or(acc)
            })
    }

    /// Get current milestone
    pub fn current_milestone_info(&self) -> Option<&Milestone> {
        self.milestones.get(self.current_milestone)
    }

    /// Check if all milestones completed
    pub fn is_complete(&self) -> bool {
        self.current_milestone >= self.milestones.len()
    }

    /// Get completion percentage
    pub fn completion_percentage(&self) -> f64 {
        if self.milestones.is_empty() {
            return 0.0;
        }
        (self.current_milestone as f64 / self.milestones.len() as f64) * 100.0
    }
}

/// Grant milestone
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Milestone {
    /// Milestone description
    pub description: String,
    /// Amount to disburse on completion
    pub amount: Amount,
    /// Deadline
    pub deadline: Timestamp,
    /// Completion status
    pub completed: bool,
    /// Completion timestamp
    pub completed_at: Option<Timestamp>,
}

/// Grant status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GrantStatus {
    Proposed,
    Approved,
    Active,
    Completed,
    Rejected,
    Cancelled,
}

/// Treasury allocation category
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreasuryAllocation {
    /// Category name
    pub category: String,
    /// Allocated amount
    pub allocated: Amount,
    /// Spent amount
    pub spent: Amount,
    /// Reserved for ongoing grants
    pub reserved: Amount,
}

impl TreasuryAllocation {
    /// Create new allocation
    pub fn new(category: String, allocated: Amount) -> Self {
        Self {
            category,
            allocated,
            spent: Amount::zero(),
            reserved: Amount::zero(),
        }
    }

    /// Get available amount
    pub fn available(&self) -> Amount {
        self.allocated.checked_sub(&self.spent)
            .and_then(|a| a.checked_sub(&self.reserved))
            .unwrap_or_else(Amount::zero)
    }

    /// Get utilization rate
    pub fn utilization_rate(&self) -> f64 {
        if self.allocated.is_zero() {
            return 0.0;
        }

        let used = self.spent.checked_add(&self.reserved)
            .unwrap_or_else(|| self.spent.clone());

        let used_val = used.inner().to_u64_digits().first().copied().unwrap_or(0) as f64;
        let allocated_val = self.allocated.inner().to_u64_digits().first().copied().unwrap_or(1) as f64;

        (used_val / allocated_val) * 100.0
    }
}

/// Network treasury for ecosystem development
pub struct NetworkTreasury {
    /// Base liquidity pool
    base: LiquidityPool,
    /// Active grants
    grants: HashMap<Hash, Grant>,
    /// Budget allocations by category
    allocations: HashMap<String, TreasuryAllocation>,
    /// Total granted amount
    total_granted: Amount,
    /// Total disbursed
    total_disbursed: Amount,
    /// Governance threshold (votes needed for approval)
    governance_threshold: u64,
}

impl NetworkTreasury {
    /// Create new network treasury
    pub fn new(base: LiquidityPool, governance_threshold: u64) -> Self {
        let mut allocations = HashMap::new();
        
        // Initialize standard allocation categories
        allocations.insert(
            "Development".to_string(),
            TreasuryAllocation::new("Development".to_string(), Amount::zero()),
        );
        allocations.insert(
            "Marketing".to_string(),
            TreasuryAllocation::new("Marketing".to_string(), Amount::zero()),
        );
        allocations.insert(
            "Research".to_string(),
            TreasuryAllocation::new("Research".to_string(), Amount::zero()),
        );
        allocations.insert(
            "Community".to_string(),
            TreasuryAllocation::new("Community".to_string(), Amount::zero()),
        );

        Self {
            base,
            grants: HashMap::new(),
            allocations,
            total_granted: Amount::zero(),
            total_disbursed: Amount::zero(),
            governance_threshold,
        }
    }

    /// Deposit funds to treasury
    pub fn deposit(&mut self, depositor: Address, amount: Amount) -> LiquidityResult<()> {
        self.base.deposit(depositor, amount)
    }

    /// Set category allocation
    pub fn set_allocation(&mut self, category: String, amount: Amount) -> LiquidityResult<()> {
        self.allocations.entry(category.clone())
            .and_modify(|a| a.allocated = amount.clone())
            .or_insert_with(|| TreasuryAllocation::new(category, amount));

        Ok(())
    }

    /// Submit grant proposal
    pub fn propose_grant(
        &mut self,
        recipient: Address,
        amount: Amount,
        purpose: String,
        milestones: Vec<Milestone>,
        category: String,
    ) -> LiquidityResult<Hash> {
        // Check category exists and has allocation
        let allocation = self.allocations.get(&category)
            .ok_or_else(|| LiquidityError::PoolError(
                format!("Category '{}' not found", category)
            ))?;

        // Check sufficient allocation
        if allocation.available().inner() < amount.inner() {
            return Err(LiquidityError::InsufficientLiquidity {
                required: amount,
                available: allocation.available(),
            });
        }

        // Validate milestones sum to total
        let mut grant = Grant::new(recipient, amount.clone(), purpose, milestones);
        let milestone_total = grant.total_milestone_amounts();
        
        if milestone_total.inner() != amount.inner() {
            return Err(LiquidityError::InvalidAllocation(
                format!("Milestone amounts ({}) don't match grant amount ({})",
                    milestone_total, amount)
            ));
        }

        grant.id = self.generate_grant_id(&grant);
        let grant_id = grant.id;

        // Reserve funds
        let allocation = self.allocations.get_mut(&category).unwrap();
        allocation.reserved = allocation.reserved.checked_add(&amount)
            .ok_or_else(|| LiquidityError::CalculationError("Reserved overflow".into()))?;

        self.grants.insert(grant_id, grant);

        Ok(grant_id)
    }

    /// Approve grant (governance action)
    pub fn approve_grant(&mut self, grant_id: Hash) -> LiquidityResult<()> {
        let grant = self.grants.get_mut(&grant_id)
            .ok_or_else(|| LiquidityError::PositionNotFound(grant_id.to_hex()))?;

        if grant.status != GrantStatus::Proposed {
            return Err(LiquidityError::PoolError("Grant is not in proposed status".into()));
        }

        grant.status = GrantStatus::Approved;
        grant.approved_at = Some(current_timestamp());

        self.total_granted = self.total_granted.checked_add(&grant.amount)
            .ok_or_else(|| LiquidityError::CalculationError("Total granted overflow".into()))?;

        Ok(())
    }

    /// Disburse milestone payment
    pub fn disburse_milestone(
        &mut self,
        grant_id: Hash,
        category: String,
    ) -> LiquidityResult<Amount> {
        let grant = self.grants.get_mut(&grant_id)
            .ok_or_else(|| LiquidityError::PositionNotFound(grant_id.to_hex()))?;

        if grant.status != GrantStatus::Approved && grant.status != GrantStatus::Active {
            return Err(LiquidityError::PoolError("Grant not approved".into()));
        }

        // Check if current milestone is complete
        if grant.is_complete() {
            grant.status = GrantStatus::Completed;
            return Err(LiquidityError::PoolError("All milestones completed".into()));
        }

        let milestone = grant.milestones.get_mut(grant.current_milestone)
            .ok_or_else(|| LiquidityError::PoolError("Milestone not found".into()))?;

        if milestone.completed {
            return Err(LiquidityError::PoolError("Milestone already completed".into()));
        }

        // Mark milestone as completed
        milestone.completed = true;
        milestone.completed_at = Some(current_timestamp());

        let amount = milestone.amount.clone();

        // Update grant
        grant.disbursed = grant.disbursed.checked_add(&amount)
            .ok_or_else(|| LiquidityError::CalculationError("Disbursed overflow".into()))?;
        grant.current_milestone += 1;

        if grant.current_milestone == 1 {
            grant.status = GrantStatus::Active;
        }

        if grant.is_complete() {
            grant.status = GrantStatus::Completed;
        }

        // Update allocation
        let allocation = self.allocations.get_mut(&category)
            .ok_or_else(|| LiquidityError::PoolError(
                format!("Category '{}' not found", category)
            ))?;

        allocation.spent = allocation.spent.checked_add(&amount)
            .ok_or_else(|| LiquidityError::CalculationError("Spent overflow".into()))?;

        allocation.reserved = allocation.reserved.checked_sub(&amount)
            .ok_or_else(|| LiquidityError::CalculationError("Reserved underflow".into()))?;

        // Update total disbursed
        self.total_disbursed = self.total_disbursed.checked_add(&amount)
            .ok_or_else(|| LiquidityError::CalculationError("Total disbursed overflow".into()))?;

        Ok(amount)
    }

    /// Cancel grant
    pub fn cancel_grant(&mut self, grant_id: Hash, category: String) -> LiquidityResult<()> {
        let grant = self.grants.get_mut(&grant_id)
            .ok_or_else(|| LiquidityError::PositionNotFound(grant_id.to_hex()))?;

        if grant.status == GrantStatus::Completed || grant.status == GrantStatus::Cancelled {
            return Err(LiquidityError::PoolError("Cannot cancel completed or cancelled grant".into()));
        }

        // Calculate unreleased funds
        let unreleased = grant.amount.checked_sub(&grant.disbursed).
            ok_or_else(|| LiquidityError::CalculationError("Unreleased calculation error".into()))?;

        // Release reserved funds
        let allocation = self.allocations.get_mut(&category)
            .ok_or_else(|| LiquidityError::PoolError(format!("Category '{}' not found", category)))?;

        allocation.reserved = allocation.reserved.checked_sub(&unreleased)
            .unwrap_or_else(Amount::zero);

        grant.status = GrantStatus::Cancelled;

        Ok(())
    }

    /// Get grant
    pub fn get_grant(&self, grant_id: &Hash) -> Option<&Grant> {
        self.grants.get(grant_id)
    }

    /// Get all grants with status
    pub fn grants_by_status(&self, status: GrantStatus) -> Vec<&Grant> {
        self.grants.values()
            .filter(|g| g.status == status)
            .collect()
    }

    /// Get allocation
    pub fn get_allocation(&self, category: &str) -> Option<&TreasuryAllocation> {
        self.allocations.get(category)
    }

    /// Get all allocations
    pub fn allocations(&self) -> &HashMap<String, TreasuryAllocation> {
        &self.allocations
    }

    /// Get treasury balance
    pub fn balance(&self) -> Amount {
        self.base.info.tvl.clone()
    }

    /// Generate grant ID (simplified)
    fn generate_grant_id(&self, grant: &Grant) -> Hash {
        let data = format!("{:?}{}", grant.recipient, grant.proposed_at);
        blockchain_crypto::hash::Hashable::hash(data.as_bytes())
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

    fn create_test_treasury() -> NetworkTreasury {
        let pool_info = PoolInfo::new(
            1,
            PoolType::Treasury,
            "Test Treasury".into(),
            Address::zero(),
            500,
            100,
        );
        let base = LiquidityPool::new(pool_info);
        
        NetworkTreasury::new(base, 100)
    }

    #[test]
    fn test_treasury_deposit() {
        let mut treasury = create_test_treasury();
        let depositor = Address::zero();
        
        treasury.deposit(depositor, Amount::from_u64(100000)).unwrap();
        assert_eq!(treasury.balance(), Amount::from_u64(100000));
    }

    #[test]
    fn test_set_allocation() {
        let mut treasury = create_test_treasury();
        
        treasury.set_allocation("Development".to_string(), Amount::from_u64(50000)).unwrap();
        
        let allocation = treasury.get_allocation("Development").unwrap();
        assert_eq!(allocation.allocated, Amount::from_u64(50000));
    }

    #[test]
    fn test_grant_proposal() {
        let mut treasury = create_test_treasury();
        let depositor = Address::zero();
        let recipient = Address::zero();
        
        treasury.deposit(depositor, Amount::from_u64(100000)).unwrap();
        treasury.set_allocation("Development".to_string(), Amount::from_u64(50000)).unwrap();

        let milestones = vec![
            Milestone {
                description: "Phase 1".to_string(),
                amount: Amount::from_u64(5000),
                deadline: current_timestamp() + 30 * 24 * 3600,
                completed: false,
                completed_at: None,
            },
            Milestone {
                description: "Phase 2".to_string(),
                amount: Amount::from_u64(5000),
                deadline: current_timestamp() + 60 * 24 * 3600,
                completed: false,
                completed_at: None,
            },
        ];

        let grant_id = treasury.propose_grant(
            recipient,
            Amount::from_u64(10000),
            "Build feature X".to_string(),
            milestones,
            "Development".to_string(),
        ).unwrap();

        assert!(treasury.get_grant(&grant_id).is_some());
    }

    #[test]
    fn test_milestone_disbursement() {
        let mut treasury = create_test_treasury();
        let depositor = Address::zero();
        let recipient = Address::zero();
        
        treasury.deposit(depositor, Amount::from_u64(100000)).unwrap();
        treasury.set_allocation("Development".to_string(), Amount::from_u64(50000)).unwrap();

        let milestones = vec![
            Milestone {
                description: "Phase 1".to_string(),
                amount: Amount::from_u64(10000),
                deadline: current_timestamp() + 30 * 24 * 3600,
                completed: false,
                completed_at: None,
            },
        ];

        let grant_id = treasury.propose_grant(
            recipient,
            Amount::from_u64(10000),
            "Build feature X".to_string(),
            milestones,
            "Development".to_string(),
        ).unwrap();

        treasury.approve_grant(grant_id).unwrap();
        
        let disbursed = treasury.disburse_milestone(grant_id, "Development".to_string()).unwrap();
        assert_eq!(disbursed, Amount::from_u64(10000));

        let grant = treasury.get_grant(&grant_id).unwrap();
        assert_eq!(grant.status, GrantStatus::Completed);
    }
}