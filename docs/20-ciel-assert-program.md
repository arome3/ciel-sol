# 20: CielAssert On-Chain Program

## Overview

This unit implements the on-chain Solana program that verifies Ciel attestations within transactions. It reads the `sysvar::instructions` account to find the Ed25519SigVerify precompile instruction, extracts the attestation payload, verifies the signer matches the expected Ciel public key, checks the verdict, and validates slot freshness. This program is consumed by all three enforcement paths (Lighthouse, Squads wrapper, Jito bundles).

> Authoritative reference: see [Section 8.1](../ciel-technical-spec.md#81-lighthouse-guard-instructions) for the on-chain verification pseudocode and [Section 7.1](../ciel-technical-spec.md#71-attestation-payload-schema) for the attestation struct the program deserializes.

## Technical Specifications

- **Verification flow**: Read instruction sysvar → find Ed25519SigVerify ix → parse attestation → verify signer → check verdict → check slot freshness. See [Section 8.1](../ciel-technical-spec.md#81-lighthouse-guard-instructions) pseudocode.
- **Ed25519 precompile**: program ID `Ed25519SigVerify111111111111111111111111111111`, 0 CU cost. See [Section 7.3](../ciel-technical-spec.md#73-ed25519-signing-v1).
- **Attestation schema**: 132-byte Borsh CielAttestation. See [Section 7.1](../ciel-technical-spec.md#71-attestation-payload-schema).
- **Slot freshness**: `current_slot <= attestation.expiry_slot`. See [Section 7.6](../ciel-technical-spec.md#76-expiry-and-slot-pinning-semantics).

## Key Capabilities

- [ ] Verify a valid Ciel attestation on-chain — verified by devnet deployment + test transaction
- [ ] Reject an expired attestation (slot too old) — verified by test with stale slot
- [ ] Reject an attestation with wrong signer — verified by test with different pubkey
- [ ] Reject when verdict is BLOCK (only APPROVE and WARN pass) — verified by test
- [ ] Program deploys to devnet and fits within compute budget — verified by deployment

## Implementation Guide

1. **Create the Anchor or native Solana program** at `programs/ciel-assert/`
2. **Implement the `assert_attestation` instruction**: reads `sysvar::instructions`, finds Ed25519 ix at a specified index, parses the message bytes as CielAttestation
3. **Deploy to devnet** and test with a crafted transaction
4. **Write client-side helpers** for building transactions that include the CielAssert instruction

**Key gotchas**:
- The Ed25519 precompile CANNOT be called via CPI — you must use the instruction sysvar introspection pattern
- The instruction index of the Ed25519 ix must be passed as an argument to the CielAssert instruction
- Borsh deserialization on-chain must match the off-chain struct exactly — same field order, same types

**Files / modules to create**:
- `programs/ciel-assert/Cargo.toml`
- `programs/ciel-assert/src/lib.rs` — the on-chain program
- `crates/ciel-assert-sdk/` — client-side helpers for building CielAssert instructions

## Dependencies

### Upstream (units this depends on)
- `04-attestation-signer` — defines the CielAttestation struct the program deserializes

### Downstream (units that depend on this)
- `21-lighthouse-integration` — Lighthouse transactions include the CielAssert instruction
- `23-jito-integration` — Jito bundles include CielAssert in the verification tx

## Prompt for Claude Code

```
Implement Unit 20: CielAssert On-Chain Program

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

Required reading before you write any code
Read this unit doc first: ./docs/20-ciel-assert-program.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 8.1 (Lighthouse Guard Instructions): the CielAssert pseudocode showing instruction sysvar introspection, signer verification, verdict check, and slot freshness
- Section 7.1 (Attestation Payload Schema): the exact CielAttestation struct the program must Borsh-deserialize
- Section 7.3 (Ed25519 Signing v1): Ed25519SigVerify program ID, instruction data layout, 0 CU cost
- Section 7.6 (Expiry and Slot-Pinning): slot freshness check logic
- Section 17.2 (Integration Tests Against Devnet): the four integration test cases

Also read: ./docs/04-attestation-signer.md — the CielAttestation struct definition you must match exactly

Scope: what to build
In scope:
- Solana program at programs/ciel-assert/ (Anchor or native — Anchor recommended for faster dev)
- assert_attestation instruction handler
- Instruction sysvar introspection to find Ed25519SigVerify ix
- CielAttestation Borsh deserialization on-chain
- Signer verification (expected_signer passed as account or instruction data)
- Verdict check (only APPROVE=0 and WARN=1 pass)
- Slot freshness check (current_slot <= expiry_slot)
- Client SDK crate at crates/ciel-assert-sdk/ for building CielAssert instructions
- Devnet deployment and 4 integration tests from Section 17.2

Out of scope: Lighthouse integration, Squads integration, Jito bundle assembly — those units use this program

Implementation constraints
- Language: Rust (Anchor framework recommended)
- Libraries: anchor-lang (or solana-program for native), borsh
- File location: programs/ciel-assert/ for the program, crates/ciel-assert-sdk/ for the client
- The CielAttestation struct definition MUST exactly match the one in crates/ciel-signer/
- Consider sharing the struct via a common crate

Verification steps
1. `anchor build` (or `cargo build-sbf`) succeeds
2. Deploy to devnet: `anchor deploy --provider.cluster devnet`
3. Test: valid attestation → transaction succeeds
4. Test: expired attestation → transaction reverts with CielError::AttestationExpired
5. Test: wrong signer → transaction reverts with CielError::InvalidSigner
6. Test: BLOCK verdict → transaction reverts with CielError::Blocked

What to report when finished
- Program ID on devnet
- Files created, test results (4 integration tests)
- CU consumption per CielAssert instruction
- Estimated next unit: 21-lighthouse-integration

What NOT to do
- Do not implement Lighthouse, Squads, or Jito integration (those are separate units)
- Do not modify the attestation struct (it must match the signer)
- Do not modify ./ciel-technical-spec.md
```
