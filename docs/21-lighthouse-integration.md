# 21: Lighthouse Integration

## Overview

This unit integrates Lighthouse Protocol guard instructions with the CielAssert program to create the end-user enforcement path. A transaction is sandwiched between the attestation verification (Ed25519SigVerify + CielAssert at the start) and Lighthouse state assertions (at the end), ensuring both pre-execution verdict and post-execution state validation.

> Authoritative reference: see [Section 8.1](../ciel-technical-spec.md#81-lighthouse-guard-instructions) of the technical spec for the full transaction layout and pseudocode.

## Technical Specifications

- **Transaction layout**: Ed25519SigVerify → CielAssert → Payload → Lighthouse assertion. See [Section 8.1](../ciel-technical-spec.md#81-lighthouse-guard-instructions).
- **Lighthouse program**: `L2TExMFKdjpN9kozasaurPirfHy9P8sbXoAN1qA3S95` (v2.0.0). See [Section 2.1](../ciel-technical-spec.md#21-core-technologies).
- **Assertion types**: TokenAccountBalance, AccountBalance, AccountData. See [Section 8.1](../ciel-technical-spec.md#81-lighthouse-guard-instructions).
- **TIMEOUT handling**: configurable — default reject. See [Section 8.1](../ciel-technical-spec.md#81-lighthouse-guard-instructions).

## Key Capabilities

- [ ] Build a transaction with Ed25519Verify + CielAssert + payload + Lighthouse assertion — verified by successful devnet execution
- [ ] Transaction reverts when attestation is invalid — verified by submitting with a bad signature
- [ ] Lighthouse assertion reverts the transaction when post-state diverges — verified by asserting an impossible balance

## Implementation Guide

1. **Create transaction builder**: function that takes attestation + payload instructions + Lighthouse assertion config and assembles the full transaction
2. **Integrate Lighthouse SDK**: use the Lighthouse assertion builder for TokenAccountBalance checks
3. **Test on devnet**: deploy CielAssert, build a full Lighthouse-guarded transaction, submit

**Files / modules to create**:
- `crates/ciel-enforcement/src/lighthouse.rs` — Lighthouse transaction builder
- `crates/ciel-enforcement/Cargo.toml`

## Dependencies

### Upstream (units this depends on)
- `20-ciel-assert-program` — the on-chain CielAssert program included in the transaction

### Downstream (units that depend on this)
- `26-override-mechanism` — override logic interacts with Lighthouse enforcement path

## Prompt for Claude Code

```
Implement Unit 21: Lighthouse Integration

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana. The full technical specification is at ./ciel-technical-spec.md.

Required reading before you write any code
Read this unit doc first: ./docs/21-lighthouse-integration.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 8.1 (Lighthouse Guard Instructions): full transaction layout, pseudocode, TIMEOUT handling, Lighthouse program ID
- Section 2.1 (Technology Stack): Lighthouse entry with version and links

Also read: ./docs/20-ciel-assert-program.md — the CielAssert program and SDK you'll use

Scope: what to build
In scope:
- Rust module at crates/ciel-enforcement/src/lighthouse.rs
- build_lighthouse_guarded_tx(attestation, signature, payload_instructions, lighthouse_assertions) -> Transaction
- Integration with Lighthouse SDK for assertion building
- Devnet test: full Lighthouse-guarded transaction succeeds with valid attestation
- Devnet test: reverts with invalid attestation

Out of scope: Squads integration (./docs/22-squads-integration.md), Jito integration (./docs/23-jito-integration.md)

Implementation constraints
- Language: Rust + TypeScript for client-side testing
- Libraries: lighthouse-sdk (or manual instruction construction if SDK is TypeScript-only), solana-sdk
- Lighthouse program ID: L2TExMFKdjpN9kozasaurPirfHy9P8sbXoAN1qA3S95

Verification steps
1. Build a Lighthouse-guarded transaction with valid attestation → submit to devnet → success
2. Build with invalid attestation → submit to devnet → transaction reverts
3. Confirm compute budget is within acceptable limits (< 400K CU total)

What to report when finished
- Files created, test results, CU consumption
- Whether Lighthouse SDK supports Rust or only TypeScript (resolves Open Question #3)
- Estimated next unit: 22-squads-integration

What NOT to do
- Do not implement Squads or Jito integration
- Do not modify ./ciel-technical-spec.md
```
