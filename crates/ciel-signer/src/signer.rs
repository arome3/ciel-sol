// Ciel Ed25519 Signing and Verification
// See spec Section 7.3 for signing semantics.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

use crate::attestation::{CielAttestation, OverrideAttestation, PolicyAttestation};
use crate::{SignerError, SignerResult};

// ---------------------------------------------------------------------------
// CielSigner
// ---------------------------------------------------------------------------

/// Wraps an Ed25519 signing key for producing attestation signatures.
/// See spec Section 7.3.
pub struct CielSigner {
    signing_key: SigningKey,
}

impl CielSigner {
    /// Wrap an existing ed25519-dalek `SigningKey`.
    pub fn new(signing_key: SigningKey) -> Self {
        Self { signing_key }
    }

    /// Create from a 32-byte secret key seed.
    /// Any 32 bytes form a valid Ed25519 secret key in ed25519-dalek 2.x.
    pub fn from_bytes(secret: &[u8; 32]) -> Self {
        Self {
            signing_key: SigningKey::from_bytes(secret),
        }
    }

    /// Create from a 64-byte Solana keypair (first 32 bytes = secret key,
    /// last 32 bytes = public key). Validates that the public key half
    /// matches the secret key.
    pub fn from_keypair_bytes(bytes: &[u8; 64]) -> SignerResult<Self> {
        let signing_key =
            SigningKey::from_keypair_bytes(bytes).map_err(|e| SignerError::InvalidKey(e.to_string()))?;
        Ok(Self { signing_key })
    }

    /// Return the 32-byte Ed25519 public key.
    pub fn pubkey_bytes(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes()
    }

    /// Return a reference to the inner `SigningKey`.
    pub fn signing_key(&self) -> &SigningKey {
        &self.signing_key
    }

    /// Borsh-serialize and sign a `CielAttestation`.
    /// Returns the serialized bytes and the Ed25519 signature.
    pub fn sign_attestation(
        &self,
        attestation: &CielAttestation,
    ) -> SignerResult<(Vec<u8>, Signature)> {
        let bytes =
            borsh::to_vec(attestation).map_err(|e| SignerError::Serialization(e.to_string()))?;
        let signature = self.signing_key.sign(&bytes);
        Ok((bytes, signature))
    }

    /// Borsh-serialize and sign a `PolicyAttestation`.
    pub fn sign_policy_attestation(
        &self,
        attestation: &PolicyAttestation,
    ) -> SignerResult<(Vec<u8>, Signature)> {
        let bytes =
            borsh::to_vec(attestation).map_err(|e| SignerError::Serialization(e.to_string()))?;
        let signature = self.signing_key.sign(&bytes);
        Ok((bytes, signature))
    }

    /// Borsh-serialize and sign an `OverrideAttestation`.
    pub fn sign_override_attestation(
        &self,
        attestation: &OverrideAttestation,
    ) -> SignerResult<(Vec<u8>, Signature)> {
        let bytes =
            borsh::to_vec(attestation).map_err(|e| SignerError::Serialization(e.to_string()))?;
        let signature = self.signing_key.sign(&bytes);
        Ok((bytes, signature))
    }
}

// ---------------------------------------------------------------------------
// Standalone verification
// ---------------------------------------------------------------------------

/// Verify an Ed25519 signature over a message using `verify_strict`.
///
/// Uses `verify_strict` (not `verify`) to match on-chain Ed25519SigVerify
/// precompile behavior on mainnet, which rejects malleable signatures.
/// See spec Section 7.3.
pub fn verify_attestation(pubkey: &[u8; 32], message_bytes: &[u8], signature: &[u8; 64]) -> bool {
    let Ok(verifying_key) = VerifyingKey::from_bytes(pubkey) else {
        return false;
    };
    let sig = Signature::from_bytes(signature);
    verifying_key.verify_strict(message_bytes, &sig).is_ok()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attestation::{CielAttestation, Verdict};

    fn test_signing_key() -> SigningKey {
        // Deterministic key for tests
        SigningKey::from_bytes(&[42u8; 32])
    }

    fn test_attestation(signer_pubkey: [u8; 32]) -> CielAttestation {
        CielAttestation::new(
            [0xAA; 32],
            Verdict::Block,
            2000,
            1000,
            [0xBB; 32],
            100,
            signer_pubkey,
            1_700_000_000,
            0,
        )
    }

    #[test]
    fn test_sign_verify_roundtrip() {
        let signer = CielSigner::new(test_signing_key());
        let attestation = test_attestation(signer.pubkey_bytes());

        let (bytes, signature) = signer.sign_attestation(&attestation).unwrap();
        assert_eq!(bytes.len(), 132);

        let pubkey = signer.pubkey_bytes();
        assert!(verify_attestation(
            &pubkey,
            &bytes,
            &signature.to_bytes()
        ));
    }

    #[test]
    fn test_tampered_message_fails_verification() {
        let signer = CielSigner::new(test_signing_key());
        let attestation = test_attestation(signer.pubkey_bytes());

        let (mut bytes, signature) = signer.sign_attestation(&attestation).unwrap();

        // Tamper with one byte
        bytes[10] ^= 0xFF;

        let pubkey = signer.pubkey_bytes();
        assert!(
            !verify_attestation(&pubkey, &bytes, &signature.to_bytes()),
            "verification should fail after tampering"
        );
    }

    #[test]
    fn test_wrong_key_fails_verification() {
        let signer_a = CielSigner::new(test_signing_key());
        let signer_b = CielSigner::from_bytes(&[99u8; 32]);

        let attestation = test_attestation(signer_a.pubkey_bytes());
        let (bytes, signature) = signer_a.sign_attestation(&attestation).unwrap();

        // Verify with wrong key
        let wrong_pubkey = signer_b.pubkey_bytes();
        assert!(
            !verify_attestation(&wrong_pubkey, &bytes, &signature.to_bytes()),
            "verification should fail with wrong public key"
        );
    }

    #[test]
    fn test_pubkey_bytes_matches_verifying_key() {
        let key = test_signing_key();
        let signer = CielSigner::new(key.clone());
        assert_eq!(signer.pubkey_bytes(), key.verifying_key().to_bytes());
    }

    #[test]
    fn test_from_keypair_bytes() {
        let key = test_signing_key();
        let verifying = key.verifying_key();

        let mut keypair_bytes = [0u8; 64];
        keypair_bytes[..32].copy_from_slice(key.as_bytes());
        keypair_bytes[32..].copy_from_slice(verifying.as_bytes());

        let signer = CielSigner::from_keypair_bytes(&keypair_bytes).unwrap();
        assert_eq!(signer.pubkey_bytes(), verifying.to_bytes());
    }

    #[test]
    fn test_from_keypair_bytes_invalid() {
        // Public key half doesn't match secret key
        let mut bad_keypair = [0u8; 64];
        bad_keypair[..32].copy_from_slice(&[42u8; 32]); // valid secret
        bad_keypair[32..].copy_from_slice(&[0u8; 32]); // wrong public key

        assert!(CielSigner::from_keypair_bytes(&bad_keypair).is_err());
    }

    #[test]
    fn test_sign_policy_attestation() {
        let signer = CielSigner::new(test_signing_key());
        let att = crate::attestation::PolicyAttestation::new(
            [0xDD; 32],
            signer.pubkey_bytes(),
            200,
            1_700_100_000,
        );

        let (bytes, signature) = signer.sign_policy_attestation(&att).unwrap();
        assert_eq!(bytes.len(), 86);
        assert!(verify_attestation(
            &signer.pubkey_bytes(),
            &bytes,
            &signature.to_bytes()
        ));
    }

    #[test]
    fn test_sign_override_attestation() {
        let signer = CielSigner::new(test_signing_key());
        let att = crate::attestation::OverrideAttestation::new(
            [0xFF; 32],
            0,
            signer.pubkey_bytes(),
            300,
            1_700_200_000,
        );

        let (bytes, signature) = signer.sign_override_attestation(&att).unwrap();
        assert_eq!(bytes.len(), 86);
        assert!(verify_attestation(
            &signer.pubkey_bytes(),
            &bytes,
            &signature.to_bytes()
        ));
    }
}
