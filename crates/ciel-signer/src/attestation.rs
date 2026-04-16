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

    /// Sanity check: hardcoded byte assertion for the [0xAA; 32]-sample CielAttestation.
    /// This test does NOT use the committed binary fixture file — that's what the
    /// `_v1` tests below do. This test exists as a quick local check that the
    /// Borsh field order has not been reordered by accident.
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
    }

    /// Same hardcoded sanity check for PolicyAttestation. The byte-distinct
    /// `_v1` test uses the committed fixture file as the source of truth.
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

    // -----------------------------------------------------------------------
    // v1 wire-format fixtures: byte-distinct values + committed binary fixture
    // -----------------------------------------------------------------------
    //
    // These tests are the contract that pins the Borsh wire format for
    // cross-version compatibility. Both ciel-signer (off-chain, borsh 1.x)
    // and ciel-assert (on-chain, Anchor/borsh) MUST produce and consume
    // identical bytes for these fixtures. See CLAUDE.md Invariant #6.
    //
    // The tests are structured as a triangle:
    //
    //   attestation_fixtures.json  ─┐
    //                               ├─ rust struct ── borsh ── byte_vec
    //   *.bin (committed bytes)  ──┘                              │
    //                                                              ▼
    //                                                    assert byte-equal
    //
    // If any of the three points (JSON values, Rust struct field order,
    // committed bytes) drifts, the test fails. The on-chain Unit 20 program
    // adds a fourth point: it deserializes the same .bin file and asserts
    // structural equality.

    /// Decode lowercase hex string to bytes. Test-only helper.
    fn hex_to_bytes(hex: &str) -> Vec<u8> {
        assert!(hex.len().is_multiple_of(2), "hex string must have even length");
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("valid hex digit"))
            .collect()
    }

    /// Decode a hex string into a fixed-size byte array.
    fn hex_to_array<const N: usize>(hex: &str) -> [u8; N] {
        let v = hex_to_bytes(hex);
        assert_eq!(v.len(), N, "hex string decodes to {} bytes, expected {N}", v.len());
        let mut out = [0u8; N];
        out.copy_from_slice(&v);
        out
    }

    /// Path to the attestation fixtures JSON manifest.
    fn manifest_path() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join("attestation_fixtures.json")
    }

    /// Path to a fixture binary file by name.
    fn fixture_bin_path(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name)
    }

    /// Load and parse the attestation fixtures JSON manifest.
    fn load_manifest() -> serde_json::Value {
        let s = std::fs::read_to_string(manifest_path())
            .expect("attestation_fixtures.json must exist — see CLAUDE.md Invariant #6");
        serde_json::from_str(&s).expect("attestation_fixtures.json must be valid JSON")
    }

    /// Construct CielAttestation from the JSON manifest's v1 entry.
    fn ciel_attestation_v1_from_manifest(manifest: &serde_json::Value) -> CielAttestation {
        let f = &manifest["ciel_attestation_v1"]["fields"];
        CielAttestation {
            magic: hex_to_array::<4>(f["magic"].as_str().unwrap()),
            version: f["version"].as_u64().unwrap() as u8,
            tx_hash: hex_to_array::<32>(f["tx_hash"].as_str().unwrap()),
            verdict: f["verdict"].as_u64().unwrap() as u8,
            safety_score: f["safety_score"].as_u64().unwrap() as u16,
            optimality_score: f["optimality_score"].as_u64().unwrap() as u16,
            checker_outputs_hash: hex_to_array::<32>(f["checker_outputs_hash"].as_str().unwrap()),
            slot: f["slot"].as_u64().unwrap(),
            expiry_slot: f["expiry_slot"].as_u64().unwrap(),
            signer: hex_to_array::<32>(f["signer"].as_str().unwrap()),
            timestamp: f["timestamp"].as_i64().unwrap(),
            timeout_at_ms: f["timeout_at_ms"].as_u64().unwrap() as u16,
        }
    }

    /// Construct PolicyAttestation from the JSON manifest's v1 entry.
    fn policy_attestation_v1_from_manifest(manifest: &serde_json::Value) -> PolicyAttestation {
        let f = &manifest["policy_attestation_v1"]["fields"];
        PolicyAttestation {
            magic: hex_to_array::<4>(f["magic"].as_str().unwrap()),
            version: f["version"].as_u64().unwrap() as u8,
            policy_hash: hex_to_array::<32>(f["policy_hash"].as_str().unwrap()),
            signer: hex_to_array::<32>(f["signer"].as_str().unwrap()),
            issued_slot: f["issued_slot"].as_u64().unwrap(),
            expires_at: f["expires_at"].as_i64().unwrap(),
            revoked: f["revoked"].as_bool().unwrap(),
        }
    }

    /// Construct OverrideAttestation from the JSON manifest's v1 entry.
    fn override_attestation_v1_from_manifest(manifest: &serde_json::Value) -> OverrideAttestation {
        let f = &manifest["override_attestation_v1"]["fields"];
        OverrideAttestation {
            magic: hex_to_array::<4>(f["magic"].as_str().unwrap()),
            version: f["version"].as_u64().unwrap() as u8,
            original_attestation_hash: hex_to_array::<32>(
                f["original_attestation_hash"].as_str().unwrap(),
            ),
            override_type: f["override_type"].as_u64().unwrap() as u8,
            overrider: hex_to_array::<32>(f["overrider"].as_str().unwrap()),
            slot: f["slot"].as_u64().unwrap(),
            timestamp: f["timestamp"].as_i64().unwrap(),
        }
    }

    /// Wire-format pin: serialized JSON-manifest attestation == committed fixture bytes.
    /// Failing this means either the Borsh field layout, the JSON manifest, or the
    /// committed binary has drifted. On-chain CielAssert deserialization breaks the
    /// moment any of those three goes out of sync. See CLAUDE.md Invariant #6.
    #[test]
    fn test_ciel_attestation_wire_fixture_v1() {
        let manifest = load_manifest();
        let att = ciel_attestation_v1_from_manifest(&manifest);
        let serialized = borsh::to_vec(&att).expect("serialization must succeed");
        assert_eq!(
            serialized.len(),
            132,
            "CielAttestation must be exactly 132 bytes"
        );

        let fixture_bytes = std::fs::read(fixture_bin_path("ciel_attestation_v1.bin"))
            .expect("fixtures/ciel_attestation_v1.bin must exist");
        assert_eq!(
            serialized.as_slice(),
            fixture_bytes.as_slice(),
            "CielAttestation v1 wire format drift — JSON manifest, Rust struct, \
             and committed fixture must agree byte-for-byte. See CLAUDE.md Invariant #6."
        );
    }

    /// Same wire-format pin for PolicyAttestation v1.
    #[test]
    fn test_policy_attestation_wire_fixture_v1() {
        let manifest = load_manifest();
        let att = policy_attestation_v1_from_manifest(&manifest);
        let serialized = borsh::to_vec(&att).expect("serialization must succeed");
        assert_eq!(
            serialized.len(),
            86,
            "PolicyAttestation must be exactly 86 bytes"
        );

        let fixture_bytes = std::fs::read(fixture_bin_path("policy_attestation_v1.bin"))
            .expect("fixtures/policy_attestation_v1.bin must exist");
        assert_eq!(
            serialized.as_slice(),
            fixture_bytes.as_slice(),
            "PolicyAttestation v1 wire format drift — JSON manifest, Rust struct, \
             and committed fixture must agree byte-for-byte. See CLAUDE.md Invariant #6."
        );
    }

    /// Same wire-format pin for OverrideAttestation v1.
    #[test]
    fn test_override_attestation_wire_fixture_v1() {
        let manifest = load_manifest();
        let att = override_attestation_v1_from_manifest(&manifest);
        let serialized = borsh::to_vec(&att).expect("serialization must succeed");
        assert_eq!(
            serialized.len(),
            86,
            "OverrideAttestation must be exactly 86 bytes"
        );

        let fixture_bytes = std::fs::read(fixture_bin_path("override_attestation_v1.bin"))
            .expect("fixtures/override_attestation_v1.bin must exist");
        assert_eq!(
            serialized.as_slice(),
            fixture_bytes.as_slice(),
            "OverrideAttestation v1 wire format drift — JSON manifest, Rust struct, \
             and committed fixture must agree byte-for-byte. See CLAUDE.md Invariant #6."
        );
    }

    /// Round-trip: load fixture bytes → Borsh-deserialize → re-serialize → bytes must equal.
    /// Catches asymmetric serialize/deserialize bugs (e.g., reading an extra padding byte
    /// that doesn't get written back).
    #[test]
    fn test_ciel_attestation_wire_fixture_v1_roundtrip() {
        let original = std::fs::read(fixture_bin_path("ciel_attestation_v1.bin"))
            .expect("fixtures/ciel_attestation_v1.bin must exist");
        let decoded: CielAttestation =
            borsh::from_slice(&original).expect("must deserialize from committed fixture");
        let reserialized = borsh::to_vec(&decoded).expect("must re-serialize");
        assert_eq!(
            original.as_slice(),
            reserialized.as_slice(),
            "round-trip drift on CielAttestation"
        );

        // Verify the decoded values match the manifest as a sanity check.
        let manifest = load_manifest();
        let expected = ciel_attestation_v1_from_manifest(&manifest);
        assert_eq!(decoded.magic, expected.magic);
        assert_eq!(decoded.version, expected.version);
        assert_eq!(decoded.tx_hash, expected.tx_hash);
        assert_eq!(decoded.verdict, expected.verdict);
        assert_eq!(decoded.safety_score, expected.safety_score);
        assert_eq!(decoded.optimality_score, expected.optimality_score);
        assert_eq!(decoded.checker_outputs_hash, expected.checker_outputs_hash);
        assert_eq!(decoded.slot, expected.slot);
        assert_eq!(decoded.expiry_slot, expected.expiry_slot);
        assert_eq!(decoded.signer, expected.signer);
        assert_eq!(decoded.timestamp, expected.timestamp);
        assert_eq!(decoded.timeout_at_ms, expected.timeout_at_ms);
    }

    #[test]
    fn test_policy_attestation_wire_fixture_v1_roundtrip() {
        let original = std::fs::read(fixture_bin_path("policy_attestation_v1.bin"))
            .expect("fixtures/policy_attestation_v1.bin must exist");
        let decoded: PolicyAttestation =
            borsh::from_slice(&original).expect("must deserialize from committed fixture");
        let reserialized = borsh::to_vec(&decoded).expect("must re-serialize");
        assert_eq!(
            original.as_slice(),
            reserialized.as_slice(),
            "round-trip drift on PolicyAttestation"
        );

        let manifest = load_manifest();
        let expected = policy_attestation_v1_from_manifest(&manifest);
        assert_eq!(decoded.magic, expected.magic);
        assert_eq!(decoded.version, expected.version);
        assert_eq!(decoded.policy_hash, expected.policy_hash);
        assert_eq!(decoded.signer, expected.signer);
        assert_eq!(decoded.issued_slot, expected.issued_slot);
        assert_eq!(decoded.expires_at, expected.expires_at);
        assert_eq!(decoded.revoked, expected.revoked);
    }

    #[test]
    fn test_override_attestation_wire_fixture_v1_roundtrip() {
        let original = std::fs::read(fixture_bin_path("override_attestation_v1.bin"))
            .expect("fixtures/override_attestation_v1.bin must exist");
        let decoded: OverrideAttestation =
            borsh::from_slice(&original).expect("must deserialize from committed fixture");
        let reserialized = borsh::to_vec(&decoded).expect("must re-serialize");
        assert_eq!(
            original.as_slice(),
            reserialized.as_slice(),
            "round-trip drift on OverrideAttestation"
        );

        let manifest = load_manifest();
        let expected = override_attestation_v1_from_manifest(&manifest);
        assert_eq!(decoded.magic, expected.magic);
        assert_eq!(decoded.version, expected.version);
        assert_eq!(
            decoded.original_attestation_hash,
            expected.original_attestation_hash
        );
        assert_eq!(decoded.override_type, expected.override_type);
        assert_eq!(decoded.overrider, expected.overrider);
        assert_eq!(decoded.slot, expected.slot);
        assert_eq!(decoded.timestamp, expected.timestamp);
    }

    /// One-shot bootstrap helper. Run with:
    ///   cargo test --package ciel-signer write_attestation_v1_fixtures -- --ignored --nocapture
    /// Writes the three v1 binary fixtures to disk based on the JSON manifest values.
    /// Intended to run only when the JSON manifest changes; the committed .bin files
    /// are the source of truth that on-chain Unit 20 deserializes against.
    #[test]
    #[ignore]
    fn write_attestation_v1_fixtures() {
        let manifest = load_manifest();

        let ciel = ciel_attestation_v1_from_manifest(&manifest);
        let ciel_bytes = borsh::to_vec(&ciel).expect("serialize CielAttestation");
        std::fs::write(fixture_bin_path("ciel_attestation_v1.bin"), &ciel_bytes)
            .expect("write ciel_attestation_v1.bin");
        eprintln!(
            "wrote {} bytes to ciel_attestation_v1.bin",
            ciel_bytes.len()
        );

        let policy = policy_attestation_v1_from_manifest(&manifest);
        let policy_bytes = borsh::to_vec(&policy).expect("serialize PolicyAttestation");
        std::fs::write(fixture_bin_path("policy_attestation_v1.bin"), &policy_bytes)
            .expect("write policy_attestation_v1.bin");
        eprintln!(
            "wrote {} bytes to policy_attestation_v1.bin",
            policy_bytes.len()
        );

        let ovr = override_attestation_v1_from_manifest(&manifest);
        let ovr_bytes = borsh::to_vec(&ovr).expect("serialize OverrideAttestation");
        std::fs::write(fixture_bin_path("override_attestation_v1.bin"), &ovr_bytes)
            .expect("write override_attestation_v1.bin");
        eprintln!(
            "wrote {} bytes to override_attestation_v1.bin",
            ovr_bytes.len()
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
