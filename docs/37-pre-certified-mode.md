# 37: Pre-Certified Mode

## Overview

This unit implements pre-certified mode for high-frequency searchers who need <20ms verdict latency. An agent submits a policy template, Ciel evaluates and signs a `PolicyAttestation`, and at transaction time Ciel verifies the tx matches the policy via deterministic pattern matching without fork simulation.

> Authoritative reference: see [Section 5.6](../ciel-technical-spec.md#56-pre-certified-mode) (policy template schema, flow, revocation) and [Section 7.5](../ciel-technical-spec.md#75-policyattestation-schema-pre-certified-mode) (PolicyAttestation struct).

## Technical Specifications

- **Policy template**: JSON schema with rules, constraints, valid_hours. See [Section 5.6](../ciel-technical-spec.md#56-pre-certified-mode).
- **PolicyAttestation**: 86-byte Borsh struct. See [Section 7.5](../ciel-technical-spec.md#75-policyattestation-schema-pre-certified-mode).
- **Runtime check**: <20ms pattern match, no fork simulation. See [Section 5.6](../ciel-technical-spec.md#56-pre-certified-mode).
- **Revocation**: on risk landscape change (oracle anomaly, etc.). See [Section 5.6](../ciel-technical-spec.md#56-pre-certified-mode).

## Key Capabilities

- [ ] Accept a policy template and return a signed PolicyAttestation — verified by round-trip
- [ ] Verify a transaction matches a pre-certified policy in <20ms — verified with timing test
- [ ] Reject transactions that don't match the policy — verified with a non-matching tx
- [ ] Revoke a policy attestation — verified by checking revoked flag

## Implementation Guide

1. **Implement policy template parser**: validate the policy JSON schema
2. **Implement policy evaluation**: check the policy constraints against current risk landscape
3. **Implement PolicyAttestation signing**: using the 86-byte Borsh schema from Section 7.5
4. **Implement fast-path matcher**: deterministic pattern match of tx against policy rules (<20ms)
5. **Add API endpoints**: POST /v1/pre-certify, POST /v1/verdict with policy_id parameter

**Files / modules to create**:
- `crates/ciel-pipeline/src/pre_certified.rs`

## Dependencies

### Upstream (units this depends on)
- `04-attestation-signer` — provides PolicyAttestation signing

### Downstream (units that depend on this)
None (leaf unit).

## Prompt for Claude Code

```
Implement Unit 37: Pre-Certified Mode

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

This prompt covers ONE unit. Do not implement adjacent units. Do not restate or modify the technical spec.

Required reading before you write any code
Read this unit doc first: ./docs/37-pre-certified-mode.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 5.6 (Pre-Certified Mode): the full policy template JSON schema, the pre-signing flow (submit policy → evaluate → sign PolicyAttestation), runtime check (<20ms pattern match), revocation on risk landscape change
- Section 7.5 (PolicyAttestation Schema): the 86-byte Borsh struct with magic "CILP", policy_hash, signer, issued_slot, expires_at, revoked flag
- Section 12.1 (SDK Surface): pre_certify() and check_pre_certified() method signatures in the SDK

Also read these unit docs for upstream dependencies:
- ./docs/04-attestation-signer.md — PolicyAttestation struct definition and Ed25519 signing

Scope: what to build
In scope:
- Policy template parsing and validation at crates/ciel-pipeline/src/pre_certified.rs
- Pre-certification endpoint: POST /v1/pre-certify { policy_template } → signed PolicyAttestation
- Fast-path verdict endpoint: POST /v1/verdict { tx, policy_id } → <20ms pattern match verdict
- Policy storage: in-memory HashMap<PolicyId, (PolicyAttestation, PolicyTemplate)> with expiry cleanup
- Deterministic pattern matcher: check tx instructions against policy rules (program, instruction name, constraints)
- Policy revocation: revoke(policy_id) → sets revoked=true, subsequent checks fail
- API endpoints wired into the server

Out of scope (these belong to other units):
- Full pipeline evaluation (fork sim + checkers) — owned by ./docs/06-pipeline-integration.md
- Enforcement integrations — PolicyAttestations are consumed the same way as CielAttestations

Implementation constraints
- Language: Rust
- Libraries: borsh (for PolicyAttestation), ed25519-dalek (for signing), serde_json (for policy template)
- File location: crates/ciel-pipeline/src/pre_certified.rs
- Pattern match MUST complete in <20ms — no fork simulation, no checker execution, no LLM calls
- PolicyAttestation must be exactly 86 bytes when Borsh-serialized

Verification steps
Before declaring this unit complete, run and report results for every step:
1. Run `cargo test --package ciel-pipeline` and confirm all pre_certified tests pass
2. Submit a policy template with rules for Jupiter swaps under $10K → receive signed PolicyAttestation (86 bytes)
3. Submit a matching tx (Jupiter swap, $5K) with the policy_id → verdict returned in <20ms
4. Submit a non-matching tx (Jupiter swap, $50K) with the policy_id → rejected
5. Revoke the policy → subsequent checks with that policy_id fail
6. Expired policy → checks fail with "policy expired" error
7. Measure fast-path latency for 100 runs — report P50 (must be <20ms)

What to report when finished
- List of files created or modified with path
- Test results (pass/fail counts)
- Measured fast-path P50 latency
- Estimated next unit to build: 40-demo-harness (start of Week 5)

What NOT to do
- Do not implement fork simulation or checker execution in the fast path
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
- Do not declare the unit complete if any test fails
```
