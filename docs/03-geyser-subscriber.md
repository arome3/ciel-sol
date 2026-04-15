# 03: Geyser Subscriber

## Overview

This unit implements the Helius LaserStream gRPC subscriber that streams real-time account updates into the fork simulator's cache. It handles connection management, reconnection with exponential backoff, slot-gap detection, and gap-fill via RPC batch fetches. This is the Wednesday deliverable of Week 1 and is critical for state parity — if the geyser stream drops, the fork diverges from mainnet.

> Authoritative reference: see [Section 3.2](../ciel-technical-spec.md#32-account-hot-swap-strategy) (cache architecture), [Section 3.4](../ciel-technical-spec.md#34-geyser-reconnection-and-gap-fill-strategy) (reconnection/gap-fill), and [Section 2.1](../ciel-technical-spec.md#21-core-technologies) (Helius LaserStream).

## Technical Specifications

- **Geyser endpoint**: Helius LaserStream gRPC (Yellowstone-compatible). See [Section 3.5](../ciel-technical-spec.md#35-rpc-provider-configuration).
- **Cache target**: DashMap in the ForkSimulator. See [Section 3.2](../ciel-technical-spec.md#32-account-hot-swap-strategy).
- **Reconnection**: Exponential backoff 100ms → 5s max. See [Section 3.4](../ciel-technical-spec.md#34-geyser-reconnection-and-gap-fill-strategy).
- **Gap-fill**: getMultipleAccounts at confirmed slot on reconnect. See [Section 3.4](../ciel-technical-spec.md#34-geyser-reconnection-and-gap-fill-strategy).
- **Staleness thresholds**: 3s → WARN downgrade, 10s → TIMEOUT. See [Section 3.4](../ciel-technical-spec.md#34-geyser-reconnection-and-gap-fill-strategy).

## Key Capabilities

- [ ] Connect to Helius LaserStream gRPC and receive account updates — verified by subscribing to a known active account and receiving an update
- [ ] Write received account updates to the ForkSimulator's DashMap cache — verified by checking cache after an update arrives
- [ ] Detect gRPC stream disconnection and reconnect with exponential backoff — verified by killing the stream and confirming reconnect
- [ ] Detect slot gaps on reconnection and trigger gap-fill via getMultipleAccounts — verified by simulating a gap scenario
- [ ] Expose a staleness metric (slots behind mainnet) — verified by reading the metric after a disconnect

## Implementation Guide

1. **Create the subscriber module**: Rust async task using tonic gRPC client to connect to LaserStream
2. **Implement account filter**: subscribe to specific account pubkeys (monitored set from Section 3.2)
3. **Implement cache writer**: on each update, write to the shared DashMap cache with a timestamp
4. **Implement reconnection loop**: on stream error/EOF, backoff and reconnect; track last-received slot
5. **Implement gap-fill**: on reconnection, compare last-known slot to current slot; if gap > 1, fetch all monitored accounts via RPC

**Key gotchas**:
- The gRPC subscription filter format depends on LaserStream's API — verify if it supports per-account-pubkey filters (Open Question #6 in Section 20)
- The subscriber must run as a background tokio task, not blocking the verdict pipeline
- Cache writes must use `DashMap::insert` which is lock-free for concurrent readers

**Files / modules to create**:
- `crates/ciel-fork/src/geyser.rs` — GeyserSubscriber struct and background task
- `crates/ciel-fork/src/staleness.rs` — staleness tracking and threshold logic

## Dependencies

### Upstream (units this depends on)
- `01-fork-simulator` — provides the DashMap cache to write into

### Downstream (units that depend on this)
- `06-pipeline-integration` — the pipeline relies on a warm cache from the geyser subscriber
- `24-contagion-map-checker` — uses real-time account data for anomaly detection

## Prompt for Claude Code

```
Implement Unit 03: Geyser Subscriber

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

This prompt covers ONE unit. Do not implement adjacent units. Do not restate or modify the technical spec.

Required reading before you write any code
Read this unit doc first: ./docs/03-geyser-subscriber.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 3.2 (Account Hot-Swap Strategy): the cache architecture and monitored account set
- Section 3.4 (Geyser Reconnection and Gap-Fill Strategy): reconnection backoff, gap detection, gap-fill procedure, staleness thresholds
- Section 3.5 (RPC Provider Configuration): geyser endpoint URL and config
- Section 2.1 (Technology Stack): Helius LaserStream as Yellowstone-compatible gRPC

Also read these unit docs for upstream dependencies:
- ./docs/01-fork-simulator.md — the ForkSimulator whose DashMap cache you'll write into

Scope: what to build
A background async task that streams account updates from Helius LaserStream gRPC into the fork simulator's account cache, with reconnection and gap-fill logic.

In scope:
- GeyserSubscriber struct with connect(), run_loop(), and gap_fill() methods
- tonic gRPC client connecting to Helius LaserStream
- Account update handler writing to the shared DashMap cache
- Exponential backoff reconnection (100ms base, 5s max)
- Slot-gap detection and gap-fill via getMultipleAccounts RPC call
- Staleness tracking: expose current lag in slots, support the 3s/10s thresholds
- Unit tests with mocked gRPC stream

Out of scope (these belong to other units):
- The ForkSimulator struct and DashMap cache creation — owned by ./docs/01-fork-simulator.md
- Transaction simulation — owned by ./docs/02-simulation-trace.md
- Verdict downgrade logic on staleness — owned by ./docs/06-pipeline-integration.md

Implementation constraints
- Language: Rust
- Libraries: tonic (gRPC client), tokio (async runtime, spawn for background task), dashmap
- File location: crates/ciel-fork/src/geyser.rs
- The subscriber must run as a spawned tokio task that lives for the process lifetime
- Environment variable: HELIUS_LASERSTREAM_URL for the gRPC endpoint

Verification steps
Before declaring this unit complete, run and report results for every step:
1. Run `cargo test --package ciel-fork` and confirm all tests pass
2. If Helius LaserStream is available: test connects, receives at least one account update, and writes it to the cache
3. If LaserStream is not available: test the reconnection loop with a mock gRPC server that drops the connection, confirm exponential backoff fires
4. Test gap-fill: simulate a scenario where last_slot = 100, current_slot = 105, confirm getMultipleAccounts is called for all monitored accounts
5. Confirm staleness metric reflects actual gap in slots

What to report when finished
- List of files created or modified with path
- Test results (pass/fail counts)
- Whether LaserStream supports per-account-pubkey subscription filters (resolves Open Question #6)
- Any deviations from the technical spec, with justification
- Estimated next unit to build: 04-attestation-signer

What NOT to do
- Do not implement transaction simulation or checker logic
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
- Do not declare the unit complete if any test fails
```
