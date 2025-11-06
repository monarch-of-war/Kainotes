// blockchain-crypto/src/merkle.rs

use crate::{hash::Hashable, CryptoError, CryptoResult, Hash};
use serde::{Deserialize, Serialize};

/// Merkle tree for efficient verification of large datasets
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleTree {
    /// All nodes in the tree (stored as a flat array)
    nodes: Vec<Hash>,
    /// Number of leaf nodes
    leaf_count: usize,
}

impl MerkleTree {
    /// Build a Merkle tree from leaf data
    pub fn new<T: AsRef<[u8]>>(leaves: &[T]) -> CryptoResult<Self> {
        if leaves.is_empty() {
            return Err(CryptoError::MerkleError("Cannot create empty tree".into()));
        }

        let leaf_count = leaves.len();
        let total_nodes = Self::total_nodes(leaf_count);
        let mut nodes = vec![Hash::zero(); total_nodes];

        // Hash all leaves
        let leaf_start = total_nodes - leaf_count;
        for (i, leaf) in leaves.iter().enumerate() {
            nodes[leaf_start + i] = leaf.as_ref().hash();
        }

        // Build tree from bottom up
        Self::build_tree(&mut nodes, leaf_start);

        Ok(Self { nodes, leaf_count })
    }

    /// Get the root hash of the tree
    pub fn root(&self) -> Hash {
        self.nodes[0]
    }

    /// Get the number of leaves
    pub fn leaf_count(&self) -> usize {
        self.leaf_count
    }

    /// Generate a Merkle proof for a specific leaf
    pub fn proof(&self, index: usize) -> CryptoResult<MerkleProof> {
        if index >= self.leaf_count {
            return Err(CryptoError::MerkleError("Index out of bounds".into()));
        }

        let mut proof_hashes = Vec::new();
        let mut current_index = self.leaf_start() + index;

        while current_index > 0 {
            let sibling_index = Self::sibling_index(current_index);
            proof_hashes.push(self.nodes[sibling_index]);
            current_index = Self::parent_index(current_index);
        }

        Ok(MerkleProof {
            leaf_index: index,
            leaf_hash: self.nodes[self.leaf_start() + index],
            proof_hashes,
        })
    }

    /// Verify a Merkle proof
    pub fn verify_proof(root: Hash, proof: &MerkleProof, leaf_data: &[u8]) -> bool {
        let leaf_hash = leaf_data.hash();
        if leaf_hash != proof.leaf_hash {
            return false;
        }

        let mut current_hash = leaf_hash;
        let mut index = proof.leaf_index;

        for proof_hash in &proof.proof_hashes {
            current_hash = if index % 2 == 0 {
                Self::combine_hashes(current_hash, *proof_hash)
            } else {
                Self::combine_hashes(*proof_hash, current_hash)
            };
            index /= 2;
        }

        current_hash == root
    }

    // Helper functions

    fn total_nodes(leaf_count: usize) -> usize {
        // For a binary tree: 2*n - 1 nodes for n leaves
        // But we pad to next power of 2 for simplicity
        let padded_leaves = leaf_count.next_power_of_two();
        2 * padded_leaves - 1
    }

    fn leaf_start(&self) -> usize {
        self.nodes.len() - self.leaf_count.next_power_of_two()
    }

    fn parent_index(index: usize) -> usize {
        (index - 1) / 2
    }

    fn sibling_index(index: usize) -> usize {
        if index % 2 == 0 {
            index - 1
        } else {
            index + 1
        }
    }

    fn build_tree(nodes: &mut [Hash], leaf_start: usize) {
        let mut level_start = leaf_start;
        
        while level_start > 0 {
            let parent_start = Self::parent_index(level_start);
            let level_size = level_start - parent_start;
            
            for i in 0..level_size {
                let left_index = level_start + i * 2;
                let right_index = left_index + 1;
                
                let combined = if right_index < nodes.len() {
                    Self::combine_hashes(nodes[left_index], nodes[right_index])
                } else {
                    nodes[left_index]
                };
                
                nodes[parent_start + i] = combined;
            }
            
            level_start = parent_start;
        }
    }

    fn combine_hashes(left: Hash, right: Hash) -> Hash {
        let mut combined = Vec::with_capacity(64);
        combined.extend_from_slice(left.as_bytes());
        combined.extend_from_slice(right.as_bytes());
        combined.hash()
    }
}

/// Merkle proof for verifying a leaf is in the tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleProof {
    leaf_index: usize,
    leaf_hash: Hash,
    proof_hashes: Vec<Hash>,
}

impl MerkleProof {
    pub fn leaf_index(&self) -> usize {
        self.leaf_index
    }

    pub fn leaf_hash(&self) -> Hash {
        self.leaf_hash
    }

    pub fn proof_hashes(&self) -> &[Hash] {
        &self.proof_hashes
    }

    pub fn verify(&self, root: Hash, leaf_data: &[u8]) -> bool {
        MerkleTree::verify_proof(root, self, leaf_data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merkle_tree_basic() {
        let leaves = vec![b"leaf1", b"leaf2", b"leaf3", b"leaf4"];
        let tree = MerkleTree::new(&leaves).unwrap();
        
        assert_eq!(tree.leaf_count(), 4);
        assert_ne!(tree.root(), Hash::zero());
    }

    #[test]
    fn test_merkle_proof() {
        let leaves = vec![b"apple", b"banan", b"chery", b"dates"];
        let tree = MerkleTree::new(&leaves).unwrap();
        
        for i in 0..leaves.len() {
            let proof = tree.proof(i).unwrap();
            assert!(proof.verify(tree.root(), leaves[i]));
        }
    }

    #[test]
    fn test_merkle_proof_invalid() {
        let leaves = vec![b"apple", b"banan", b"chery"];
        let tree = MerkleTree::new(&leaves).unwrap();
        
        let proof = tree.proof(0).unwrap();
        assert!(!proof.verify(tree.root(), b"invalid"));
    }

    #[test]
    fn test_single_leaf() {
        let leaves = vec![b"single"];
        let tree = MerkleTree::new(&leaves).unwrap();
        let proof = tree.proof(0).unwrap();
        assert!(proof.verify(tree.root(), b"single"));
    }
}