# 31: Candidate Generator

## Overview

This unit integrates with the Jupiter quote API to generate candidate execution plans for a structured intent. Given an Intent, it queries Jupiter for the top N routes and builds candidate transactions. For v1, this is hardcoded Jupiter routing — the PoIN-style external agent competition is deferred to v2.

> Authoritative reference: see [Section 10.3](../ciel-technical-spec.md#103-candidate-plan-generation-v1) of the technical spec for the Jupiter integration code and the PoIN design decision.

## Technical Specifications

- **Route source**: Jupiter quote API, top 3 routes. See [Section 10.3](../ciel-technical-spec.md#103-candidate-plan-generation-v1).
- **Design decision**: Internal routing replaces PoIN-style external competition for v1. See [Section 10.3](../ciel-technical-spec.md#103-candidate-plan-generation-v1).
- **Token resolution**: resolve_mint() maps token symbols to mint pubkeys. See [Section 10.3](../ciel-technical-spec.md#103-candidate-plan-generation-v1).

## Key Capabilities

- [ ] Query Jupiter for multiple routes given an Intent — verified with a USDC→SOL intent
- [ ] Build valid Solana transactions from Jupiter quotes — verified by deserializing results
- [ ] Return top 3 candidates ranked by expected output — verified by checking count and ordering

## Implementation Guide

1. **Implement Jupiter API client**: async HTTP to Jupiter's quote and swap-instructions endpoints
2. **Implement token resolver**: map common symbols (USDC, SOL, BONK) to mint pubkeys
3. **Implement `generate_candidates`**: takes Intent, returns Vec<Transaction>

**Files / modules to create**:
- `crates/ciel-intent/src/candidates.rs`
- `crates/ciel-intent/src/jupiter.rs` — Jupiter API client
- `crates/ciel-intent/src/tokens.rs` — token symbol to mint mapping

## Dependencies

### Upstream (units this depends on)
- `30-intent-compiler` — provides the compiled Intent struct

### Downstream (units that depend on this)
- `32-parallel-scoring` — scores the candidates produced here

## Prompt for Claude Code

```
Implement Unit 31: Candidate Generator

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

This prompt covers ONE unit. Do not implement adjacent units. Do not restate or modify the technical spec.

Required reading before you write any code
Read this unit doc first: ./docs/31-candidate-generator.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 10.3 (Candidate Plan Generation v1): the generate_candidates code, Jupiter quote API integration, the design decision about PoIN deferral to v2, the resolve_mint token resolver
- Section 10.2 (Intent JSON Schema): the Intent struct you consume (goal, constraints, budget)
- Section 1.4 (Data Flows) Flow B: structured intent → candidate generation → parallel scoring

Also read these unit docs for upstream dependencies:
- ./docs/30-intent-compiler.md — the Intent struct definition

Scope: what to build
In scope:
- Jupiter API client at crates/ciel-intent/src/jupiter.rs: async HTTP calls to Jupiter v6 quote and swap-instructions APIs
- Token resolver at crates/ciel-intent/src/tokens.rs: maps common symbols (USDC, SOL, BONK, JTO, etc.) to Solana mint pubkeys
- generate_candidates(intent: &Intent) -> Vec<Transaction> function at crates/ciel-intent/src/candidates.rs
- Return top 3 routes from Jupiter, built into unsigned Solana transactions
- Unit tests with mocked Jupiter API responses

Out of scope (these belong to other units):
- Parallel scoring of candidates — owned by ./docs/32-parallel-scoring.md
- Jito bundle assembly — owned by ./docs/33-intent-bundle-assembly.md
- Intent compilation (NL → struct) — owned by ./docs/30-intent-compiler.md

Implementation constraints
- Language: Rust
- Libraries: reqwest (async HTTP to Jupiter API), solana-sdk (Transaction), serde_json
- File location: crates/ciel-intent/src/candidates.rs, jupiter.rs, tokens.rs
- Jupiter quote API: GET https://quote-api.jup.ag/v6/quote with inputMint, outputMint, amount, slippageBps
- Jupiter swap API: POST https://quote-api.jup.ag/v6/swap-instructions
- Top 3 routes, sorted by expected output amount descending

Verification steps
Before declaring this unit complete, run and report results for every step:
1. Run `cargo test --package ciel-intent` and confirm all candidate tests pass
2. Token resolver: "USDC" → EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v, "SOL" → native SOL mint
3. With Jupiter API available: generate_candidates for a USDC→SOL swap → 3 valid transactions returned
4. With mocked Jupiter response: verify transaction deserialization succeeds for all 3 candidates
5. Candidates are sorted by expected output (best route first)

What to report when finished
- List of files created or modified with path
- Test results (pass/fail counts)
- Jupiter API version used, any API differences from expected
- Estimated next unit to build: 32-parallel-scoring

What NOT to do
- Do not implement scoring or Jito bundle assembly
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
```
