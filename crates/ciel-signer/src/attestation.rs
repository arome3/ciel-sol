// Ciel Attestation Type Definitions
// See spec Section 7.1 (CielAttestation), 7.5 (PolicyAttestation), 7.7 (OverrideAttestation).

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Magic bytes for CielAttestation: "CIEL" in ASCII.
pub const CIEL_ATTESTATION_MAGIC: [u8; 4] = [0x43, 0x49, 0x45, 0x4C];

/// Magic bytes for PolicyAttestation: "CILP" in ASCII.
pub const POLICY_ATTESTATION_MAGIC: [u8; 4] = [0x43, 0x49, 0x4C, 0x50];

/// Magic bytes for OverrideAttestation: "CLOV" in ASCII.
pub const OVERRIDE_ATTESTATION_MAGIC: [u8; 4] = [0x43, 0x4C, 0x4F, 0x56];

/// Current attestation schema version.
pub const ATTESTATION_VERSION: u8 = 1;

/// Sentinel value for safety_score and optimality_score under TIMEOUT verdict.
pub const TIMEOUT_SENTINEL: u16 = 0xFFFF;

/// Number of slots added to `slot` to compute `expiry_slot`. See spec Section 7.6.
pub const EXPIRY_SLOTS: u64 = 2;

// ---------------------------------------------------------------------------
// Verdict
// ---------------------------------------------------------------------------

/// Verdict outcome of a Ciel evaluation. See spec Section 7.1.
///
/// TIMEOUT is a first-class verdict indicating infrastructure failure,
/// not a risk judgment. It is distinct from WARN. See spec Section 9.3.
#[derive(
    BorshSerialize, BorshDeserialize, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq,
)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum Verdict {
    Approve = 0,
    Warn = 1,
    Block = 2,
    Timeout = 3,
}

impl From<Verdict> for u8 {
    fn from(v: Verdict) -> Self {
        v as u8
    }
}

// ---------------------------------------------------------------------------
// CielAttestation — 132 bytes Borsh
// ---------------------------------------------------------------------------

/// Primary attestation payload signed by the Ciel signer.
/// See spec Section 7.1 for field semantics.
///
/// WARNING: Field order is the Borsh serialization order. Do not reorder.
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Clone, Debug)]
pub struct CielAttestation {
    /// Magic bytes: [0x43, 0x49, 0x45, 0x4C] ("CIEL").
    pub magic: [u8; 4],
    /// Schema version (currently 1).
    pub version: u8,
    /// SHA-256 of the evaluated transaction.
    pub tx_hash: [u8; 32],
    /// Verdict as u8: 0=APPROVE, 1=WARN, 2=BLOCK, 3=TIMEOUT.
    pub verdict: u8,
    /// Fixed-point safety score (score * 10000). 0xFFFF if TIMEOUT.
    pub safety_score: u16,
    /// Fixed-point optimality score. 0 for raw tx mode. 0xFFFF if TIMEOUT.
    pub optimality_score: u16,
    /// SHA-256 of concatenated checker outputs.
    pub checker_outputs_hash: [u8; 32],
    /// Confirmed mainnet slot at fork time.
    pub slot: u64,
    /// slot + 2. Enforcement contracts verify current_slot <= expiry_slot.
    pub expiry_slot: u64,
    /// Ed25519 public key of the Ciel signer.
    pub signer: [u8; 32],
    /// Unix timestamp of attestation creation.
    pub timestamp: i64,
    /// Milliseconds elapsed before timeout (0 unless verdict=TIMEOUT).
    pub timeout_at_ms: u16,
}

impl CielAttestation {
    /// Create a new attestation. Sets magic, version, and expiry_slot automatically.
    /// See spec Section 7.6 for expiry semantics.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tx_hash: [u8; 32],
        verdict: Verdict,
        safety_score: u16,
        optimality_score: u16,
        checker_outputs_hash: [u8; 32],
        slot: u64,
        signer: [u8; 32],
        timestamp: i64,
        timeout_at_ms: u16,
    ) -> Self {
        Self {
            magic: CIEL_ATTESTATION_MAGIC,
            version: ATTESTATION_VERSION,
            tx_hash,
            verdict: verdict as u8,
            safety_score,
            optimality_score,
            checker_outputs_hash,
            slot,
            expiry_slot: slot + EXPIRY_SLOTS,
            signer,
            timestamp,
            timeout_at_ms,
        }
    }

    /// Create a TIMEOUT attestation with sentinel values. See spec Section 9.3.
    pub fn new_timeout(
        tx_hash: [u8; 32],
        checker_outputs_hash: [u8; 32],
        slot: u64,
        signer: [u8; 32],
        timestamp: i64,
        timeout_at_ms: u16,
    ) -> Self {
        Self::new(
            tx_hash,
            Verdict::Timeout,
            TIMEOUT_SENTINEL,
            TIMEOUT_SENTINEL,
            checker_outputs_hash,
            slot,
            signer,
            timestamp,
            timeout_at_ms,
        )
    }
}

// ---------------------------------------------------------------------------
// PolicyAttestation — 86 bytes Borsh
// ---------------------------------------------------------------------------

/// Pre-certified mode attestation. See spec Section 7.5.
///
/// WARNING: Field order is the Borsh serialization order. Do not reorder.
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Clone, Debug)]
pub struct PolicyAttestation {
    /// Magic bytes: [0x43, 0x49, 0x4C, 0x50] ("CILP").
    pub magic: [u8; 4],
    /// Schema version (currently 1).
    pub version: u8,
    /// SHA-256 of the policy template JSON.
    pub policy_hash: [u8; 32],
    /// Ed25519 public key of the Ciel signer.
    pub signer: [u8; 32],
    /// Slot at which the policy was evaluated.
    pub issued_slot: u64,
    /// Unix timestamp expiry.
    pub expires_at: i64,
    /// Revocation flag.
    pub revoked: bool,
}

impl PolicyAttestation {
    pub fn new(
        policy_hash: [u8; 32],
        signer: [u8; 32],
        issued_slot: u64,
        expires_at: i64,
    ) -> Self {
        Self {
            magic: POLICY_ATTESTATION_MAGIC,
            version: ATTESTATION_VERSION,
            policy_hash,
            signer,
            issued_slot,
            expires_at,
            revoked: false,
        }
    }
}

// ---------------------------------------------------------------------------
// OverrideAttestation — 86 bytes Borsh
// ---------------------------------------------------------------------------

/// Override attestation for overriding a BLOCK verdict. See spec Section 7.7.
///
/// WARNING: Field order is the Borsh serialization order. Do not reorder.
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Clone, Debug)]
pub struct OverrideAttestation {
    /// Magic bytes: [0x43, 0x4C, 0x4F, 0x56] ("CLOV").
    pub magic: [u8; 4],
    /// Schema version (currently 1).
    pub version: u8,
    /// SHA-256 hash of the BLOCK attestation being overridden.
    pub original_attestation_hash: [u8; 32],
    /// Override type: 0 = OVERRIDE_APPROVED.
    pub override_type: u8,
    /// Ed25519 public key of the entity approving the override.
    pub overrider: [u8; 32],
    /// Slot at time of override.
    pub slot: u64,
    /// Unix timestamp of override.
    pub timestamp: i64,
}

impl OverrideAttestation {
    pub fn new(
        original_attestation_hash: [u8; 32],
        override_type: u8,
        overrider: [u8; 32],
        slot: u64,
        timestamp: i64,
    ) -> Self {
        Self {
            magic: OVERRIDE_ATTESTATION_MAGIC,
            version: ATTESTATION_VERSION,
            original_attestation_hash,
            override_type,
            overrider,
            slot,
            timestamp,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_ciel_attestation() -> CielAttestation {
        CielAttestation::new(
            [0xAA; 32],    // tx_hash
            Verdict::Approve,
            9500,          // safety_score
            8000,          // optimality_score
            [0xBB; 32],    // checker_outputs_hash
            100,           // slot
            [0xCC; 32],    // signer
            1_700_000_000, // timestamp
            0,             // timeout_at_ms
        )
    }

    fn sample_policy_attestation() -> PolicyAttestation {
        PolicyAttestation::new(
            [0xDD; 32],    // policy_hash
            [0xEE; 32],    // signer
            200,           // issued_slot
            1_700_100_000, // expires_at
        )
    }

    fn sample_override_attestation() -> OverrideAttestation {
        OverrideAttestation::new(
            [0xFF; 32],    // original_attestation_hash
            0,             // override_type = OVERRIDE_APPROVED
            [0x11; 32],    // overrider
            300,           // slot
            1_700_200_000, // timestamp
        )
    }

    #[test]
    fn test_ciel_attestation_serialization_size() {
        let att = sample_ciel_attestation();
        let bytes = borsh::to_vec(&att).unwrap();
        assert_eq!(bytes.len(), 132, "CielAttestation must be exactly 132 bytes");
    }

    #[test]
    fn test_policy_attestation_serialization_size() {
        let att = sample_policy_attestation();
        let bytes = borsh::to_vec(&att).unwrap();
        assert_eq!(bytes.len(), 86, "PolicyAttestation must be exactly 86 bytes");
    }

    #[test]
    fn test_override_attestation_serialization_size() {
        let att = sample_override_attestation();
        let bytes = borsh::to_vec(&att).unwrap();
        assert_eq!(
            bytes.len(),
            86,
            "OverrideAttestation must be exactly 86 bytes"
        );
    }

    #[test]
    fn test_ciel_attestation_roundtrip() {
        let original = sample_ciel_attestation();
        let bytes = borsh::to_vec(&original).unwrap();
        let decoded: CielAttestation = borsh::from_slice(&bytes).unwrap();

        assert_eq!(decoded.magic, CIEL_ATTESTATION_MAGIC);
        assert_eq!(decoded.version, ATTESTATION_VERSION);
        assert_eq!(decoded.tx_hash, [0xAA; 32]);
        assert_eq!(decoded.verdict, Verdict::Approve as u8);
        assert_eq!(decoded.safety_score, 9500);
        assert_eq!(decoded.optimality_score, 8000);
        assert_eq!(decoded.checker_outputs_hash, [0xBB; 32]);
        assert_eq!(decoded.slot, 100);
        assert_eq!(decoded.expiry_slot, 102); // slot + EXPIRY_SLOTS
        assert_eq!(decoded.signer, [0xCC; 32]);
        assert_eq!(decoded.timestamp, 1_700_000_000);
        assert_eq!(decoded.timeout_at_ms, 0);
    }

    #[test]
    fn test_policy_attestation_roundtrip() {
        let original = sample_policy_attestation();
        let bytes = borsh::to_vec(&original).unwrap();
        let decoded: PolicyAttestation = borsh::from_slice(&bytes).unwrap();

        assert_eq!(decoded.magic, POLICY_ATTESTATION_MAGIC);
        assert_eq!(decoded.version, ATTESTATION_VERSION);
        assert_eq!(decoded.policy_hash, [0xDD; 32]);
        assert_eq!(decoded.signer, [0xEE; 32]);
        assert_eq!(decoded.issued_slot, 200);
        assert_eq!(decoded.expires_at, 1_700_100_000);
        assert!(!decoded.revoked);
    }

    #[test]
    fn test_override_attestation_roundtrip() {
        let original = sample_override_attestation();
        let bytes = borsh::to_vec(&original).unwrap();
        let decoded: OverrideAttestation = borsh::from_slice(&bytes).unwrap();

        assert_eq!(decoded.magic, OVERRIDE_ATTESTATION_MAGIC);
        assert_eq!(decoded.version, ATTESTATION_VERSION);
        assert_eq!(decoded.original_attestation_hash, [0xFF; 32]);
        assert_eq!(decoded.override_type, 0);
        assert_eq!(decoded.overrider, [0x11; 32]);
        assert_eq!(decoded.slot, 300);
        assert_eq!(decoded.timestamp, 1_700_200_000);
    }

    #[test]
    fn test_timeout_attestation_sentinel_values() {
        let att = CielAttestation::new_timeout(
            [0xAA; 32],    // tx_hash
            [0xBB; 32],    // checker_outputs_hash
            100,           // slot
            [0xCC; 32],    // signer
            1_700_000_000, // timestamp
            450,           // timeout_at_ms
        );

        assert_eq!(att.verdict, Verdict::Timeout as u8);
        assert_eq!(att.safety_score, TIMEOUT_SENTINEL);
        assert_eq!(att.optimality_score, TIMEOUT_SENTINEL);
        assert_eq!(att.timeout_at_ms, 450);

        // Round-trip through Borsh
        let bytes = borsh::to_vec(&att).unwrap();
        assert_eq!(bytes.len(), 132);
        let decoded: CielAttestation = borsh::from_slice(&bytes).unwrap();
        assert_eq!(decoded.verdict, 3); // TIMEOUT
        assert_eq!(decoded.safety_score, 0xFFFF);
        assert_eq!(decoded.optimality_score, 0xFFFF);
        assert_eq!(decoded.timeout_at_ms, 450);
    }

    #[test]
    fn test_verdict_values() {
        assert_eq!(Verdict::Approve as u8, 0);
        assert_eq!(Verdict::Warn as u8, 1);
        assert_eq!(Verdict::Block as u8, 2);
        assert_eq!(Verdict::Timeout as u8, 3);
    }

    #[test]
    fn test_magic_bytes_in_serialized_output() {
        // CielAttestation magic
        let ciel = sample_ciel_attestation();
        let ciel_bytes = borsh::to_vec(&ciel).unwrap();
        assert_eq!(&ciel_bytes[0..4], &CIEL_ATTESTATION_MAGIC);

        // PolicyAttestation magic
        let policy = sample_policy_attestation();
        let policy_bytes = borsh::to_vec(&policy).unwrap();
        assert_eq!(&policy_bytes[0..4], &POLICY_ATTESTATION_MAGIC);

        // OverrideAttestation magic
        let ovr = sample_override_attestation();
        let ovr_bytes = borsh::to_vec(&ovr).unwrap();
        assert_eq!(&ovr_bytes[0..4], &OVERRIDE_ATTESTATION_MAGIC);
    }

    /// Canonical fixture for cross-version wire compatibility.
    /// These bytes are the ground truth — if this test fails, the Borsh
    /// wire format has changed and on-chain deserialization will break.
    /// See also: crates/ciel-signer/fixtures/ciel_attestation_v1.bin
    #[test]
    fn test_ciel_attestation_wire_fixture() {
        let att = sample_ciel_attestation();
        let bytes = borsh::to_vec(&att).unwrap();

        // Hardcoded expected bytes — field by field:
        //   magic(4) version(1) tx_hash(32) verdict(1) safety_score(2)
        //   optimality_score(2) checker_outputs_hash(32) slot(8) expiry_slot(8)
        //   signer(32) timestamp(8) timeout_at_ms(2)
        #[rustfmt::skip]
        let expected: [u8; 132] = [
            // magic: "CIEL"
            0x43, 0x49, 0x45, 0x4C,
            // version: 1
            0x01,
            // tx_hash: [0xAA; 32]
            0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
            0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
            0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
            0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
            // verdict: 0 (APPROVE)
            0x00,
            // safety_score: 9500 = 0x251C LE
            0x1C, 0x25,
            // optimality_score: 8000 = 0x1F40 LE
            0x40, 0x1F,
            // checker_outputs_hash: [0xBB; 32]
            0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB,
            0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB,
            0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB,
            0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB,
            // slot: 100 LE u64
            0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            // expiry_slot: 102 LE u64
            0x66, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            // signer: [0xCC; 32]
            0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC,
            0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC,
            0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC,
            0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC,
            // timestamp: 1_700_000_000 = 0x6553F100 LE i64
            0x00, 0xF1, 0x53, 0x65, 0x00, 0x00, 0x00, 0x00,
            // timeout_at_ms: 0 LE u16
            0x00, 0x00,
        ];

        assert_eq!(
            bytes.as_slice(),
            &expected[..],
            "CielAttestation wire format has changed — this breaks on-chain deserialization"
        );

        // Also verify the fixture file if it exists (written once, committed to repo).
        let fixture_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/ciel_attestation_v1.bin"
        );
        if std::path::Path::new(fixture_path).exists() {
            let fixture_bytes = std::fs::read(fixture_path).unwrap();
            assert_eq!(
                bytes.as_slice(),
                fixture_bytes.as_slice(),
                "serialization does not match committed fixture file"
            );
        } else {
            // Generate the fixture file on first run
            std::fs::create_dir_all(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures")).unwrap();
            std::fs::write(fixture_path, &bytes).unwrap();
        }
    }

    /// Same fixture test for PolicyAttestation.
    #[test]
    fn test_policy_attestation_wire_fixture() {
        let att = sample_policy_attestation();
        let bytes = borsh::to_vec(&att).unwrap();

        #[rustfmt::skip]
        let expected: [u8; 86] = [
            // magic: "CILP"
            0x43, 0x49, 0x4C, 0x50,
            // version: 1
            0x01,
            // policy_hash: [0xDD; 32]
            0xDD, 0xDD, 0xDD, 0xDD, 0xDD, 0xDD, 0xDD, 0xDD,
            0xDD, 0xDD, 0xDD, 0xDD, 0xDD, 0xDD, 0xDD, 0xDD,
            0xDD, 0xDD, 0xDD, 0xDD, 0xDD, 0xDD, 0xDD, 0xDD,
            0xDD, 0xDD, 0xDD, 0xDD, 0xDD, 0xDD, 0xDD, 0xDD,
            // signer: [0xEE; 32]
            0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE,
            0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE,
            0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE,
            0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE,
            // issued_slot: 200 = 0xC8 LE u64
            0xC8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            // expires_at: 1_700_100_000 = 0x655577A0 LE i64
            0xA0, 0x77, 0x55, 0x65, 0x00, 0x00, 0x00, 0x00,
            // revoked: false
            0x00,
        ];

        assert_eq!(
            bytes.as_slice(),
            &expected[..],
            "PolicyAttestation wire format has changed — this breaks on-chain deserialization"
        );
    }

    #[test]
    fn test_expiry_slot_computation() {
        let att = CielAttestation::new(
            [0; 32],
            Verdict::Approve,
            10000,
            5000,
            [0; 32],
            500, // slot
            [0; 32],
            0,
            0,
        );
        assert_eq!(att.expiry_slot, 502); // 500 + EXPIRY_SLOTS(2)
    }
}
