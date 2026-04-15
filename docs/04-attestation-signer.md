# 04: Attestation Signer

## Overview

This unit defines the `CielAttestation` Borsh schema (132 bytes), the `OverrideAttestation` schema, the `PolicyAttestation` schema, and the Ed25519 signing/verification module. This is the cryptographic core of Ciel — the attestation is the product. Thursday deliverable of Week 1.

> Authoritative reference: see [Section 7](../ciel-technical-spec.md#7-attestation-and-signing) of the technical spec for full detail.

## Technical Specifications

- **CielAttestation struct**: 132-byte Borsh payload with magic, version, tx_hash, verdict, scores, checker_outputs_hash, slot, expiry, signer, timestamp, timeout_at_ms. See [Section 7.1](../ciel-technical-spec.md#71-attestation-payload-schema).
- **PolicyAttestation struct**: 86-byte Borsh payload for pre-certified mode. See [Section 7.5](../ciel-technical-spec.md#75-policyattestation-schema-pre-certified-mode).
- **OverrideAttestation struct**: override type for BLOCK verdict overrides. See [Section 7.7](../ciel-technical-spec.md#77-override-attestation-type).
- **Serialization**: Borsh, deterministic. See [Section 7.2](../ciel-technical-spec.md#72-canonical-serialization-borsh).
- **Signing**: ed25519-dalek 2.x, SigningKey/VerifyingKey API. See [Section 7.3](../ciel-technical-spec.md#73-ed25519-signing-v1).
- **Expiry**: 2-slot window (~800ms). See [Section 7.6](../ciel-technical-spec.md#76-expiry-and-slot-pinning-semantics).

## Key Capabilities

- [ ] Borsh-serialize a CielAttestation to exactly 132 bytes — verified by asserting serialized length
- [ ] Ed25519 sign an attestation and verify the signature — verified by round-trip sign/verify test
- [ ] Construct a Solana Ed25519SigVerify instruction from the signed attestation — verified by checking instruction layout matches the native program's expected format
- [ ] TIMEOUT verdict produces valid attestation with sentinel score values (0xFFFF) — verified by serializing a TIMEOUT attestation
- [ ] PolicyAttestation serializes to exactly 86 bytes — verified by asserting serialized length

## Implementation Guide

1. **Define attestation structs**: CielAttestation, OverrideAttestation, PolicyAttestation with BorshSerialize/BorshDeserialize derives
2. **Implement Signer module**: wraps ed25519-dalek SigningKey, provides sign(attestation) → (bytes, signature)
3. **Implement verification helper**: verify(pubkey, message_bytes, signature) → bool
4. **Implement Ed25519 instruction builder**: constructs the Solana native Ed25519SigVerify instruction data layout for on-chain verification
5. **Write tests**: round-trip sign/verify, serialization size assertions, Ed25519 instruction layout test

**Key gotchas**:
- Borsh field ordering matters — the struct field order in Rust IS the serialization order. Don't reorder fields.
- The Ed25519 instruction data layout has specific offset semantics (Section 7.3 in the spec references the native program's instruction format). The instruction index fields must be set correctly.
- ed25519-dalek 2.x uses `SigningKey` not `Keypair` — check the Solana SDK compatibility

**Files / modules to create**:
- `crates/ciel-signer/Cargo.toml`
- `crates/ciel-signer/src/lib.rs`
- `crates/ciel-signer/src/attestation.rs` — CielAttestation, OverrideAttestation, PolicyAttestation structs
- `crates/ciel-signer/src/signer.rs` — Ed25519 signing/verification
- `crates/ciel-signer/src/instruction.rs` — Ed25519SigVerify instruction builder

## Dependencies

### Upstream (units this depends on)
None.

### Downstream (units that depend on this)
- `06-pipeline-integration` — the pipeline signs attestations after scoring
- `20-ciel-assert-program` — the on-chain program verifies attestations signed by this module
- `26-override-mechanism` — signs OverrideAttestation structs
- `37-pre-certified-mode` — uses PolicyAttestation signing

## Prompt for Claude Code

```
Implement Unit 04: Attestation Signer

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

This prompt covers ONE unit. Do not implement adjacent units. Do not restate or modify the technical spec.

Required reading before you write any code
Read this unit doc first: ./docs/04-attestation-signer.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 7.1 (Attestation Payload Schema): the exact CielAttestation struct with every field and type
- Section 7.2 (Canonical Serialization: Borsh): why Borsh, determinism requirement
- Section 7.3 (Ed25519 Signing v1): the signing code pattern, ed25519-dalek API, on-chain verification flow
- Section 7.5 (PolicyAttestation Schema): the pre-certified mode attestation struct
- Section 7.6 (Expiry and Slot-Pinning Semantics): 2-slot expiry window
- Section 7.7 (Override Attestation Type): the OverrideAttestation struct
- Section 2.1 (Technology Stack): ed25519-dalek version, Borsh version

No upstream unit docs to read — this unit has no dependencies.

Scope: what to build
The attestation type definitions, Ed25519 signing module, and Ed25519SigVerify instruction builder.

In scope:
- Rust crate at crates/ciel-signer/
- CielAttestation struct (132 bytes Borsh) with all fields from Section 7.1
- OverrideAttestation struct from Section 7.7
- PolicyAttestation struct (86 bytes Borsh) from Section 7.5
- Verdict enum: APPROVE=0, WARN=1, BLOCK=2, TIMEOUT=3
- CielSigner struct wrapping ed25519-dalek::SigningKey
- sign_attestation(attestation) -> (Vec<u8>, Signature) function
- verify_attestation(pubkey, bytes, signature) -> bool function
- build_ed25519_verify_instruction(pubkey, message, signature) -> Instruction function for the Solana Ed25519SigVerify native program
- Comprehensive tests for all of the above

Out of scope (these belong to other units):
- Scorer logic that produces safety_score — owned by ./docs/15-scorer.md
- On-chain CielAssert program — owned by ./docs/20-ciel-assert-program.md
- FROST threshold signing — v2 only, not implemented in v1

Implementation constraints
- Language: Rust
- Libraries: borsh 1.x, ed25519-dalek 2.x (SigningKey API), solana-sdk (for Instruction, Pubkey)
- File location: crates/ciel-signer/
- CielAttestation must derive BorshSerialize, BorshDeserialize, Serialize, Deserialize, Clone
- Magic bytes for CielAttestation: [0x43, 0x49, 0x45, 0x4C] ("CIEL")
- Magic bytes for PolicyAttestation: [0x43, 0x49, 0x4C, 0x50] ("CILP")
- TIMEOUT sentinel values: safety_score=0xFFFF, optimality_score=0xFFFF

Verification steps
Before declaring this unit complete, run and report results for every step:
1. Run `cargo test --package ciel-signer` and confirm all tests pass
2. Serialize a CielAttestation and assert the byte length is exactly 132
3. Serialize a PolicyAttestation and assert the byte length is exactly 86
4. Sign a test attestation, then verify the signature — confirm round-trip succeeds
5. Sign a test attestation, tamper with one byte, re-verify — confirm verification fails
6. Build an Ed25519SigVerify instruction and verify its data layout matches the expected format (num_signatures=1, correct offsets)
7. Create a TIMEOUT attestation with sentinel values and confirm it serializes/deserializes correctly

What to report when finished
- List of files created or modified with path
- Test results (pass/fail counts)
- Exact serialized byte sizes for each attestation type
- Any ed25519-dalek API differences from what the spec assumes
- Estimated next unit to build: 05-checker-framework

What NOT to do
- Do not implement the scorer or any checker logic
- Do not implement the on-chain verification program
- Do not implement FROST threshold signing (v2)
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
- Do not declare the unit complete if any test fails
```
