# 22: Squads Integration

## Overview

This unit implements the Squads v4 policy gate — Ciel's signing key is added as a required member of a Squads multisig, and Ciel programmatically approves or withholds approval based on the verdict. The `time_lock` feature provides the override mechanism for BLOCK verdicts (24h for treasuries, 1h for agents).

> Authoritative reference: see [Section 8.2](../ciel-technical-spec.md#82-squads-policy-gate) of the technical spec for the integration pattern and pseudocode.

## Technical Specifications

- **SDK**: `@squads-protocol/multisig` (npm). See [Section 2.1](../ciel-technical-spec.md#21-core-technologies).
- **Pattern**: Ciel key as multisig member, programmatic approval. See [Section 8.2](../ciel-technical-spec.md#82-squads-policy-gate).
- **Time lock**: Squads v4 native `time_lock` field for override delay. See [Section 8.2](../ciel-technical-spec.md#82-squads-policy-gate).
- **TIMEOUT handling**: TIMEOUT = Ciel does not approve → treasury uses time_lock override. See [Section 8.2](../ciel-technical-spec.md#82-squads-policy-gate).

## Key Capabilities

- [ ] Create a Squads multisig with Ciel key as a member — verified on devnet
- [ ] Ciel programmatically approves a proposal when verdict is APPROVE — verified on devnet
- [ ] Ciel withholds approval on BLOCK verdict — verified by checking proposal remains unapproved
- [ ] time_lock delays execution after override — verified by checking execution fails before delay

## Implementation Guide

1. **Set up TypeScript integration module** using `@squads-protocol/multisig`
2. **Implement Squads webhook handler**: listens for new proposals, triggers verdict evaluation
3. **Implement programmatic approval**: calls `proposalApprove` with Ciel's keypair on APPROVE
4. **Test on devnet**: create a multisig, create a proposal, have Ciel approve it, execute

**Files / modules to create**:
- `crates/ciel-enforcement/src/squads.rs` — Rust-side Squads integration logic
- `sdk/typescript/src/squads.ts` — TypeScript Squads SDK wrapper (may be needed for v4 SDK)

## Dependencies

### Upstream (units this depends on)
- `07-api-server` — the API server context where Squads webhook handler runs

### Downstream (units that depend on this)
- `26-override-mechanism` — override uses Squads time_lock

## Prompt for Claude Code

```
Implement Unit 22: Squads Integration

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana. The full technical specification is at ./ciel-technical-spec.md.

Required reading before you write any code
Read this unit doc first: ./docs/22-squads-integration.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 8.2 (Squads Policy Gate): the full integration pattern, TypeScript pseudocode, time_lock config, TIMEOUT handling
- Section 9.2 (Time Delays Per Segment): 24h for treasuries, 1h for agents
- Section 2.1 (Technology Stack): Squads v4 entries

Also read: ./docs/07-api-server.md — the server context

Scope: what to build
In scope:
- Squads v4 integration using @squads-protocol/multisig
- Multisig creation with Ciel key as member
- Programmatic proposal approval on APPROVE verdict
- Withhold approval on BLOCK/TIMEOUT verdict
- time_lock configuration for override delays
- Devnet test: full proposal lifecycle

Out of scope: Lighthouse (./docs/21-lighthouse-integration.md), Jito (./docs/23-jito-integration.md)

Implementation constraints
- Language: TypeScript for Squads SDK integration, Rust for the server-side handler
- Libraries: @squads-protocol/multisig, @solana/web3.js
- Squads program ID: SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf

Verification steps
1. Create a 2-of-3 multisig on devnet with Ciel as one member
2. Create a proposal, have Ciel approve → confirm proposal reaches Approved status
3. Withhold Ciel's approval → confirm proposal stays below threshold
4. Confirm time_lock is set correctly in the multisig config

What to report when finished
- Files created, test results
- Whether Squads v4 has added native policy hooks (resolves Open Question #4)
- Estimated next unit: 23-jito-integration

What NOT to do
- Do not implement Lighthouse or Jito enforcement
- Do not modify ./ciel-technical-spec.md
```
