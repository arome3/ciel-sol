# 40: Demo Harness

## Overview

This unit builds the CLI demo tool that makes the Ciel verdict pipeline visible and dramatic. It accepts a transaction or intent, calls the Ciel API, and displays the full pipeline trace with colored output, per-stage timing, checker results, verdict, and attestation details. This is the tool used to record Demo 1 and Demo 2.

> Authoritative reference: see [Section 18 Week 5](../ciel-technical-spec.md#week-5--demo--submission) of the technical spec for demo harness requirements and critical demo artifacts.

## Technical Specifications

- **CLI features**: colored output, timing display, pipeline visualization, optional enforcement submission. See [Section 18](../ciel-technical-spec.md#18-five-week-build-plan-engineering-view).
- **Demo 1**: Drift exploit replay → BLOCK verdict. See [Section 18 Week 5](../ciel-technical-spec.md#week-5--demo--submission).
- **Demo 2**: "swap 10k USDC→SOL" intent → parallel scoring → Jito execution. See [Section 18 Week 5](../ciel-technical-spec.md#week-5--demo--submission).

## Key Capabilities

- [ ] Submit a raw tx to Ciel and display the full verdict trace with timing — verified visually
- [ ] Submit an NL intent and display candidate scoring — verified visually
- [ ] Colored terminal output showing APPROVE (green), WARN (yellow), BLOCK (red) — verified visually
- [ ] Per-stage timing breakdown matching Section 1.5 stages — verified by comparing with server logs

## Implementation Guide

1. **Build a Rust CLI** using `clap` for argument parsing
2. **Implement verdict display**: colored output using `colored` crate, timing bars, checker result table
3. **Implement two modes**: `ciel-demo replay <fixture>` (Demo 1) and `ciel-demo intent "<text>"` (Demo 2)
4. **Call the Ciel API** using the Rust SDK from unit 35

**Files / modules to create**:
- `crates/ciel-demo/Cargo.toml`
- `crates/ciel-demo/src/main.rs` — CLI entry point
- `crates/ciel-demo/src/display.rs` — colored output formatter

## Dependencies

### Upstream (units this depends on)
- `07-api-server` — the server the demo calls

### Downstream (units that depend on this)
- `41-demo1-drift-replay` — uses this harness for Demo 1
- `42-demo2-intent-flow` — uses this harness for Demo 2

## Prompt for Claude Code

```
Implement Unit 40: Demo Harness

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

This prompt covers ONE unit. Do not implement adjacent units. Do not restate or modify the technical spec.

Required reading before you write any code
Read this unit doc first: ./docs/40-demo-harness.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 18 Week 5 (Demo + Submission): demo harness requirements — CLI with colored output, timing display, pipeline visualization, optional enforcement submission
- Section 1.5 (Latency Budget): the pipeline stages (fork sim, checkers, LLM, scorer, signer) and their timing targets — these are what the CLI displays
- Section 12.1 (SDK Surface): VerdictResponse struct — this is what the CLI receives from the API

Also read these unit docs for upstream dependencies:
- ./docs/07-api-server.md — the API server the demo CLI calls

Scope: what to build
In scope:
- Rust CLI tool at crates/ciel-demo/
- clap subcommands: `ciel-demo replay <fixture-path>` for Demo 1, `ciel-demo intent "<text>"` for Demo 2
- Pipeline visualization: colored output showing each stage (fork sim → checkers → scorer → verdict)
- Per-stage timing bars (e.g., "fork_sim: 32ms ████████░░")
- Checker result table: checker name | passed/flagged | severity | flag code
- Verdict display: APPROVE (green), WARN (yellow), BLOCK (red), TIMEOUT (gray)
- Attestation summary: tx_hash, safety_score, slot, expiry, signature preview
- For intent mode: candidate scoring table with final_score per candidate, winner highlighted
- Calls the Ciel API via the Rust SDK (crates/ciel-sdk/)

Out of scope (these belong to other units):
- Demo scripting and narration — owned by ./docs/41-demo1-drift-replay.md and ./docs/42-demo2-intent-flow.md
- Video recording — owned by ./docs/43-video-and-submission.md

Implementation constraints
- Language: Rust
- Libraries: clap 4.x (CLI framework), colored (terminal colors), tabled or comfy-table (table formatting), ciel-sdk (API client)
- File location: crates/ciel-demo/
- The CLI must connect to a running Ciel server (configurable via --endpoint flag, default localhost:8080)
- Output must look good in both light and dark terminal themes

Verification steps
Before declaring this unit complete, run and report results for every step:
1. `ciel-demo replay fixtures/drift-exploit/` → pipeline trace displayed with BLOCK in red, checker table shows Oracle Sanity and Authority Diff flagged
2. `ciel-demo intent "swap 100 USDC for SOL"` → candidate scoring table displayed, winner highlighted
3. Per-stage timing is shown (fork_sim_ms, checkers_ms, llm_ms, signing_ms)
4. `ciel-demo --help` shows usage information for both subcommands
5. Output renders correctly in a standard terminal (80-column minimum)

What to report when finished
- List of files created or modified with path
- Terminal output sample (copy-paste from a test run)
- Estimated next unit to build: 41-demo1-drift-replay

What NOT to do
- Do not implement demo scripting or narration
- Do not implement video recording automation
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
```
