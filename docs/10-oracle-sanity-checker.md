# 10: Oracle Sanity Checker

## Overview

This unit implements the Oracle Sanity checker — the flagship checker that catches Drift-class oracle manipulation attacks. It cross-references Switchboard On-Demand and Pyth Lazer price feeds for the same asset, flags deviations exceeding 3 sigma, and detects reads from non-canonical oracle accounts. This is the checker that makes Demo 1 work.

> Authoritative reference: see [Section 4.3.1](../ciel-technical-spec.md#431-oracle-sanity-checker) of the technical spec for the full algorithm, input/output schema, and test strategy.

## Technical Specifications

- **Algorithm**: Cross-reference Switchboard + Pyth, compute sigma deviation, flag > 3σ. See [Section 4.3.1](../ciel-technical-spec.md#431-oracle-sanity-checker).
- **Oracle SDKs**: `switchboard-on-demand` (Rust) and `pyth-solana-receiver-sdk` (Rust). See [Section 2.1](../ciel-technical-spec.md#21-core-technologies).
- **Output schema**: JSON with deviation_sigma, prices from both sources. See [Section 4.3.1](../ciel-technical-spec.md#431-oracle-sanity-checker).
- **Deterministic guarantee**: fully deterministic given the same oracle cache snapshot. See [Section 4.3.1](../ciel-technical-spec.md#431-oracle-sanity-checker).

## Key Capabilities

- [ ] Identify oracle account reads in a SimulationTrace — verified by parsing a trace containing Switchboard/Pyth account reads
- [ ] Cross-reference Switchboard and Pyth prices for the same asset — verified with known test prices
- [ ] Flag deviations > 3 sigma as Critical severity — verified with a 4σ test case
- [ ] Pass on deviations < 3 sigma — verified with a 2σ test case
- [ ] Flag reads from non-canonical oracle accounts — verified by injecting an unknown oracle pubkey

## Implementation Guide

1. **Implement Checker trait** for OracleSanityChecker
2. **Build oracle feed mapping**: hardcoded map of asset → (Switchboard pubkey, Pyth feed ID) for top 20 pairs
3. **Parse oracle reads from SimulationTrace**: identify accounts owned by Switchboard/Pyth programs
4. **Compute deviation**: `|price_a - price_b| / max(std_dev_a, confidence_b)`
5. **Threshold check**: configurable sigma threshold (default 3.0)

**Key gotchas**:
- Oracle account data formats differ between Switchboard and Pyth — need different deserializers
- Pyth Lazer uses confidence intervals, Switchboard uses std_dev — normalize both to a comparable uncertainty measure
- The oracle cache must be populated before the checker runs (populated by the Geyser subscriber or pre-fetched)

**Files / modules to create**:
- `crates/ciel-checkers/src/oracle_sanity.rs`
- `crates/ciel-checkers/src/oracle_cache.rs` — oracle price cache and feed mapping

## Dependencies

### Upstream (units this depends on)
- `05-checker-framework` — provides the Checker trait to implement

### Downstream (units that depend on this)
- `15-scorer` — consumes this checker's output for safety_score
- `17-drift-replay-e2e` — the Drift replay asserts this checker fires

## Prompt for Claude Code

```
Implement Unit 10: Oracle Sanity Checker

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

This prompt covers ONE unit. Do not implement adjacent units. Do not restate or modify the technical spec.

Required reading before you write any code
Read this unit doc first: ./docs/10-oracle-sanity-checker.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 4.3.1 (Oracle Sanity Checker): the full algorithm, output schema, false positive/negative modes, and unit test strategy
- Section 4.1 (Checker Plugin Interface): the Checker trait and CheckerContext you must implement
- Section 2.1 (Technology Stack): Switchboard On-Demand and Pyth Lazer entries — SDK names and versions

Also read these unit docs for upstream dependencies:
- ./docs/05-checker-framework.md — the Checker trait and CheckerOutput types to implement

Scope: what to build
The Oracle Sanity checker that cross-references Switchboard and Pyth price feeds and flags oracle manipulation.

In scope:
- OracleSanityChecker struct implementing the Checker trait
- Oracle feed mapping: hardcoded HashMap<String, (Pubkey, String)> for top 20 asset pairs (asset → Switchboard pubkey, Pyth feed ID)
- Oracle account read parsing from SimulationTrace
- Sigma deviation computation
- Non-canonical oracle account detection
- Stale feed detection (> 30s)
- Abnormally wide confidence interval detection
- OracleCache struct for holding fetched oracle prices
- Unit tests per Section 4.3.1 test strategy: 2.9σ (pass), 3.1σ (flag), non-canonical account (flag)

Out of scope (these belong to other units):
- Other checkers — owned by ./docs/11-authority-diff-checker.md through ./docs/14-sim-spoof-checker.md
- Scorer — owned by ./docs/15-scorer.md
- Geyser subscriber that populates oracle cache — owned by ./docs/03-geyser-subscriber.md

Implementation constraints
- Language: Rust
- Libraries: switchboard-on-demand (or switchboard-on-demand-client), pyth-solana-receiver-sdk (or pyth-sdk-solana), serde_json
- File location: crates/ciel-checkers/src/oracle_sanity.rs
- The sigma threshold must be configurable (default 3.0)
- The checker must be deterministic given the same OracleCache snapshot

Verification steps
Before declaring this unit complete, run and report results for every step:
1. Run `cargo test --package ciel-checkers` and confirm all oracle_sanity tests pass
2. Test with 3.1σ deviation → assert passed: false, severity: Critical, flag code ORACLE_DEVIATION_3_SIGMA
3. Test with 2.9σ deviation → assert passed: true
4. Test with non-canonical oracle account → assert flag code includes UNKNOWN_ORACLE_ACCOUNT or equivalent
5. Test with stale feed (> 30s) → assert flagged

What to report when finished
- List of files created or modified with path
- Test results (pass/fail counts)
- Any differences in Switchboard/Pyth SDK APIs from what the spec assumes
- Estimated next unit to build: 11-authority-diff-checker

What NOT to do
- Do not implement other checkers
- Do not implement the scorer
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
- Do not declare the unit complete if any test fails
```
