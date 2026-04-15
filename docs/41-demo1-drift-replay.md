# 41: Demo 1 — Drift Replay

## Overview

This unit scripts and rehearses Demo 1: replaying the Drift exploit transaction through Ciel and showing it being blocked in real time, with enforcement rejection on-chain. This is the centerpiece of the hackathon submission — "Here is the exact Drift exploit. Watch: Oracle Sanity checker fires, Authority Diff fires, verdict is BLOCK, enforcement rejects. $285M saved in 190ms."

> Authoritative reference: see [Section 18 Week 5](../ciel-technical-spec.md#week-5--demo--submission) Demo 1 description and [Section 17.3](../ciel-technical-spec.md#173-end-to-end-drift-exploit-replay).

## Technical Specifications

- **7 assertions**: Oracle Sanity fires, Authority Diff fires, safety_score < 0.4, verdict == BLOCK, attestation signed, signature valid, rationale non-null (if LLM available). See [Section 17.3](../ciel-technical-spec.md#173-end-to-end-drift-exploit-replay).
- **Latency target**: total pipeline under 300ms P95 for the demo. See [Section 1.5](../ciel-technical-spec.md#15-latency-budget).
- **Demo narrative**: "Here is the exact Drift exploit. $285M saved in 190ms." See [Section 18 Week 5](../ciel-technical-spec.md#week-5--demo--submission).

## Key Capabilities

- [ ] Demo 1 runs end-to-end without errors — verified by full rehearsal
- [ ] Output clearly shows which checkers fire and why — verified visually
- [ ] Enforcement rejection is visible (Lighthouse tx revert or Jito bundle drop) — verified on devnet
- [ ] Total latency is under 300ms P95 — verified by timing display

## Implementation Guide

1. **Write a demo script** that automates: load fixture → call Ciel → display verdict → attempt enforcement → show rejection
2. **Rehearse 3 times** — ensure it works consistently
3. **Prepare narration notes** for the video recording

**Files / modules to create**:
- `demos/demo1-drift-replay.sh` — shell script orchestrating the demo
- `demos/demo1-narration.md` — talking points for the video

## Dependencies

### Upstream (units this depends on)
- `17-drift-replay-e2e` — confirms the pipeline produces BLOCK for the Drift fixture
- `40-demo-harness` — the CLI tool used to run and display the demo

### Downstream (units that depend on this)
- `43-video-and-submission` — Demo 1 recording is part of the submission video

## Prompt for Claude Code

```
Implement Unit 41: Demo 1 — Drift Replay

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

This prompt covers ONE unit. Do not implement adjacent units. Do not restate or modify the technical spec.

Required reading before you write any code
Read this unit doc first: ./docs/41-demo1-drift-replay.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 18 Week 5 (Demo + Submission): Demo 1 description — "Drift exploit replay → BLOCK verdict → enforcement rejects"
- Section 17.3 (End-to-End: Drift Exploit Replay): the 7 assertions that must hold — Oracle Sanity fires, Authority Diff fires, safety_score < 0.4, verdict == BLOCK, attestation signed, signature valid, rationale non-null

Also read these unit docs for upstream dependencies:
- ./docs/17-drift-replay-e2e.md — the E2E test confirming the pipeline produces BLOCK
- ./docs/40-demo-harness.md — the ciel-demo CLI tool you'll use to run the demo

Scope: what to build
In scope:
- Shell script at demos/demo1-drift-replay.sh that orchestrates the full Demo 1 flow:
  1. Start the Ciel server (or confirm it's running)
  2. Run `ciel-demo replay fixtures/drift-exploit/`
  3. Display the pipeline trace with BLOCK verdict
  4. Optionally: submit the attestation to a Lighthouse-guarded tx on devnet and show the enforcement rejection
- Narration talking points at demos/demo1-narration.md — what to say during the video for each section of the demo output
- Rehearsal: run the script 3 times and confirm consistent BLOCK results each time
- The 30-second pitch story: "Here is the exact Drift exploit transaction. I submit it to Ciel. Watch: Oracle Sanity checker fires, Authority Diff checker fires, verdict is BLOCK, here's the signed attestation, here's Lighthouse rejecting the transaction on-chain. $285M saved in 190ms."

Out of scope (these belong to other units):
- Video recording and editing — owned by ./docs/43-video-and-submission.md
- Demo 2 (intent flow) — owned by ./docs/42-demo2-intent-flow.md
- The ciel-demo CLI itself — owned by ./docs/40-demo-harness.md

Implementation constraints
- Shell script must work on macOS and Linux
- The script should have clear echo statements narrating what's happening at each step
- If the Ciel server is not running, the script should start it (or fail with a clear message)
- Total demo run time target: under 30 seconds

Verification steps
Before declaring this unit complete, run and report results for every step:
1. Run `bash demos/demo1-drift-replay.sh` → BLOCK verdict displayed with checker details
2. Oracle Sanity checker shows as flagged in the output
3. Authority Diff checker shows as flagged in the output
4. Total pipeline latency displayed is under 300ms P95
5. Run 3 times → consistent BLOCK result each time (no flaky behavior)
6. Narration notes cover all key moments in the demo output

What to report when finished
- Demo output log (copy-paste of one successful run)
- Consistency report: 3/3 runs produced BLOCK
- Measured latency from 3 runs
- Estimated next unit to build: 42-demo2-intent-flow

What NOT to do
- Do not modify the pipeline or checkers to make the demo work — if it doesn't produce BLOCK, that's a bug in upstream units
- Do not implement video recording
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
```
