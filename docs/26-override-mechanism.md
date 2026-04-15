# 26: Override Mechanism

## Overview

This unit implements the BLOCK verdict override with time delay. When a verdict is BLOCK, the calling party can request an override; after the configured time delay (24h for treasuries, 1h for agents, 10min for users), Ciel issues an `OVERRIDE_APPROVED` attestation. Overrides are recorded on-chain and fed into the learning loop as training data. TIMEOUT verdicts are explicitly excluded from override.

> Authoritative reference: see [Section 9](../ciel-technical-spec.md#9-override-with-time-delay) of the technical spec for the full override specification.

## Technical Specifications

- **Override flow**: BLOCK verdict → override request → time delay → OVERRIDE_APPROVED attestation. See [Section 9.1](../ciel-technical-spec.md#91-override_approved-specification).
- **Time delays**: 24h treasury, 1h agent, 10min user. See [Section 9.2](../ciel-technical-spec.md#92-time-delays-per-segment).
- **TIMEOUT excluded**: TIMEOUT is not overridable. See [Section 9.3](../ciel-technical-spec.md#93-timeout-is-not-overridable).
- **On-chain recording**: Original BLOCK hash, overrider pubkey, timestamp. See [Section 9.4](../ciel-technical-spec.md#94-on-chain-recording).
- **Training pipeline**: Overrides feed the learning loop as negative-label training data. See [Section 9.5](../ciel-technical-spec.md#95-override-data-pipeline).

## Key Capabilities

- [ ] Accept an override request for a BLOCK verdict — verified by API call
- [ ] Reject override requests for TIMEOUT verdicts — verified by attempting override on TIMEOUT
- [ ] Enforce time delay before issuing OVERRIDE_APPROVED — verified by checking timing
- [ ] Record override events in the verdict store — verified by querying DB
- [ ] Issue a signed OverrideAttestation after delay — verified by signature check

## Implementation Guide

1. **Implement override request handler**: accepts override request, validates the original verdict was BLOCK
2. **Implement time delay enforcement**: store override request with timestamp, issue attestation only after delay
3. **Implement OverrideAttestation signing**: using the schema from Section 7.7
4. **Wire into the verdict store**: log override events with is_override=true

**Files / modules to create**:
- `crates/ciel-pipeline/src/override_handler.rs`
- API endpoint: POST /v1/override in the server

## Dependencies

### Upstream (units this depends on)
- `04-attestation-signer` — provides OverrideAttestation struct and signing
- `21-lighthouse-integration`, `22-squads-integration`, `23-jito-integration` — the enforcement paths the override interacts with

### Downstream (units that depend on this)
None (leaf unit).

## Prompt for Claude Code

```
Implement Unit 26: Override Mechanism

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

This prompt covers ONE unit. Do not implement adjacent units. Do not restate or modify the technical spec.

Required reading before you write any code
Read this unit doc first: ./docs/26-override-mechanism.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 9.1 (OVERRIDE_APPROVED Specification): the override flow from BLOCK → request → delay → OVERRIDE_APPROVED attestation
- Section 9.2 (Time Delays Per Segment): 24h treasury, 1h agent, 10min user — configurable
- Section 9.3 (TIMEOUT Is NOT Overridable): TIMEOUT verdicts must be explicitly rejected from the override flow
- Section 9.4 (On-Chain Recording): what gets recorded on-chain — original BLOCK hash, overrider pubkey, timestamp
- Section 9.5 (Override Data Pipeline): overrides feed the learning loop as training signals — log with is_override=true
- Section 7.7 (Override Attestation Type): the OverrideAttestation Borsh struct
- Section 13.1 (Verdict Log Schema): the is_override and override_reason fields in the verdict_log table

Also read these unit docs for upstream dependencies:
- ./docs/04-attestation-signer.md — OverrideAttestation struct definition and signing
- ./docs/21-lighthouse-integration.md — Lighthouse enforcement path (override interacts with it)
- ./docs/22-squads-integration.md — Squads time_lock provides the override delay mechanism
- ./docs/23-jito-integration.md — Jito enforcement path (override means resubmitting without attestation)

Scope: what to build
In scope:
- Override request handler at crates/ciel-pipeline/src/override_handler.rs
- API endpoint: POST /v1/override { original_attestation_hash, override_reason }
- Validate the original verdict was BLOCK (not TIMEOUT, not APPROVE, not WARN)
- Enforce configurable time delay per segment before issuing OVERRIDE_APPROVED
- Sign the OverrideAttestation using the CielSigner
- Log override events to verdict_log with is_override=true, override_reason, original_verdict_id
- Store pending override requests with timestamps for delay enforcement

Out of scope (these belong to other units):
- Enforcement path modifications — owned by units 21/22/23
- Checker changes — owned by Week 2 checker units
- On-chain override recording program — can be a lightweight extension to CielAssert (./docs/20-ciel-assert-program.md)

Implementation constraints
- Language: Rust
- Libraries: sqlx (Postgres), ed25519-dalek (for OverrideAttestation signing), tokio (for delay scheduling)
- File location: crates/ciel-pipeline/src/override_handler.rs
- Time delays must be configurable per segment via config, not hardcoded
- Override requests for TIMEOUT verdicts must return a clear error: "TIMEOUT verdicts are not overridable"

Verification steps
Before declaring this unit complete, run and report results for every step:
1. Request override on a BLOCK verdict → accepted, stored as pending with timestamp
2. Request override on a TIMEOUT verdict → rejected with error message
3. Request override on an APPROVE verdict → rejected (only BLOCK can be overridden)
4. After configured time delay elapses → OVERRIDE_APPROVED attestation issued with valid Ed25519 signature
5. Before delay elapses → override request returns "pending, N seconds remaining"
6. Override event appears in verdict_log with is_override=true and original_verdict_id pointing to the BLOCK verdict

What to report when finished
- List of files created or modified with path
- Test results (pass/fail counts)
- Any deviations from the technical spec, with justification
- Estimated next unit to build: 30-intent-compiler (start of Week 4)

What NOT to do
- Do not modify the enforcement integration code
- Do not modify checker logic
- Do not allow TIMEOUT overrides
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
- Do not declare the unit complete if any test fails
```
