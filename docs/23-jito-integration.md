# 23: Jito Integration

## Overview

This unit implements Jito bundle assembly with the Ciel attestation as a precondition. The attestation verification transaction is the first tx in the bundle — if it fails, the Jito Block Engine drops the entire bundle atomically, preventing the payload from landing on-chain. This is the enforcement path for MEV searchers and agents.

> Authoritative reference: see [Section 8.3](../ciel-technical-spec.md#83-jito-bundle-precondition) of the technical spec for the full bundle layout and TypeScript pseudocode.

## Technical Specifications

- **Bundle API**: JSON-RPC at `mainnet.block-engine.jito.wtf/api/v1/bundles`. See [Section 8.3](../ciel-technical-spec.md#83-jito-bundle-precondition).
- **Bundle layout**: [verify_tx, payload_tx, tip_tx]. See [Section 8.3](../ciel-technical-spec.md#83-jito-bundle-precondition).
- **Atomic guarantee**: if verify_tx fails, entire bundle is dropped. See [Section 8.3](../ciel-technical-spec.md#83-jito-bundle-precondition).
- **Tip mechanics**: SOL transfer to one of 8 tip accounts as last instruction. See [Section 8.3](../ciel-technical-spec.md#83-jito-bundle-precondition).
- **Max bundle size**: 5 transactions. See [Section 8.3](../ciel-technical-spec.md#83-jito-bundle-precondition).

## Key Capabilities

- [ ] Build a Jito bundle with attestation verification as tx[0] — verified by assembling a test bundle
- [ ] Submit bundle via sendBundle JSON-RPC — verified by submission (mainnet with tiny tip or testnet)
- [ ] Include correct tip transaction — verified by checking tip goes to a valid tip account
- [ ] Bundle is rejected when attestation verification fails — verified by submitting with invalid attestation

## Implementation Guide

1. **Implement `build_jito_bundle`**: takes attestation + payload instructions + tip config, returns Vec<Transaction>
2. **Implement `submit_bundle`**: calls the Jito `sendBundle` JSON-RPC endpoint
3. **Implement `get_tip_accounts`**: calls `getTipAccounts` to fetch current tip account list
4. **Test**: assemble a bundle with a valid attestation and a simple transfer payload

**Key gotchas**:
- The verify_tx must include BOTH the Ed25519SigVerify instruction AND the CielAssert instruction
- Each transaction in the bundle must be independently signed and serialized
- Tip amount must be >= 1000 lamports
- The `response.signature` from the SDK (not `attestation.signature`) is the Ed25519 signature — see the corrected TypeScript in Section 8.3

**Files / modules to create**:
- `crates/ciel-enforcement/src/jito.rs` — bundle builder and submitter

## Dependencies

### Upstream (units this depends on)
- `20-ciel-assert-program` — the CielAssert program included in the verify_tx

### Downstream (units that depend on this)
- `26-override-mechanism` — override interacts with Jito enforcement
- `33-intent-bundle-assembly` — intent mode assembles Jito bundles for winning candidates

## Prompt for Claude Code

```
Implement Unit 23: Jito Integration

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana. The full technical specification is at ./ciel-technical-spec.md.

Required reading before you write any code
Read this unit doc first: ./docs/23-jito-integration.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 8.3 (Jito Bundle Precondition): full bundle layout, TypeScript pseudocode (note the corrected response.signature reference), tip mechanics, TIMEOUT handling
- Section 2.1 (Technology Stack): Jito entries with API URLs

Also read: ./docs/20-ciel-assert-program.md — the CielAssert SDK for building the verify tx

Scope: what to build
In scope:
- build_jito_bundle(attestation, signature, payload_txs, payer, tip_lamports) -> Vec<Transaction>
- submit_bundle(bundle) -> BundleId via JSON-RPC to Jito Block Engine
- get_tip_accounts() -> Vec<Pubkey>
- get_bundle_status(bundle_id) -> BundleStatus
- Unit tests for bundle assembly, integration test for submission

Out of scope: Lighthouse integration, Squads integration, intent-mode bundle assembly (./docs/33-intent-bundle-assembly.md)

Implementation constraints
- Language: Rust
- Libraries: reqwest (for JSON-RPC), solana-sdk, serde_json
- File location: crates/ciel-enforcement/src/jito.rs
- Jito endpoint: configurable via JITO_BLOCK_ENGINE_URL env var

Verification steps
1. Assemble a test bundle and verify it has the correct structure (verify_tx first, tip_tx last)
2. Verify the verify_tx contains Ed25519SigVerify and CielAssert instructions
3. If mainnet is available: submit a bundle with a minimal tip and verify getBundleStatuses returns a result
4. Verify bundle has <= 5 transactions

What to report when finished
- Files created, test results
- Whether Jito testnet/devnet Block Engine is available (resolves question from spec)
- Estimated next unit: 24-contagion-map-checker

What NOT to do
- Do not implement Lighthouse or Squads enforcement
- Do not implement intent-mode logic
- Do not modify ./ciel-technical-spec.md
```
