# 35: Agent SDK

## Overview

This unit implements the Rust and TypeScript client libraries for the Ciel verdict API. The SDK provides `evaluate_tx`, `evaluate_intent`, `evaluate_nl`, `pre_certify`, and `override_block` functions that handle serialization, API calls, and response parsing.

> Authoritative reference: see [Section 12.1](../ciel-technical-spec.md#121-sdk-surface) of the technical spec for function signatures and [Section 12.3](../ciel-technical-spec.md#123-authentication) for auth methods.

## Technical Specifications

- **Rust client**: CielClient struct with 5 async methods. See [Section 12.1](../ciel-technical-spec.md#121-sdk-surface).
- **TypeScript client**: CielClient class with 5 async methods. See [Section 12.1](../ciel-technical-spec.md#121-sdk-surface).
- **Authentication**: x402 micropayment, API key (Bearer), Ed25519 signature. See [Section 12.3](../ciel-technical-spec.md#123-authentication).

## Key Capabilities

- [ ] Rust: evaluate_tx returns a VerdictResponse with attestation and signature — verified against running server
- [ ] TypeScript: evaluateTx returns equivalent response — verified against running server
- [ ] x402 payment is handled transparently by the SDK — verified by monitoring payment
- [ ] API key auth works as Bearer token — verified with test key

## Implementation Guide

1. **Rust crate** at `crates/ciel-sdk/` — wraps reqwest HTTP calls to the Ciel API
2. **TypeScript package** at `sdk/typescript/` — wraps fetch calls
3. **Implement VerdictResponse parsing** matching Section 12.1 schema
4. **Publish as npm package and crates.io crate** (or document how to)

**Files / modules to create**:
- `crates/ciel-sdk/Cargo.toml` and `src/lib.rs`
- `sdk/typescript/package.json` and `src/index.ts`

## Dependencies

### Upstream (units this depends on)
- `07-api-server` — the server the SDK calls

### Downstream (units that depend on this)
- `36-mcp-server` — the MCP server wraps the SDK

## Prompt for Claude Code

```
Implement Unit 35: Agent SDK

Context
You are implementing one unit of the Ciel project. The full technical specification is at ./ciel-technical-spec.md.

Required reading before you write any code
Read this unit doc first: ./docs/35-agent-sdk.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 12.1 (SDK Surface): Rust and TypeScript function signatures, VerdictResponse struct
- Section 12.3 (Authentication): three auth methods
- Section 11.1 (x402 Endpoint): how x402 payment works from the client side

Also read: ./docs/07-api-server.md — the API endpoints the SDK calls

Scope: Rust client crate (crates/ciel-sdk/), TypeScript client package (sdk/typescript/), both with all 5 methods from Section 12.1.

Out of scope: MCP server (./docs/36-mcp-server.md), x402 gateway server-side (./docs/34-x402-gateway.md).

Implementation constraints
- Rust: reqwest (async HTTP), serde, serde_json, ed25519-dalek (for Ed25519 auth)
- TypeScript: native fetch (no heavy HTTP libraries), @solana/web3.js (for Keypair)
- Rust crate: crates/ciel-sdk/ — published to the project, not crates.io yet
- TypeScript package: sdk/typescript/ — published to npm (or documented how to install from git)
- Both clients must handle x402 402 responses transparently: receive 402 → make USDC payment → retry with X-Payment header
- VerdictResponse struct must match Section 12.1 exactly in both languages

Verification steps
Before declaring this unit complete, run and report results for every step:
1. Run `cargo test --package ciel-sdk` — all Rust SDK tests pass
2. Run `npm test` in sdk/typescript/ — all TypeScript SDK tests pass
3. Rust: evaluate_tx against running Ciel server → receive VerdictResponse with valid attestation and signature
4. TypeScript: evaluateTx against running Ciel server → receive equivalent VerdictResponse
5. API key auth: Rust client sends request with Bearer token → accepted
6. x402 flow: client sends request without payment → receives 402 → client makes payment → retries → succeeds
7. Both SDKs export types matching Section 12.1 (VerdictResponse, CielAttestation)

What to report when finished
- List of files created or modified with path
- Test results (pass/fail counts) for both Rust and TypeScript
- Any differences between Rust and TypeScript VerdictResponse types
- Estimated next unit to build: 36-mcp-server

What NOT to do
- Do not implement the MCP server (that is unit 36)
- Do not implement the x402 gateway server-side (that is unit 34)
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
- Do not declare the unit complete if any test fails
```
