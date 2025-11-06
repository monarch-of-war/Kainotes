// blockchain-crypto/src/signature.rs

use serde::{Deserialize, Serialize};
use std::fmt;

/// Supported signature schemes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignatureScheme {
    /// Ed25519 (fast, deterministic)
    Ed25519,
    /// SECP256k1 (Ethereum-compatible)
    Secp256k1,
}

/// Digital signature wrapper
#[derive(Clone, Serialize, Deserialize)]
pub struct Signature {
    scheme: SignatureScheme,
    bytes: Vec<u8>,
}

impl Signature {
    /// Create a new signature
    pub fn new(scheme: SignatureScheme, bytes: Vec<u8>) -> Self {
        Self { scheme, bytes }
    }

    /// Get the signature scheme
    pub fn scheme(&self) -> SignatureScheme {
        self.scheme
    }

    /// Get the signature bytes
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Get owned bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(&self.bytes)
    }

    /// Parse from hex string
    pub fn from_hex(scheme: SignatureScheme, s: &str) -> Result<Self, crate::CryptoError> {
        let bytes = hex::decode(s)
            .map_err(|e| crate::CryptoError::DeserializationError(e.to_string()))?;
        Ok(Self::new(scheme, bytes))
    }

    /// Verify this signature is valid for the given message and public key
    pub fn verify(
        &self,
        message: &[u8],
        public_key: &crate::PublicKey,
    ) -> Result<bool, crate::CryptoError> {
        if self.scheme != public_key.scheme() {
            return Ok(false);
        }

        match self.scheme {
            SignatureScheme::Ed25519 => self.verify_ed25519(message, public_key),
            SignatureScheme::Secp256k1 => self.verify_secp256k1(message, public_key),
        }
    }

    fn verify_ed25519(
        &self,
        message: &[u8],
        public_key: &crate::PublicKey,
    ) -> Result<bool, crate::CryptoError> {
        use ed25519_dalek::{Signature as Ed25519Sig, Verifier, VerifyingKey};

        let sig = Ed25519Sig::from_slice(&self.bytes)
            .map_err(|_| crate::CryptoError::InvalidSignature)?;

        let pk = VerifyingKey::from_bytes(
            public_key.as_bytes().try_into()
                .map_err(|_| crate::CryptoError::InvalidPublicKey)?
        ).map_err(|_| crate::CryptoError::InvalidPublicKey)?;

        Ok(pk.verify(message, &sig).is_ok())
    }

    fn verify_secp256k1(
        &self,
        message: &[u8],
        public_key: &crate::PublicKey,
    ) -> Result<bool, crate::CryptoError> {
        use secp256k1::{ecdsa::Signature as Secp256k1Sig, Message, PublicKey as Secp256k1Pk, Secp256k1};

        let secp = Secp256k1::verification_only();
        
        let sig = Secp256k1Sig::from_compact(&self.bytes)
            .map_err(|_| crate::CryptoError::InvalidSignature)?;

        let pk = Secp256k1Pk::from_slice(public_key.as_bytes())
            .map_err(|_| crate::CryptoError::InvalidPublicKey)?;

        // Hash the message for SECP256k1
        let msg_hash = crate::hash::Hashable::hash(message);
        let msg = Message::from_digest_slice(msg_hash.as_bytes())
            .map_err(|_| crate::CryptoError::InvalidSignature)?;

        Ok(secp.verify_ecdsa(&msg, &sig, &pk).is_ok())
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Signature({:?}, {}...)",
            self.scheme,
            hex::encode(&self.bytes[..8.min(self.bytes.len())])
        )
    }
}

impl PartialEq for Signature {
    fn eq(&self, other: &Self) -> bool {
        self.scheme == other.scheme && self.bytes == other.bytes
    }
}

impl Eq for Signature {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KeyPair;

    #[test]
    fn test_signature_ed25519() {
        let keypair = KeyPair::generate(SignatureScheme::Ed25519).unwrap();
        let message = b"Test message";
        
        let signature = keypair.sign(message).unwrap();
        assert!(signature.verify(message, keypair.public_key()).unwrap());
        
        let wrong_message = b"Wrong message";
        assert!(!signature.verify(wrong_message, keypair.public_key()).unwrap());
    }

    #[test]
    fn test_signature_secp256k1() {
        let keypair = KeyPair::generate(SignatureScheme::Secp256k1).unwrap();
        let message = b"Test message";
        
        let signature = keypair.sign(message).unwrap();
        assert!(signature.verify(message, keypair.public_key()).unwrap());
    }

    #[test]
    fn test_signature_hex() {
        let keypair = KeyPair::generate(SignatureScheme::Ed25519).unwrap();
        let message = b"Test";
        let sig = keypair.sign(message).unwrap();
        
        let hex = sig.to_hex();
        let parsed = Signature::from_hex(SignatureScheme::Ed25519, &hex).unwrap();
        
        assert_eq!(sig, parsed);
    }
}