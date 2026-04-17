// Ciel Attestation and Signing
// See spec Section 7 for full design.

pub mod attestation;
pub mod instruction;
pub mod pea;
pub mod signer;

pub use attestation::{
    CielAttestation, OverrideAttestation, PolicyAttestation, Verdict, ATTESTATION_VERSION,
    CIEL_ATTESTATION_MAGIC, EXPIRY_SLOTS, OVERRIDE_ATTESTATION_MAGIC, POLICY_ATTESTATION_MAGIC,
    TIMEOUT_SENTINEL,
};
pub use instruction::build_ed25519_verify_instruction;
pub use pea::{
    from_pea_json, timeout_score_sentinel, to_pea_json, verdict_label, PeaFlag, PeaFlagSeverity,
    PeaIntent, PEA_FIXTURE_VERSION, PEA_SPEC_VERSION,
};
pub use signer::{verify_attestation, CielSigner};

/// Error type for signer operations.
/// See spec Section 7.3 for Ed25519 signing semantics.
#[derive(Debug, thiserror::Error)]
pub enum SignerError {
    #[error("Borsh serialization failed: {0}")]
    Serialization(String),

    #[error("invalid signing key: {0}")]
    InvalidKey(String),

    #[error("signature verification failed")]
    VerificationFailed,
}

pub type SignerResult<T> = Result<T, SignerError>;
