// Ciel PEA / 1.0 — JSON envelope conversion helpers.
//
// Implements the wire-compatible JSON envelope specified at
// https://spec.ciel.xyz/ciel-pea-1.0. The envelope wraps the existing
// 132-byte Borsh CielAttestation with a base64 wire field, hex-encoded
// signer and signature, and optional flags/intent/issuer metadata.
//
// Conformance: see spec/ciel-pea-1.0-test-vectors.md. A roundtrip test
// against the pinned ciel_attestation_v1.bin fixture lives at the bottom
// of this module.

use crate::attestation::{CielAttestation, TIMEOUT_SENTINEL, Verdict};
use crate::SignerResult;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Fixture version pinned by the v1.x series. See spec §10 Compatibility Policy.
pub const PEA_FIXTURE_VERSION: &str = "ciel_attestation_v1";

/// Spec version string. See spec §3.1.
pub const PEA_SPEC_VERSION: &str = "ciel-pea/1.0";

/// A flag in a PEA envelope. See spec §7.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PeaFlag {
    pub code: String,
    pub severity: PeaFlagSeverity,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub detail: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "UPPERCASE")]
pub enum PeaFlagSeverity {
    Info,
    Warn,
    Block,
}

/// Optional intent-binding block. See spec §8.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct PeaIntent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent_nl: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent_satisfied: Option<bool>,
}

/// Convert a signed CielAttestation into a PEA JSON envelope (serde_json::Value).
///
/// `signature` MUST be the 64-byte Ed25519 signature of the Borsh-serialized
/// attestation bytes (132 bytes). Obtain via [`CielSigner::sign`].
pub fn to_pea_json(
    att: &CielAttestation,
    signature: &[u8; 64],
    flags: &[PeaFlag],
    intent: Option<&PeaIntent>,
) -> SignerResult<Value> {
    let wire_bytes = borsh::to_vec(att).map_err(|e| crate::SignerError::Serialization(e.to_string()))?;
    debug_assert_eq!(wire_bytes.len(), 132, "CielAttestation wire size changed — spec breaking");

    let attestation = json!({
        "magic": std::str::from_utf8(&att.magic).unwrap_or("????"),
        "version": att.version,
        "tx_hash": hex::encode(att.tx_hash),
        "verdict": att.verdict,
        "safety_score": att.safety_score,
        "optimality_score": att.optimality_score,
        "checker_outputs_hash": hex::encode(att.checker_outputs_hash),
        "slot": att.slot,
        "expiry_slot": att.expiry_slot,
        "signer": hex::encode(att.signer),
        "timestamp": att.timestamp,
        "timeout_at_ms": att.timeout_at_ms,
    });

    let mut envelope = json!({
        "spec_version": PEA_SPEC_VERSION,
        "attestation": attestation,
        "signature": hex::encode(signature),
        "wire": {
            "borsh_b64": B64.encode(&wire_bytes),
            "fixture_version": PEA_FIXTURE_VERSION,
        },
    });

    if !flags.is_empty() {
        envelope["flags"] = serde_json::to_value(flags)
            .map_err(|e| crate::SignerError::Serialization(e.to_string()))?;
    }
    if let Some(intent) = intent {
        envelope["intent"] = serde_json::to_value(intent)
            .map_err(|e| crate::SignerError::Serialization(e.to_string()))?;
    }
    Ok(envelope)
}

/// Parse a PEA JSON envelope into a `(CielAttestation, signature)` pair.
///
/// Verifies:
/// - `spec_version == "ciel-pea/1.0"`
/// - `wire.fixture_version == "ciel_attestation_v1"`
/// - `wire.borsh_b64` decodes to exactly 132 bytes
/// - Those 132 bytes match a Borsh re-serialization of the attestation object
///
/// Does NOT verify the signature — that's the caller's responsibility via
/// [`crate::verify_attestation`].
pub fn from_pea_json(envelope: &Value) -> SignerResult<(CielAttestation, [u8; 64])> {
    let spec_version = envelope
        .get("spec_version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| crate::SignerError::Serialization("missing spec_version".into()))?;
    if spec_version != PEA_SPEC_VERSION {
        return Err(crate::SignerError::Serialization(format!(
            "unsupported spec_version: {spec_version}"
        )));
    }

    let wire = envelope
        .get("wire")
        .ok_or_else(|| crate::SignerError::Serialization("missing wire".into()))?;
    let fixture_version = wire
        .get("fixture_version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| crate::SignerError::Serialization("missing wire.fixture_version".into()))?;
    if fixture_version != PEA_FIXTURE_VERSION {
        return Err(crate::SignerError::Serialization(format!(
            "unsupported fixture_version: {fixture_version}"
        )));
    }

    let b64 = wire
        .get("borsh_b64")
        .and_then(|v| v.as_str())
        .ok_or_else(|| crate::SignerError::Serialization("missing wire.borsh_b64".into()))?;
    let wire_bytes = B64
        .decode(b64)
        .map_err(|e| crate::SignerError::Serialization(format!("base64: {e}")))?;
    if wire_bytes.len() != 132 {
        return Err(crate::SignerError::Serialization(format!(
            "wire length {} != 132",
            wire_bytes.len()
        )));
    }

    let att = CielAttestation::try_from_slice(&wire_bytes)
        .map_err(|e| crate::SignerError::Serialization(e.to_string()))?;

    // Re-serialize and byte-compare so that any drift between wire.borsh_b64
    // and the JSON attestation object is caught.
    let round = borsh::to_vec(&att).map_err(|e| crate::SignerError::Serialization(e.to_string()))?;
    if round != wire_bytes {
        return Err(crate::SignerError::Serialization(
            "wire bytes did not roundtrip — tamper or encoder drift".into(),
        ));
    }

    let signature_hex = envelope
        .get("signature")
        .and_then(|v| v.as_str())
        .ok_or_else(|| crate::SignerError::Serialization("missing signature".into()))?;
    let signature_bytes = hex::decode(signature_hex)
        .map_err(|e| crate::SignerError::Serialization(format!("signature hex: {e}")))?;
    let signature: [u8; 64] = signature_bytes
        .as_slice()
        .try_into()
        .map_err(|_| crate::SignerError::Serialization("signature not 64 bytes".into()))?;

    Ok((att, signature))
}

/// Compute TIMEOUT sentinel score value. Kept as a helper so downstream
/// code doesn't hardcode 0xFFFF.
pub fn timeout_score_sentinel() -> u16 {
    TIMEOUT_SENTINEL
}

/// Human-readable verdict string for a u8 verdict field.
pub fn verdict_label(verdict: u8) -> &'static str {
    match verdict {
        0 => "APPROVE",
        1 => "WARN",
        2 => "BLOCK",
        3 => "TIMEOUT",
        _ => "UNKNOWN",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attestation::{ATTESTATION_VERSION, CIEL_ATTESTATION_MAGIC};
    use std::fs;
    use std::path::PathBuf;

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name)
    }

    fn load_canonical_attestation() -> CielAttestation {
        // Values from fixtures/attestation_fixtures.json — MUST match the bytes
        // pinned in ciel_attestation_v1.bin.
        CielAttestation {
            magic: CIEL_ATTESTATION_MAGIC,
            version: ATTESTATION_VERSION,
            tx_hash: {
                let mut h = [0u8; 32];
                for (i, b) in h.iter_mut().enumerate() {
                    *b = (i + 1) as u8;
                }
                h
            },
            verdict: Verdict::Block as u8,
            safety_score: 3500,
            optimality_score: 0,
            checker_outputs_hash: {
                let mut h = [0u8; 32];
                for (i, b) in h.iter_mut().enumerate() {
                    *b = 0x21 + i as u8;
                }
                h
            },
            slot: 350_000_000,
            expiry_slot: 350_000_002,
            signer: {
                let mut h = [0u8; 32];
                for (i, b) in h.iter_mut().enumerate() {
                    *b = 0x41 + i as u8;
                }
                h
            },
            timestamp: 1_712_000_000,
            timeout_at_ms: 0,
        }
    }

    #[test]
    fn test_to_pea_json_structure() {
        let att = load_canonical_attestation();
        let sig = [0xAAu8; 64];
        let envelope = to_pea_json(&att, &sig, &[], None).unwrap();

        assert_eq!(envelope["spec_version"], PEA_SPEC_VERSION);
        assert_eq!(envelope["wire"]["fixture_version"], PEA_FIXTURE_VERSION);
        assert_eq!(envelope["attestation"]["magic"], "CIEL");
        assert_eq!(envelope["attestation"]["verdict"], 2);
        assert_eq!(envelope["attestation"]["safety_score"], 3500);
        assert_eq!(envelope["signature"], hex::encode(sig));
    }

    #[test]
    fn test_pea_roundtrip_against_pinned_fixture() {
        let att = load_canonical_attestation();
        let sig = [0xBBu8; 64];
        let envelope = to_pea_json(&att, &sig, &[], None).unwrap();

        // Reload from envelope.
        let (decoded_att, decoded_sig) = from_pea_json(&envelope).unwrap();
        let original_wire = borsh::to_vec(&att).unwrap();
        let decoded_wire = borsh::to_vec(&decoded_att).unwrap();
        assert_eq!(original_wire, decoded_wire, "wire bytes must roundtrip exactly");
        assert_eq!(decoded_sig, sig);

        // Verify the envelope's wire.borsh_b64 matches the pinned fixture.
        let fixture_bytes = fs::read(fixture_path("ciel_attestation_v1.bin"))
            .expect("ciel_attestation_v1.bin present");
        let decoded_b64 = B64
            .decode(envelope["wire"]["borsh_b64"].as_str().unwrap())
            .unwrap();
        assert_eq!(
            decoded_b64, fixture_bytes,
            "PEA envelope wire bytes must equal the pinned fixture ciel_attestation_v1.bin"
        );
    }

    #[test]
    fn test_from_pea_json_rejects_wrong_spec_version() {
        let att = load_canonical_attestation();
        let mut envelope = to_pea_json(&att, &[0u8; 64], &[], None).unwrap();
        envelope["spec_version"] = json!("ciel-pea/2.0");
        let err = from_pea_json(&envelope).unwrap_err();
        assert!(matches!(err, crate::SignerError::Serialization(_)));
    }

    #[test]
    fn test_from_pea_json_rejects_wrong_fixture_version() {
        let att = load_canonical_attestation();
        let mut envelope = to_pea_json(&att, &[0u8; 64], &[], None).unwrap();
        envelope["wire"]["fixture_version"] = json!("ciel_attestation_v2");
        let err = from_pea_json(&envelope).unwrap_err();
        assert!(matches!(err, crate::SignerError::Serialization(_)));
    }

    #[test]
    fn test_flags_and_intent_round_trip() {
        let att = load_canonical_attestation();
        let sig = [0xCCu8; 64];
        let flags = vec![PeaFlag {
            code: "ORACLE_DEVIATION".into(),
            severity: PeaFlagSeverity::Block,
            detail: Some("spread 42%".into()),
        }];
        let intent = PeaIntent {
            intent_nl: Some("deposit CVT and borrow SOL".into()),
            intent_fingerprint: Some("ab".repeat(32)),
            intent_satisfied: Some(false),
        };
        let envelope = to_pea_json(&att, &sig, &flags, Some(&intent)).unwrap();
        assert_eq!(envelope["flags"][0]["code"], "ORACLE_DEVIATION");
        assert_eq!(envelope["flags"][0]["severity"], "BLOCK");
        assert_eq!(envelope["intent"]["intent_satisfied"], false);
    }
}
