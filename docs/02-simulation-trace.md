# 02: Simulation Trace

## Overview

This unit adds transaction execution to the fork simulator and defines the `SimulationTrace` struct — the core data object that every checker consumes. Given a transaction and forked state, this module executes the transaction in LiteSVM and captures balance deltas, the CPI call graph, account changes, oracle reads, program logs, and token approvals. This is the Tuesday deliverable of Week 1.

> Authoritative reference: see [Section 3.1](../ciel-technical-spec.md#31-engine-choice-surfpool-litesvm) (execution), [Section 4.1](../ciel-technical-spec.md#41-checker-plugin-interface) (CheckerContext/SimulationTrace usage), and [Section 1.5](../ciel-technical-spec.md#15-latency-budget) (20ms P50 target for simulation).

## Technical Specifications

- **SimulationTrace struct**: balance deltas, CPI call graph, account changes, logs, oracle reads, token approvals. See [Section 4.1](../ciel-technical-spec.md#41-checker-plugin-interface) — the `CheckerContext.trace` field.
- **Execution target**: single transaction execution via LiteSVM's `send_transaction`. See [Section 3.1](../ciel-technical-spec.md#31-engine-choice-surfpool-litesvm).
- **Latency target**: 20ms P50 for simulation of a single tx. See [Section 1.5](../ciel-technical-spec.md#15-latency-budget).
- **Account pre-loading**: all accounts referenced by the transaction must be loaded before execution. See [Section 3.2](../ciel-technical-spec.md#32-account-hot-swap-strategy).

## Key Capabilities

- [ ] Execute a real mainnet transaction against forked state and capture the result — verified by executing a known simple transfer
- [ ] Produce a SimulationTrace with balance deltas for every account modified — verified by asserting SOL balance changes on a transfer
- [ ] Capture the CPI call graph (program invocations with depth) — verified by executing a swap through Jupiter and confirming inner program calls appear
- [ ] Load the Drift exploit fixture accounts and execute the exploit tx — verified by confirming simulation succeeds (accounts exist, tx executes)
- [ ] Measure simulation latency under 40ms P95 — verified with a timing harness

## Implementation Guide

1. **Define `SimulationTrace`**: struct with fields for balance_deltas, cpi_graph, account_changes, logs, oracle_reads, token_approvals
2. **Implement `execute_transaction`**: takes a ForkSimulator + Transaction, pre-loads all accounts, executes via LiteSVM, diffs pre/post account states to compute deltas
3. **CPI graph extraction**: parse the transaction's inner instructions / log messages to build a tree of program invocations
4. **Drift fixture test**: load the Drift fixture (from unit 00), execute the exploit tx, confirm it produces a non-empty trace

**Key gotchas**:
- Pre/post account state diff must happen atomically — snapshot all accounts before execution, compare after
- LiteSVM may not emit CPI trace data in the same format as mainnet; may need to parse compute budget logs
- The transaction may fail on execution (e.g., the exploit might revert in simulation) — capture the failure reason as part of the trace

**Files / modules to create**:
- `crates/ciel-fork/src/trace.rs` — SimulationTrace struct and builder
- `crates/ciel-fork/src/executor.rs` — execute_transaction function
- Tests in `crates/ciel-fork/tests/`

## Dependencies

### Upstream (units this depends on)
- `00-drift-exploit-fixture` — provides the Drift transaction and accounts for verification testing
- `01-fork-simulator` — provides ForkSimulator with account loading and LiteSVM instance

### Downstream (units that depend on this)
- `05-checker-framework` — checkers consume SimulationTrace
- `06-pipeline-integration` — the pipeline calls execute_transaction

## Prompt for Claude Code

```
Implement Unit 02: Simulation Trace

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

This prompt covers ONE unit. Do not implement adjacent units. Do not restate or modify the technical spec.

Required reading before you write any code
Read this unit doc first: ./docs/02-simulation-trace.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 3.1 (Engine Choice): how LiteSVM executes transactions, the code example with send_transaction
- Section 4.1 (Checker Plugin Interface): the CheckerContext struct that contains SimulationTrace — this defines what fields the trace must have
- Section 1.5 (Latency Budget): 20ms P50 target for transaction simulation
- Section 3.2 (Account Hot-Swap Strategy): accounts must be pre-loaded before execution

Also read these unit docs for upstream dependencies:
- ./docs/00-drift-exploit-fixture.md — the fixture you'll use for testing
- ./docs/01-fork-simulator.md — the ForkSimulator API you'll call

Scope: what to build
The SimulationTrace struct and the execute_transaction function that runs a transaction in the fork and captures all state changes.

In scope:
- SimulationTrace struct definition with: balance_deltas (HashMap<Pubkey, i64>), cpi_graph (Vec<CpiCall>), account_changes (Vec<AccountChange>), logs (Vec<String>), oracle_reads (Vec<OracleRead>), token_approvals (Vec<TokenApproval>)
- execute_transaction(fork: &mut ForkSimulator, tx: &Transaction) -> Result<SimulationTrace> function
- Pre/post account state diffing to compute balance deltas
- CPI call graph extraction from transaction execution results
- Integration test using the Drift exploit fixture

Out of scope (these belong to other units):
- Checker logic that consumes SimulationTrace — owned by ./docs/05-checker-framework.md and individual checker units
- Geyser account streaming — owned by ./docs/03-geyser-subscriber.md
- The full verdict pipeline — owned by ./docs/06-pipeline-integration.md

Implementation constraints
- Language: Rust
- Libraries: litesvm, solana-sdk, solana-transaction-status (for parsing)
- File location: crates/ciel-fork/src/trace.rs and crates/ciel-fork/src/executor.rs
- SimulationTrace must derive Serialize, Deserialize, Clone
- All account pre-loading should use the ForkSimulator.load_account API from unit 01

Verification steps
Before declaring this unit complete, run and report results for every step:
1. Run `cargo test --package ciel-fork` and confirm all tests pass
2. Execute a simple SOL transfer in the fork and verify balance_deltas shows sender decrease and recipient increase
3. Load the Drift exploit fixture, execute the transaction, and confirm the trace is non-empty (balance deltas exist, CPI graph has entries)
4. Measure execution time for 10 runs; confirm P50 < 40ms (relaxed target for early development)

What to report when finished
- List of files created or modified with path
- Test results (pass/fail counts)
- SimulationTrace field list as implemented (may differ from spec if LiteSVM output format requires adaptation)
- Drift fixture simulation result: success/failure and trace summary
- Estimated next unit to build: 03-geyser-subscriber (or 04-attestation-signer if working in parallel)

What NOT to do
- Do not implement checker logic
- Do not implement the API server
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
- Do not declare the unit complete if any test fails
```
