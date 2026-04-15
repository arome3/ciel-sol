# 17: Drift Replay End-to-End Test

## Overview

This unit is the culminating integration test for Week 2: the Drift exploit transaction is replayed through the full verdict pipeline (fork simulation → all checkers → scorer → signer) and must produce a BLOCK verdict. This test is the technical foundation of Demo 1. It validates that Oracle Sanity and Authority Diff checkers fire, safety_score < 0.4, and the final verdict is BLOCK.

> Authoritative reference: see [Section 17.3](../ciel-technical-spec.md#173-end-to-end-drift-exploit-replay) of the technical spec for the 7 assertions.

## Technical Specifications

- **7 assertions**: Oracle Sanity fires, Authority Diff fires, safety_score < 0.4, verdict == BLOCK, attestation is signed, signature is valid, rationale is non-null. See [Section 17.3](../ciel-technical-spec.md#173-end-to-end-drift-exploit-replay).

## Key Capabilities

- [ ] Load the Drift fixture and run it through the full pipeline — verified by test execution
- [ ] Oracle Sanity checker flags oracle manipulation — verified by asserting checker output
- [ ] Authority Diff checker flags admin key transfer — verified by asserting checker output
- [ ] safety_score < 0.4 (BLOCK threshold) — verified by asserting the score
- [ ] verdict == BLOCK — verified by asserting attestation.verdict
- [ ] Attestation signature is valid — verified by Ed25519 verification
- [ ] Rationale string is non-null when LLM is available (LLM generated explanation) — verified by asserting non-null if Groq API key is set; skip this assertion in offline/CI mode

## Implementation Guide

1. **Write the integration test**: load Drift fixture → create ForkSimulator → execute → run all real checkers → score → sign → verify
2. **Assert all 7 conditions** from Section 17.3
3. This is a test, not a library — it lives in `tests/` or `crates/ciel-pipeline/tests/`

**Key gotchas**:
- This test exercises the full dependency chain: fixture (00) + fork sim (01) + trace (02) + all checkers (10-14) + scorer (15) + LLM (16) + signer (04) + pipeline (06)
- If the Drift fixture uses a backup (synthetic) exploit, the assertions must be adjusted to match what that fixture triggers
- The test should be marked `#[ignore]` for CI if it requires network access (Groq API for rationale)

**Files / modules to create**:
- `crates/ciel-pipeline/tests/drift_replay_e2e.rs`

## Dependencies

### Upstream (units this depends on)
- `00-drift-exploit-fixture` — provides the Drift transaction and accounts
- `15-scorer` — produces the safety_score and verdict
- `16-llm-client` — generates the rationale string

### Downstream (units that depend on this)
- `41-demo1-drift-replay` — the demo script wraps this test into a visual demonstration

## Prompt for Claude Code

```
Implement Unit 17: Drift Replay End-to-End Test

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

Required reading before you write any code
Read this unit doc first: ./docs/17-drift-replay-e2e.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 17.3 (End-to-End: Drift Exploit Replay): the 7 assertions this test must make
- Section 4.3.1 (Oracle Sanity Checker): what this checker should flag in the Drift exploit
- Section 4.3.2 (Authority Diff Checker): what this checker should flag in the Drift exploit
- Section 6.1 (Safety Score): the threshold logic (< 0.4 = BLOCK)

Also read upstream unit docs:
- ./docs/00-drift-exploit-fixture.md — how to load the fixture
- ./docs/06-pipeline-integration.md — the VerdictPipeline API
- ./docs/15-scorer.md — safety_score computation
- ./docs/16-llm-client.md — rationale generation

Scope: what to build
In scope:
- Integration test file at crates/ciel-pipeline/tests/drift_replay_e2e.rs
- Load Drift fixture → run full pipeline → assert 7 conditions from Section 17.3
- If LLM API is unavailable, test should still pass with rationale = None (adjust assertion)

Out of scope: demo harness, video recording, enforcement integration

Implementation constraints
- Language: Rust
- The test should work with `cargo test --package ciel-pipeline -- drift_replay`
- Mark as `#[ignore]` if it requires network access that CI can't provide
- Use #[tokio::test] for async execution

Verification steps
1. Run the test and confirm all 7 assertions pass
2. If any assertion fails, document which one and why
3. Report the safety_score and which checkers fired

What to report when finished
- Test results with specific assertion outcomes
- safety_score value, checkers that fired
- Whether real or backup fixture was used
- Estimated next unit: 20-ciel-assert-program (start of Week 3)

What NOT to do
- Do not build a demo harness or UI
- Do not modify the pipeline or checker logic to make the test pass — if the test fails, it reveals a real bug
- Do not modify ./ciel-technical-spec.md
```
