# Ciel

**Pre-execution verdict layer for Solana.**

Ciel evaluates transactions *before* they land on-chain. It forks mainnet, simulates the transaction, runs 7 deterministic safety checkers in parallel, and returns a signed Ed25519 attestation verifiable by Solana's native precompile — all in under 200ms.

Two modes. One pipeline. Cryptographic proof at the end.

---

## The Problem

On April 1, 2026, the Drift protocol lost user funds through a series of admin transactions that *looked routine to every human who reviewed them*. The Squads multisig signers approved. The transactions executed. The exploit was invisible at the transaction level because it was designed to be.

Monitoring tools caught it after the fact. Audits reviewed the code months earlier. Neither could have stopped it — the exploit was in the *execution context*, not the code or the transaction format.

Ciel operates in the gap between "the transaction is submitted" and "the transaction lands." That's where exploits actually happen.

---

## Two Modes

### Defensive Mode — "Is this transaction safe?"

You already have a transaction. You want to know if it's going to do something unexpected.

```python
tx = build_swap_tx(input="USDC", amount=10_000, output="SOL")

verdict = await ciel.evaluate(tx)

if verdict.verdict == "APPROVE":
    bundle = [
        ciel.build_attestation_verify_ix(verdict),
        tx,
        jito_tip_ix(),
    ]
    await jito.send_bundle(bundle)

elif verdict.verdict == "BLOCK":
    log.warn(f"Ciel blocked: {verdict.rationale}")
```

Ciel forks mainnet, simulates your transaction, and runs 7 checkers against the simulation trace: oracle deviation, authority changes, intent mismatch, protocol contagion, MEV/sandwich detection, approval abuse, and simulation spoofing.

The signed attestation is 132 bytes. It verifies on-chain via the Ed25519 precompile at zero compute units. If anything changes between verdict and execution, the on-chain guard reverts the transaction atomically.

### Intent Mode — "Here's what I want; figure out the safest way to do it."

You have a goal but haven't decided how to execute it.

```python
intent = "Swap 10,000 USDC for SOL. Minimize slippage. MEV-protected."

verdict = await ciel.evaluate_intent(intent)

if verdict.verdict == "APPROVE":
    bundle = [
        ciel.build_attestation_verify_ix(verdict),
        verdict.winning_tx,
        jito_tip_ix(),
    ]
    await jito.send_bundle(bundle)
```

Ciel compiles your intent into structured constraints, generates multiple candidate execution paths (via Jupiter, direct DEX routes, etc.), simulates *all* of them against forked mainnet, scores each for both safety and optimality, and returns the best safe path as a ready-to-sign transaction.

The attestation covers both the safety score and the optimality score. One API call replaces separate routing + safety tooling.

---

## Who Uses This

### Autonomous Agents

An agent running 24/7 — yield optimizer, treasury rebalancer, arbitrage searcher — wraps its transaction submission with one line: `await ciel.evaluate(tx)`. When the next exploit propagates, the agent stops submitting to the compromised protocol within the same minute. The agent's developer wrote no exploit-detection logic.

**Cost**: $0.002 per evaluation via x402 micropayment. No API key needed.

### Protocol Treasuries on Squads

A DAO treasury adds Ciel as a member of its Squads multisig. Existing workflow continues unchanged — human signers propose and approve transactions in the Squads UI. Ciel evaluates each transaction before adding its approval. If unsafe, the multisig blocks. Humans can override via the Squads time-lock mechanism (24h delay).

The Drift exploit transactions would have been caught by the authority diff and intent diff checkers. The 24h override window would have given the team time to investigate.

**Cost**: ~$2,500/month SaaS subscription. Unlimited verdicts.

### DeFi Protocols

A lending market, DEX, or yield protocol wraps its frontend transaction builder with Ciel + Lighthouse guard instructions. Users see the same wallet popup, same approve button, same UX. Behind the scenes, every transaction is pre-evaluated, and the on-chain guard reverts atomically if the execution state doesn't match the attestation. Invisible to users, automatic for the protocol.

**Cost**: Per-verdict or SaaS tier depending on volume.

---

## How It Works

```
                   ┌──────────────────────────────────────────────┐
                   │            Rust Process (< 200ms)            │
Internet --> x402  │  API Server --> Fork Sim --> 7 Checkers -->  │
  Gateway   (TS)   │  Scorer --> Signer --> Signed Attestation    │
  (proxy)  ------> │                                              │
                   │  Background: Geyser Subscriber (live state)  │
                   └──────────────────────┬───────────────────────┘
                                          │
                                     PostgreSQL
                                   (verdict log)
```

**Fork Simulator** — Forks mainnet state via LiteSVM with Helius LaserStream keeping the cache warm. Simulates the transaction and captures a complete trace: balance deltas, CPI call graph, account changes, oracle reads, token approvals.

**7 Checkers** (parallel, 80ms deadline) — Oracle Sanity, Authority Diff, Intent Diff, Contagion Map, MEV/Sandwich, Approval Abuse, Sim-Spoof Detection. Each is deterministic: same input, same output, always.

**Scorer** — Penalty-based safety score from checker results. Critical finding = immediate BLOCK. Score + verdict thresholds produce APPROVE / WARN / BLOCK.

**Signer** — Ed25519 attestation (132 bytes Borsh). Contains: tx_hash, verdict, safety_score, optimality_score, checker_outputs_hash, slot, expiry (2 slots), signer pubkey, timestamp.

**On-chain Enforcement** — Three paths:
- **Lighthouse**: Guard instructions assert post-execution state matches attestation
- **Squads**: Ciel as multisig member, time-lock override for human override
- **Jito**: Attestation as bundle precondition — invalid attestation = bundle dropped

---

## Quick Start

```bash
# Clone and build
git clone https://github.com/[org]/ciel.git && cd ciel
cp .env.example .env  # Fill in API keys

# Build (macOS: see CLAUDE.md for C++ header setup)
cargo build

# Run tests
cargo test

# The most important test: Drift exploit replay
cargo test --package ciel-pipeline drift_fixture
```

### Integration (Agent SDK)

```typescript
import { CielClient } from "@ciel/sdk";

const ciel = new CielClient("https://api.ciel.dev");

// Defensive mode
const verdict = await ciel.evaluate(transaction);

// Intent mode
const verdict = await ciel.evaluateIntent("Swap 10K USDC to SOL, minimize slippage");

// MCP tool (for LLM-powered agents)
// Ciel appears as a tool the agent can call directly
```

---

## Project Structure

```
crates/
  ciel-fork/          Fork simulator, account cache, geyser subscriber
  ciel-signer/        Ed25519 attestation signing (132-byte CielAttestation)
  ciel-checkers/      Checker trait, 7 checkers, parallel runner
  ciel-pipeline/      Verdict pipeline: fork sim -> checkers -> scorer -> signer
  ciel-llm/           Groq/Fireworks async LLM client
  ciel-server/        axum REST + tonic gRPC API server
  ciel-enforcement/   Lighthouse, Squads, Jito integration
  ciel-sdk/           Rust client library
  ciel-intent/        Intent compiler, candidate generator
  ciel-demo/          Demo CLI harness
programs/
  ciel-assert/        On-chain Anchor program for attestation verification
gateway/              x402 micropayment gateway (TypeScript)
sdk/
  typescript/         TypeScript Agent SDK
  mcp-server/         MCP tool server
```

---

## The Positioning

STRIDE audits your code before deployment. SIRN coordinates response after a loss. Ciel runs on every transaction, before it lands.

Annual audit. Post-incident response. Real-time pre-execution defense.

You need all three.

---

*Colosseum Frontier Hackathon submission (April 6 -- May 11, 2026). Built by Abraham Arome Onoja.*
