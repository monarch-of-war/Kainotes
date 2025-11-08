// blockchain-core/src/fork.rs

use crate::{Block, BlockNumber, BlockchainError, BlockchainResult};
use blockchain_crypto::Hash;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// Fork choice rule
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ForkChoice {
    /// Longest chain (most blocks)
    LongestChain,
    /// Heaviest chain (most cumulative difficulty/work)
    HeaviestChain,
    /// Latest justified checkpoint (for finality)
    LatestJustified,
}

/// Fork information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkInfo {
    /// Fork point block number
    pub fork_point: BlockNumber,
    /// Fork point hash
    pub fork_hash: Hash,
    /// Main chain tip
    pub main_tip: Hash,
    /// Fork chain tip  
    pub fork_tip: Hash,
    /// Main chain length from fork
    pub main_length: u64,
    /// Fork chain length from fork
    pub fork_length: u64,
}

/// Fork resolver for handling chain reorganizations
pub struct ForkResolver {
    /// Fork choice rule
    choice: ForkChoice,
    /// Maximum reorg depth allowed
    max_reorg_depth: u64,
    /// Fork history
    fork_history: Vec<ForkInfo>,
}

impl ForkResolver {
    /// Create new fork resolver
    pub fn new(choice: ForkChoice, max_reorg_depth: u64) -> Self {
        Self {
            choice,
            max_reorg_depth,
            fork_history: Vec::new(),
        }
    }

    /// Detect if there's a fork
    pub fn detect_fork(
        &self,
        current_head: &Block,
        new_block: &Block,
    ) -> Option<ForkInfo> {
        // Check if new block builds on current head
        if new_block.header.parent_hash == current_head.hash() {
            return None; // No fork
        }

        // Fork detected
        Some(ForkInfo {
            fork_point: current_head.number(),
            fork_hash: current_head.hash(),
            main_tip: current_head.hash(),
            fork_tip: new_block.hash(),
            main_length: 0,
            fork_length: 1,
        })
    }

    /// Choose between two competing chains
    pub fn choose_chain(
        &self,
        main_chain: &[Block],
        fork_chain: &[Block],
    ) -> BlockchainResult<bool> {
        if fork_chain.is_empty() {
            return Ok(false); // Keep main chain
        }

        match self.choice {
            ForkChoice::LongestChain => {
                Ok(fork_chain.len() > main_chain.len())
            }
            ForkChoice::HeaviestChain => {
                // Calculate cumulative work (simplified)
                let main_work = main_chain.len() as u64;
                let fork_work = fork_chain.len() as u64;
                Ok(fork_work > main_work)
            }
            ForkChoice::LatestJustified => {
                // Would check for justified checkpoints
                // Simplified: use longest chain
                Ok(fork_chain.len() > main_chain.len())
            }
        }
    }

    /// Find common ancestor between two chains
    pub fn find_common_ancestor(
        &self,
        chain_a: &HashMap<Hash, Block>,
        chain_b: &HashMap<Hash, Block>,
        head_a: &Hash,
        head_b: &Hash,
    ) -> Option<Hash> {
        let mut current_a = *head_a;
        let mut current_b = *head_b;
        
        let mut visited_a = std::collections::HashSet::new();
        let mut visited_b = std::collections::HashSet::new();

        loop {
            // Check if we've found common block
            if visited_b.contains(&current_a) {
                return Some(current_a);
            }
            if visited_a.contains(&current_b) {
                return Some(current_b);
            }

            // Mark as visited
            visited_a.insert(current_a);
            visited_b.insert(current_b);

            // Move to parents
            let block_a = chain_a.get(&current_a)?;
            let block_b = chain_b.get(&current_b)?;

            if block_a.is_genesis() && block_b.is_genesis() {
                return Some(current_a); // Genesis is common ancestor
            }

            current_a = block_a.header.parent_hash;
            current_b = block_b.header.parent_hash;

            // Safety check - don't search forever
            if visited_a.len() > 10000 || visited_b.len() > 10000 {
                return None;
            }
        }
    }

    /// Calculate reorganization path
    pub fn calculate_reorg_path(
        &self,
        blocks: &HashMap<Hash, Block>,
        old_head: &Hash,
        new_head: &Hash,
    ) -> BlockchainResult<ReorgPath> {
        let common_ancestor = self.find_common_ancestor(blocks, blocks, old_head, new_head)
            .ok_or_else(|| BlockchainError::ForkDetected(
                "No common ancestor found".into()
            ))?;

        // Build path from old_head to common ancestor (to be reverted)
        let mut revert = Vec::new();
        let mut current = *old_head;
        while current != common_ancestor {
            let block = blocks.get(&current)
                .ok_or_else(|| BlockchainError::BlockNotFound(current))?;
            revert.push(block.clone());
            current = block.header.parent_hash;

            if revert.len() > self.max_reorg_depth as usize {
                return Err(BlockchainError::ReorgTooDeep {
                    depth: revert.len() as u64,
                });
            }
        }

        // Build path from common ancestor to new_head (to be applied)
        let mut apply = Vec::new();
        let mut current = *new_head;
        while current != common_ancestor {
            let block = blocks.get(&current)
                .ok_or_else(|| BlockchainError::BlockNotFound(current))?;
            apply.push(block.clone());
            current = block.header.parent_hash;
        }
        apply.reverse(); // Apply in forward order

        Ok(ReorgPath {
            common_ancestor,
            revert_blocks: revert.clone(),
            apply_blocks: apply,
            depth: revert.len() as u64,
        })
    }

    /// Record fork in history
    pub fn record_fork(&mut self, fork_info: ForkInfo) {
        self.fork_history.push(fork_info);
        
        // Keep only recent forks
        if self.fork_history.len() > 100 {
            self.fork_history.remove(0);
        }
    }

    /// Get fork history
    pub fn fork_history(&self) -> &[ForkInfo] {
        &self.fork_history
    }

    /// Get fork choice rule
    pub fn fork_choice(&self) -> ForkChoice {
        self.choice
    }

    /// Update fork choice rule
    pub fn set_fork_choice(&mut self, choice: ForkChoice) {
        self.choice = choice;
    }
}

/// Reorganization path
#[derive(Debug, Clone)]
pub struct ReorgPath {
    /// Common ancestor block hash
    pub common_ancestor: Hash,
    /// Blocks to revert (in reverse order)
    pub revert_blocks: Vec<Block>,
    /// Blocks to apply (in forward order)
    pub apply_blocks: Vec<Block>,
    /// Reorg depth
    pub depth: u64,
}

impl ReorgPath {
    /// Check if this is a simple reorganization
    pub fn is_simple(&self) -> bool {
        self.depth <= 1
    }

    /// Get total blocks involved
    pub fn total_blocks(&self) -> usize {
        self.revert_blocks.len() + self.apply_blocks.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fork_choice_longest_chain() {
        let resolver = ForkResolver::new(ForkChoice::LongestChain, 100);
        
        let main_chain = vec![Block::genesis(Hash::zero())];
        let fork_chain = vec![
            Block::genesis(Hash::zero()),
            Block::genesis(Hash::zero()),
        ];

        let choice = resolver.choose_chain(&main_chain, &fork_chain).unwrap();
        assert!(choice); // Fork chain is longer
    }

    #[test]
    fn test_detect_fork() {
        let resolver = ForkResolver::new(ForkChoice::LongestChain, 100);
        
        let genesis = Block::genesis(Hash::zero());
        let block1 = Block::new(
            1,
            genesis.hash(),
            Hash::zero(),
            blockchain_crypto::Address::zero(),
            vec![],
            10_000_000,
        ).unwrap();

        let block2 = Block::new(
            1,
            Hash::zero(), // Different parent
            Hash::zero(),
            blockchain_crypto::Address::zero(),
            vec![],
            10_000_000,
        ).unwrap();

        let fork = resolver.detect_fork(&block1, &block2);
        assert!(fork.is_some());
    }
}