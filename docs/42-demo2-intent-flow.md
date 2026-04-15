# 42: Demo 2 — Intent Flow

## Overview

This unit scripts and rehearses Demo 2: submitting "swap 10k USDC → SOL, minimize slippage" as an NL intent, showing parallel candidate scoring, safety × optimality ranking, winner selection, and Jito bundle execution. This demonstrates Ciel's "offensive mode" — not just blocking bad transactions, but finding the safest optimal execution path.

> Authoritative reference: see [Section 18 Week 5](../ciel-technical-spec.md#week-5--demo--submission) Demo 2 description and [Section 10](../ciel-technical-spec.md#10-intent-layer).

## Technical Specifications

- **Intent flow**: NL → structured intent → 3 Jupiter candidates → parallel scoring → winner → Jito bundle. See [Section 10](../ciel-technical-spec.md#10-intent-layer).
- **Scoring formula**: `final_score = optimality_score × safety_multiplier`. See [Section 6.3](../ciel-technical-spec.md#63-combination-rule).
- **Demo narrative**: safety as an auction dimension — unsafe candidates get score 0 regardless of optimality. See [Section 18 Week 5](../ciel-technical-spec.md#week-5--demo--submission).

## Key Capabilities

- [ ] Demo 2 runs end-to-end: NL intent → structured intent → 3 candidates → parallel scoring → winner → Jito bundle — verified by full rehearsal
- [ ] Candidate scoring table shows safety_score and optimality_score per candidate — verified visually
- [ ] Winner has highest `optimality × safety_multiplier` — verified by checking scores
- [ ] Jito bundle submission succeeds (or simulated if testnet) — verified by bundle status

## Implementation Guide

1. **Write a demo script** that automates: submit NL intent → display compilation → show 3 candidates → display scoring → show winner → submit Jito bundle
2. **Create an intent demo fixture** if needed (pre-configured oracle prices, candidate routes)
3. **Rehearse 3 times**

**Files / modules to create**:
- `demos/demo2-intent-flow.sh`
- `demos/demo2-narration.md`
- `fixtures/intent-demo/` — pre-configured intent data if needed

## Dependencies

### Upstream (units this depends on)
- `33-intent-bundle-assembly` — the intent pipeline that produces the bundle
- `40-demo-harness` — the CLI tool

### Downstream (units that depend on this)
- `43-video-and-submission` — Demo 2 recording is part of the submission video

## Prompt for Claude Code

```
Implement Unit 42: Demo 2 — Intent Flow

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

This prompt covers ONE unit. Do not implement adjacent units. Do not restate or modify the technical spec.

Required reading before you write any code
Read this unit doc first: ./docs/42-demo2-intent-flow.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 18 Week 5 (Demo + Submission): Demo 2 description — "swap 10k USDC→SOL intent → parallel scoring → winner executes via Jito"
- Section 10 (Intent Layer): full intent flow from NL to structured intent to candidate generation to parallel scoring to Jito bundle
- Section 6.4 (Parallel Candidate Scoring): the scoring architecture showing N candidates evaluated in parallel

Also read these unit docs for upstream dependencies:
- ./docs/33-intent-bundle-assembly.md — intent bundle assembly (the final step of the intent flow)
- ./docs/40-demo-harness.md — the ciel-demo CLI tool you'll use

Scope: what to build
In scope:
- Shell script at demos/demo2-intent-flow.sh that orchestrates the full Demo 2 flow:
  1. Start the Ciel server (or confirm running)
  2. Run `ciel-demo intent "swap 10k USDC to SOL, minimize slippage, MEV protected"`
  3. Display: NL compilation result, 3 candidate routes, scoring table (safety × optimality per candidate), winner selection, Jito bundle assembly
- Intent demo fixture at fixtures/intent-demo/ if needed (pre-configured oracle prices for consistent scoring)
- Narration talking points at demos/demo2-narration.md
- Rehearsal: run 3 times, confirm consistent winner selection

Out of scope (these belong to other units):
- Video recording — owned by ./docs/43-video-and-submission.md
- Demo 1 — owned by ./docs/41-demo1-drift-replay.md
- The ciel-demo CLI — owned by ./docs/40-demo-harness.md

Implementation constraints
- Shell script must work on macOS and Linux
- If Jupiter API is unavailable, use a pre-cached fixture with 3 known candidate routes
- The demo should clearly show that unsafe candidates (if any) get final_score = 0

Verification steps
Before declaring this unit complete, run and report results for every step:
1. Run `bash demos/demo2-intent-flow.sh` → shows NL → structured intent → 3 candidates → scoring table → winner → bundle
2. NL compilation produces a valid Intent with goal="swap USDC for SOL"
3. Candidate scoring table shows safety_score and optimality_score for each candidate
4. Winner has the highest final_score (safety_multiplier × optimality)
5. Run 3 times → consistent winner selection (same candidate wins each time, or close scores explained)
6. Narration notes cover the "safety as auction dimension" story

What to report when finished
- Demo output log (one successful run)
- Consistency report: winner selection across 3 runs
- Estimated next unit to build: 43-video-and-submission

What NOT to do
- Do not modify the intent pipeline or scoring logic
- Do not implement video recording
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
```
