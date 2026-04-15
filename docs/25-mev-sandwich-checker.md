# 25: MEV/Sandwich Checker

## Overview

This unit implements the MEV/Sandwich checker, which analyzes DEX swap instructions for sandwich vulnerability. It checks slippage tolerance, swap amount, and whether Jito bundle protection is present.

> Authoritative reference: see [Section 4.3.5](../ciel-technical-spec.md#435-mevsandwich-checker) of the technical spec for the full algorithm and output schema.

## Technical Specifications

- **Algorithm**: Parse swap instructions, check slippage > 2%, check amount > $10K without MEV protection. See [Section 4.3.5](../ciel-technical-spec.md#435-mevsandwich-checker).
- **DEXes**: Jupiter, Raydium, Orca. See [Section 4.3.5](../ciel-technical-spec.md#435-mevsandwich-checker).

## Key Capabilities

- [ ] Detect high slippage (>2%) on DEX swaps — verified with a 5% slippage test case
- [ ] Flag high-value swaps (>$10K) without Jito protection — verified with a $15K swap
- [ ] Pass when Jito bundle protection is present — verified with bundle context set to true

## Implementation Guide

1. **Parse Jupiter/Raydium/Orca swap instructions** from the SimulationTrace
2. **Extract slippage tolerance and amount** from instruction data
3. **Check BundleContext** for Jito protection flag — **Note**: `BundleContext` is not defined in the spec's `CheckerContext` struct (Section 4.1). You will need to add an `Option<BundleContext>` field to `CheckerContext` in `crates/ciel-checkers/src/traits.rs`. Define `BundleContext` as `{ is_jito_bundle: bool }`. This is a cross-unit type change — coordinate with the checker framework.

**Files / modules to create**:
- `crates/ciel-checkers/src/mev_sandwich.rs`

## Dependencies

### Upstream (units this depends on)
- `05-checker-framework` — provides the Checker trait

### Downstream (units that depend on this)
- `15-scorer` — consumes this checker's output

## Prompt for Claude Code

```
Implement Unit 25: MEV/Sandwich Checker

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

This prompt covers ONE unit. Do not implement adjacent units. Do not restate or modify the technical spec.

Required reading before you write any code
Read this unit doc first: ./docs/25-mev-sandwich-checker.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 4.3.5 (MEV/Sandwich Checker): the full algorithm — parse DEX swap instructions, check slippage tolerance vs 2% threshold, check swap amount vs $10K threshold, check Jito bundle context, output schema with flag code HIGH_SLIPPAGE_NO_MEV_PROTECTION
- Section 4.1 (Checker Plugin Interface): Checker trait, CheckerContext (contains BundleContext for Jito detection)

Also read these unit docs for upstream dependencies:
- ./docs/05-checker-framework.md — the Checker trait and CheckerOutput types to implement

Scope: what to build
In scope:
- MevSandwichChecker struct implementing the Checker trait
- DEX instruction parsers for Jupiter, Raydium, and Orca swap instructions (extract slippage_bps and amount from instruction data)
- Slippage threshold check (configurable, default 2%)
- High-value swap detection ($10K+ without MEV protection)
- BundleContext check for Jito protection
- Unit tests per Section 4.3.5: high slippage + high value + no Jito (flag Medium), same with Jito (pass), low slippage (pass)

Out of scope (these belong to other units):
- Jito bundle submission — owned by ./docs/23-jito-integration.md
- Other checkers — owned by sibling unit docs

Implementation constraints
- Language: Rust
- Libraries: solana-sdk, spl-token (for amount parsing)
- File location: crates/ciel-checkers/src/mev_sandwich.rs
- Slippage threshold must be configurable (MEV_SLIPPAGE_THRESHOLD_PCT, default 2.0)
- Amount threshold must be configurable (MEV_AMOUNT_THRESHOLD_USD, default 10000.0)
- Price conversion for USD amount estimation can use the oracle cache

Verification steps
Before declaring this unit complete, run and report results for every step:
1. Run `cargo test --package ciel-checkers` and confirm all mev_sandwich tests pass
2. Jupiter swap with 5% slippage, $15K amount, no Jito → assert passed: false, severity: Medium, flag HIGH_SLIPPAGE_NO_MEV_PROTECTION
3. Same swap with Jito bundle context → assert passed: true
4. Jupiter swap with 0.5% slippage, $50K amount → assert passed: true (slippage is within threshold)
5. Low-value swap ($500) with high slippage (10%) → assert passed: true or Low severity (not High — small amount limits sandwich profitability)

What to report when finished
- List of files created or modified with path
- Test results (pass/fail counts)
- Which DEX instruction formats you were able to parse (Jupiter, Raydium, Orca)
- Estimated next unit to build: 26-override-mechanism

What NOT to do
- Do not implement Jito bundle submission
- Do not implement other checkers
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
```
