# 24: Contagion Map Checker

## Overview

This unit implements the Contagion Map checker, which detects when a target protocol's upstream dependencies are behaving anomalously. It uses a hardcoded dependency graph of major Solana protocols and checks for recent anomalous events (authority changes, large TVL movements, oracle deviations) in any dependency.

> Authoritative reference: see [Section 4.3.4](../ciel-technical-spec.md#434-contagion-map-checker) of the technical spec for the full algorithm, dependency graph, and output schema.

## Technical Specifications

- **Dependency graph**: Hardcoded for top 20 protocols (v1), dynamic graph DB in v2. See [Section 4.3.4](../ciel-technical-spec.md#434-contagion-map-checker).
- **Anomaly types**: authority change, >10% TVL movement in 1h, oracle deviation, program upgrade. See [Section 4.3.4](../ciel-technical-spec.md#434-contagion-map-checker).
- **Deterministic**: Yes, given the same RecentEventCache snapshot. See [Section 4.3.4](../ciel-technical-spec.md#434-contagion-map-checker).

## Key Capabilities

- [ ] Look up dependencies for a target protocol — verified with Drift → {Pyth, Switchboard, Jupiter, Marinade}
- [ ] Detect anomalous events in dependency protocols from the RecentEventCache — verified with injected anomaly
- [ ] Flag contagion risk with the dependency chain — verified by asserting output

## Implementation Guide

1. **Build dependency graph**: HashMap<Pubkey, Vec<Pubkey>> for top 20 protocols per Section 4.3.4
2. **Build RecentEventCache**: populated by the Geyser subscriber, snapshotted per verdict
3. **Implement Checker trait** for ContagionMapChecker

**Files / modules to create**:
- `crates/ciel-checkers/src/contagion_map.rs`
- `crates/ciel-checkers/src/dependency_graph.rs`
- `crates/ciel-checkers/src/event_cache.rs`

## Dependencies

### Upstream (units this depends on)
- `03-geyser-subscriber` — populates the RecentEventCache with real-time anomaly data
- `05-checker-framework` — provides the Checker trait

### Downstream (units that depend on this)
- `15-scorer` — consumes this checker's output (already deployed by Week 2)

## Prompt for Claude Code

```
Implement Unit 24: Contagion Map Checker

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

This prompt covers ONE unit. Do not implement adjacent units. Do not restate or modify the technical spec.

Required reading before you write any code
Read this unit doc first: ./docs/24-contagion-map-checker.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 4.3.4 (Contagion Map Checker): the full algorithm, hardcoded dependency graph (Drift → {Pyth, Switchboard, Jupiter, Marinade}), anomaly types (authority change, >10% TVL movement, oracle deviation, program upgrade), output schema, and determinism guarantee
- Section 4.1 (Checker Plugin Interface): the Checker trait, CheckerContext (contains the RecentEventCache you will read from), CheckerOutput struct
- Section 3.4 (Geyser Reconnection): the geyser subscriber populates the event cache — understand what data is available

Also read these unit docs for upstream dependencies:
- ./docs/05-checker-framework.md — the Checker trait and CheckerOutput types to implement

Scope: what to build
In scope:
- ContagionMapChecker struct implementing the Checker trait
- DependencyGraph: hardcoded HashMap<Pubkey, Vec<Pubkey>> for top 20 Solana protocols per Section 4.3.4 (Drift, Jupiter, Marinade, Raydium, Orca, Meteora, etc.)
- RecentEventCache struct: stores recent anomalous events per protocol (populated by geyser subscriber, snapshotted per verdict)
- Anomaly detection: for each dependency of the target protocol, check the event cache for recent anomalies
- Output with dependency chain information (which dependency, what anomaly, how many slots ago)
- Unit tests: Drift tx with Pyth anomaly (flag), Drift tx with clean dependencies (pass), unknown protocol (pass with no dependencies to check)

Out of scope (these belong to other units):
- Other checkers — owned by sibling unit docs
- Scorer modifications — owned by ./docs/15-scorer.md
- Geyser subscriber that populates the event cache — owned by ./docs/03-geyser-subscriber.md

Implementation constraints
- Language: Rust
- File location: crates/ciel-checkers/src/contagion_map.rs and crates/ciel-checkers/src/dependency_graph.rs and crates/ciel-checkers/src/event_cache.rs
- The dependency graph is hardcoded in v1 — stored as a static HashMap, not a config file or database
- The checker must be deterministic given the same RecentEventCache snapshot

Verification steps
Before declaring this unit complete, run and report results for every step:
1. Run `cargo test --package ciel-checkers` and confirm all contagion_map tests pass
2. Drift transaction with Pyth anomaly injected into event cache → assert passed: false, severity: High, flag DEPENDENCY_ANOMALY
3. Clean transaction with no dependency anomalies → assert passed: true
4. Transaction targeting an unknown protocol (not in graph) → assert passed: true with no flags (no dependencies to check)
5. Verify dependency graph contains at least 10 protocols with realistic dependency relationships

What to report when finished
- List of files created or modified with path
- Test results (pass/fail counts)
- Number of protocols in the dependency graph
- Any deviations from the technical spec, with justification
- Estimated next unit to build: 25-mev-sandwich-checker

What NOT to do
- Do not implement other checkers
- Do not modify the scorer
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
- Do not declare the unit complete if any test fails
```
