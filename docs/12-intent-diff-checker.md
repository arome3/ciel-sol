# 12: Intent Diff Checker

## Overview

This unit implements the Intent Diff checker — a fully deterministic balance-delta comparison checker that verifies whether a transaction's outcome matches the stated intent. For unrecognized intent patterns, it returns `INTENT_VERIFICATION_INCONCLUSIVE`. An optional LLM metadata enrichment runs in parallel but never affects the checker's verdict-contributing output.

> Authoritative reference: see [Section 4.3.3](../ciel-technical-spec.md#433-intent-diff-checker) of the technical spec for the full algorithm, three output schemas (satisfied, violated, inconclusive), and determinism guarantees.

## Technical Specifications

- **Algorithm**: Deterministic balance-delta comparison against a fixed rule table. See [Section 4.3.3](../ciel-technical-spec.md#433-intent-diff-checker).
- **Rule table patterns**: swap, transfer, deposit. See [Section 4.3.3](../ciel-technical-spec.md#433-intent-diff-checker).
- **Inconclusive path**: unrecognized patterns return `passed: true, INTENT_VERIFICATION_INCONCLUSIVE`. See [Section 4.3.3](../ciel-technical-spec.md#433-intent-diff-checker).
- **LLM enrichment**: optional parallel metadata, never in CheckerOutput or checker_outputs_hash. See [Section 4.3.3](../ciel-technical-spec.md#433-intent-diff-checker) and [Section 5.5](../ciel-technical-spec.md#55-llm-output-is-metadata-only).
- **Deterministic guarantee**: fully deterministic — two independent verifiers produce identical output. See [Section 4.3.3](../ciel-technical-spec.md#433-intent-diff-checker).

## Key Capabilities

- [ ] Match "swap X A for B" intents against balance deltas — verified with matching and mismatching test cases
- [ ] Return INCONCLUSIVE for unrecognized intent patterns — verified with a multi-leg intent
- [ ] LLM enrichment output is written to a separate field, not CheckerOutput — verified by checking that CheckerOutput is identical with and without LLM
- [ ] Two independent runs produce identical checker_outputs_hash — verified by determinism test

## Implementation Guide

1. **Build the intent rule table**: enum of recognized patterns (Swap, Transfer, Deposit) with expected delta signatures
2. **Implement intent pattern parser**: regex or structured matching to classify intent.goal into a rule table entry
3. **Implement balance-delta comparator**: check actual deltas against expected pattern with ±1% tolerance
4. **Implement INCONCLUSIVE fallback**: any unrecognized pattern returns the deterministic INCONCLUSIVE flag
5. **LLM enrichment**: spawn a background task that calls the LLM client (from unit 16) and returns `intent_diff_llm_analysis` — keep this completely separate from CheckerOutput

**Key gotchas**:
- The simple/complex classification MUST be deterministic — use the rule table, not heuristics
- The LLM enrichment must never write to any field that Borsh-serializes into checker_outputs_hash
- Tolerance for balance amounts (±1%) accounts for fees and rounding

**Files / modules to create**:
- `crates/ciel-checkers/src/intent_diff.rs`
- `crates/ciel-checkers/src/intent_rules.rs` — deterministic rule table

## Dependencies

### Upstream (units this depends on)
- `05-checker-framework` — provides the Checker trait
- `16-llm-client` — optional: the LLM metadata enrichment (not verdict-affecting) calls the LLM client when available

### Downstream (units that depend on this)
- `15-scorer` — consumes this checker's output

## Prompt for Claude Code

```
Implement Unit 12: Intent Diff Checker

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

This prompt covers ONE unit. Do not implement adjacent units. Do not restate or modify the technical spec.

Required reading before you write any code
Read this unit doc first: ./docs/12-intent-diff-checker.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 4.3.3 (Intent Diff Checker): the FULL section — algorithm, three output schemas, determinism guarantee, LLM enrichment separation, and all four unit test cases. Read carefully — this checker was recently rewritten to fix a determinism contradiction.
- Section 5.5 (LLM Output Is Metadata Only): the invariant that LLM output never enters checker_outputs_hash
- Section 4.1 (Checker Plugin Interface): the Checker trait

Also read these unit docs for upstream dependencies:
- ./docs/05-checker-framework.md — the Checker trait and CheckerOutput types

Scope: what to build
The Intent Diff checker with deterministic balance-delta comparison and optional LLM metadata enrichment.

In scope:
- IntentDiffChecker struct implementing the Checker trait
- Intent rule table: recognized patterns (Swap, Transfer, Deposit) with expected delta signatures
- Intent goal parser: classify intent.goal string into a rule table entry
- Balance-delta comparator with ±1% tolerance
- INCONCLUSIVE fallback for unrecognized patterns
- Stub for LLM enrichment (actual LLM call added when unit 16 is ready) — writes to a separate output field
- All four unit tests from Section 4.3.3: mismatch, happy path, inconclusive, determinism verification

Out of scope:
- Other checkers, scorer, LLM client implementation

Implementation constraints
- Language: Rust
- File location: crates/ciel-checkers/src/intent_diff.rs and intent_rules.rs
- The checker's CheckerOutput must be IDENTICAL regardless of LLM availability
- The rule table is a versioned static data structure — not a config file

Verification steps
1. Run `cargo test --package ciel-checkers` and confirm all intent_diff tests pass
2. Intent "swap 100 USDC for SOL" + trace USDC -100, SOL +0.67 → assert passed: true
3. Intent "swap 100 USDC for SOL" + trace USDC -100, ETH +1 → assert passed: false, flag INTENT_BALANCE_MISMATCH
4. Intent "rebalance portfolio to 60/30/10 split" → assert passed: true, flag INTENT_VERIFICATION_INCONCLUSIVE
5. Run same input twice → assert checker_outputs_hash is identical (determinism proof)

What to report when finished
- List of files created or modified with path
- Test results (pass/fail counts)
- Estimated next unit to build: 13-approval-abuse-checker

What NOT to do
- Do not let LLM output influence CheckerOutput in any way
- Do not implement other checkers
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
```
