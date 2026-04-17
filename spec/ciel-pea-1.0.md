# Ciel PEA / 1.0 — Pre-Execution Attestation Specification

**Status:** Published
**Version:** 1.0
**Date:** 2026-04-17
**License:** Apache License 2.0
**Canonical URL:** `https://spec.ciel.xyz/ciel-pea-1.0`

---

## 1. Abstract

This document specifies **Pre-Execution Attestation (PEA) / 1.0**, a cryptographically signed, deterministic, and schema-stable payload format that encodes a safety verdict about a specific transaction *before* that transaction is submitted to a blockchain network. The format is designed for consumption by wallets, autonomous agents, policy engines, and on-chain verifier programs without requiring trust in the issuer's runtime.

A PEA asserts, at a specific slot, that the issuer evaluated the bound transaction against a named checker suite and arrived at one of four discrete verdicts — `APPROVE`, `WARN`, `BLOCK`, or `TIMEOUT` — with a deterministic hash committing to the checker outputs that produced the verdict. The issuer signs a canonical Borsh wire-format payload with an Ed25519 private key. Consumers verify the signature against the issuer's public key, then check verdict freshness against the current slot.

This specification defines:

1. The JSON-level envelope consumed by off-chain integrators.
2. The pinned Borsh wire format used for signing and on-chain verification.
3. The verdict semantics, including expiry and freshness rules.
4. The versioning and compatibility policy (additive-only within `1.x`).

PEA is a category. "Ciel PEA" is the reference implementation.

## 2. Terminology

- **Attestation** — a signed statement about a fact.
- **Issuer** — the entity producing and signing the PEA. Identified by its Ed25519 public key.
- **Verifier** — any party reading and validating a PEA.
- **Verdict** — one of `APPROVE`, `WARN`, `BLOCK`, `TIMEOUT`. See §5.
- **Checker outputs hash** — a 32-byte Blake3-or-SHA256 digest committing to the full set of checker results that produced the verdict.
- **Wire bytes** — the canonical Borsh-encoded byte sequence that is signed and verified.
- **Slot** — the monotonic slot number of the target blockchain at which the attestation was issued.
- **Conformance window** — the slot range `[issued_slot, expires_at_slot]` during which a PEA is considered fresh.

The terms **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** are used per [RFC 2119](https://datatracker.ietf.org/doc/html/rfc2119).

## 3. Envelope format (JSON)

A PEA / 1.0 payload is a JSON object with the following top-level structure. Field ordering is not semantically significant at the JSON level; canonical ordering is enforced at the wire level (§4).

```jsonc
{
  "spec_version": "ciel-pea/1.0",
  "attestation": {
    "magic":                "string (4 ASCII chars, MUST be 'CIEL')",
    "version":              "integer (MUST be 1 for this spec)",
    "tx_hash":              "hex string (32 bytes, lowercase)",
    "verdict":              "integer (0|1|2|3 — APPROVE|WARN|BLOCK|TIMEOUT)",
    "safety_score":         "integer (0..10000 normally; 65535 under TIMEOUT)",
    "optimality_score":     "integer (0..10000 normally; 65535 under TIMEOUT)",
    "checker_outputs_hash": "hex string (32 bytes, lowercase)",
    "slot":                 "integer (u64, issue slot)",
    "expiry_slot":          "integer (u64, slot at which verdict expires)",
    "signer":               "hex string (32 bytes, Ed25519 public key)",
    "timestamp":            "integer (i64, unix seconds)",
    "timeout_at_ms":        "integer (u16, 0 unless verdict=TIMEOUT)"
  },
  "signature":  "hex string (64 bytes, Ed25519 signature over the wire bytes)",
  "wire": {
    "borsh_b64":      "string (base64 of the canonical wire bytes)",
    "fixture_version":"string (MUST be 'ciel_attestation_v1' for this spec)"
  },
  "flags":    "array (optional, zero or more semantic flag objects — see §7)",
  "intent":   "object (optional — see §8)",
  "issuer":   "object (optional — see §9)"
}
```

### 3.1 Required fields

- `spec_version` — MUST equal `"ciel-pea/1.0"` exactly.
- `attestation` — MUST contain all fields listed under §3.
- `signature` — MUST be a 64-byte Ed25519 signature, hex-encoded, computed over `wire.borsh_b64` decoded to bytes (see §4).
- `wire.borsh_b64` — MUST be the base64 encoding of the canonical 132-byte Borsh serialization of the `attestation` object (see §4).
- `wire.fixture_version` — MUST equal `"ciel_attestation_v1"` for this spec.

### 3.2 Optional fields

- `flags` — A list of flag objects, each with `code` (string), `severity` (`INFO` | `WARN` | `BLOCK`), and optional `detail` (string). See §7.
- `intent` — If the attestation binds to a declared user intent, this object carries `intent_nl` (string), `intent_fingerprint` (hex string), and `intent_satisfied` (bool or null). See §8.
- `issuer` — Optional issuer metadata: `agent_id`, `operator_pubkey_hash`, `identity_proof`. See §9.

Additional top-level fields beyond those listed SHOULD be ignored by verifiers. This allows additive evolution without breaking existing implementations (see §10).

## 4. Wire format (Borsh)

The wire format is a 132-byte [Borsh](https://borsh.io) serialization of the `attestation` object in the exact field order defined in §3. This ordering MUST NOT change within `1.x` (see §10 Compatibility Policy).

Field encoding (in serialization order):

| Offset | Field | Type | Bytes |
|---:|---|---|---:|
| 0 | `magic` | 4 × u8 (ASCII `CIEL`) | 4 |
| 4 | `version` | u8 | 1 |
| 5 | `tx_hash` | 32 × u8 | 32 |
| 37 | `verdict` | u8 | 1 |
| 38 | `safety_score` | u16 LE | 2 |
| 40 | `optimality_score` | u16 LE | 2 |
| 42 | `checker_outputs_hash` | 32 × u8 | 32 |
| 74 | `slot` | u64 LE | 8 |
| 82 | `expiry_slot` | u64 LE | 8 |
| 90 | `signer` | 32 × u8 | 32 |
| 122 | `timestamp` | i64 LE | 8 |
| 130 | `timeout_at_ms` | u16 LE | 2 |
| — | **Total** | | **132** |

Signatures are computed over these 132 bytes using Ed25519 (`RFC 8032`). Verifiers MUST use strict verification (`verify_strict` in ed25519-dalek, or equivalent implementations that reject non-canonical R and small-order points).

The authoritative test fixture for these bytes is `crates/ciel-signer/fixtures/ciel_attestation_v1.bin` (132 bytes) in the Ciel reference implementation. See [ciel-pea-1.0-test-vectors.md](./ciel-pea-1.0-test-vectors.md).

## 5. Verdict semantics

`verdict` is one of:

- `0` — **APPROVE.** The issuer asserts the transaction passed all applicable checks.
- `1` — **WARN.** The transaction is allowed to execute but carries a notable caveat (e.g., oracle spread elevated, MEV exposure). Flags MUST be populated for non-empty WARN verdicts.
- `2` — **BLOCK.** The issuer asserts the transaction should not be executed. Flags SHOULD identify at least one BLOCK-severity reason.
- `3` — **TIMEOUT.** The issuer's infrastructure failed to produce a verdict within its deadline. TIMEOUT is **not equivalent to WARN** and **MUST NOT be overridden** by downstream consumers. See §5.2.

### 5.1 Freshness (expiry)

A PEA is fresh if the current slot of the target network is **less than or equal to** `expiry_slot`. Verifiers MUST reject stale PEAs.

Typical issuer configurations use `expiry_slot = slot + 2` (approximately 800ms on Solana). Other networks MAY adopt different defaults; consumers MUST NOT assume a specific window.

### 5.2 TIMEOUT handling

TIMEOUT verdicts:

- MUST NOT be converted to APPROVE by any consumer.
- MUST be surfaced to the calling context as a distinct outcome from WARN.
- SHOULD prompt the consumer to retry the verdict request with a fresh submission.
- MUST NOT be overrideable through the `OverrideAttestation` flow defined by the reference implementation; overrides apply only to BLOCK.

## 6. Signature verification

To verify a PEA:

1. Parse the envelope per §3.
2. Base64-decode `wire.borsh_b64` to obtain the 132 wire bytes.
3. Verify that the Borsh-decoded `attestation` fields match the JSON `attestation` fields exactly. A mismatch MUST result in rejection.
4. Run Ed25519 strict verification of `signature` over the 132 wire bytes using `attestation.signer` as the public key. A failure MUST result in rejection.
5. Check `attestation.expiry_slot >= current_slot`. A stale attestation MUST be rejected.
6. If the consumer maintains an allowlist of trusted issuers, check `attestation.signer` against it. Signature validity alone does not imply trust — the signer must be recognized.

A full worked example is provided in [ciel-pea-1.0-test-vectors.md](./ciel-pea-1.0-test-vectors.md).

## 7. Flags

Flags are structured hints about *why* a given verdict was issued. They are advisory metadata for human operators and downstream logic; the authoritative decision is the `verdict` field.

Each flag has:

- `code` (string, REQUIRED) — a stable identifier. The reference implementation uses codes such as `ORACLE_DEVIATION`, `AUTHORITY_HIJACK`, `INTENT_BALANCE_MISMATCH`, `MEV_SANDWICH_RISK`, etc. External implementations MAY define additional codes; unknown codes SHOULD be passed through.
- `severity` (string, REQUIRED) — one of `INFO`, `WARN`, `BLOCK`.
- `detail` (string, OPTIONAL) — human-readable context.

Flags are NOT covered by the wire signature. They are off-chain metadata. Consumers requiring cryptographic commitment to a specific flag set MUST rely on `checker_outputs_hash` (§3), which commits to the underlying deterministic checker outputs.

## 8. Intent binding (optional)

If the attestation concerns a transaction produced in response to a declared intent (for example, an agent given the natural-language instruction "swap 10 USDC for SOL"), the `intent` object captures this binding:

- `intent_nl` — the natural-language intent, verbatim.
- `intent_fingerprint` — 32-byte hex hash of the intent's declared outcomes (token deltas, recipients, etc.), independent of specific amounts.
- `intent_satisfied` — `true` if the transaction's observed outcome matches the declared intent within the issuer's tolerance; `false` if a mismatch was detected; `null` if the intent could not be classified.

Issuers MAY include `intent` for verdicts involving an agent or declared-intent flow.

## 9. Issuer / identity binding (optional)

If the verdict is associated with an identified operator or agent, the `issuer` object carries:

- `agent_id` — stable identifier for the agent or wallet.
- `operator_pubkey_hash` — hex hash of the human operator's public key, if applicable.
- `identity_proof` — object `{ kind: 'world_id' | 'ens' | 'none' | 'revoked_world_id', value: string }`.

These fields enable anti-Sybil and reputation systems to bind verdicts to real-world or pseudonymous operators without exposing identity material.

## 10. Compatibility Policy

This spec uses an **additive-only versioning rule within the major version**:

1. Fields MAY be added to the optional sections (`flags`, `intent`, `issuer`, and the top-level envelope).
2. Required fields in §3.1 MUST NOT be removed or renamed in any `1.x` release.
3. Wire format ordering (§4) MUST NOT change in any `1.x` release. `wire.fixture_version` is pinned to `"ciel_attestation_v1"` for the entire `1.x` series.
4. Verdict integer values MUST NOT be remapped (§5).
5. Any change that would break §10.1 through §10.4 requires a major version bump to `2.0`.

Conformance rule for verifiers: an implementation conforms to `ciel-pea/1.0` if, and only if, it accepts every valid PEA/1.x payload that includes the minimum required fields, even if the payload contains additional unknown top-level fields or flag codes. Verifiers MUST NOT reject a PEA solely for containing unrecognized optional fields.

Conformance rule for issuers: an implementation conforms to `ciel-pea/1.0` if every PEA it emits validates against [ciel-pea-1.0.schema.json](./ciel-pea-1.0.schema.json) and produces wire bytes that byte-compare equal to the Borsh re-serialization of the JSON `attestation` object.

The reference implementation's `attestation_fixtures.json` and `ciel_attestation_v1.bin` are the ultimate arbiter of correct wire-byte output. See [ciel-pea-1.0-test-vectors.md](./ciel-pea-1.0-test-vectors.md).

## 11. Security considerations

- **Strict signature verification is mandatory.** Verifiers MUST use strict Ed25519 verification; non-strict verification is vulnerable to signature malleability.
- **Expiry must be enforced against the target network's slot, not wall-clock time.** Using wall-clock time introduces drift attacks; the attestation's meaning is slot-scoped.
- **Signer trust is out of band.** A valid signature proves the holder of the private key issued the PEA, not that the issuer is trustworthy. Consumers MUST maintain their own allowlist of trusted issuer public keys.
- **Flags are not signed.** Downstream systems relying cryptographically on specific flags MUST read the `checker_outputs_hash` commitment and re-derive flags from the underlying checker outputs via the issuer's published methodology.
- **TIMEOUT must not be softened.** Interpreting TIMEOUT as equivalent to APPROVE or WARN re-introduces the infrastructure-failure silent-approve class of vulnerabilities this spec is designed to prevent.

## 12. Relationship to other standards

- **Solana Attestation Service (SAS):** SAS attests to static facts about an account (KYC, accreditation). Ciel PEA attests to the safety of a specific transaction. The two are orthogonal and can coexist within a single verification pipeline.
- **Ethereum Attestation Service (EAS):** EAS is a general-purpose attestation registry. A PEA MAY be registered as an EAS attestation for cross-chain portability; this spec does not mandate that.
- **Lighthouse (Solana):** Lighthouse is an on-chain assertion program. A PEA MAY be consumed by a Lighthouse-style assertion to gate execution on a freshly verified attestation.
- **Squads Policy Network (SPN):** SPN nodes MAY use PEAs as the underlying verdict payload when acting as conditional signers on Squads smart accounts. This spec does not require SPN integration; it defines the verdict payload that SPN nodes (or independent verifiers) can produce and consume.

## 13. Appendix — schema

The machine-readable JSON Schema for PEA / 1.0 is [ciel-pea-1.0.schema.json](./ciel-pea-1.0.schema.json). A conforming issuer's output MUST validate against that schema.

## 14. Appendix — reference implementation

The reference implementation lives in the Ciel repository:

- Rust: `crates/ciel-signer/src/pea.rs` — `to_pea()` / `from_pea()` on `CielAttestation`, with a roundtrip test against `crates/ciel-signer/fixtures/ciel_attestation_v1.bin`.
- TypeScript: `sdk/typescript/src/pea.ts` — JSON-level helpers.

Both implementations are Apache 2.0 licensed.
