# 14: Sim-Spoof Detection Checker

## Overview

This unit implements the v1 Sim-Spoof Detection checker, which uses pattern-based detection to identify malicious contracts that probe for sandbox environments. It maintains a registry of known simulation-detection patterns (Clock probes, SlotHashes reads, CPI depth checks) and flags programs matching those patterns.

> Authoritative reference: see [Section 4.3.7](../ciel-technical-spec.md#437-sim-spoof-detection-checker) of the technical spec for the full algorithm, v1 limitations, and v2 design sketch.

## Technical Specifications

- **Algorithm**: Pattern matching against a curated registry of sim-detection opcodes. See [Section 4.3.7](../ciel-technical-spec.md#437-sim-spoof-detection-checker).
- **Patterns**: Clock probes, SlotHashes reads, RecentBlockhashes comparisons, stack height probes. See [Section 4.3.7](../ciel-technical-spec.md#437-sim-spoof-detection-checker).
- **v1 limitation**: Single fork, pattern-based only. Differential execution is v2. See [Section 4.3.7](../ciel-technical-spec.md#437-sim-spoof-detection-checker).

## Key Capabilities

- [ ] Match programs against known sim-detection patterns — verified with a test program using SlotHashes probe
- [ ] Flag matched patterns with confidence score and pattern ID — verified by asserting output schema
- [ ] Pass on programs that don't match any patterns — verified with a clean program trace

## Implementation Guide

1. **Build SimDetectionRegistry**: a list of patterns, each with an ID, description, and matching function
2. **Implement pattern matchers**: functions that inspect CPI graph and logs for each known pattern
3. **Implement Checker trait** for SimSpoofChecker — iterate patterns, return flags for matches

**Files / modules to create**:
- `crates/ciel-checkers/src/sim_spoof.rs`
- `crates/ciel-checkers/src/sim_patterns.rs` — pattern registry

## Dependencies

### Upstream (units this depends on)
- `05-checker-framework` — provides the Checker trait

### Downstream (units that depend on this)
- `15-scorer` — consumes this checker's output

## Prompt for Claude Code

```
Implement Unit 14: Sim-Spoof Detection Checker

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

Required reading before you write any code
Read this unit doc first: ./docs/14-sim-spoof-checker.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 4.3.7 (Sim-Spoof Detection Checker): full algorithm, v1 pattern-based approach, output schema
- Section 3.6 (Anti-Sandbox-Detection): what sysvars reveal sandbox state
- Section 4.1 (Checker Plugin Interface): Checker trait

Also read: ./docs/05-checker-framework.md

Scope: what to build
In scope:
- SimSpoofChecker struct implementing Checker trait
- SimDetectionRegistry with known patterns (SLOT_HASHES_PROBE, CLOCK_PROBE, CPI_DEPTH_PROBE, RECENT_BLOCKHASHES_PROBE)
- Pattern matching against SimulationTrace CPI graph and logs
- Unit tests: matching pattern (flag), clean program (pass)

Out of scope: differential execution (v2), other checkers

Implementation constraints
- Language: Rust
- File location: crates/ciel-checkers/src/sim_spoof.rs and sim_patterns.rs
- Patterns are hardcoded in v1 — each pattern has an ID string and a matching closure

Verification steps
1. Run `cargo test --package ciel-checkers` and confirm all sim_spoof tests pass
2. Test with a trace containing SlotHashes sysvar read → assert flagged
3. Test with a clean transfer trace → assert passed: true

What to report when finished
- Files created, test results, estimated next unit: 15-scorer

What NOT to do
- Do not implement differential execution (v2)
- Do not modify ./ciel-technical-spec.md
```
