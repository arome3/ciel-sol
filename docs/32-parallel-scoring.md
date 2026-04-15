# 32: Parallel Scoring

## Overview

This unit implements parallel candidate scoring for intent mode. Multiple candidate transactions are evaluated concurrently through the full verdict pipeline (fork sim → checkers → scorer), ranked by `final_score = optimality_score × safety_multiplier`, and the winner is selected. Candidates that timeout are eliminated, not the entire request.

> Authoritative reference: see [Section 6.4](../ciel-technical-spec.md#64-intent-mode-parallel-candidate-scoring) (scoring architecture), [Section 10.4](../ciel-technical-spec.md#104-partial-timeout-handling) (timeout handling), and [Section 1.4](../ciel-technical-spec.md#14-data-flows) Flow B.

## Technical Specifications

- **Parallel evaluation**: `futures::join_all` over candidates with per-candidate timeout. See [Section 6.4](../ciel-technical-spec.md#64-intent-mode-parallel-candidate-scoring).
- **Ranking**: `final_score = optimality_score × safety_multiplier`. See [Section 6.3](../ciel-technical-spec.md#63-combination-rule).
- **Partial timeout**: timed-out candidate eliminated, not entire request. See [Section 10.4](../ciel-technical-spec.md#104-partial-timeout-handling).

## Key Capabilities

- [ ] Score 3 candidates in parallel — verified with timing test (should be ~1x latency, not 3x)
- [ ] Select winner with highest final_score — verified with known candidates
- [ ] Eliminate timed-out candidates — verified by injecting a slow candidate
- [ ] Return TIMEOUT when all candidates fail — verified with all-timeout scenario

## Implementation Guide

1. **Implement `score_candidates`** from Section 6.4 — uses join_all + per-candidate timeout
2. **Wire into the pipeline**: add `evaluate_intent` method to VerdictPipeline alongside evaluate_raw_tx
3. **Implement the full Flow B**: Intent → candidates → parallel eval → winner → sign attestation

**Files / modules to create**:
- `crates/ciel-pipeline/src/intent_pipeline.rs`

## Dependencies

### Upstream (units this depends on)
- `15-scorer` — provides scoring functions
- `31-candidate-generator` — provides candidate transactions

### Downstream (units that depend on this)
- `33-intent-bundle-assembly` — assembles Jito bundle for the winner

## Prompt for Claude Code

```
Implement Unit 32: Parallel Scoring

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

This prompt covers ONE unit. Do not implement adjacent units. Do not restate or modify the technical spec.

Required reading before you write any code
Read this unit doc first: ./docs/32-parallel-scoring.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 6.4 (Intent Mode Parallel Candidate Scoring): the score_candidates async function with join_all + per-candidate timeout — read the full Rust code
- Section 6.3 (Combination Rule): final_score = optimality_score × safety_multiplier where multiplier = 0 if safety fails
- Section 6.2 (Optimality Score Calculation): price efficiency (60%), fee efficiency (20%), slippage (20%)
- Section 10.4 (Partial-Timeout Handling): timed-out candidate eliminated (not entire request), all-timeout → TIMEOUT
- Section 1.4 (Data Flows) Flow B: intent → candidates → parallel eval → winner

Also read these unit docs for upstream dependencies:
- ./docs/15-scorer.md — compute_safety_score, compute_optimality_score, compute_final_score functions
- ./docs/31-candidate-generator.md — the Vec<Transaction> of candidates you score

Scope: what to build
In scope:
- score_candidates(candidates, intent, fork_sim, checkers) -> VerdictResult function per Section 6.4
- evaluate_intent method on VerdictPipeline: runs generate_candidates → score_candidates → sign winner
- Per-candidate evaluation: fork sim → checkers → scorer for each candidate in parallel
- Partial timeout handling: timed-out candidate gets None, remaining are ranked
- All-timeout handling: return VerdictResult::Timeout
- Winner selection: highest final_score where safety passes
- Integration test: 3 mock candidates with known scores, verify winner selection

Out of scope (these belong to other units):
- NL compilation — owned by ./docs/30-intent-compiler.md
- Candidate generation — owned by ./docs/31-candidate-generator.md
- Jito bundle assembly for winner — owned by ./docs/33-intent-bundle-assembly.md

Implementation constraints
- Language: Rust
- Libraries: tokio, futures (join_all), per Section 2.2
- File location: crates/ciel-pipeline/src/intent_pipeline.rs
- Per-candidate timeout: 200ms (configurable)
- The evaluate_intent method extends VerdictPipeline — do not create a separate pipeline struct

Verification steps
Before declaring this unit complete, run and report results for every step:
1. Run `cargo test --package ciel-pipeline` and confirm all intent scoring tests pass
2. Score 3 mock candidates: (safety=0.9, optimality=0.8), (safety=0.5, optimality=0.95), (safety=0.95, optimality=0.7) → winner is candidate 1 (final_score = 0.8 × 1.0 = 0.8)
3. One candidate times out (inject 500ms delay) → eliminated, other 2 still ranked normally
4. All 3 candidates timeout → VerdictResult::Timeout returned
5. Unsafe candidate (safety=0.3, optimality=1.0) → final_score = 0.0 (safety_multiplier = 0)

What to report when finished
- List of files created or modified with path
- Test results (pass/fail counts)
- Measured parallel scoring latency for 3 candidates
- Estimated next unit to build: 33-intent-bundle-assembly

What NOT to do
- Do not implement candidate generation or NL compilation
- Do not implement Jito bundle assembly
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
```
