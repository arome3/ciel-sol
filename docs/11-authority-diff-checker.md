# 11: Authority Diff Checker

## Overview

This unit implements the Authority Diff checker, which parses the CPI call graph for hidden `SetAuthority`, `Upgrade`, `CloseAccount`, and `InitializeAccount` instructions that may be disguised within routine operations like deposits. This is the second key checker for the Drift replay demo — it detects admin key transfers.

> Authoritative reference: see [Section 4.3.2](../ciel-technical-spec.md#432-authority-diff-checker) of the technical spec for the full algorithm, output schema, and test strategy.

## Technical Specifications

- **Algorithm**: Parse CPI graph for authority-changing instructions, cross-reference with intent and known-programs registry. See [Section 4.3.2](../ciel-technical-spec.md#432-authority-diff-checker).
- **Instructions detected**: SetAuthority, Upgrade, CloseAccount, InitializeAccount (with different authority). See [Section 4.3.2](../ciel-technical-spec.md#432-authority-diff-checker).
- **Deterministic**: Yes — pure CPI graph parsing. See [Section 4.3.2](../ciel-technical-spec.md#432-authority-diff-checker).

## Key Capabilities

- [ ] Detect SetAuthority instructions in the CPI graph — verified with a test trace containing SetAuthority
- [ ] Detect Upgrade instructions targeting known protocol programs — verified with a test trace
- [ ] Flag as Critical when a known protocol's upgrade authority is changed — verified against the program registry
- [ ] Pass when no authority-changing instructions are present — verified with a clean transfer trace

## Implementation Guide

1. **Implement Checker trait** for AuthorityDiffChecker
2. **Build CPI instruction matcher**: pattern-match on instruction discriminators for SetAuthority, Upgrade, CloseAccount
3. **Build known-programs registry**: hardcoded set of known protocol program pubkeys with their expected authorities
4. **Cross-reference with intent**: if intent says "deposit" but tx includes SetAuthority, flag it

**Key gotchas**:
- SetAuthority can appear as a CPI deep in the call stack — must traverse the full CPI tree, not just top-level instructions
- SPL Token's SetAuthority instruction has a specific discriminator byte (6) — match on this

**Files / modules to create**:
- `crates/ciel-checkers/src/authority_diff.rs`
- `crates/ciel-checkers/src/program_registry.rs` — known-good programs and their expected authorities

## Dependencies

### Upstream (units this depends on)
- `05-checker-framework` — provides the Checker trait to implement

### Downstream (units that depend on this)
- `15-scorer` — consumes this checker's output
- `17-drift-replay-e2e` — the Drift replay asserts this checker fires

## Prompt for Claude Code

```
Implement Unit 11: Authority Diff Checker

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

This prompt covers ONE unit. Do not implement adjacent units. Do not restate or modify the technical spec.

Required reading before you write any code
Read this unit doc first: ./docs/11-authority-diff-checker.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 4.3.2 (Authority Diff Checker): the full algorithm, output schema, and unit test strategy
- Section 4.1 (Checker Plugin Interface): the Checker trait and CheckerContext

Also read these unit docs for upstream dependencies:
- ./docs/05-checker-framework.md — the Checker trait and CheckerOutput types

Scope: what to build
The Authority Diff checker that detects hidden authority transfers in the CPI call graph.

In scope:
- AuthorityDiffChecker struct implementing the Checker trait
- CPI graph traversal for SetAuthority, Upgrade, CloseAccount, InitializeAccount
- ProgramRegistry struct with known-good programs and expected authorities
- Intent cross-reference (flag authority changes not declared in intent)
- Unit tests: hidden SetAuthority in a deposit tx (flag), clean transfer (pass)

Out of scope:
- Other checkers — owned by sibling unit docs
- Scorer — owned by ./docs/15-scorer.md

Implementation constraints
- Language: Rust
- Libraries: solana-sdk (for instruction parsing), spl-token (for SetAuthority discriminator)
- File location: crates/ciel-checkers/src/authority_diff.rs
- The checker must traverse the full CPI depth, not just top-level instructions

Verification steps
1. Run `cargo test --package ciel-checkers` and confirm all authority_diff tests pass
2. Test with a trace containing hidden SetAuthority → assert passed: false, severity: Critical
3. Test with a clean SOL transfer trace → assert passed: true
4. Test with SetAuthority on a known protocol program → assert flag includes program pubkey

What to report when finished
- List of files created or modified with path
- Test results (pass/fail counts)
- Estimated next unit to build: 12-intent-diff-checker

What NOT to do
- Do not implement other checkers or the scorer
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
- Do not declare the unit complete if any test fails
```
