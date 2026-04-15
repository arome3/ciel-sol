# 33: Intent Bundle Assembly

## Overview

This unit assembles the winning intent candidate into a Jito bundle with attestation verification, payload transaction, and tip. It connects the intent scoring pipeline to the Jito enforcement path for Demo 2.

> Authoritative reference: see [Section 10.5](../ciel-technical-spec.md#105-jito-bundle-assembly-for-winning-plan) of the technical spec.

## Technical Specifications

- **Assembly function**: `assemble_jito_bundle(winning_tx, attestation, payer, tip_lamports) -> Vec<Transaction>`. See [Section 10.5](../ciel-technical-spec.md#105-jito-bundle-assembly-for-winning-plan).

## Key Capabilities

- [ ] Assemble a Jito bundle from the winning candidate + attestation — verified by bundle structure check
- [ ] Submit the assembled bundle to Jito Block Engine — verified by submission

## Implementation Guide

1. **Implement `assemble_jito_bundle`** per Section 10.5
2. **Wire into the intent pipeline**: after parallel scoring selects a winner, assemble and optionally submit

**Files / modules to create**:
- `crates/ciel-intent/src/bundle.rs`

## Dependencies

### Upstream (units this depends on)
- `23-jito-integration` — provides build_jito_bundle and submit_bundle functions
- `32-parallel-scoring` — provides the winning candidate and attestation

### Downstream (units that depend on this)
- `42-demo2-intent-flow` — Demo 2 uses intent bundle assembly

## Prompt for Claude Code

```
Implement Unit 33: Intent Bundle Assembly

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

This prompt covers ONE unit. Do not implement adjacent units. Do not restate or modify the technical spec.

Required reading before you write any code
Read this unit doc first: ./docs/33-intent-bundle-assembly.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 10.5 (Jito Bundle Assembly for Winning Plan): the assemble_jito_bundle function — takes winning_tx, attestation, payer, tip_lamports and returns Vec<Transaction>
- Section 8.3 (Jito Bundle Precondition): the full bundle layout [verify_tx, payload_tx, tip_tx], tip mechanics, atomic guarantee

Also read these unit docs for upstream dependencies:
- ./docs/23-jito-integration.md — the build_jito_bundle and submit_bundle functions you will reuse
- ./docs/32-parallel-scoring.md — the parallel scoring output (winning candidate + attestation)

Scope: what to build
In scope:
- assemble_jito_bundle(winning_tx, attestation, signature, payer, tip_lamports) -> Vec<Transaction> at crates/ciel-intent/src/bundle.rs
- Wire into the intent pipeline: after score_candidates selects a winner, call assemble_jito_bundle
- Optionally submit the bundle via the Jito API client from unit 23
- Return the assembled bundle + attestation in the VerdictResponse for intent mode
- Unit test: assemble a bundle from a mock winner + test attestation, verify structure

Out of scope (these belong to other units):
- Jito API client implementation — reuse from ./docs/23-jito-integration.md
- Parallel scoring — owned by ./docs/32-parallel-scoring.md
- Candidate generation — owned by ./docs/31-candidate-generator.md

Implementation constraints
- Language: Rust
- File location: crates/ciel-intent/src/bundle.rs
- Reuse the Jito bundle builder from crates/ciel-enforcement/src/jito.rs (unit 23)
- The verify_tx must include Ed25519SigVerify + CielAssert for the winning candidate's attestation

Verification steps
Before declaring this unit complete, run and report results for every step:
1. Run `cargo test --package ciel-intent` and confirm all bundle tests pass
2. Assemble bundle from mock winner + attestation → 3 transactions (verify, payload, tip)
3. verify_tx contains Ed25519SigVerify and CielAssert instructions
4. tip_tx contains SOL transfer to a Jito tip account
5. Bundle has <= 5 transactions total

What to report when finished
- List of files created or modified with path
- Test results (pass/fail counts)
- Estimated next unit to build: 34-x402-gateway

What NOT to do
- Do not reimplement the Jito API client (reuse unit 23)
- Do not implement scoring or candidate generation
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
```
