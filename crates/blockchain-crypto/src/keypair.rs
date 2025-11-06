// blockchain-crypto/src/keypair.rs

use crate::{CryptoError, CryptoResult, Signature, SignatureScheme};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Public key wrapper
#[derive(Clone, Serialize, Deserialize)]
pub struct PublicKey {
    scheme: SignatureScheme,
    bytes: Vec<u8>,
}

impl PublicKey {
    pub fn new(scheme: SignatureScheme, bytes: Vec<u8>) -> Self {
        Self { scheme, bytes }
    }

    pub fn scheme(&self) -> SignatureScheme {
        self.scheme
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }

    pub fn to_hex(&self) -> String {
        hex::encode(&self.bytes)
    }

    pub fn from_hex(scheme: SignatureScheme, s: &str) -> CryptoResult<Self> {
        let bytes = hex::decode(s)
            .map_err(|e| CryptoError::DeserializationError(e.to_string()))?;
        Ok(Self::new(scheme, bytes))
    }

    /// Verify a signature
    pub fn verify(&self, message: &[u8], signature: &Signature) -> CryptoResult<bool> {
        signature.verify(message, self)
    }

    /// Derive an address from this public key
    pub fn to_address(&self) -> Address {
        Address::from_public_key(self)
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PublicKey({:?}, {}...)",
            self.scheme,
            hex::encode(&self.bytes[..8.min(self.bytes.len())])
        )
    }
}

impl PartialEq for PublicKey {
    fn eq(&self, other: &Self) -> bool {
        self.scheme == other.scheme && self.bytes == other.bytes
    }
}

impl Eq for PublicKey {}

/// Secret/Private key wrapper (kept private)
pub struct SecretKey {
    scheme: SignatureScheme,
    bytes: Vec<u8>,
}

impl SecretKey {
    pub fn new(scheme: SignatureScheme, bytes: Vec<u8>) -> Self {
        Self { scheme, bytes }
    }

    pub fn scheme(&self) -> SignatureScheme {
        self.scheme
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn to_hex(&self) -> String {
        hex::encode(&self.bytes)
    }

    pub fn from_hex(scheme: SignatureScheme, s: &str) -> CryptoResult<Self> {
        let bytes = hex::decode(s)
            .map_err(|e| CryptoError::DeserializationError(e.to_string()))?;
        Ok(Self::new(scheme, bytes))
    }
}

impl Drop for SecretKey {
    fn drop(&mut self) {
        // Zero out the key material on drop for security
        self.bytes.iter_mut().for_each(|b| *b = 0);
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretKey({:?}, [REDACTED])", self.scheme)
    }
}

/// Key pair containing both public and secret keys
pub struct KeyPair {
    scheme: SignatureScheme,
    public_key: PublicKey,
    secret_key: SecretKey,
}

impl KeyPair {
    /// Generate a new random keypair
    pub fn generate(scheme: SignatureScheme) -> CryptoResult<Self> {
        match scheme {
            SignatureScheme::Ed25519 => Self::generate_ed25519(),
            SignatureScheme::Secp256k1 => Self::generate_secp256k1(),
        }
    }

    fn generate_ed25519() -> CryptoResult<Self> {
        use ed25519_dalek::{SigningKey, VerifyingKey};
        use rand::rngs::OsRng;

        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key: VerifyingKey = (&signing_key).into();

        Ok(Self {
            scheme: SignatureScheme::Ed25519,
            public_key: PublicKey::new(
                SignatureScheme::Ed25519,
                verifying_key.to_bytes().to_vec(),
            ),
            secret_key: SecretKey::new(
                SignatureScheme::Ed25519,
                signing_key.to_bytes().to_vec(),
            ),
        })
    }

    fn generate_secp256k1() -> CryptoResult<Self> {
        use rand::rngs::OsRng;
        use secp256k1::{PublicKey as Secp256k1Pk, SecretKey as Secp256k1Sk, Secp256k1};

        let secp = Secp256k1::new();
        let mut rng = OsRng;
        
        let secret_key = Secp256k1Sk::new(&mut rng);
        let public_key = Secp256k1Pk::from_secret_key(&secp, &secret_key);

        Ok(Self {
            scheme: SignatureScheme::Secp256k1,
            public_key: PublicKey::new(
                SignatureScheme::Secp256k1,
                public_key.serialize().to_vec(),
            ),
            secret_key: SecretKey::new(
                SignatureScheme::Secp256k1,
                secret_key.secret_bytes().to_vec(),
            ),
        })
    }

    /// Create keypair from existing keys
    pub fn from_keys(public_key: PublicKey, secret_key: SecretKey) -> CryptoResult<Self> {
        if public_key.scheme() != secret_key.scheme() {
            return Err(CryptoError::InvalidSecretKey);
        }
        
        Ok(Self {
            scheme: public_key.scheme(),
            public_key,
            secret_key,
        })
    }

    pub fn scheme(&self) -> SignatureScheme {
        self.scheme
    }

    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    pub fn secret_key(&self) -> &SecretKey {
        &self.secret_key
    }

    /// Sign a message
    pub fn sign(&self, message: &[u8]) -> CryptoResult<Signature> {
        match self.scheme {
            SignatureScheme::Ed25519 => self.sign_ed25519(message),
            SignatureScheme::Secp256k1 => self.sign_secp256k1(message),
        }
    }

    fn sign_ed25519(&self, message: &[u8]) -> CryptoResult<Signature> {
        use ed25519_dalek::{Signature as Ed25519Sig, Signer, SigningKey};

        let signing_key = SigningKey::from_bytes(
            self.secret_key.as_bytes().try_into()
                .map_err(|_| CryptoError::InvalidSecretKey)?
        );

        let signature: Ed25519Sig = signing_key.sign(message);
        Ok(Signature::new(
            SignatureScheme::Ed25519,
            signature.to_bytes().to_vec(),
        ))
    }

    fn sign_secp256k1(&self, message: &[u8]) -> CryptoResult<Signature> {
        use secp256k1::{ecdsa::Signature as Secp256k1Sig, Message, SecretKey as Secp256k1Sk, Secp256k1};

        let secp = Secp256k1::signing_only();
        
        let secret_key = Secp256k1Sk::from_slice(self.secret_key.as_bytes())
            .map_err(|_| CryptoError::InvalidSecretKey)?;

        // Hash the message
        let msg_hash = crate::hash::Hashable::hash(message);
        let msg = Message::from_digest_slice(msg_hash.as_bytes())
            .map_err(|_| CryptoError::InvalidSignature)?;

        let signature: Secp256k1Sig = secp.sign_ecdsa(&msg, &secret_key);
        Ok(Signature::new(
            SignatureScheme::Secp256k1,
            signature.serialize_compact().to_vec(),
        ))
    }
}

impl fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyPair")
            .field("scheme", &self.scheme)
            .field("public_key", &self.public_key)
            .field("secret_key", &"[REDACTED]")
            .finish()
    }
}

/// Blockchain address derived from public key
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Ord, PartialOrd)]
pub struct Address([u8; 20]);

impl Address {
    /// Create address from bytes
    pub fn new(bytes: [u8; 20]) -> Self {
        Self(bytes)
    }

    /// Derive address from public key (similar to Ethereum)
    pub fn from_public_key(public_key: &PublicKey) -> Self {
        use crate::hash::Hashable;
        
        let hash = public_key.as_bytes().hash();
        let mut address = [0u8; 20];
        address.copy_from_slice(&hash.as_bytes()[12..32]);
        Self(address)
    }

    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        format!("0x{}", hex::encode(self.0))
    }

    pub fn from_hex(s: &str) -> CryptoResult<Self> {
        let s = s.strip_prefix("0x").unwrap_or(s);
        let bytes = hex::decode(s)
            .map_err(|e| CryptoError::DeserializationError(e.to_string()))?;
        if bytes.len() != 20 {
            return Err(CryptoError::DeserializationError("Invalid address length".into()));
        }
        let mut arr = [0u8; 20];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }

    pub fn zero() -> Self {
        Self([0u8; 20])
    }
}

impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Address({})", self.to_hex())
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_generation() {
        let kp1 = KeyPair::generate(SignatureScheme::Ed25519).unwrap();
        let kp2 = KeyPair::generate(SignatureScheme::Ed25519).unwrap();
        assert_ne!(kp1.public_key(), kp2.public_key());
    }

    #[test]
    fn test_sign_verify() {
        let keypair = KeyPair::generate(SignatureScheme::Ed25519).unwrap();
        let message = b"Hello, blockchain!";
        
        let signature = keypair.sign(message).unwrap();
        assert!(keypair.public_key().verify(message, &signature).unwrap());
    }

    #[test]
    fn test_address_derivation() {
        let keypair = KeyPair::generate(SignatureScheme::Ed25519).unwrap();
        let address1 = keypair.public_key().to_address();
        let address2 = Address::from_public_key(keypair.public_key());
        assert_eq!(address1, address2);
    }

    #[test]
    fn test_address_hex() {
        let address = Address::zero();
        let hex = address.to_hex();
        let parsed = Address::from_hex(&hex).unwrap();
        assert_eq!(address, parsed);
    }
}