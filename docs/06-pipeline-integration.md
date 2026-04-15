# 06: Pipeline Integration

## Overview

This unit wires the fork simulator, checker framework, scorer (stub), and attestation signer into a single verdict pipeline. Given a raw transaction, it executes the full flow: fork simulation → checker fan-out → scoring → Ed25519 signing → signed attestation. This is the Saturday integration day deliverable of Week 1.

> Authoritative reference: see [Section 1.4](../ciel-technical-spec.md#14-data-flows) (Flow A: Raw Transaction), [Section 1.5](../ciel-technical-spec.md#15-latency-budget) (latency budget), and [Section 6.1](../ciel-technical-spec.md#61-safety-score-calculation) (scoring stub).

## Technical Specifications

- **Pipeline flow**: deserialize tx → fork sim → checkers → scorer → signer → attestation. See [Section 1.4](../ciel-technical-spec.md#14-data-flows) Flow A.
- **Latency target**: ~150ms P50 end-to-end. See [Section 1.5](../ciel-technical-spec.md#15-latency-budget).
- **Scorer (stub for now)**: safety_score = 1.0 if all checkers pass, 0.0 if any Critical. See [Section 6.1](../ciel-technical-spec.md#61-safety-score-calculation).
- **Staleness downgrade**: if geyser gap > 3s, verdicts return WARN. See [Section 3.4](../ciel-technical-spec.md#34-geyser-reconnection-and-gap-fill-strategy).
- **Verdict store**: write verdict to Postgres (schema from Section 13.1). See [Section 13.1](../ciel-technical-spec.md#131-verdict-log-schema).

## Key Capabilities

- [ ] Process a raw transaction through the full pipeline and return a signed CielAttestation — verified by submitting a test tx and checking the attestation signature
- [ ] All pipeline stages execute within the latency budget — verified by timing instrumentation
- [ ] Verdict is logged to PostgreSQL — verified by querying the verdict_log table after a pipeline run
- [ ] Pipeline returns TIMEOUT if total deadline (200ms) is exceeded — verified by injecting delays
- [ ] Staleness state from geyser affects verdict (WARN downgrade) — verified by simulating a stale cache

## Implementation Guide

1. **Define `VerdictPipeline` struct**: holds references to ForkSimulator, checker list, stub scorer, CielSigner, database pool
2. **Implement `evaluate_raw_tx`**: the main pipeline function — async, traces through each stage
3. **Implement stub scorer**: simple threshold-based scoring (Section 6.1 penalties) — just enough to produce verdicts
4. **Implement verdict logging**: insert into verdict_log table (create the table from Section 13.1 schema)
5. **Add instrumentation**: `tracing::instrument` on each stage for timing data
6. **Integration test**: feed a real mainnet transaction through the pipeline and verify a signed attestation comes out

**Key gotchas**:
- The pipeline has a top-level timeout (200ms for P50) — use `tokio::time::timeout` wrapping the entire pipeline
- The scorer at this stage is a stub; the real scorer is unit 15. Keep it simple but correct.
- Database insertion should not block the pipeline — use `tokio::spawn` for async write

**Files / modules to create**:
- `crates/ciel-pipeline/Cargo.toml`
- `crates/ciel-pipeline/src/lib.rs`
- `crates/ciel-pipeline/src/pipeline.rs` — VerdictPipeline struct and evaluate_raw_tx
- `crates/ciel-pipeline/src/scorer_stub.rs` — placeholder scorer
- `crates/ciel-pipeline/src/verdict_store.rs` — Postgres verdict logging
- `migrations/001_verdict_log.sql` — create verdict_log table from Section 13.1

## Dependencies

### Upstream (units this depends on)
- `02-simulation-trace` — provides execute_transaction and SimulationTrace
- `03-geyser-subscriber` — provides the staleness signal that triggers WARN downgrade (Section 3.4)
- `04-attestation-signer` — provides CielSigner and CielAttestation
- `05-checker-framework` — provides run_checkers and stub checkers

### Downstream (units that depend on this)
- `07-api-server` — the API server calls VerdictPipeline.evaluate_raw_tx

## Prompt for Claude Code

```
Implement Unit 06: Pipeline Integration

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

This prompt covers ONE unit. Do not implement adjacent units. Do not restate or modify the technical spec.

Required reading before you write any code
Read this unit doc first: ./docs/06-pipeline-integration.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 1.4 (Data Flows): Flow A (Raw Transaction) — the end-to-end pipeline flow
- Section 1.5 (Latency Budget): P50/P95 targets per stage and total
- Section 6.1 (Safety Score Calculation): the penalty-based scoring algorithm (implement as a stub now, real scorer in unit 15)
- Section 13.1 (Verdict Log Schema): the PostgreSQL table schema for verdict logging
- Section 3.4 (Geyser Reconnection): staleness thresholds that trigger WARN downgrade

Also read these unit docs for upstream dependencies:
- ./docs/02-simulation-trace.md — SimulationTrace and execute_transaction API
- ./docs/03-geyser-subscriber.md — staleness signal interface for WARN downgrade
- ./docs/04-attestation-signer.md — CielSigner and CielAttestation API
- ./docs/05-checker-framework.md — run_checkers and CheckerResults API

Scope: what to build
The VerdictPipeline that orchestrates all stages of the verdict flow for raw transactions.

In scope:
- Rust crate at crates/ciel-pipeline/
- VerdictPipeline struct holding ForkSimulator, checker list, scorer, signer, DB pool
- evaluate_raw_tx(tx: &Transaction) -> Result<VerdictResponse> function
- Stub scorer implementing safety_score computation from Section 6.1
- Verdict logging to PostgreSQL using the schema from Section 13.1
- SQL migration file creating the verdict_log table
- Per-stage tracing instrumentation (using the tracing crate)
- Top-level 200ms timeout wrapping the entire pipeline
- Integration test with a real or mocked transaction

Out of scope (these belong to other units):
- API server (HTTP/gRPC endpoint) — owned by ./docs/07-api-server.md
- Real scorer with optimality_score — owned by ./docs/15-scorer.md
- Real checker implementations — owned by Week 2 unit docs
- Intent mode pipeline (Flow B/C) — owned by ./docs/32-parallel-scoring.md

Implementation constraints
- Language: Rust
- Libraries: tokio, sqlx (Postgres async driver), tracing, tracing-subscriber
- File location: crates/ciel-pipeline/
- Use sqlx::PgPool for database connections
- Verdict logging must not block the pipeline response — spawn the DB write as a background task
- The stub scorer is temporary — keep it in a separate file so unit 15 can replace it cleanly

Verification steps
Before declaring this unit complete, run and report results for every step:
1. Run `cargo test --package ciel-pipeline` and confirm all tests pass
2. Feed a simple SOL transfer through the pipeline and receive a signed CielAttestation
3. Verify the attestation signature is valid using the signer's public key
4. Verify the verdict appears in the PostgreSQL verdict_log table
5. Measure end-to-end pipeline latency for 5 runs; report P50
6. Inject a 300ms delay in a checker and confirm the pipeline returns TIMEOUT verdict

What to report when finished
- List of files created or modified with path
- Test results (pass/fail counts)
- Measured P50 latency for the stub pipeline
- Any deviations from the technical spec, with justification
- Estimated next unit to build: 07-api-server

What NOT to do
- Do not implement the API server
- Do not implement real checkers or the real scorer
- Do not implement intent mode
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
- Do not declare the unit complete if any test fails
```
