# 01: Fork Simulator

## Overview

This unit integrates Surfpool/LiteSVM as the in-process Solana fork simulator. It establishes the ability to load mainnet account state into a local SVM instance and provides the foundation for all transaction simulation. This is the "load-bearing wall" of the entire system — every subsequent unit depends on reliable mainnet state forking.

> Authoritative reference: see [Section 3](../ciel-technical-spec.md#3-fork-simulator) of the technical spec for full detail.

## Technical Specifications

- **Engine**: Surfpool/LiteSVM, in-process Rust library. See [Section 3.1](../ciel-technical-spec.md#31-engine-choice-surfpool-litesvm).
- **Account loading**: Lazy fetch from Helius RPC on first access, cached locally. See [Section 3.2](../ciel-technical-spec.md#32-account-hot-swap-strategy).
- **State parity**: Slot pinning, blockhash pinning, Clock sysvar mirroring. See [Section 3.3](../ciel-technical-spec.md#33-state-parity-guarantees).
- **RPC config**: Helius primary, Triton One fallback, 100ms failover. See [Section 3.5](../ciel-technical-spec.md#35-rpc-provider-configuration).
- **Anti-spoofing**: Real sysvars loaded from mainnet. See [Section 3.6](../ciel-technical-spec.md#36-anti-sandbox-detection-v1).

## Key Capabilities

- [ ] Load a mainnet account into LiteSVM by pubkey via Helius RPC — verified by fetching a known account (e.g., USDC mint) and reading its data
- [ ] Cache loaded accounts in a DashMap for reuse across verdicts — verified by loading same account twice, confirming second load hits cache
- [ ] Set the fork's Clock sysvar to match mainnet — verified by reading Clock in the SVM and comparing to RPC
- [ ] Failover to Triton One when Helius times out (100ms) — verified by blocking the primary endpoint and confirming fallback fires
- [ ] Load all sysvar accounts from mainnet for anti-spoofing — verified by reading SlotHashes sysvar in the fork

## Implementation Guide

1. **Set up the Rust crate**: Create `crates/ciel-fork/` with LiteSVM as a dependency
2. **Implement `ForkSimulator` struct**: wraps LiteSVM, holds the account cache (DashMap), RPC client config
3. **Implement `load_account`**: async function that checks cache first, then fetches from primary RPC, falls back to secondary on timeout
4. **Implement `initialize_sysvars`**: loads Clock, SlotHashes, Rent, and other sysvars from mainnet into the SVM
5. **Write integration tests**: test account loading from Helius devnet (or mainnet with rate-limit awareness)

**Key gotchas**:
- LiteSVM's `set_account` API is the critical method — verify it exists and accepts arbitrary account data (this is Open Question #1 in Section 20)
- The DashMap cache must be thread-safe since verdicts may be processed concurrently
- Sysvars must be loaded BEFORE any transaction simulation, not lazily

**Files / modules to create**:
- `crates/ciel-fork/Cargo.toml`
- `crates/ciel-fork/src/lib.rs` — public API
- `crates/ciel-fork/src/simulator.rs` — ForkSimulator struct
- `crates/ciel-fork/src/cache.rs` — DashMap account cache
- `crates/ciel-fork/src/rpc.rs` — primary/fallback RPC client with failover

## Dependencies

### Upstream (units this depends on)
None.

### Downstream (units that depend on this)
- `02-simulation-trace` — uses ForkSimulator to execute transactions
- `03-geyser-subscriber` — feeds account updates into ForkSimulator's cache

## Prompt for Claude Code

```
Implement Unit 01: Fork Simulator

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

This prompt covers ONE unit. Do not implement adjacent units. Do not restate or modify the technical spec.

Required reading before you write any code
Read this unit doc first: ./docs/01-fork-simulator.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 3.1 (Engine Choice: Surfpool/LiteSVM): the forking engine and its API
- Section 3.2 (Account Hot-Swap Strategy): cache architecture, DashMap, monitored account set
- Section 3.3 (State Parity Guarantees): slot pinning, blockhash pinning, Clock sysvar
- Section 3.5 (RPC Provider Configuration): Helius primary, Triton One fallback, failover logic
- Section 3.6 (Anti-Sandbox-Detection): sysvar loading requirements
- Section 2.1 (Technology Stack): versions and crates for LiteSVM, Helius, tokio, DashMap

No upstream unit docs to read — this unit has no dependencies.

Scope: what to build
The ForkSimulator module that wraps LiteSVM with account caching, RPC failover, and sysvar initialization.

In scope:
- Rust crate at crates/ciel-fork/
- ForkSimulator struct wrapping LiteSVM with a DashMap<Pubkey, TimestampedAccount> cache
- Async account loading: cache → primary RPC (Helius) → fallback RPC (Triton One) with 100ms timeout
- Sysvar initialization: load Clock, SlotHashes, Rent, RecentBlockhashes from mainnet
- Circuit breaker for RPC failover (5 failures in 10s → open for 30s)
- Unit tests for cache hit/miss, failover, and sysvar loading

Out of scope (these belong to other units):
- Transaction execution and SimulationTrace capture — owned by ./docs/02-simulation-trace.md
- Geyser streaming subscriber — owned by ./docs/03-geyser-subscriber.md
- Checker logic — owned by ./docs/05-checker-framework.md

Implementation constraints
- Language: Rust
- Libraries: litesvm, solana-sdk, solana-client, dashmap, tokio, reqwest (for RPC)
- File location: crates/ciel-fork/
- RPC endpoints should be configurable via environment variables (HELIUS_API_KEY, TRITON_API_KEY)
- All async operations must use tokio runtime

Verification steps
Before declaring this unit complete, run and report results for every step:
1. Run `cargo test --package ciel-fork` and confirm all tests pass
2. Test loads a known mainnet account (e.g., USDC mint EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v) and reads its data correctly
3. Test confirms cache hit on second load of the same account
4. Test confirms Clock sysvar is populated after initialization
5. If Helius is unavailable during testing, confirm the fallback fires and the test documents this

What to report when finished
- List of files created or modified with path
- Test results (pass/fail counts)
- Any deviations from the technical spec, with justification
- Whether LiteSVM's set_account API works as expected (resolves Open Question #1)
- Estimated next unit to build: 02-simulation-trace

What NOT to do
- Do not implement transaction execution or trace capture (that is unit 02)
- Do not implement the Geyser subscriber (that is unit 03)
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
- Do not declare the unit complete if any test fails
```
