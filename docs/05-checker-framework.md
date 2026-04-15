# 05: Checker Framework

## Overview

This unit defines the `Checker` trait, the `CheckerContext`, `CheckerOutput`, and `CheckerResults` types, and implements the parallel fan-out/fan-in execution model with an 80ms hard deadline using `tokio::join_all` + `tokio::time::timeout`. It ships with 7 stub checkers that return `passed: true` as placeholders. This is the Friday deliverable of Week 1.

> Authoritative reference: see [Section 4.1](../ciel-technical-spec.md#41-checker-plugin-interface) (trait definition), [Section 4.2](../ciel-technical-spec.md#42-parallel-execution-model) (fan-out pattern), and [Section 2.2](../ciel-technical-spec.md#22-tokio-async-runtime-configuration) (tokio pattern).

## Technical Specifications

- **Checker trait**: async `check(&self, ctx: &CheckerContext) -> CheckerOutput`. See [Section 4.1](../ciel-technical-spec.md#41-checker-plugin-interface).
- **CheckerContext**: wraps SimulationTrace, original tx, optional intent, slot, oracle cache, program registry. See [Section 4.1](../ciel-technical-spec.md#41-checker-plugin-interface).
- **CheckerOutput**: checker_name, passed, severity, flags, details. See [Section 4.1](../ciel-technical-spec.md#41-checker-plugin-interface).
- **Parallel execution**: `futures::join_all` with per-checker `tokio::time::timeout(80ms)`. See [Section 4.2](../ciel-technical-spec.md#42-parallel-execution-model) and [Section 2.2](../ciel-technical-spec.md#22-tokio-async-runtime-configuration).
- **Timeout handling**: timed-out checkers produce `CheckerStatus::TimedOut`, not errors. See [Section 4.2](../ciel-technical-spec.md#42-parallel-execution-model).

## Key Capabilities

- [ ] Define Checker trait that all 7 checkers implement — verified by implementing a stub checker
- [ ] Run 7 stub checkers in parallel and collect results within 80ms deadline — verified by timing test
- [ ] Handle individual checker timeout without failing the entire fan-out — verified by injecting a slow stub checker
- [ ] Produce CheckerResults with completed outputs and timed-out checker names — verified by asserting result structure
- [ ] CheckerOutput is Borsh-serializable (needed for checker_outputs_hash) — verified by Borsh round-trip

## Implementation Guide

1. **Define types**: Checker trait, CheckerContext, CheckerOutput, CheckerResults, Severity enum, Flag struct, CheckerStatus enum
2. **Implement `run_checkers`**: the parallel fan-out function from Section 4.2
3. **Create 7 stub checkers**: OracleSanityStub, AuthorityDiffStub, etc. — all return `passed: true`
4. **Implement `checker_outputs_hash`**: SHA-256 over Borsh-serialized concatenation of all completed CheckerOutputs
5. **Write tests**: parallel execution timing, timeout handling, hash determinism

**Key gotchas**:
- The `Checker` trait must be `Send + Sync` because checkers run concurrently
- `CheckerContext` must be `Clone` since each parallel task gets its own reference
- The 80ms timeout is per-checker, not for the entire fan-out — `join_all` wraps each future individually

**Files / modules to create**:
- `crates/ciel-checkers/Cargo.toml`
- `crates/ciel-checkers/src/lib.rs` — public exports
- `crates/ciel-checkers/src/traits.rs` — Checker trait, CheckerContext, CheckerOutput, CheckerResults
- `crates/ciel-checkers/src/runner.rs` — run_checkers parallel fan-out
- `crates/ciel-checkers/src/stubs.rs` — 7 stub checker implementations
- `crates/ciel-checkers/src/hash.rs` — checker_outputs_hash computation

## Dependencies

### Upstream (units this depends on)
- `02-simulation-trace` — provides the SimulationTrace type that CheckerContext wraps

### Downstream (units that depend on this)
- `06-pipeline-integration` — the pipeline calls run_checkers
- `10-oracle-sanity-checker` through `14-sim-spoof-checker` — real checkers implement the Checker trait
- `24-contagion-map-checker` and `25-mev-sandwich-checker` — Week 3 checkers

## Prompt for Claude Code

```
Implement Unit 05: Checker Framework

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

This prompt covers ONE unit. Do not implement adjacent units. Do not restate or modify the technical spec.

Required reading before you write any code
Read this unit doc first: ./docs/05-checker-framework.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 4.1 (Checker Plugin Interface): the exact trait definition, CheckerContext, CheckerOutput, Severity enum, Flag struct — read the full Rust code blocks
- Section 4.2 (Parallel Execution Model): the run_checkers function with tokio::join_all and per-checker timeout
- Section 2.2 (Tokio Async Runtime Configuration): the fan-out code pattern
- Section 7.1 (Attestation Payload Schema): checker_outputs_hash field — you need to know what hash the signer will compute over

Also read these unit docs for upstream dependencies:
- ./docs/02-simulation-trace.md — the SimulationTrace type your CheckerContext will wrap

Scope: what to build
The checker trait system, parallel runner, stub checkers, and checker-outputs hash computation.

In scope:
- Rust crate at crates/ciel-checkers/
- Checker trait (async, Send + Sync) with name() and check() methods
- CheckerContext struct wrapping SimulationTrace, Transaction, optional Intent, slot, OracleCache, ProgramRegistry
- CheckerOutput struct with checker_name, passed, severity, flags, details
- CheckerStatus enum: Completed(CheckerOutput) | TimedOut
- CheckerResults struct: outputs HashMap, total_duration_ms
- Severity enum: None, Low, Medium, High, Critical
- Flag struct: code, message, data (serde_json::Value)
- run_checkers() async function: parallel fan-out with 80ms per-checker timeout
- checker_outputs_hash(): SHA-256 of Borsh-serialized concatenated CheckerOutputs
- 7 stub checkers (oracle_sanity, authority_diff, intent_diff, contagion_map, mev_sandwich, approval_abuse, sim_spoof) that return passed: true
- Tests for parallel execution, timeout handling, hash determinism

Out of scope (these belong to other units):
- Real checker implementations — owned by ./docs/10-oracle-sanity-checker.md through ./docs/14-sim-spoof-checker.md
- Scorer logic — owned by ./docs/15-scorer.md
- The verdict pipeline — owned by ./docs/06-pipeline-integration.md

Implementation constraints
- Language: Rust
- Libraries: tokio, futures (join_all), borsh, serde, serde_json, sha2 (for SHA-256)
- File location: crates/ciel-checkers/
- CheckerOutput must derive BorshSerialize, BorshDeserialize, Serialize, Deserialize, Clone
- The Checker trait must use #[async_trait] from the async-trait crate
- The 80ms timeout is configurable via a constant (CHECKER_DEADLINE_MS)

Verification steps
Before declaring this unit complete, run and report results for every step:
1. Run `cargo test --package ciel-checkers` and confirm all tests pass
2. Run 7 stub checkers in parallel and confirm all complete within 80ms
3. Inject a stub checker with a 200ms sleep; confirm it times out and the other 6 complete normally
4. Compute checker_outputs_hash twice for the same input; confirm identical hashes (determinism)
5. Verify CheckerOutput Borsh round-trip: serialize then deserialize and confirm equality

What to report when finished
- List of files created or modified with path
- Test results (pass/fail counts)
- Any deviations from the technical spec, with justification
- Estimated next unit to build: 06-pipeline-integration

What NOT to do
- Do not implement real checker logic (oracle cross-reference, CPI parsing, etc.)
- Do not implement the scorer
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
- Do not declare the unit complete if any test fails
```
