// blockchain-crypto/src/lib.rs

//! Cryptographic primitives for the utility-backed blockchain protocol
//!
//! This crate provides:
//! - Hashing functions (SHA256, SHA3, Blake3)
//! - Digital signatures (Ed25519, SECP256k1)
//! - Key pair generation and management
//! - Merkle tree implementation

pub mod hash;
pub mod signature;
pub mod keypair;
pub mod merkle;

pub use hash::{Hash, HashAlgorithm, Hashable};
pub use signature::{Signature, SignatureScheme};
pub use keypair::{KeyPair, PublicKey, SecretKey, Address};
pub use merkle::MerkleTree;

/// Result type for cryptographic operations
pub type CryptoResult<T> = Result<T, CryptoError>;

/// Errors that can occur during cryptographic operations
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("Invalid signature")]
    InvalidSignature,
    
    #[error("Invalid public key")]
    InvalidPublicKey,
    
    #[error("Invalid secret key")]
    InvalidSecretKey,
    
    #[error("Invalid hash")]
    InvalidHash,
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    #[error("Deserialization error: {0}")]
    DeserializationError(String),
    
    #[error("Random number generation failed")]
    RngError,
    
    #[error("Merkle tree error: {0}")]
    MerkleError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crypto_basics() {
        // Basic smoke test
        let keypair = KeyPair::generate(SignatureScheme::Ed25519).unwrap();
        let message = b"Hello, blockchain!";
        let signature = keypair.sign(message).unwrap();
        assert!(keypair.public_key().verify(message, &signature).unwrap());
    }
}