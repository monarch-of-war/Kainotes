// blockchain-crypto/src/hash.rs

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sha3::Sha3_256;
use std::fmt;

/// Hash output size in bytes
pub const HASH_SIZE: usize = 32;

/// Supported hash algorithms
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HashAlgorithm {
    Sha256,
    Sha3_256,
    Blake3,
}

/// A 32-byte hash value
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hash([u8; HASH_SIZE]);

impl Hash {
    /// Create a new hash from bytes
    pub fn new(bytes: [u8; HASH_SIZE]) -> Self {
        Self(bytes)
    }

    /// Create a hash from a slice (returns error if wrong length)
    pub fn from_slice(slice: &[u8]) -> Result<Self, crate::CryptoError> {
        if slice.len() != HASH_SIZE {
            return Err(crate::CryptoError::InvalidHash);
        }
        let mut bytes = [0u8; HASH_SIZE];
        bytes.copy_from_slice(slice);
        Ok(Self(bytes))
    }

    /// Get the hash as a byte slice
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Get the hash as a fixed-size array
    pub fn to_bytes(&self) -> [u8; HASH_SIZE] {
        self.0
    }

    /// Create a zero hash (useful for genesis)
    pub fn zero() -> Self {
        Self([0u8; HASH_SIZE])
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Parse from hex string
    pub fn from_hex(s: &str) -> Result<Self, crate::CryptoError> {
        let bytes = hex::decode(s)
            .map_err(|e| crate::CryptoError::DeserializationError(e.to_string()))?;
        Self::from_slice(&bytes)
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash({}...{})", 
               hex::encode(&self.0[..4]), 
               hex::encode(&self.0[28..]))
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl Default for Hash {
    fn default() -> Self {
        Self::zero()
    }
}

/// Trait for types that can be hashed
pub trait Hashable {
    fn hash(&self) -> Hash;
    fn hash_with(&self, algorithm: HashAlgorithm) -> Hash;
}

impl Hashable for [u8] {
    fn hash(&self) -> Hash {
        self.hash_with(HashAlgorithm::Sha256)
    }

    fn hash_with(&self, algorithm: HashAlgorithm) -> Hash {
        match algorithm {
            HashAlgorithm::Sha256 => {
                let mut hasher = Sha256::new();
                hasher.update(self);
                Hash::new(hasher.finalize().into())
            }
            HashAlgorithm::Sha3_256 => {
                let mut hasher = Sha3_256::new();
                hasher.update(self);
                Hash::new(hasher.finalize().into())
            }
            HashAlgorithm::Blake3 => {
                let hash = blake3::hash(self);
                Hash::new(*hash.as_bytes())
            }
        }
    }
}

impl Hashable for Vec<u8> {
    fn hash(&self) -> Hash {
        self.as_slice().hash()
    }

    fn hash_with(&self, algorithm: HashAlgorithm) -> Hash {
        self.as_slice().hash_with(algorithm)
    }
}

impl Hashable for &str {
    fn hash(&self) -> Hash {
        self.as_bytes().hash()
    }

    fn hash_with(&self, algorithm: HashAlgorithm) -> Hash {
        self.as_bytes().hash_with(algorithm)
    }
}

/// Double hash (hash of hash) - commonly used for additional security
pub fn double_hash(data: &[u8]) -> Hash {
    let first = data.hash();
    first.as_bytes().hash()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_basic() {
        let data = b"Hello, World!";
        let hash1 = data.hash();
        let hash2 = data.hash();
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_algorithms() {
        let data = b"test data";
        let sha256 = data.hash_with(HashAlgorithm::Sha256);
        let sha3 = data.hash_with(HashAlgorithm::Sha3_256);
        let blake3 = data.hash_with(HashAlgorithm::Blake3);
        
        assert_ne!(sha256, sha3);
        assert_ne!(sha256, blake3);
        assert_ne!(sha3, blake3);
    }

    #[test]
    fn test_hash_hex() {
        let data = b"test";
        let hash = data.hash();
        let hex = hash.to_hex();
        let parsed = Hash::from_hex(&hex).unwrap();
        assert_eq!(hash, parsed);
    }

    #[test]
    fn test_double_hash() {
        let data = b"double hash test";
        let single = data.hash();
        let double = double_hash(data);
        assert_ne!(single, double);
    }
}