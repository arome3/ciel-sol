# 15: Scorer

## Overview

This unit replaces the stub scorer from unit 06 with the full `safety_score` and `optimality_score` computation, verdict threshold logic, and the parallel candidate scoring architecture for intent mode. The scorer converts checker outputs into numeric scores and final APPROVE/WARN/BLOCK verdicts.

> Authoritative reference: see [Section 6](../ciel-technical-spec.md#6-scorer) of the technical spec for all scoring algorithms, thresholds, and the combination rule.

## Technical Specifications

- **safety_score**: Penalty-based, starts at 1.0, subtracts per-severity penalties. See [Section 6.1](../ciel-technical-spec.md#61-safety-score-calculation).
- **optimality_score**: Price efficiency (60%), fee efficiency (20%), slippage (20%). See [Section 6.2](../ciel-technical-spec.md#62-optimality-score-calculation).
- **Verdict thresholds**: ≥0.7 APPROVE, 0.4-0.7 WARN, <0.4 BLOCK, any Critical → immediate BLOCK. See [Section 6.1](../ciel-technical-spec.md#61-safety-score-calculation).
- **Combination rule**: `final_score = optimality × safety_multiplier` where multiplier = 0 if safety fails. See [Section 6.3](../ciel-technical-spec.md#63-combination-rule).
- **Parallel candidate scoring**: `futures::join_all` over candidates, partial timeout eliminates candidate. See [Section 6.4](../ciel-technical-spec.md#64-intent-mode-parallel-candidate-scoring).

## Key Capabilities

- [ ] Compute safety_score from CheckerResults — verified with known checker outputs
- [ ] Apply correct severity penalties (0.05 Low, 0.15 Medium, 0.40 High, 1.0 Critical) — verified per level
- [ ] Critical severity → immediate BLOCK (score = 0.0) — verified with a Critical checker output
- [ ] Timed-out checkers contribute 0.10 penalty — verified with a partial timeout
- [ ] Verdict thresholds: APPROVE ≥ 0.7, WARN 0.4-0.7, BLOCK < 0.4 — verified at boundary values
- [ ] Parallel candidate scoring selects highest final_score — verified with 3 candidates

## Implementation Guide

1. **Implement `compute_safety_score`**: iterate checker results, apply severity penalties per Section 6.1
2. **Implement `compute_optimality_score`**: price/fee/slippage efficiency per Section 6.2
3. **Implement `compute_final_score`**: combination rule per Section 6.3
4. **Implement `score_candidates`**: parallel scoring per Section 6.4 — join_all with per-candidate timeout
5. **Replace the stub scorer in ciel-pipeline**

**Files / modules to create**:
- `crates/ciel-checkers/src/scorer.rs` — scoring functions
- Update `crates/ciel-pipeline/src/scorer_stub.rs` → replace with real scorer import

## Dependencies

### Upstream (units this depends on)
- `10-oracle-sanity-checker` through `14-sim-spoof-checker` — produce the CheckerOutputs that the scorer consumes
- Note: `24-contagion-map-checker` and `25-mev-sandwich-checker` (Week 3) also produce CheckerOutputs. The scorer consumes all checker outputs generically via the CheckerResults type — no scorer code changes are needed when new checkers are added.

### Downstream (units that depend on this)
- `17-drift-replay-e2e` — asserts safety_score < 0.4 for Drift replay
- `32-parallel-scoring` — uses score_candidates for intent mode

## Prompt for Claude Code

```
Implement Unit 15: Scorer

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

Required reading before you write any code
Read this unit doc first: ./docs/15-scorer.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 6.1 (Safety Score Calculation): the full Rust code for penalty-based scoring
- Section 6.2 (Optimality Score Calculation): price/fee/slippage efficiency formula
- Section 6.3 (Combination Rule): final_score = optimality × safety_multiplier
- Section 6.4 (Intent Mode Parallel Candidate Scoring): the score_candidates async function

Also read:
- ./docs/05-checker-framework.md — CheckerResults and CheckerOutput types
- ./docs/10-oracle-sanity-checker.md through ./docs/14-sim-spoof-checker.md — the checkers whose outputs you score

Scope: what to build
In scope:
- compute_safety_score(checker_results) -> f64
- compute_optimality_score(intent, trace) -> f64
- compute_final_score(safety, optimality) -> f64
- Verdict derivation from safety_score thresholds
- score_candidates() parallel scoring for intent mode
- Replace the stub scorer in ciel-pipeline
- Unit tests for every threshold boundary and penalty level

Out of scope: API server changes, LLM client, enforcement integrations

Implementation constraints
- Language: Rust
- File location: crates/ciel-checkers/src/scorer.rs
- Severity penalties must match Section 6.1 exactly: None=0.0, Low=0.05, Medium=0.15, High=0.40, Critical=1.0
- TimedOut checker penalty: 0.10

Verification steps
1. Run `cargo test --package ciel-checkers` and confirm all scorer tests pass
2. All checkers pass → assert safety_score = 1.0, verdict = APPROVE
3. One Critical checker → assert safety_score = 0.0, verdict = BLOCK
4. One High + one Low → assert safety_score = 0.55, verdict = WARN
5. Boundary: safety_score exactly 0.7 → assert APPROVE (≥ threshold)
6. Boundary: safety_score exactly 0.399 → assert BLOCK (< 0.4)

What to report when finished
- Files created/modified, test results
- Estimated next unit: 16-llm-client

What NOT to do
- Do not implement checkers, LLM client, or enforcement logic
- Do not modify ./ciel-technical-spec.md
```
