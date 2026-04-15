# Ciel Technical Specification v1.0

**Pre-execution verdict layer for Solana.**

Author: Abraham Arome Onoja
Date: April 13, 2026
Status: Implementation-ready for Colosseum Frontier Hackathon (Week 1 start: April 14)
Companion doc: `ciel_product_spec.md`

---

## 1. System Overview

### 1.1 Architecture Diagram

```mermaid
graph TD
    A[Client: Agent / Protocol / Wallet] -->|RawTx or Intent| B[API Server - Rust axum]
    B --> C{Input Type?}
    C -->|RawTx base64| D[Fork Simulator - Surfpool/LiteSVM]
    C -->|Structured Intent JSON| D
    C -->|Natural Language| E[LLM Intent Compiler - Groq Llama 3.x 8B]
    E -->|Structured Intent| F[Candidate Generator]
    F -->|N candidate txs| D
    D -->|SimulationTrace per tx| G[Checker Fan-Out - tokio::join_all]
    G --> G1[Oracle Sanity]
    G --> G2[Authority Diff]
    G --> G3[Intent Diff]
    G --> G4[Contagion Map]
    G --> G5[MEV/Sandwich]
    G --> G6[Approval Abuse]
    G --> G7[Sim-Spoof Detection]
    G1 & G2 & G3 & G4 & G5 & G6 & G7 -->|CheckerOutput[]| H[Scorer]
    H -->|safety_score, optimality_score| I{Verdict}
    I -->|scores + checker_outputs| J[LLM Rationale Aggregator - Groq]
    J -->|rationale string metadata| K[Ed25519 Signer]
    I -->|verdict + scores| K
    K -->|SignedAttestation| L[Response to Client]
    L --> M1[Lighthouse Guard Ix]
    L --> M2[Squads Policy Gate]
    L --> M3[Jito Bundle Precondition]
    K -->|verdict log| N[Append-Only Verdict Store - Postgres]
```

### 1.2 Component Inventory

| Component | Responsibility | Language | Process |
|-----------|---------------|----------|---------|
| **API Server** | Accept verdict requests (gRPC + REST), route to pipeline, return attestations | Rust (axum + tonic) | Primary |
| **Fork Simulator** | Fork mainnet state, execute candidate tx, capture trace | Rust (Surfpool/LiteSVM) | Primary (in-process) |
| **Checker Framework** | Fan-out 7 checkers in parallel, collect results within deadline | Rust (tokio) | Primary |
| **7 Checkers** | Deterministic analysis of simulation trace | Rust | Primary |
| **Scorer** | Compute safety_score, optimality_score, verdict | Rust | Primary |
| **LLM Client** | Intent compilation, rationale aggregation | Rust (reqwest async HTTP) | Primary |
| **Signer** | Ed25519 sign attestation payload | Rust (ed25519-dalek) | Primary |
| **Geyser Subscriber** | Stream account updates from Helius LaserStream gRPC | Rust (tonic gRPC client) | Background task |
| **Verdict Store** | Append-only log of all verdicts + outcomes | PostgreSQL 16 | Sidecar |
| **Agent SDK** | Client library + MCP tool server | TypeScript + Rust | Separate package |
| **x402 Middleware** | Payment verification for per-verdict micropayments | TypeScript (Express middleware) | Gateway |

### 1.3 Process Boundary Decision: Option A — Single Rust Process

All latency-critical components run in a single Rust process. TypeScript is used only for the Agent SDK client library, the MCP tool server wrapper, and the x402 payment gateway (which sits in front of the Rust API server as a reverse proxy).

```
                   ┌─────────────────────────────────────────────┐
                   │           Rust Process (primary)             │
Internet ──► x402  │  API Server ──► Fork Sim ──► Checkers ──►   │
  Gateway   (TS)   │  Scorer ──► LLM Client ──► Signer           │
  (proxy)  ──────► │                                              │
                   │  Background: Geyser Subscriber               │
                   └──────────────────────┬──────────────────────┘
                                          │
                                     PostgreSQL
```

**Latency consequence**: 0 IPC crossings in the verdict hot path. The x402 gateway adds ~1-2ms for payment verification (HTTP proxy hop), but this is outside the 200ms verdict pipeline measurement. LLM calls are external HTTP to Groq API — not IPC.

**Deployment**: Single Docker container for the Rust process + sidecar Postgres. x402 gateway runs as a separate container in the same pod.

### 1.4 Data Flows

**Flow A: Raw Transaction (defensive mode)**
```
Client POST /v1/verdict { tx: base64 }
  → Deserialize tx
  → Fork Simulator: execute tx against forked mainnet state
  → 7 Checkers run in parallel (80ms deadline)
  → Scorer computes safety_score → verdict
  → LLM generates rationale string (async, non-blocking if timeout)
  → Signer: Ed25519 sign (tx_hash, verdict, safety_score, checker_outputs_hash, slot, expiry)
  → Return SignedAttestation to client
  → Log verdict to Postgres
```

**Flow B: Structured Intent**
```
Client POST /v1/verdict { intent: { goal, constraints, budget, deadline } }
  → Candidate Generator: produce N candidate execution plans
  → For each candidate: Fork Sim → Checkers → Scorer
  → Rank by final_score = optimality_score × safety_multiplier
  → Select winner (highest final_score where safety passes)
  → Sign attestation over winning candidate
  → Return SignedAttestation + winning tx
```

**Flow C: Natural Language Intent**
```
Client POST /v1/verdict { nl_intent: "swap 10k USDC to SOL, minimize slippage" }
  → LLM Intent Compiler: NL → structured intent JSON (~40ms)
  → [continues as Flow B]
```

### 1.5 Latency Budget

| Stage | P50 Target | P95 Target | Notes |
|-------|-----------|-----------|-------|
| API parsing + deserialization | 1ms | 2ms | |
| Fork state hot-swap (warm cache) | 15ms | 30ms | Surfpool in-process; accounts pre-cached via Geyser |
| Transaction simulation | 20ms | 40ms | Single tx execution in LiteSVM |
| Checker fan-out (7 parallel) | 50ms | 80ms | Hard deadline: 80ms. Partial results on timeout. |
| Scorer | 2ms | 3ms | Arithmetic only |
| LLM rationale aggregation | 60ms | 120ms | Groq Llama 3.x 8B, ~150 output tokens |
| Ed25519 signing | 1ms | 2ms | ed25519-dalek in-process |
| Serialization + response | 1ms | 2ms | |
| **Total (raw tx mode)** | **~150ms** | **~280ms** | |
| **Total (intent mode, per candidate)** | **~150ms** | **~280ms** | ×N candidates; parallel |
| LLM intent compilation (NL only) | 40ms | 80ms | Prepended to intent flow |

**Note**: The product spec targets P50 ≤ 200ms. The budget above achieves ~150ms P50 for the happy path. The LLM rationale aggregation is the most variable component; it runs concurrently with signing when possible. If the LLM times out, the verdict is still returned with `rationale: null` — rationale is metadata, not part of the signed payload.

---

## 2. Technology Stack and Rationale

### 2.1 Core Technologies

| Technology | Version | Role | Why Chosen | Alternatives Evaluated | Citation |
|-----------|---------|------|------------|----------------------|---------|
| **Rust** | 1.78+ | Primary language | Zero-cost abstractions, Solana-native ecosystem, in-process SVM embedding | TypeScript (too slow for simulation), Go (poor Solana SDK support) | — |
| **Surfpool** | Latest (2026) | Fork simulator engine | Lazy mainnet forking, cheatcodes (`surfnet_setAccount`), full RPC compat, in-process via LiteSVM | solana-test-validator (subprocess overhead, no cheatcodes), raw LiteSVM (no mainnet fork support) | [Helius blog](https://www.helius.dev/blog/surfpool), [Solana docs](https://solana.com/docs/intro/installation/surfpool-cli-basics), [GitHub](https://github.com/txtx/surfpool) |
| **LiteSVM** | Latest | Underlying SVM for Surfpool | In-process Solana VM, fast simulation, fine-grained account control | solana-program-test (slower, less control), BanksClient (limited) | [GitHub](https://github.com/LiteSVM/litesvm), [crates.io](https://crates.io/crates/litesvm), [Anchor docs](https://www.anchor-lang.com/docs/testing/litesvm) |
| **Helius** | Professional plan | Primary RPC + Geyser | LaserStream gRPC (Yellowstone-compatible), 500 req/s, enhanced APIs, auto-reconnect | Triton One (no LaserStream), QuickNode (higher cost), Shyft (smaller scale) | [Helius pricing](https://www.helius.dev/docs/billing/plans), [Helius docs](https://www.helius.dev) |
| **Triton One** | Public RPC | Fallback RPC | Free tier public RPC, acceptable for failover, different infrastructure from Helius | QuickNode (paid), Chainstack (less Solana focus) | [Triton docs](https://docs.triton.one) |
| **Groq** | LPU API | LLM inference (primary) | Sub-100ms TTFT for 8B models, ~$0.05/M input tokens, JSON mode support | Cerebras (less battle-tested API), Fireworks (3-5x slower), SambaNova (too slow for 8B) | [Groq](https://console.groq.com), [Benchmark comparison](https://speko.ai/benchmark/groq-vs-cerebras) |
| **Fireworks AI** | API | LLM inference (fallback) | Best structured output (grammar-constrained decoding), reliable, acceptable latency for fallback | Together AI (similar profile), Cerebras (less structured output maturity) | [Fireworks](https://fireworks.ai) |
| **ed25519-dalek** | 2.x | Ed25519 signing | Standard Rust Ed25519 library, compatible with Solana's Ed25519SigVerify precompile | ring (less Solana ecosystem integration), sodiumoxide (C dependency) | [crates.io](https://crates.io/crates/ed25519-dalek) |
| **axum** | 0.7+ | HTTP/gRPC API server | Tokio-native, high performance, tower middleware ecosystem | actix-web (less tokio integration), warp (less mature) | [GitHub](https://github.com/tokio-rs/axum) |
| **tonic** | 0.12+ | gRPC server + Geyser client | Rust-native gRPC, pairs with axum | grpc-rs (C++ dependency) | [GitHub](https://github.com/hyperium/tonic) |
| **PostgreSQL** | 16 | Verdict log store | ACID, append-only writes, JSON columns for checker outputs, mature | ClickHouse (overkill for v1 volume), DuckDB (no concurrent access), Parquet/S3 (no real-time queries) | [PostgreSQL](https://www.postgresql.org) |
| **Borsh** | 1.x | Attestation serialization | Solana-native, deterministic, Anchor-compatible, zero-copy deserialization | SSZ (Ethereum-native, no Solana ecosystem), bincode (not deterministic), MessagePack (no Solana precedent) | [Borsh](https://borsh.io), [crates.io](https://crates.io/crates/borsh) |
| **Switchboard On-Demand** | Latest | Oracle data (primary) | Pull-based feeds, std_dev for deviation detection, Solana-native | Pyth-only (single source risk) | [Switchboard docs](https://docs.switchboard.xyz), [GitHub](https://github.com/switchboard-xyz/on-demand), [crates.io](https://docs.rs/switchboard-on-demand) |
| **Pyth Lazer** | Latest | Oracle data (cross-reference) | Sub-millisecond updates, 15K CU, confidence intervals | Pyth Pull (higher latency via Wormhole), Chainlink (not on Solana natively) | [Pyth Lazer blog](https://www.pyth.network/blog/introducing-pyth-lazer-launching-defi-into-real-time), [Pyth docs](https://docs.pyth.network) |
| **Lighthouse** | v2.0.0 | On-chain assertion enforcement | State-change assertions, low CU, transaction-level guards | Custom program (more dev work), none (weaker enforcement) | [GitHub](https://github.com/Jac0xb/lighthouse), [lighthouse.voyage](https://www.lighthouse.voyage) |
| **Squads v4** | Latest | Multisig treasury enforcement | Time locks, spending limits, $10B+ secured, devnet available | Realms (governance, not operational policy), custom multisig (no ecosystem) | [GitHub](https://github.com/Squads-Protocol/v4), [Squads docs](https://docs.squads.so), [npm](https://www.npmjs.com/package/@squads-protocol/multisig) |
| **Jito Block Engine** | Latest | Bundle enforcement + MEV protection | Atomic bundles (fail = drop entire bundle), ~90% validator coverage, gRPC API | Direct validator submission (no atomicity guarantee) | [Jito docs](https://docs.jito.wtf), [GitHub](https://github.com/jito-labs/mev-protos) |
| **x402** | 1.x spec | Per-verdict micropayments | HTTP 402-native, Solana SDKs available, validated by multiple hackathon winners | Custom payment (non-standard), Stripe (too slow for per-call), Lightning (wrong chain) | [Solana x402](https://solana.com/x402/what-is-x402), [x402 toolkit](https://github.com/BOBER3r/x402-solana-toolkit) |
| **MCP** | Spec 2025-11-25 | Agent SDK protocol | 97M installs, universal agent compatibility, JSON-RPC 2.0, Linux Foundation governance | Custom REST SDK (no ecosystem), gRPC-only (agents prefer MCP) | [MCP spec](https://modelcontextprotocol.io/specification/2025-11-25), [GitHub](https://github.com/modelcontextprotocol) |

### 2.2 Tokio Async Runtime Configuration

All parallel execution uses raw `tokio` primitives. No orchestration frameworks.

```rust
// Checker fan-out pattern
use tokio::time::{timeout, Duration};
use futures::future::join_all;

let deadline = Duration::from_millis(80);
let checker_futures = vec![
    timeout(deadline, oracle_sanity.check(&trace)),
    timeout(deadline, authority_diff.check(&trace)),
    timeout(deadline, intent_diff.check(&trace)),
    timeout(deadline, contagion_map.check(&trace)),
    timeout(deadline, mev_sandwich.check(&trace)),
    timeout(deadline, approval_abuse.check(&trace)),
    timeout(deadline, sim_spoof.check(&trace)),
];
let results: Vec<Result<CheckerOutput, _>> = join_all(checker_futures).await;
// Timed-out checkers return Err(Elapsed) — handled as partial results
```

---

## 3. Fork Simulator

### 3.1 Engine Choice: Surfpool (LiteSVM)

Surfpool is the Foundry `anvil` equivalent for Solana. It provides:
- **Lazy mainnet forking**: accounts are fetched from mainnet RPC on first access, then cached locally
- **Cheatcodes**: `surfnet_setAccount` for arbitrary account state manipulation
- **Full RPC compatibility**: standard Solana RPC methods work out of the box
- **In-process execution**: runs inside the Rust process via the LiteSVM wrapper — no subprocess overhead

For Ciel's use case, Surfpool is embedded as a Rust library (via LiteSVM). The verdict pipeline does not start a CLI Surfpool instance — it uses the LiteSVM API directly for maximum performance.

```rust
use litesvm::LiteSvm;

let mut svm = LiteSvm::new();
// Load accounts from cache or RPC
for pubkey in required_accounts {
    let account = account_cache.get_or_fetch(pubkey, &rpc_client).await?;
    svm.set_account(pubkey, account)?;
}
// Execute the candidate transaction
let result = svm.send_transaction(tx)?;
// Extract: balance deltas, CPI graph, account changes, logs
```

### 3.2 Account Hot-Swap Strategy

Accounts are maintained in a warm cache backed by Helius LaserStream (geyser gRPC).

**Cache Architecture:**
```
Helius LaserStream (gRPC) ──► Account Cache (DashMap<Pubkey, TimestampedAccount>)
                                    │
                                    ▼
                              LiteSVM instance
                              (accounts loaded from cache per-verdict)
```

**Cache Population:**
1. **Bootstrap**: On startup, fetch a seed set of accounts (top DeFi programs, major token mints, oracle feeds) via `getMultipleAccounts`
2. **Streaming**: Subscribe to Helius LaserStream gRPC for real-time account updates on monitored accounts
3. **Lazy fetch**: For accounts not in cache, fetch from Helius RPC on first access per-verdict and add to cache
4. **Eviction**: LRU eviction with 5-minute TTL for accounts not in the monitored set

**Monitored Account Set (v1):**
- All Switchboard and Pyth oracle feed accounts for top 50 trading pairs
- Token program, Associated Token program, System program
- Major DeFi program accounts (Drift, Jupiter, Raydium, Orca, Marinade)
- Any account touched by a verdict request (added to monitored set for 10 minutes)

### 3.3 State Parity Guarantees

State parity is the load-bearing wall. If the fork diverges from mainnet, attestations get rejected downstream.

**Slot pinning**: Every attestation is pinned to a specific `(slot, blockhash)` tuple from the latest confirmed slot.

**Blockhash pinning**: The transaction's `recent_blockhash` in the simulation matches the mainnet blockhash at the pinned slot.

**Expiry window**: Attestations expire after 2 slots (~800ms). Enforcement contracts verify `attestation.slot >= current_confirmed_slot - 2`.

**Clock sysvar**: The fork's `Clock` sysvar is updated to match mainnet's clock at the pinned slot.

### 3.4 Geyser Reconnection and Gap-Fill Strategy

**On disconnect:**
1. Detect disconnect via gRPC stream error / EOF
2. Begin exponential backoff reconnection: 100ms, 200ms, 400ms, 800ms, max 5s
3. Track the last successfully received slot number

**Gap detection:**
- Monitor slot sequence numbers on the geyser stream
- If `received_slot - last_slot > 1`, a gap exists

**Gap-fill procedure:**
1. On reconnection, compute the gap range: `[last_known_slot + 1, current_slot]`
2. Fetch all monitored accounts via `getMultipleAccounts` at `commitment: confirmed` for the current slot
3. Compare fetched state with cached state; apply deltas
4. Resume streaming from the current slot

**Staleness threshold:**
- If the gap duration exceeds **3 seconds** (approximately 7-8 slots), all in-flight verdicts are downgraded to `WARN` with `reason: "state_parity_degraded"`
- If the gap exceeds **10 seconds**, new verdict requests receive `TIMEOUT` until the cache is refreshed
- These thresholds are configurable per deployment

### 3.5 RPC Provider Configuration

```toml
[rpc]
primary = "https://mainnet.helius-rpc.com/?api-key=HELIUS_KEY"
fallback = "https://api.triton.one/TRITON_KEY"
failover_timeout_ms = 100  # switch to fallback if primary doesn't respond within 100ms

[geyser]
endpoint = "https://laserstream.helius-rpc.com"  # Helius LaserStream gRPC
reconnect_backoff_base_ms = 100
reconnect_backoff_max_ms = 5000
staleness_warn_threshold_ms = 3000
staleness_timeout_threshold_ms = 10000
```

**Failover logic:**
1. Every RPC call uses `tokio::time::timeout(100ms, primary_call)`
2. On timeout or error, immediately retry with fallback provider
3. If both fail, return `TIMEOUT` verdict
4. Track provider health with a circuit breaker (5 failures in 10s → open circuit for 30s)

### 3.6 Anti-Sandbox-Detection (v1)

The fork exposes production-equivalent values to make sandbox detection harder:

1. **Real slot and blockhash**: The LiteSVM `Clock` sysvar is set to the actual mainnet slot and epoch. `recent_blockhash` is a real mainnet blockhash.
2. **Real validator identity**: Not applicable for LiteSVM (no validator identity concept). Programs that check `sysvar::slot_hashes` will see real mainnet data because slot hashes are loaded from the account cache.
3. **Known pattern registry**: Maintain a list of known simulation-detection patterns:
   - Programs that call `Clock::get()` and compare against expected slot ranges
   - Programs that check `rent::Rent` sysvar for non-standard values
   - Programs that probe `sysvar::slot_hashes` for empty or synthetic entries
   - Programs that check validator identity via `sysvar::slot_hashes` cross-reference
4. **Mitigation**: Pre-load all sysvar accounts from mainnet into the fork's account cache before every simulation.

**v1 limitation (documented)**: A single fork implementation. Differential execution (running the same tx against two independent fork implementations and comparing results) is v2. This is a deliberate latency-budget tradeoff.

---

## 4. Risk Graph and Checker Framework

### 4.1 Checker Plugin Interface

```rust
/// Every checker implements this trait.
#[async_trait]
pub trait Checker: Send + Sync {
    /// Unique identifier for this checker.
    fn name(&self) -> &'static str;

    /// Run the check against a simulation trace.
    /// Must be deterministic: same input → same output.
    async fn check(&self, ctx: &CheckerContext) -> CheckerOutput;
}

/// Input provided to every checker.
pub struct CheckerContext {
    pub trace: SimulationTrace,       // balance deltas, CPI graph, account changes, logs
    pub original_tx: Transaction,     // the raw transaction being evaluated
    pub intent: Option<Intent>,       // if this was an intent-mode request
    pub slot: u64,                    // mainnet slot at fork time
    pub oracle_cache: OracleCache,    // cached oracle prices for cross-reference
    pub known_programs: ProgramRegistry, // registry of known-good programs
}

/// Output from every checker. Deterministic and serializable.
#[derive(Serialize, Deserialize, BorshSerialize, BorshDeserialize, Clone)]
pub struct CheckerOutput {
    pub checker_name: String,
    pub passed: bool,                 // true = no issues found
    pub severity: Severity,           // None, Low, Medium, High, Critical
    pub flags: Vec<Flag>,             // specific findings
    pub details: String,              // human-readable explanation
}

#[derive(Serialize, Deserialize, BorshSerialize, BorshDeserialize, Clone)]
pub enum Severity { None, Low, Medium, High, Critical }

#[derive(Serialize, Deserialize, BorshSerialize, BorshDeserialize, Clone)]
pub struct Flag {
    pub code: String,                 // e.g., "ORACLE_DEVIATION_3_SIGMA"
    pub message: String,
    pub data: serde_json::Value,      // arbitrary structured data
}
```

### 4.2 Parallel Execution Model

```rust
pub async fn run_checkers(ctx: &CheckerContext, checkers: &[Box<dyn Checker>]) -> CheckerResults {
    let deadline = Duration::from_millis(80);
    let futures: Vec<_> = checkers.iter()
        .map(|c| {
            let ctx = ctx.clone();
            let name = c.name().to_string();
            async move {
                match timeout(deadline, c.check(&ctx)).await {
                    Ok(output) => (name, CheckerStatus::Completed(output)),
                    Err(_) => (name, CheckerStatus::TimedOut),
                }
            }
        })
        .collect();

    let results = join_all(futures).await;
    CheckerResults {
        outputs: results.into_iter().collect(),
        total_duration_ms: start.elapsed().as_millis() as u64,
    }
}
```

### 4.3 Checker Specifications

#### 4.3.1 Oracle Sanity Checker

**Detects**: Oracle price manipulation (Drift-class attacks).

**Algorithm**:
1. Parse the simulation trace for all account reads from known oracle programs (Switchboard, Pyth)
2. For each oracle read, fetch the independent price from the other oracle source (cross-reference)
3. Compute deviation in sigma units: `|price_a - price_b| / max(std_dev_a, confidence_b)`
4. Flag if deviation > 3 sigma (configurable threshold)
5. Also flag: reads from non-canonical oracle accounts, stale feeds (> 30s), abnormally wide confidence intervals

**Input**: SimulationTrace (account reads), OracleCache (Switchboard + Pyth prices)

**Output schema**:
```json
{
  "checker_name": "oracle_sanity",
  "passed": false,
  "severity": "Critical",
  "flags": [{
    "code": "ORACLE_DEVIATION_3_SIGMA",
    "message": "SOL/USD price deviation 4.2 sigma between Switchboard and Pyth",
    "data": {
      "asset": "SOL/USD",
      "switchboard_price": 142.50,
      "pyth_price": 138.20,
      "deviation_sigma": 4.2,
      "switchboard_std_dev": 0.45,
      "pyth_confidence": 0.38
    }
  }]
}
```

**Deterministic guarantee**: Given the same oracle cache state and simulation trace, the output is identical. The oracle cache is snapshotted at the pinned slot.

**False positives**: Legitimate price divergence during high volatility. Mitigation: use 3-sigma threshold (99.7% of normal data falls within).

**False negatives**: Attacker manipulates both oracles simultaneously. Mitigation: v2 adds a third source (off-chain API like Binance/CoinGecko).

**Unit test strategy**: Construct a simulation trace with a known-manipulated oracle read. Assert the checker flags it. Test with deviations at 2.9σ (should pass) and 3.1σ (should flag).

#### 4.3.2 Authority Diff Checker

**Detects**: Hidden authority transfers, program upgrades, or account closures in transactions disguised as routine operations.

**Algorithm**:
1. Parse the CPI call graph from the simulation trace
2. Search for instructions matching: `SetAuthority`, `Upgrade`, `CloseAccount`, `InitializeAccount` (with different authority)
3. Cross-reference with the stated intent (if available) — if the user says "deposit 100 USDC" but the tx includes a `SetAuthority`, flag it
4. Check the known-programs registry: if a `SetAuthority` targets a known protocol's upgrade authority, flag as Critical

**Input**: SimulationTrace (CPI graph), Intent (optional), ProgramRegistry

**Output schema**:
```json
{
  "checker_name": "authority_diff",
  "passed": false,
  "severity": "Critical",
  "flags": [{
    "code": "HIDDEN_SET_AUTHORITY",
    "message": "Transaction contains SetAuthority on Drift vault program, not declared in intent",
    "data": {
      "instruction_index": 3,
      "program": "dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH",
      "authority_change": { "from": "7xKp...", "to": "9yBq..." }
    }
  }]
}
```

**Deterministic**: Yes. CPI graph parsing is pure function over trace data.

**False positives**: Legitimate authority rotations. Mitigation: known-good authority rotation patterns can be whitelisted per program.

**Unit test**: Craft a tx with a `SystemProgram::Transfer` that also includes a hidden `SetAuthority` CPI. Assert the checker detects the CPI.

#### 4.3.3 Intent Diff Checker

**Detects**: Transaction outcome diverges from stated intent (requires an intent to be provided).

**Algorithm**:
1. If no intent is provided, return `passed: true` with `severity: None` (not applicable for raw tx mode without intent). This is a deterministic no-op.
2. Parse the intent's `goal` into an expected set of balance deltas using a deterministic rule table:
   - `"swap {amount} {token_A} for {token_B}"` → expect token_A balance decrease ≈ amount, token_B balance increase > 0
   - `"transfer {amount} {token} to {address}"` → expect token balance decrease ≈ amount at sender, increase at recipient
   - `"deposit {amount} {token} into {protocol}"` → expect token balance decrease ≈ amount, protocol receipt token increase > 0
   - Unrecognized goal patterns → cannot verify (see step 4)
3. Compare expected balance deltas against the simulation's actual balance deltas:
   - If all expected deltas are satisfied within a tolerance (±1% for amounts, correct token mints, correct direction): `passed: true`
   - If any expected delta is violated (wrong token, wrong direction, amount outside tolerance): `passed: false`, flag the specific mismatch
4. If the intent goal does not match any recognized pattern in the rule table (e.g., complex multi-leg intents, conditional outcomes, intents with ambiguous goals), the checker returns `passed: true` with `severity: None` and a flag of code `INTENT_VERIFICATION_INCONCLUSIVE`. This is a deterministic outcome — the checker has determined, deterministically, that it cannot verify the intent via balance-delta comparison alone. It does not attempt non-deterministic classification.

**LLM enrichment (metadata only)**: An optional LLM judge (Groq, Llama 3.x 8B) runs in parallel with the deterministic checker. It performs a semantic comparison of the intent against the simulation trace and produces a structured `{match: bool, confidence: float, explanation: string}`. This output is written to a separate `intent_diff_llm_analysis` field in the API response and logged to the verdict store. It is **never** part of the `CheckerOutput`, never influences `passed`, `severity`, or `flags`, never contributes to `checker_outputs_hash`, and never affects the verdict. It exists solely for human auditors reviewing verdicts after the fact.

**Input**: SimulationTrace, Intent (the LLM client is NOT an input to the checker — it runs as a separate parallel task outside the checker)

**Output schema (intent satisfied)**:
```json
{
  "checker_name": "intent_diff",
  "passed": true,
  "severity": "None",
  "flags": [],
  "details": "Intent 'swap 100 USDC for SOL' matches simulation: USDC -100.00, SOL +0.67"
}
```

**Output schema (intent violated)**:
```json
{
  "checker_name": "intent_diff",
  "passed": false,
  "severity": "High",
  "flags": [{
    "code": "INTENT_BALANCE_MISMATCH",
    "message": "Intent expects SOL increase but simulation shows ETH increase",
    "data": {
      "expected_token": "SOL",
      "actual_token": "ETH",
      "intent_goal": "swap 100 USDC for SOL"
    }
  }],
  "details": "Intent 'swap 100 USDC for SOL' does NOT match simulation: USDC -100.00, ETH +1.00 (wrong output token)"
}
```

**Output schema (inconclusive)**:
```json
{
  "checker_name": "intent_diff",
  "passed": true,
  "severity": "None",
  "flags": [{
    "code": "INTENT_VERIFICATION_INCONCLUSIVE",
    "message": "Intent goal does not match a recognized verifiable pattern; balance-delta comparison cannot confirm or refute",
    "data": {
      "intent_goal": "rebalance portfolio to 60% SOL 30% USDC 10% BONK with minimal market impact",
      "reason": "multi_leg_unrecognized_pattern"
    }
  }],
  "details": "Intent verification inconclusive — goal pattern not in deterministic rule table"
}
```

**Deterministic guarantee**: Fully deterministic. The checker's contribution to `checker_outputs_hash` depends only on balance-delta comparison against a fixed rule table and the deterministic `INCONCLUSIVE` classification for unrecognized patterns. The rule table is versioned and shipped with the checker code. LLM analysis is logged separately as `intent_diff_llm_analysis` and never enters the signed payload. Two independent verifiers running the same input with the same checker version will always produce identical `CheckerOutput` for Intent Diff, regardless of LLM availability.

**False positives**: Balance-delta comparison may flag a mismatch when the intent was satisfied through an indirect path (e.g., intermediate token hops in a multi-hop swap). Mitigation: the rule table accounts for common DEX aggregator patterns (Jupiter multi-hop routes produce the expected final delta even through intermediate tokens).

**False negatives**: The `INCONCLUSIVE` path means complex intents pass without verification. Mitigation: the LLM metadata enrichment provides a secondary signal for human reviewers; v2 can expand the rule table to cover more patterns.

**Unit test strategy**:
- **Mismatch case**: Intent "swap 100 USDC for SOL" with simulation showing USDC -100, ETH +1 (wrong token). Assert `passed: false`, flag code `INTENT_BALANCE_MISMATCH`.
- **Happy path**: Intent "swap 100 USDC for SOL" with simulation showing USDC -100, SOL +0.67. Assert `passed: true`, no flags.
- **Inconclusive case**: Intent "rebalance portfolio to 60/30/10 split" (unrecognized pattern). Assert `passed: true`, `severity: None`, flag code `INTENT_VERIFICATION_INCONCLUSIVE`.
- **Determinism verification**: Run the same input on two independent checker instances (one with LLM available, one without). Assert both produce identical `CheckerOutput` and identical contribution to `checker_outputs_hash`.

#### 4.3.4 Contagion Map Checker

**Detects**: The target protocol's dependencies are behaving anomalously (contagion risk from upstream exploits).

**Algorithm**:
1. Maintain a static dependency graph of major Solana protocols (hardcoded in v1, graph DB in v2):
   ```
   Drift → {Pyth, Switchboard, Jupiter, Marinade}
   Jupiter → {Raydium, Orca, Meteora, Pyth}
   Marinade → {Stake Pool, Pyth}
   ```
2. For the target protocol in the transaction, look up its dependencies
3. For each dependency, check: has there been an anomalous event in the last N slots?
   - Anomalous: authority change, large TVL movement (>10% in 1 hour), oracle deviation, program upgrade
4. If any dependency is flagged, raise a warning with the dependency chain

**Input**: SimulationTrace (target programs), DependencyGraph, RecentEventCache

**Output schema**:
```json
{
  "checker_name": "contagion_map",
  "passed": false,
  "severity": "High",
  "flags": [{
    "code": "DEPENDENCY_ANOMALY",
    "message": "Drift depends on Pyth, which reported 5-sigma oracle deviation 2 slots ago",
    "data": {
      "target_protocol": "Drift",
      "affected_dependency": "Pyth",
      "anomaly_type": "oracle_deviation",
      "slots_ago": 2
    }
  }]
}
```

**Deterministic**: Yes, given the same RecentEventCache snapshot. The cache is populated by the Geyser subscriber and snapshotted per-verdict.

**v1 limitation**: Dependency graph is hardcoded for top 20 protocols. v2 builds a dynamic graph from on-chain CPI patterns.

**Unit test**: Inject an anomaly event for Pyth into the RecentEventCache, then evaluate a Drift transaction. Assert the contagion checker flags it.

#### 4.3.5 MEV/Sandwich Checker

**Detects**: Transaction is vulnerable to being front-run or sandwiched.

**Algorithm**:
1. Analyze the transaction's instructions for DEX swap operations (Jupiter, Raydium, Orca)
2. Check slippage tolerance: extract the `minimum_amount_out` or equivalent from the instruction data
3. Flag if slippage tolerance is > 2% (configurable) — high slippage makes sandwiching profitable
4. Check for Jito bundle inclusion: if the tx is submitted as part of a Jito bundle, MEV protection is inherent
5. For high-value swaps (>$10K), flag as High severity if no MEV protection is present

**Input**: SimulationTrace (instructions, token amounts), BundleContext (is this a Jito bundle?)

**Output schema**:
```json
{
  "checker_name": "mev_sandwich",
  "passed": false,
  "severity": "Medium",
  "flags": [{
    "code": "HIGH_SLIPPAGE_NO_MEV_PROTECTION",
    "message": "Swap of $15,000 USDC on Jupiter with 5% slippage tolerance and no Jito bundle protection",
    "data": {
      "swap_amount_usd": 15000,
      "slippage_tolerance_pct": 5.0,
      "has_jito_bundle": false,
      "dex": "Jupiter"
    }
  }]
}
```

**Deterministic**: Yes. Instruction analysis is pure parsing.

**False positives**: High slippage may be intentional for illiquid tokens. Mitigation: whitelist known illiquid pairs.

**Unit test**: Construct a Jupiter swap instruction with 5% slippage and $20K amount. Assert the checker flags it.

#### 4.3.6 Approval Abuse Checker

**Detects**: Unlimited token approvals to unknown or risky programs.

**Algorithm**:
1. Parse the CPI graph for `Approve` and `ApproveChecked` instructions on SPL Token accounts
2. Check the `amount` field: if it equals `u64::MAX` (unlimited approval), flag it
3. Check the `delegate` program against the known-good program registry
4. Flag if: unlimited approval AND delegate is not in the known-good registry
5. Also check for `Revoke` instructions that might be missing (approval granted but never revoked in the same tx)

**Input**: SimulationTrace (CPI graph, token instructions), ProgramRegistry

**Output schema**:
```json
{
  "checker_name": "approval_abuse",
  "passed": false,
  "severity": "High",
  "flags": [{
    "code": "UNLIMITED_APPROVAL_UNKNOWN_PROGRAM",
    "message": "Unlimited token approval granted to unrecognized program 9xYz...",
    "data": {
      "token_mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
      "delegate": "9xYz...",
      "amount": "18446744073709551615",
      "in_known_registry": false
    }
  }]
}
```

**Deterministic**: Yes.

**Unit test**: Construct a tx with `Approve { amount: u64::MAX }` to an unknown program. Assert the checker flags it.

#### 4.3.7 Sim-Spoof Detection Checker

**Detects**: Malicious contracts that detect sandbox environments and alter behavior.

**Algorithm (v1, single-fork)**:
1. Parse the CPI graph for calls to known simulation-detection patterns:
   - `Clock::get()` followed by conditional branches based on slot ranges
   - `sysvar::slot_hashes` reads (programs checking for real validator slot history)
   - `sysvar::recent_blockhashes` reads with specific value comparisons
   - Excessive `get_stack_height()` calls (probing CPI depth for sandbox detection)
2. Maintain a registry of known sim-detection opcode patterns (manually curated from security research)
3. Flag programs that match registered patterns

**Input**: SimulationTrace (CPI graph, program logs, instruction data), SimDetectionRegistry

**Output schema**:
```json
{
  "checker_name": "sim_spoof_detection",
  "passed": false,
  "severity": "High",
  "flags": [{
    "code": "KNOWN_SIM_DETECTION_PATTERN",
    "message": "Program 5xAb... matches known simulation detection pattern: slot_hashes probe",
    "data": {
      "program": "5xAb...",
      "pattern_id": "SLOT_HASHES_PROBE_V1",
      "confidence": 0.85
    }
  }]
}
```

**v1 limitation**: Pattern-based detection only. No differential execution.

**v2 design sketch**: Run the same tx against two independent fork implementations (Surfpool + solana-test-validator). Compare execution traces. Any divergence in program behavior between the two forks indicates simulation-aware behavior. Doubles compute but provides strong detection.

**Unit test**: Deploy a test program that checks `sysvar::slot_hashes` and behaves differently in simulation vs mainnet. Assert the checker flags the pattern.

### 4.4 Third-Party Checker Contribution (v2 Path)

v2 opens the checker interface to third-party contributors:

1. Checker implementors publish a Rust crate implementing the `Checker` trait
2. The crate is submitted as a PR to the Ciel checker registry
3. Each checker is reviewed, versioned, and assigned a reputation score based on false-positive/false-negative rates
4. Ciel node operators choose which checker set to run (minimum: the 7 core checkers)
5. Checker Providers earn fees proportional to the verdicts their checkers contribute to

This is described at the design level only for v1. The `Checker` trait interface is stable enough to support this without breaking changes.

---

## 5. LLM Orchestration Layer

### 5.1 Model Choices

| Role | Model | Provider | Latency Target | Cost/1M Tokens |
|------|-------|----------|---------------|----------------|
| Intent compilation (NL → structured JSON) | Llama 3.x 8B | Groq (primary), Fireworks (fallback) | P50 40ms | ~$0.05 input, $0.08 output |
| Rationale aggregation | Llama 3.x 8B | Groq (primary), Fireworks (fallback) | P50 60ms | ~$0.05 input, $0.08 output |

**Cost per verdict** (LLM component): ~$0.00004 at Groq pricing. This is 2% of the $0.002 verdict revenue.

### 5.2 Structured Output Schemas

**Intent Compilation Output:**
```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "type": "object",
  "required": ["goal", "constraints", "budget"],
  "properties": {
    "goal": {
      "type": "string",
      "description": "The primary action to execute",
      "examples": ["swap USDC for SOL"]
    },
    "constraints": {
      "type": "object",
      "properties": {
        "max_slippage_pct": { "type": "number" },
        "min_output_amount": { "type": "number" },
        "preferred_dex": { "type": "string" },
        "mev_protection": { "type": "boolean" }
      }
    },
    "budget": {
      "type": "object",
      "properties": {
        "input_token": { "type": "string" },
        "input_amount": { "type": "number" },
        "max_fee_lamports": { "type": "number" }
      }
    },
    "deadline": {
      "type": "string",
      "format": "date-time",
      "description": "ISO 8601 deadline for execution"
    }
  }
}
```

**Rationale Aggregation Output:**
```json
{
  "type": "object",
  "required": ["summary", "risk_factors", "recommendation"],
  "properties": {
    "summary": { "type": "string", "maxLength": 500 },
    "risk_factors": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "checker": { "type": "string" },
          "finding": { "type": "string" },
          "severity": { "type": "string", "enum": ["Low", "Medium", "High", "Critical"] }
        }
      }
    },
    "recommendation": { "type": "string", "maxLength": 200 }
  }
}
```

### 5.3 Prompt Templates

**Intent Compilation Prompt:**
```
You are a Solana transaction intent compiler. Convert the user's natural language intent into a structured JSON object.

User intent: "{nl_intent}"

Output a JSON object with these fields:
- goal: the primary action (e.g., "swap USDC for SOL")
- constraints: { max_slippage_pct, min_output_amount, preferred_dex, mev_protection }
- budget: { input_token, input_amount, max_fee_lamports }
- deadline: ISO 8601 timestamp (default: 30 seconds from now)

Rules:
- If the user doesn't specify slippage, default to 1%
- If the user doesn't specify MEV protection, default to true
- Use standard token symbols (USDC, SOL, BONK, etc.)
- Amount must be in human-readable units (not lamports/atomic units)

Output ONLY the JSON object, no explanation.
```

**Rationale Aggregation Prompt:**
```
You are a transaction safety analyst. Given the following checker results for a Solana transaction, produce a concise safety rationale.

Transaction hash: {tx_hash}
Verdict: {verdict}
Safety score: {safety_score}

Checker results:
{checker_outputs_json}

Produce a JSON object with:
- summary: 1-2 sentence overall assessment
- risk_factors: array of { checker, finding, severity } for each flagged issue
- recommendation: 1 sentence action recommendation

Be precise and technical. No marketing language. Under 500 characters for summary.
```

### 5.4 Failure Modes and Fallback

| Failure | Detection | Fallback |
|---------|-----------|----------|
| Groq API timeout (>80ms) | `tokio::time::timeout` | Fall through to Fireworks AI with relaxed deadline (150ms) |
| Groq API error (5xx) | HTTP status code | Same fallback to Fireworks |
| Both providers fail | Double timeout | Deterministic rule engine: verdict based purely on checker outputs + scorer, rationale = null |
| LLM returns invalid JSON | JSON parse failure | Retry once with stricter prompt; then fallback to rule engine |
| LLM returns hallucinated intent | Schema validation failure | Return error to client: "Could not parse intent" |

**Deterministic rule engine fallback**: When LLM is unavailable, the verdict is still computed. The scorer uses checker outputs directly without LLM rationale. The rationale field in the response is `null`. This is the "deterministic-only mode" mentioned in the product spec.

### 5.5 LLM Output Is Metadata Only

**Critical design invariant**: The LLM output (rationale string, intent compilation result) is NEVER part of the Ed25519-signed attestation payload. The signed payload contains only:
- tx_hash
- verdict (enum)
- safety_score (fixed-point number)
- optimality_score (fixed-point number)
- checker_outputs_hash (SHA-256 of deterministic checker outputs)
- slot
- expiry_slot

The rationale string is returned in the API response and logged to the verdict store, but it is cryptographically separate from the attestation. Any party can reproduce a verdict from the fork snapshot and checker outputs without trusting the LLM.

### 5.6 Pre-Certified Mode

For high-frequency searchers where even 200ms is too slow.

**Policy template schema:**
```json
{
  "type": "object",
  "required": ["policy_id", "rules", "valid_hours"],
  "properties": {
    "policy_id": { "type": "string", "format": "uuid" },
    "rules": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "program": { "type": "string", "description": "Program pubkey" },
          "instruction": { "type": "string", "description": "Instruction name" },
          "constraints": {
            "type": "object",
            "properties": {
              "max_amount_usd": { "type": "number" },
              "max_slippage_pct": { "type": "number" },
              "allowed_tokens": { "type": "array", "items": { "type": "string" } }
            }
          }
        }
      }
    },
    "valid_hours": { "type": "number", "default": 24 }
  }
}
```

**Flow:**
1. Agent submits policy template → Ciel evaluates policy against current risk landscape → Ciel signs a `PolicyAttestation` covering the policy hash, valid for N hours
2. At transaction time: Ciel verifies the tx matches the pre-certified policy via deterministic pattern matching (<20ms)
3. If the risk landscape changes (oracle anomaly, contagion event), Ciel revokes the policy attestation

**Latency**: <20ms per verdict (pattern match only, no fork simulation).

---

## 6. Scorer

### 6.1 Safety Score Calculation

```rust
pub fn compute_safety_score(checker_results: &CheckerResults) -> f64 {
    let mut score = 1.0_f64;

    for (_, status) in &checker_results.outputs {
        match status {
            CheckerStatus::Completed(output) => {
                let penalty = match output.severity {
                    Severity::None => 0.0,
                    Severity::Low => 0.05,
                    Severity::Medium => 0.15,
                    Severity::High => 0.40,
                    Severity::Critical => 1.0,  // immediate BLOCK
                };
                score -= penalty;
            }
            CheckerStatus::TimedOut => {
                score -= 0.10; // timed-out checker = uncertain = mild penalty
            }
        }
    }

    score.max(0.0).min(1.0)
}
```

**Thresholds:**
- `safety_score >= 0.7` → `APPROVE`
- `0.4 <= safety_score < 0.7` → `WARN`
- `safety_score < 0.4` → `BLOCK`
- Any single `Critical` severity → immediate `BLOCK` (safety_score = 0.0)

### 6.2 Optimality Score Calculation

Only computed in intent mode. Measures how well the execution plan achieves the stated goal.

```rust
pub fn compute_optimality_score(intent: &Intent, trace: &SimulationTrace) -> f64 {
    let mut score = 0.0;

    // Price efficiency: how close to the oracle mid-price
    if let Some(swap) = trace.extract_swap_result() {
        let oracle_price = oracle_cache.get_price(&swap.output_token);
        let execution_price = swap.output_amount / swap.input_amount;
        let price_efficiency = execution_price / oracle_price; // 1.0 = perfect
        score += price_efficiency * 0.6; // 60% weight
    }

    // Fee efficiency: lower fees = better
    let fee_ratio = 1.0 - (trace.total_fees_lamports as f64 / intent.budget.max_fee_lamports as f64);
    score += fee_ratio.max(0.0) * 0.2; // 20% weight

    // Slippage: how much better than the max allowed
    if let Some(slippage) = trace.actual_slippage_pct() {
        let slippage_efficiency = 1.0 - (slippage / intent.constraints.max_slippage_pct);
        score += slippage_efficiency.max(0.0) * 0.2; // 20% weight
    }

    score
}
```

### 6.3 Combination Rule

```rust
pub fn compute_final_score(safety: f64, optimality: f64) -> f64 {
    let safety_multiplier = if safety >= 0.7 { 1.0 } else { 0.0 };
    optimality * safety_multiplier
    // If safety fails, final_score = 0 regardless of optimality.
    // Safety is an auction dimension, not a post-hoc gate.
}
```

### 6.4 Intent Mode Parallel Candidate Scoring

```rust
pub async fn score_candidates(
    candidates: Vec<Transaction>,
    intent: &Intent,
    fork_sim: &ForkSimulator,
    checkers: &[Box<dyn Checker>],
) -> VerdictResult {
    let deadline = Duration::from_millis(200); // per-candidate deadline

    let scored: Vec<_> = join_all(
        candidates.iter().map(|tx| {
            let tx = tx.clone();
            async move {
                match timeout(deadline, evaluate_single(&tx, intent, fork_sim, checkers)).await {
                    Ok(Ok(result)) => Some(result),
                    Ok(Err(_)) => None,       // evaluation error → eliminate
                    Err(_) => None,            // timeout → eliminate (not the whole request)
                }
            }
        })
    ).await;

    let valid: Vec<_> = scored.into_iter().flatten().collect();

    if valid.is_empty() {
        return VerdictResult::Timeout; // all candidates failed/timed out
    }

    let winner = valid.into_iter()
        .max_by(|a, b| a.final_score.partial_cmp(&b.final_score).unwrap())
        .unwrap();

    VerdictResult::Approved(winner)
}
```

---

## 7. Attestation and Signing

### 7.1 Attestation Payload Schema

```rust
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Clone)]
pub struct CielAttestation {
    /// Magic bytes for identification: "CIEL" (0x4349454C)
    pub magic: [u8; 4],
    /// Schema version (1 for v1)
    pub version: u8,
    /// SHA-256 hash of the evaluated transaction
    pub tx_hash: [u8; 32],
    /// Verdict: 0=APPROVE, 1=WARN, 2=BLOCK, 3=TIMEOUT
    pub verdict: u8,
    /// Safety score as fixed-point: score * 10000 (e.g., 7500 = 0.75)
    /// Nullable under TIMEOUT: set to 0xFFFF
    pub safety_score: u16,
    /// Optimality score as fixed-point: score * 10000
    /// Only set in intent mode; 0 for raw tx mode; 0xFFFF under TIMEOUT
    pub optimality_score: u16,
    /// SHA-256 hash of the concatenated checker outputs (deterministic)
    pub checker_outputs_hash: [u8; 32],
    /// Mainnet slot at which the fork was taken
    pub slot: u64,
    /// Expiry slot (slot + 2). Enforcement contracts reject if current_slot > expiry_slot
    pub expiry_slot: u64,
    /// Ciel signer public key (Ed25519, 32 bytes)
    pub signer: [u8; 32],
    /// Unix timestamp of attestation creation
    pub timestamp: i64,
    /// For TIMEOUT verdicts: milliseconds elapsed before timeout fired. 0 otherwise.
    pub timeout_at_ms: u16,
}
// Total size: 4 + 1 + 32 + 1 + 2 + 2 + 32 + 8 + 8 + 32 + 8 + 2 = 132 bytes
```

**TIMEOUT-specific behavior:**
- `verdict = 3` (TIMEOUT)
- `safety_score = 0xFFFF` (null sentinel)
- `optimality_score = 0xFFFF`
- `checker_outputs_hash` = hash of partial results (whatever checkers completed before deadline)
- `timeout_at_ms` = the wall-clock milliseconds at which the deadline fired

**Departure from product spec**: The product spec describes timeout as returning `WARN: verdict_incomplete`, treating timeout as a WARN subtype. This tech spec promotes TIMEOUT to a first-class verdict enum value (value 3) instead. Rationale: WARN implies a risk judgment was made with concerns; TIMEOUT means no judgment was possible due to infrastructure failure. These are semantically different and warrant distinct handling. Downstream enforcement contracts need to distinguish "Ciel evaluated this and has concerns" (WARN) from "Ciel could not evaluate this at all" (TIMEOUT). Making TIMEOUT its own enum value enables per-enforcement-path policies: Squads treasuries fail closed on TIMEOUT (reject), while Jito searchers may retry. If TIMEOUT were a WARN subtype, this per-path policy logic would require parsing the WARN reason string — fragile and error-prone.

### 7.2 Canonical Serialization: Borsh

Borsh is chosen because:
1. **Deterministic**: Same struct → same bytes, always. Required for reproducible verification.
2. **Solana-native**: Used by Anchor programs, familiar to Solana developers.
3. **Compact**: The 132-byte attestation fits easily within Solana's 1232-byte transaction limit alongside payload instructions.
4. **Zero-copy deserialization**: On-chain programs can deserialize with minimal CU cost.

```rust
let payload_bytes = attestation.try_to_vec()?; // Borsh serialize
assert_eq!(payload_bytes.len(), 132);
```

### 7.3 Ed25519 Signing (v1)

```rust
use ed25519_dalek::{SigningKey, Signer, Signature};

pub fn sign_attestation(attestation: &CielAttestation, signing_key: &SigningKey) -> Signature {
    let payload = attestation.try_to_vec().expect("borsh serialization");
    signing_key.sign(&payload)
}
```

**On-chain verification** uses the native Ed25519 precompile (`Ed25519SigVerify111111111111111111111111111111`):
- **0 compute units** — the precompile runs outside the CU-metered BPF execution
- The verification instruction is included in the same transaction as the enforcement instruction
- On-chain programs verify the precompile instruction was included by inspecting `sysvar::instructions`

### 7.4 FROST Threshold Signing (v2 Design Sketch)

v2 replaces the single Ed25519 signer with a FROST (Flexible Round-Optimized Schnorr Threshold) signature scheme using the `frost-ed25519` crate from the Zcash Foundation.

**Key property**: FROST-produced Ed25519 signatures are standard Ed25519 signatures, indistinguishable from single-signer signatures on-chain. Solana's `Ed25519SigVerify` precompile verifies them without modification.

**Protocol (t-of-n, e.g., 3-of-5 validators):**
1. **DKG (one-time)**: Distributed Key Generation produces key shares for each validator + a group public key
2. **Round 1**: Each participating validator generates a nonce commitment and broadcasts it (~1 network round trip)
3. **Round 2**: Each validator computes a partial signature using their key share + nonces. Partial signatures are aggregated into a single Ed25519 signature (~1 network round trip)
4. **Total**: 2 round trips. At ~50ms per round trip (co-located validators), this adds ~100-150ms to signing latency.

**Library**: [`frost-ed25519`](https://crates.io/crates/frost-ed25519) from [ZcashFoundation/frost](https://github.com/ZcashFoundation/frost) (v2.0.0, stable).

**v2 latency impact**: The 2-round-trip overhead (~100-150ms) brings total P50 to ~300ms. Acceptable for the treasury and protocol segments; searchers use pre-certified mode.

### 7.5 PolicyAttestation Schema (Pre-Certified Mode)

```rust
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Clone)]
pub struct PolicyAttestation {
    /// Magic bytes: "CILP" (Ciel Policy, 0x43494C50)
    pub magic: [u8; 4],
    /// Schema version (1 for v1)
    pub version: u8,
    /// SHA-256 hash of the policy template JSON
    pub policy_hash: [u8; 32],
    /// Ciel signer public key
    pub signer: [u8; 32],
    /// Slot at which the policy was evaluated
    pub issued_slot: u64,
    /// Expiry: unix timestamp after which this policy attestation is invalid
    pub expires_at: i64,
    /// Whether the policy was revoked (set to true on revocation)
    pub revoked: bool,
}
// Total size: 4 + 1 + 32 + 32 + 8 + 8 + 1 = 86 bytes
```

At transaction time, the pre-certified mode check verifies:
1. The tx matches the policy constraints (deterministic pattern match)
2. The `PolicyAttestation` has not expired (`now < expires_at`)
3. The `PolicyAttestation` has not been revoked
4. The `PolicyAttestation` signature is valid (Ed25519SigVerify)

Enforcement paths consume `PolicyAttestation` identically to `CielAttestation` — the Ed25519SigVerify precompile verifies the signature, and the CielAssert program checks the magic bytes to determine the attestation type.

### 7.6 Expiry and Slot-Pinning Semantics

- Attestation `slot` = the confirmed mainnet slot at the time the fork was taken
- Attestation `expiry_slot` = `slot + 2`
- Enforcement contracts verify: `current_confirmed_slot <= attestation.expiry_slot`
- With ~400ms slot times, a 2-slot expiry gives ~800ms from attestation creation to enforcement
- If the attestation expires before the enforcement tx lands, the client must re-request a fresh verdict

### 7.7 Override Attestation Type

```rust
#[derive(BorshSerialize, BorshDeserialize)]
pub struct OverrideAttestation {
    pub magic: [u8; 4],              // "CLOV" (Ciel Override)
    pub version: u8,
    pub original_attestation_hash: [u8; 32], // hash of the BLOCK attestation being overridden
    pub override_type: u8,           // 0=OVERRIDE_APPROVED
    pub overrider: [u8; 32],        // public key of the entity approving the override
    pub slot: u64,
    pub timestamp: i64,
}
```

---

## 8. Enforcement Integrations

### 8.1 Lighthouse Guard Instructions

**Program ID**: `L2TExMFKdjpN9kozasaurPirfHy9P8sbXoAN1qA3S95` (v2.0.0)

**Integration pattern**: Ciel attestation verification + Lighthouse state assertions sandwich the payload instructions.

```
Transaction layout:
  Ix 0: Ed25519SigVerify (verify Ciel attestation signature — 0 CU)
  Ix 1: CielAssert program (decode attestation, verify verdict=APPROVE, verify slot freshness)
  Ix 2..N: Payload instructions (DeFi swap, transfer, etc.)
  Ix N+1: Lighthouse assertion (verify post-execution state matches expectations)
```

**Pseudocode for CielAssert program (on-chain):**
```rust
use solana_program::sysvar::instructions;

pub fn process_assert_attestation(ctx: Context, expected_signer: Pubkey, max_slot_age: u64) -> Result<()> {
    // Read the Ed25519 precompile instruction from sysvar
    let ix_sysvar = &ctx.accounts.instructions_sysvar;
    let ed25519_ix = instructions::load_instruction_at_checked(0, ix_sysvar)?;
    require!(ed25519_ix.program_id == solana_sdk::ed25519_program::ID, CielError::InvalidPrecompile);

    // Parse the attestation from the Ed25519 instruction's message data
    let attestation: CielAttestation = BorshDeserialize::deserialize(&mut &ed25519_ix.data[offset..])?;

    // Verify signer matches expected Ciel signer
    require!(attestation.signer == expected_signer.to_bytes(), CielError::InvalidSigner);

    // Verify verdict
    require!(attestation.verdict == 0 /* APPROVE */ || attestation.verdict == 1 /* WARN */, CielError::Blocked);

    // Verify slot freshness
    let clock = Clock::get()?;
    require!(clock.slot <= attestation.expiry_slot, CielError::AttestationExpired);

    Ok(())
}
```

**TIMEOUT handling**: Lighthouse path is configurable per integration. Default: TIMEOUT = reject (transaction reverts). Protocols can configure TIMEOUT = pass-through if they prefer availability over safety.

**Failure mode**: If attestation is invalid or expired, the CielAssert instruction fails, reverting the entire transaction including payload instructions. Funds are never at risk — the failure mode is "transaction didn't execute," not "funds lost."

### 8.2 Squads Policy Gate

**Program ID**: `SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf` (Squads Multisig v4)

**SDK**: `@squads-protocol/multisig` (npm)

**Integration pattern (v1): Ciel key as multisig member**

Squads v4 does not have native policy hooks. The integration uses Ciel's signing key as a required member of the multisig.

**Setup:**
1. Treasury creates a Squads multisig with members: [Human1, Human2, ..., CielKey]
2. Threshold is set such that CielKey's approval is required (e.g., 3-of-4 where Ciel is one)
3. `time_lock` is set to the override delay (e.g., `86400` seconds = 24h for treasuries)

**Flow:**
```typescript
// 1. Human members create proposal and approve
await multisig.rpc.proposalCreate({ connection, multisigPda, transactionIndex, creator: human1 });
await multisig.rpc.proposalApprove({ connection, multisigPda, transactionIndex, member: human1 });
await multisig.rpc.proposalApprove({ connection, multisigPda, transactionIndex, member: human2 });

// 2. Ciel backend evaluates the VaultTransaction
const verdict = await cielClient.evaluate(vaultTransaction);

// 3. If APPROVE: Ciel signs the approval
if (verdict.verdict === "APPROVE") {
    await multisig.rpc.proposalApprove({
        connection, multisigPda, transactionIndex,
        member: cielKeypair, // Ciel holds this key
    });
}
// If BLOCK: Ciel does NOT approve.
// The time_lock allows override: after 24h, remaining members can proceed without Ciel.

// 4. Execute (once threshold met + time_lock elapsed)
await multisig.rpc.vaultTransactionExecute({ connection, multisigPda, transactionIndex });
```

**TIMEOUT handling**: Squads path treats TIMEOUT as reject (Ciel does not approve). The time_lock provides the override path.

**Failure mode**: If Ciel is unavailable, the multisig simply lacks one approval. The time_lock override mechanism allows the treasury to proceed after the delay, preserving protocol sovereignty.

### 8.3 Jito Bundle Precondition

**API**: JSON-RPC at `https://mainnet.block-engine.jito.wtf/api/v1/bundles` (also available via gRPC)

**Integration pattern**: Attestation verification as the first transaction in the bundle.

```typescript
// Transaction 1: Attestation verification (if this fails, entire bundle is dropped)
// Note: `response` is VerdictResponse from the SDK; `response.attestation` is the struct,
// `response.signature` is the Ed25519 signature (separate from the attestation payload).
const verifyTx = new Transaction();
verifyTx.add(
    Ed25519Program.createInstructionWithPublicKey({
        publicKey: CIEL_SIGNER_PUBKEY.toBytes(),
        message: borshSerialize(response.attestation),
        signature: response.signature,
    })
);
verifyTx.add(createCielAssertInstruction(response.attestation));

// Transaction 2-4: Payload
const payloadTx = new Transaction();
payloadTx.add(...deFiInstructions);

// Transaction 5: Jito tip
const tipTx = new Transaction();
tipTx.add(SystemProgram.transfer({
    fromPubkey: payer,
    toPubkey: tipAccounts[0], // fetched from getTipAccounts()
    lamports: tipAmount,
}));

// Submit bundle — atomic execution
const bundleId = await fetch("https://mainnet.block-engine.jito.wtf/api/v1/bundles", {
    method: "POST",
    body: JSON.stringify({
        jsonrpc: "2.0", id: 1, method: "sendBundle",
        params: [[base64(verifyTx), base64(payloadTx), base64(tipTx)]]
    })
});
```

**Key property**: If `verifyTx` fails (invalid attestation), the entire bundle is dropped by the Jito Block Engine and never lands on-chain. This is atomic enforcement.

**Bundle constraints**: Max 5 transactions per bundle. Tip must be in the last instruction of the last transaction.

**TIMEOUT handling**: TIMEOUT verdict = don't assemble the bundle. The agent retries with a fresh verdict. Since Jito bundles are latency-sensitive, the retry cost is ~200ms.

---

## 9. Override with Time Delay

### 9.1 OVERRIDE_APPROVED Specification

When a BLOCK verdict is issued, the calling party can override it with explicit additional approval after a time delay.

**Override flow:**
1. Ciel returns `BLOCK` verdict with attestation
2. The calling party creates an `OverrideRequest` (signed by the authorized overrider)
3. Ciel records the override request with a timestamp
4. After the time delay elapses, Ciel issues an `OVERRIDE_APPROVED` attestation
5. The override attestation is recorded on-chain via the relevant enforcement path

### 9.2 Time Delays Per Segment

| Segment | Time Delay | Rationale |
|---------|-----------|-----------|
| Institutional treasuries | 24 hours | Maximum caution for large capital pools |
| Autonomous agents under Squads policy | 1 hour | Balance safety with agent operational needs |
| End users (Lighthouse) | 10 minutes | Quick override for personal transactions |
| Configurable | Per integration | Enforcement contract stores the delay |

### 9.3 TIMEOUT Is NOT Overridable

A TIMEOUT verdict signals infrastructure failure (checkers didn't complete, RPC unavailable, etc.), not a risk judgment. It is meaningless to "override" a verdict that was never rendered. TIMEOUT verdicts require a retry, not an override.

### 9.4 On-Chain Recording

Override events are recorded on-chain via the Squads time_lock mechanism (for Squads path) or via a dedicated Ciel override log program (for Lighthouse/Jito paths).

The override log instruction records:
- Original BLOCK attestation hash
- Overrider's public key
- Override timestamp
- Time delay that was waited

### 9.5 Override Data Pipeline

Every override is a training signal: the checker set flagged something, but a human/policy decided it was safe. These signals feed the learning loop:
- Override events are written to the verdict store with `override: true`
- Weekly batch analysis identifies checkers with high override rates (potential false-positive tuning needed)
- Override context (why the human overrode) is captured if provided via the SDK

---

## 10. Intent Layer

### 10.1 NL-to-Structured-Intent Compiler

Architecture: Direct async HTTP call to Groq API with the intent compilation prompt (Section 5.3).

```rust
pub async fn compile_intent(nl_intent: &str, llm_client: &LlmClient) -> Result<Intent> {
    let response = llm_client.complete(IntentCompilationPrompt {
        nl_intent: nl_intent.to_string(),
    }).await?;

    let intent: Intent = serde_json::from_str(&response.text)?;
    validate_intent(&intent)?; // schema validation
    Ok(intent)
}
```

### 10.2 Intent JSON Schema

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct Intent {
    pub goal: String,                    // "swap USDC for SOL"
    pub constraints: IntentConstraints,
    pub budget: IntentBudget,
    pub deadline: Option<DateTime<Utc>>, // ISO 8601
}

#[derive(Serialize, Deserialize, Clone)]
pub struct IntentConstraints {
    pub max_slippage_pct: Option<f64>,   // default 1.0
    pub min_output_amount: Option<f64>,
    pub preferred_dex: Option<String>,   // "Jupiter", "Raydium", etc.
    pub mev_protection: bool,            // default true
}

#[derive(Serialize, Deserialize, Clone)]
pub struct IntentBudget {
    pub input_token: String,             // "USDC"
    pub input_amount: f64,               // 10000.0
    pub max_fee_lamports: Option<u64>,   // default 5_000_000 (0.005 SOL)
}
```

### 10.3 Candidate Plan Generation (v1)

**Design decision**: The product spec describes a "PoIN-style agent competition where multiple agents submit execution plans in parallel." For v1, this is simplified to Ciel internally generating N candidate routes via the Jupiter quote API and scoring them. The architectural difference: PoIN-style requires an external agent submission protocol, message queuing, and a competition window — adding significant complexity beyond the 5-week scope. Internal route generation provides the same scoring and safety demonstration (multiple candidates → parallel evaluation → winner selection) without the multi-party coordination overhead. The PoIN-style external competition is a v2 feature once the scoring and attestation infrastructure is proven.

For the hackathon, candidate generation uses hardcoded route generation via Jupiter API:

```rust
pub async fn generate_candidates(intent: &Intent) -> Vec<Transaction> {
    // v1: Query Jupiter quote API for multiple routes
    let quotes = jupiter_client.get_quotes(QuoteRequest {
        input_mint: resolve_mint(&intent.budget.input_token),
        output_mint: resolve_mint(&intent.goal), // parsed from goal
        amount: to_atomic_units(&intent.budget.input_amount, &intent.budget.input_token),
        slippage_bps: (intent.constraints.max_slippage_pct.unwrap_or(1.0) * 100.0) as u16,
        max_accounts: 64,
    }).await;

    // Return top 3 routes as candidate transactions
    quotes.into_iter()
        .take(3)
        .map(|q| jupiter_client.build_swap_tx(q))
        .collect()
}
```

### 10.4 Partial-Timeout Handling

If any candidate plan times out during parallel scoring, that candidate is eliminated (final_score = -infinity). The remaining candidates are still ranked and the best one is selected. Only if ALL candidates timeout does the verdict become TIMEOUT.

### 10.5 Jito Bundle Assembly for Winning Plan

```rust
pub fn assemble_jito_bundle(
    winning_tx: Transaction,
    attestation: SignedAttestation,
    payer: &Keypair,
    tip_lamports: u64,
) -> Vec<Transaction> {
    let verify_tx = build_attestation_verify_tx(&attestation, payer);
    let tip_tx = build_tip_tx(payer, tip_lamports);

    vec![verify_tx, winning_tx, tip_tx]
}
```

---

## 11. x402 Monetization

### 11.1 x402 Endpoint Specification

The x402 gateway sits in front of the Ciel API server as a reverse proxy (TypeScript Express middleware).

**Protected endpoint**: `POST /v1/verdict`

When a client sends a request without payment:
1. Server responds with `HTTP 402 Payment Required`
2. Response includes payment requirements in headers/body:
   ```json
   {
     "x402_version": "1",
     "payment_required": {
       "amount": "2000",
       "currency": "USDC",
       "network": "solana",
       "recipient": "CielTreasuryPubkey...",
       "description": "Ciel verdict - 1 evaluation"
     }
   }
   ```
3. Client creates a USDC transfer, signs it, and retries the request with the signed payment in the `X-Payment` header
4. Server verifies the payment on-chain, then processes the verdict request

**SDK**: [`x402-solana-toolkit`](https://github.com/BOBER3r/x402-solana-toolkit) provides Express middleware, Fastify plugin, and React hooks.

### 11.2 Per-Verdict Pricing

| Segment | Price | Payment Method |
|---------|-------|---------------|
| Agents (x402) | $0.002 per verdict | USDC micropayment per request |
| Protocols (SaaS) | $2,500/month + $0.0005 per verdict above 1M/month | API key + monthly invoice |
| Institutional | $10K+/month negotiated | API key + enterprise contract |

### 11.3 Protocol SaaS Tier Metering

API key-authenticated protocols are metered via a simple counter in PostgreSQL:

```sql
CREATE TABLE api_key_usage (
    api_key_id UUID,
    month DATE,
    verdict_count BIGINT DEFAULT 0,
    PRIMARY KEY (api_key_id, month)
);
```

At each verdict request, increment the counter. Bill overage at $0.0005/verdict above the 1M/month included threshold.

### 11.4 Rate Limiting and Abuse Prevention

| Tier | Rate Limit | Burst |
|------|-----------|-------|
| x402 (per-verdict) | 100 req/s per payer address | 200 req/s for 5s |
| SaaS | 500 req/s per API key | 1000 req/s for 5s |
| Enterprise | Negotiated | — |

Rate limiting is implemented via a token bucket in the API server (tower middleware for axum).

---

## 12. Agent SDK

### 12.1 SDK Surface

**Rust client library:**
```rust
pub struct CielClient {
    endpoint: String,
    api_key: Option<String>,
    x402_payer: Option<Keypair>,
}

impl CielClient {
    pub async fn evaluate_tx(&self, tx: &Transaction) -> Result<VerdictResponse>;
    pub async fn evaluate_intent(&self, intent: Intent) -> Result<VerdictResponse>;
    pub async fn evaluate_nl(&self, nl_intent: &str) -> Result<VerdictResponse>;
    pub async fn pre_certify(&self, policy: PolicyTemplate) -> Result<PolicyAttestation>;
    pub async fn check_pre_certified(&self, tx: &Transaction, policy_id: &str) -> Result<VerdictResponse>;
    pub async fn override_block(&self, attestation_hash: [u8; 32]) -> Result<OverrideResponse>;
}

pub struct VerdictResponse {
    pub attestation: CielAttestation,
    pub signature: [u8; 64],
    pub rationale: Option<String>,
    pub checker_details: Vec<CheckerOutput>,
    pub latency_ms: u64,
}
```

**TypeScript client library:**
```typescript
class CielClient {
    constructor(config: { endpoint: string; apiKey?: string; x402Payer?: Keypair });

    evaluateTx(tx: Transaction): Promise<VerdictResponse>;
    evaluateIntent(intent: Intent): Promise<VerdictResponse>;
    evaluateNl(nlIntent: string): Promise<VerdictResponse>;
    preCertify(policy: PolicyTemplate): Promise<PolicyAttestation>;
    overrideBlock(attestationHash: Uint8Array): Promise<OverrideResponse>;
}
```

### 12.2 MCP Server Pattern

Ciel exposes itself as an MCP tool server, allowing any MCP-compatible agent to call it.

**MCP tool definition:**
```json
{
  "name": "ciel_evaluate",
  "description": "Evaluate a Solana transaction for safety and optimality. Returns a signed attestation.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "transaction": { "type": "string", "description": "Base64-encoded unsigned Solana transaction" },
      "intent": { "type": "string", "description": "Natural language description of intent (optional)" }
    },
    "required": ["transaction"]
  }
}
```

**MCP transport**: Streamable HTTP (the default for network-accessible MCP servers per the 2025-11-25 spec).

**Authentication**: API key passed via MCP auth headers.

### 12.3 Authentication

| Method | Use Case |
|--------|---------|
| x402 micropayment | Per-verdict agent calls. No API key needed — payment IS authentication. |
| API key (Bearer token) | SaaS and enterprise clients. Key issued via dashboard. |
| Ed25519 signature | Agent signs the request with its keypair. Ciel verifies. For agents with on-chain identity. |

---

## 13. Learning Loop and Data Pipeline

### 13.1 Verdict Log Schema

```sql
CREATE TABLE verdict_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Request
    request_type VARCHAR(20) NOT NULL,  -- 'raw_tx', 'intent', 'nl_intent'
    tx_hash BYTEA NOT NULL,
    intent JSONB,
    nl_intent TEXT,

    -- Verdict
    verdict VARCHAR(10) NOT NULL,       -- 'APPROVE', 'WARN', 'BLOCK', 'TIMEOUT'
    safety_score REAL,
    optimality_score REAL,
    attestation BYTEA NOT NULL,         -- full Borsh-serialized attestation
    signature BYTEA NOT NULL,           -- Ed25519 signature

    -- Checker details
    checker_outputs JSONB NOT NULL,     -- array of CheckerOutput
    checker_outputs_hash BYTEA NOT NULL,
    checkers_timed_out TEXT[],          -- names of checkers that timed out

    -- LLM
    rationale TEXT,
    rationale_model VARCHAR(50),
    intent_diff_llm_analysis JSONB,  -- optional LLM enrichment for Intent Diff (metadata only, never in checker_outputs_hash)

    -- Timing
    total_latency_ms INTEGER NOT NULL,
    fork_sim_ms INTEGER,
    checkers_ms INTEGER,
    llm_ms INTEGER,
    signing_ms INTEGER,

    -- Post-execution (filled later)
    execution_outcome VARCHAR(20),      -- 'landed', 'reverted', 'expired', 'overridden'
    execution_slot BIGINT,
    actual_state_delta JSONB,

    -- Override
    is_override BOOLEAN DEFAULT FALSE,
    override_reason TEXT,
    original_verdict_id UUID REFERENCES verdict_log(id)
);

CREATE INDEX idx_verdict_log_created ON verdict_log(created_at);
CREATE INDEX idx_verdict_log_verdict ON verdict_log(verdict);
CREATE INDEX idx_verdict_log_tx_hash ON verdict_log(tx_hash);
```

### 13.2 Storage Choice: PostgreSQL 16

PostgreSQL is chosen over alternatives because:
- **ACID guarantees** for append-only writes
- **JSONB columns** for flexible checker output storage
- **Concurrent access** from the verdict pipeline + analytics queries
- **Mature ecosystem** (backups, replication, monitoring)
- **v1 volume is low**: even at 100 verdicts/sec, that's ~8.6M rows/day — well within Postgres capacity

ClickHouse is overkill for v1 volume. DuckDB doesn't support concurrent writes. Parquet on S3 doesn't support real-time queries. These become relevant at v2/v3 scale.

### 13.3 Post-Execution Outcome Capture

A background job monitors on-chain state after each verdict:
1. For each `APPROVE` verdict, watch for the transaction landing (via Geyser subscription on the tx signature)
2. Record: did it land? Was it reverted? What was the actual state delta?
3. For `BLOCK` verdicts that were overridden, record the outcome of the overridden execution
4. This data feeds checker accuracy metrics and model retraining

### 13.4 TIMEOUT vs BLOCK in Analytics

The verdict log schema distinguishes TIMEOUT from BLOCK:
- TIMEOUT verdicts feed **infrastructure tuning** (which checkers timeout, under what load, which RPC provider was slow)
- BLOCK verdicts feed **risk model training** (what patterns trigger blocks, are they true positives)
- The `checkers_timed_out` array identifies specific bottlenecks

---

## 14. Security and Threat Model

### 14.1 Trust Assumptions (v1)

- **Single signer**: Ciel operates a single Ed25519 signing key. The operator (Ciel team) is trusted not to produce fraudulent attestations.
- **Fork accuracy**: The fork simulator accurately reflects mainnet state at the pinned slot. This is verified by the Geyser subscription and slot-pinning.
- **Checker correctness**: The 7 core checkers are open-source and can be audited. Their outputs are deterministic.
- **RPC provider trust**: Helius/Triton One are trusted to return accurate account data. A malicious RPC provider could feed false state to the fork.

### 14.2 Trust Assumptions (v2)

- **FROST consensus**: t-of-n validators must agree on a verdict. Tolerates up to (n-t) Byzantine validators.
- **Checker Provider separation**: Independent security firms run checkers. No single firm controls the checker set.
- **SOL staking**: Validators stake SOL, slashable for dishonest attestations.

### 14.3 Attack Vectors and Mitigations

| Attack | Vector | Mitigation |
|--------|--------|-----------|
| **Sim-spoofing** | Malicious contract detects sandbox, behaves differently | v1: hardened fork with real sysvars + pattern registry. v2: differential execution. |
| **Oracle manipulation** | Attacker feeds false oracle data | Oracle Sanity checker cross-references Switchboard + Pyth. 3-sigma deviation detection. |
| **Replay attack** | Attacker replays an old attestation for a new tx | Attestations are bound to a specific `tx_hash` + `slot` + 2-slot expiry. Old attestations expire. |
| **Attestation forgery** | Attacker forges a Ciel attestation | Ed25519 signature verification on-chain. Cannot forge without the signing key. v2: FROST threshold makes compromise require t validators. |
| **RPC poisoning** | Malicious RPC returns false account data | Primary/fallback RPC from different providers. Geyser cross-validation. |
| **Denial of service** | Flood Ciel with verdict requests | Rate limiting per payer/API key. x402 payment acts as economic rate limiter. |
| **Key compromise** | Ciel signing key is stolen | v1 risk. Mitigation: HSM storage, key rotation capability. v2: FROST threshold eliminates single-key risk. |

### 14.4 Bootstrapping Trust

1. **Code audit**: Before mainnet launch, engage OtterSec or Neodyme for a security audit of the CielAssert on-chain program and the checker framework
2. **Bug bounty**: Launch a bug bounty program covering: attestation forgery, checker bypass, sim-spoofing techniques. Scope: the on-chain verification program and the fork simulator
3. **Transparency**: All checker code is open-source. Verdict logs (anonymized) are published weekly.

---

## 15. Observability

### 15.1 Metrics Per Verdict

| Metric | Type | Labels |
|--------|------|--------|
| `ciel_verdict_total` | Counter | `verdict={approve,warn,block,timeout}`, `input_type={raw_tx,intent,nl_intent}` |
| `ciel_verdict_latency_ms` | Histogram | `stage={total,fork_sim,checkers,llm,signing}` |
| `ciel_checker_result` | Counter | `checker={oracle_sanity,...}`, `passed={true,false}`, `timed_out={true,false}` |
| `ciel_rpc_latency_ms` | Histogram | `provider={helius,triton}`, `method={getAccountInfo,...}` |
| `ciel_geyser_lag_slots` | Gauge | — |
| `ciel_geyser_disconnects` | Counter | — |
| `ciel_timeout_rate` | Gauge | — Top-level health metric. Alert if > 5% over 1-minute window. |
| `ciel_llm_latency_ms` | Histogram | `provider={groq,fireworks}`, `role={intent,rationale}` |
| `ciel_x402_revenue_usd` | Counter | — |

### 15.2 Logging Strategy

Structured JSON logging via the `tracing` crate with `tracing-subscriber` JSON formatter.

Every verdict request logs:
```json
{
  "level": "info",
  "target": "ciel::pipeline",
  "span": { "verdict_id": "uuid", "tx_hash": "base58" },
  "fields": {
    "verdict": "APPROVE",
    "safety_score": 0.85,
    "total_ms": 147,
    "fork_sim_ms": 32,
    "checkers_ms": 55,
    "llm_ms": 48,
    "signing_ms": 1,
    "checkers_passed": ["oracle_sanity", "authority_diff", "..."],
    "checkers_flagged": [],
    "checkers_timed_out": []
  }
}
```

### 15.3 Tracing

OpenTelemetry with OTLP exporter. Each verdict request is a root span with child spans for each pipeline stage. Traces export to Jaeger or Grafana Tempo.

```rust
#[tracing::instrument(skip(ctx))]
async fn evaluate_verdict(ctx: VerdictContext) -> Result<VerdictResponse> {
    let _fork_span = tracing::info_span!("fork_simulator").entered();
    let trace = fork_sim.execute(&ctx.tx).await?;
    drop(_fork_span);

    let _checker_span = tracing::info_span!("checker_fanout").entered();
    let results = run_checkers(&trace, &checkers).await;
    // ...
}
```

### 15.4 Dashboards

| Dashboard | Key Panels |
|-----------|-----------|
| **Operator Health** | Verdict rate (req/s), TIMEOUT rate (alert threshold: 5%), Geyser lag (slots), RPC error rate, LLM provider health |
| **Checker Performance** | Per-checker flag rate, per-checker timeout rate, false-positive tracking (overrides/checker) |
| **Revenue** | x402 verdicts/day, SaaS metered usage, revenue by segment |
| **Latency** | P50/P95/P99 per pipeline stage, latency degradation under load |

---

## 16. Deployment Architecture

### 16.1 Infrastructure

**Cloud**: AWS (us-east-1) for hackathon. Rationale: Helius and Jito Block Engine nodes are co-located in US-East. Minimizing network hops to RPC providers is critical for the latency budget.

**Instance type**: `c7g.2xlarge` (ARM, 8 vCPU, 16GB RAM) or equivalent. The verdict pipeline is CPU-bound (simulation + checker execution), not memory-bound.

### 16.2 Container Orchestration

Docker Compose for v1 (hackathon simplicity). Three containers:

```yaml
services:
  ciel:
    build: .
    ports:
      - "50051:50051"  # gRPC
      - "8080:8080"    # REST
    environment:
      - HELIUS_API_KEY=${HELIUS_API_KEY}
      - TRITON_API_KEY=${TRITON_API_KEY}
      - GROQ_API_KEY=${GROQ_API_KEY}
      - CIEL_SIGNING_KEY_PATH=/keys/ciel.json
      - DATABASE_URL=postgres://ciel:pass@db:5432/ciel
    depends_on:
      - db

  x402-gateway:
    build: ./gateway
    ports:
      - "443:443"
    environment:
      - CIEL_UPSTREAM=http://ciel:8080
      - CIEL_TREASURY_PUBKEY=${TREASURY_PUBKEY}

  db:
    image: postgres:16
    environment:
      - POSTGRES_DB=ciel
      - POSTGRES_USER=ciel
      - POSTGRES_PASSWORD=pass
    volumes:
      - pgdata:/var/lib/postgresql/data
```

### 16.3 CI/CD

GitHub Actions:
1. `cargo test` — unit tests for all checkers
2. `cargo clippy` — lint
3. Integration tests against Solana devnet
4. Docker build + push to ECR
5. Deploy to EC2 via SSH (hackathon-simple; k8s is v2)

### 16.4 Environment Parity

| Environment | Fork Source | RPC | LLM |
|-------------|-----------|-----|-----|
| **dev** | Devnet or local Surfpool | Helius devnet (free tier) | Groq (free tier) |
| **staging** | Mainnet fork (Surfpool + Helius) | Helius Business | Groq production |
| **production** | Mainnet fork (Surfpool + Helius) | Helius Professional | Groq production |

---

## 17. Testing Strategy

### 17.1 Unit Tests Per Checker

Each checker has a dedicated test module with:
- **Happy path**: transaction that should pass all checks
- **Detection case**: transaction that should trigger the checker
- **Boundary case**: transaction at the exact threshold (e.g., 3.0 sigma for Oracle Sanity)
- **Timeout case**: checker exceeds its deadline

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn oracle_sanity_detects_3_sigma_deviation() {
        let trace = build_trace_with_oracle_deviation(3.1);
        let ctx = build_checker_context(trace);
        let result = OracleSanityChecker::new(3.0).check(&ctx).await;
        assert!(!result.passed);
        assert_eq!(result.severity, Severity::Critical);
    }

    #[tokio::test]
    async fn oracle_sanity_passes_normal_deviation() {
        let trace = build_trace_with_oracle_deviation(1.5);
        let ctx = build_checker_context(trace);
        let result = OracleSanityChecker::new(3.0).check(&ctx).await;
        assert!(result.passed);
    }
}
```

### 17.2 Integration Tests Against Devnet

- Deploy the CielAssert program to devnet
- Submit a test transaction with a valid Ciel attestation → verify it executes
- Submit a test transaction with an expired attestation → verify it reverts
- Submit a test transaction with a forged signature → verify it reverts

### 17.3 End-to-End: Drift Exploit Replay

**The most important test in the entire system.**

1. Capture the Drift exploit transaction (serialized tx + all required accounts at the exploit slot)
2. Load the captured accounts into the fork simulator
3. Run the full verdict pipeline
4. Assert: Oracle Sanity checker flags the oracle manipulation
5. Assert: Authority Diff checker flags the admin key transfer
6. Assert: safety_score < 0.4 (BLOCK threshold)
7. Assert: verdict == BLOCK

This test is captured as a fixture in Week 1 and verified continuously.

### 17.4 End-to-End Latency Measurement

| Measurement | Method | Tool |
|-------------|--------|------|
| **Per-component timing** | Instrument each stage with monotonic clock spans, emit as structured log fields + OTel spans | `tokio::time::Instant`, `tracing` |
| **End-to-end P50/P95/P99** | Load test harness: submit N transactions, record wall-clock per request, compute percentiles | Custom Rust harness + `hdrhistogram` crate |
| **Regression detection** | CI runs load test on every PR; fail build if P50 regresses >10% vs baseline | GitHub Actions + benchmark comparison |
| **Warm vs cold fork** | Measure separately: cold (first tx, accounts from RPC) vs warm (cached accounts) | Same harness with controlled cache state |
| **Under load** | Measure at 10, 50, 100 RPS. Document degradation curve. Identify the RPS where P50 exceeds 200ms. | Custom harness with configurable concurrency |

### 17.5 Load Testing

Target: 100 RPS sustained with P50 < 200ms. The load test harness replays a corpus of real mainnet transactions against the verdict pipeline.

### 17.6 Chaos Testing

| Scenario | Injection | Expected Behavior |
|----------|-----------|-------------------|
| Helius RPC down | Block primary RPC | Failover to Triton One within 100ms |
| Checker timeout | Inject 200ms sleep into Oracle Sanity checker | Checker times out, verdict uses partial results |
| Groq outage | Return 500 from LLM mock | Fallback to Fireworks; if both fail, deterministic-only mode |
| Geyser disconnect | Kill gRPC stream | Reconnect with backoff + gap-fill. Verdicts downgrade to WARN during gap. |
| Database down | Stop Postgres | Verdicts still returned (logging fails gracefully). Alert fires. |

---

## 18. Five-Week Build Plan (Engineering View)

### Week 1 — Foundation (the load-bearing week)

**Daily milestones:**

| Day | Milestone | Deliverable |
|-----|-----------|------------|
| Mon | Surfpool/LiteSVM integration, basic fork from mainnet via Helius RPC **+ Drift exploit fixture capture** (parallel track: fetch exploit tx + all required accounts from mainnet) | Can load a mainnet account into LiteSVM and read it **+ Drift fixture stored as test data** |
| Tue | Transaction simulation: execute a real mainnet tx in the fork, capture balance deltas and CPI graph. **Verify Drift fixture**: load captured accounts into fork, confirm simulation reproduces the exploit state deltas. | `SimulationTrace` struct populated **+ Drift fixture verified in fork** |
| Wed | Geyser subscriber: connect to Helius LaserStream, stream account updates into cache | Account cache populated in real-time |
| Thu | Ed25519 signing: attestation payload schema (Borsh), signing, verification in a unit test | `CielAttestation` struct + sign/verify round-trip |
| Fri | Checker framework: `Checker` trait, parallel fan-out with tokio::join_all, 80ms deadline | Framework runs 7 stub checkers in parallel |
| Sat | Integration day: wire fork sim → checker framework → signer into a single pipeline. Run a real mainnet tx through the full stub pipeline. | End-to-end stub pipeline working |
| Sun | API server skeleton (axum): accept `POST /v1/verdict`, return stub attestation | API accepts requests end-to-end |

> **On the Drift fixture timing:** The Drift exploit replay is the centerpiece of Demo 1, which is the most important asset of the submission. Capturing the fixture early in Week 1 (rather than late) provides pivot lead time. If the exploit cannot be cleanly reproduced in the fork (because of state interleaving, slot-specific account dependencies, or accounts that no longer exist on mainnet at the captured slot), an alternative demo narrative can be designed in Week 2 instead of Week 5. Pre-identified backup options: the Cypher exploit, a synthetic oracle-manipulation transaction constructed for demo purposes, or replaying any other documented Solana DeFi exploit from 2024–2026.

**Week 1 deliverable**: Raw tx in → simulation trace out → stub verdict with real Ed25519 signature. Drift fixture captured and verified.

**Critical artifacts:**
- Drift replay fixture (captured Monday, verified Tuesday)
- Demo harness CLI (basic: submit tx, show trace)

### Week 2 — Risk Graph v1

| Ticket | Description | Depends On |
|--------|-------------|-----------|
| W2-1 | Oracle Sanity checker: Switchboard + Pyth cross-reference | Sim trace, oracle cache |
| W2-2 | Authority Diff checker: CPI graph parsing for SetAuthority/Upgrade | Sim trace |
| W2-3 | Intent Diff checker: deterministic balance-delta comparison + optional LLM metadata enrichment | Sim trace (LLM enrichment uses Groq but does not affect checker output) |
| W2-4 | Approval Abuse checker: unlimited approval detection | Sim trace |
| W2-5 | Sim-Spoof Detection checker (pattern-based, v1) | Sim trace, pattern registry |
| W2-6 | Scorer: safety_score computation + threshold logic | Checker outputs |
| W2-7 | LLM integration: Groq client, rationale aggregation | reqwest async |
| W2-8 | Drift exploit replay: full pipeline produces BLOCK verdict | All of above |

**Week 2 deliverable**: tx in → APPROVE/WARN/BLOCK verdict out with rationale. Drift replay produces BLOCK.

### Week 3 — Enforcement + Design Partner

| Ticket | Description | Depends On |
|--------|-------------|-----------|
| W3-1 | CielAssert on-chain program: Ed25519 verify + attestation decode | Attestation schema |
| W3-2 | Lighthouse integration: build tx with Ed25519Verify + CielAssert + payload + Lighthouse assertion | CielAssert program |
| W3-3 | Squads integration: add Ciel key as member, programmatic approval | Squads v4 SDK |
| W3-4 | Jito integration: bundle assembly with attestation as tx[0] | Attestation, Jito API |
| W3-5 | Contagion Map checker: dependency graph + anomaly detection | Geyser data |
| W3-6 | MEV/Sandwich checker: slippage analysis | Sim trace |
| W3-7 | Override mechanism: time-delay logic, on-chain override recording | Enforcement paths |
| W3-8 | Design partner outreach: contact Mercantill, Drift | — |

**Week 3 deliverable**: End-to-end enforcement demo on devnet (at least 1 path working).

### Week 4 — Intent Layer + Monetization

| Ticket | Description | Depends On |
|--------|-------------|-----------|
| W4-1 | Intent compiler: NL → structured intent via Groq | LLM client |
| W4-2 | Candidate generator: Jupiter API integration, top-3 routes | Intent schema |
| W4-3 | Parallel candidate scoring: `futures::join_all` over candidates | Scorer, fork sim |
| W4-4 | Jito bundle assembly for winning candidate | Jito integration |
| W4-5 | x402 gateway: Express middleware, USDC payment verification | x402 SDK |
| W4-6 | Agent SDK: Rust + TypeScript client libraries | API server |
| W4-7 | MCP tool server: expose `ciel_evaluate` as MCP tool | MCP spec |
| W4-8 | Pre-certified mode: policy template + fast-path verification | Attestation schema |

**Week 4 deliverable**: NL intent in → safe optimal execution out. x402 payments working.

### Week 5 — Demo + Submission

| Day | Activity |
|-----|---------|
| Mon-Tue | Demo harness polish: CLI with colored output, timing display, pipeline visualization |
| Mon-Tue | **Demo 1 rehearsal**: Drift exploit replay → BLOCK verdict → enforcement rejects |
| Wed | **Demo 2 rehearsal**: "swap 10k USDC→SOL" intent → parallel scoring → winner executes via Jito |
| Thu | Record pitch video (3 min) and technical demo video (5 min) with asciinema/screen capture |
| Fri | Write README, finalize GitHub repo with reproducible setup instructions |
| Sat | Submit to Colosseum |
| Sun | Buffer day for issues |

**Critical demo artifacts:**
- Drift replay fixture (verified in Week 1-2)
- Intent demo fixture (scripted in Week 4)
- Demo harness CLI (basic in Week 2, polished in Week 5)
- Video recording script (asciinema + screen capture)

---

## 19. Post-v1 Roadmap Engineering Notes

### 19.1 v2: Checker Provider / Validator Split with FROST

**Architecture**:
- **Checker Providers**: publish Rust crates implementing the `Checker` trait. Each checker is versioned and reputation-scored.
- **Validators**: run the full pipeline (fork sim + checkers + scorer) and participate in FROST threshold signing.
- **Consensus**: validators execute independently, compare verdicts, and produce a FROST threshold signature if t-of-n agree.
- **Staking**: validators stake native SOL. Slashable for producing attestations that disagree with the majority after the fact (evidence-based slashing via on-chain dispute resolution).
- **FROST library**: `frost-ed25519` from ZF FROST. 2 round trips for signing (~100-150ms overhead).
- **Compatibility**: FROST output is standard Ed25519, verified by the same `Ed25519SigVerify` precompile. No on-chain changes needed.

### 19.2 v3: Cross-Chain Attestation Bridge

- Bridge Ciel attestations to EVM chains (Ethereum, Base) via a message-passing protocol
- Attestation format includes a chain-agnostic header + chain-specific payload
- EVM verification: use `ecrecover` with a Ciel-operated secp256k1 bridge key, or an Ed25519 verifier precompile (available on some L2s)
- Initial target: Base (Coinbase L2, aligned with x402 Coinbase origin)

### 19.3 v4: Permissionless Validator Onboarding

- SOL staking minimums for validator registration
- Validator set managed by an on-chain program
- DKG ceremony for new validators joining the FROST group
- Checker marketplace: third-party checkers discoverable and installable by validators

---

## 20. Open Questions and Research Spikes

### Open Questions

| # | Question | Impact | Proposed Resolution |
|---|---------|--------|-------------------|
| 1 | Does Surfpool support programmatic account injection via the LiteSVM API (not just CLI cheatcodes)? | Fork simulator design | Week 1 Day 1 spike: test LiteSVM `set_account` API with Surfpool |
| 2 | Can Groq hit P50 < 60ms for 150 output tokens with JSON mode in April 2026? | LLM latency budget | Week 1 spike: run benchmark against Groq API with exact prompt templates |
| 3 | Does the Lighthouse v2.0.0 SDK support Rust, or is it TypeScript-only? | CielAssert program design | Week 1 spike: inspect Lighthouse repo for Rust crate |
| 4 | Has Squads v4 added native policy hooks since early 2025? | Squads integration complexity | Week 1 spike: check Squads GitHub releases for guard/policy features |
| 5 | What are the exact accounts and transaction data for the Drift exploit? | Demo 1 fixture | Week 1 Monday spike (parallel with Surfpool integration): analyze the Drift exploit transaction on-chain |
| 6 | Does Helius LaserStream support subscription by account pubkey (not just program)? | Geyser cache design | Week 1 spike: test LaserStream subscription filters |
| 7 | Is the x402-solana-toolkit mature enough for production, or do we need a custom implementation? | x402 integration complexity | Week 3 spike: evaluate toolkit against our payment flow |

### Research Spikes (Week 1, time-boxed)

| Spike | Time Box | Owner | Deliverable |
|-------|----------|-------|-------------|
| Surfpool/LiteSVM API exploration | 4 hours | — | Can/cannot do programmatic account injection; fallback plan if not |
| Groq latency benchmark | 2 hours | — | P50/P95 numbers for our exact prompt + output size |
| Drift exploit transaction capture | 4 hours (Monday of Week 1, parallel with Surfpool integration) | — | Serialized tx + account snapshot as test fixture. If capture fails by end of Tuesday, trigger backup demo narrative (see Week 1 note). |
| Helius LaserStream filter test | 2 hours | — | Subscription filter capabilities confirmed |
| Lighthouse Rust SDK check | 1 hour | — | Crate exists / doesn't exist; if not, TypeScript wrapper plan |

---

## Appendix A: Cited Sources

1. [Surfpool — Helius Blog](https://www.helius.dev/blog/surfpool)
2. [Surfpool — Solana Docs](https://solana.com/docs/intro/installation/surfpool-cli-basics)
3. [Surfpool — GitHub](https://github.com/txtx/surfpool)
4. [LiteSVM — GitHub](https://github.com/LiteSVM/litesvm)
5. [LiteSVM — crates.io](https://crates.io/crates/litesvm)
6. [LiteSVM — Anchor Docs](https://www.anchor-lang.com/docs/testing/litesvm)
7. [Helius Pricing](https://www.helius.dev/docs/billing/plans)
8. [Helius — Official Site](https://www.helius.dev)
9. [Triton One Docs](https://docs.triton.one)
10. [Groq vs Cerebras Benchmark](https://speko.ai/benchmark/groq-vs-cerebras)
11. [Best Inference Providers for AI Agents 2026](https://fast.io/resources/best-inference-providers-ai-agents/)
12. [Ed25519 Signature Verification on Solana — RareSkills](https://rareskills.io/post/solana-signature-verification)
13. [Solana Ed25519 Program — Source](https://github.com/solana-labs/solana/blob/master/sdk/src/ed25519_instruction.rs)
14. [ed25519-dalek — crates.io](https://crates.io/crates/ed25519-dalek)
15. [Switchboard Docs](https://docs.switchboard.xyz)
16. [Switchboard On-Demand — GitHub](https://github.com/switchboard-xyz/on-demand)
17. [switchboard-on-demand — crates.io](https://docs.rs/switchboard-on-demand)
18. [Pyth Lazer Blog](https://www.pyth.network/blog/introducing-pyth-lazer-launching-defi-into-real-time)
19. [Pyth Docs](https://docs.pyth.network)
20. [Pyth Pull Oracle on Solana](https://www.pyth.network/blog/pyth-network-pull-oracle-on-solana)
21. [Lighthouse — GitHub](https://github.com/Jac0xb/lighthouse)
22. [Lighthouse — QuickNode Guide](https://www.quicknode.com/guides/solana-development/tooling/web3-2/lighthouse)
23. [Squads v4 — GitHub](https://github.com/Squads-Protocol/v4)
24. [Squads Docs](https://docs.squads.so)
25. [@squads-protocol/multisig — npm](https://www.npmjs.com/package/@squads-protocol/multisig)
26. [Jito Docs](https://docs.jito.wtf)
27. [Jito MEV Protos — GitHub](https://github.com/jito-labs/mev-protos)
28. [Jito Bundles — QuickNode Guide](https://www.quicknode.com/guides/solana-development/transactions/jito-bundles)
29. [x402 — Solana Foundation](https://solana.com/x402/what-is-x402)
30. [x402-solana-toolkit — GitHub](https://github.com/BOBER3r/x402-solana-toolkit)
31. [x402 Guide — Solana Developers](https://solana.com/developers/guides/getstarted/intro-to-x402)
32. [MCP Specification](https://modelcontextprotocol.io/specification/2025-11-25)
33. [MCP — GitHub](https://github.com/modelcontextprotocol)
34. [FROST — ZcashFoundation GitHub](https://github.com/ZcashFoundation/frost)
35. [frost-ed25519 — crates.io](https://crates.io/crates/frost-ed25519)
36. [Phantom — Anti-Spoofing with Lighthouse](https://phantom.com/learn/blog/anti-spoofing-security)
37. [a16z — Runtime Enforcement](https://a16zcrypto.com/posts/article/runtime-enforcement-defense-numerical-exploits)
38. [Solana Programs — Docs](https://solana.com/docs/core/programs)
39. [Borsh — Official Site](https://borsh.io)
40. [Solana Foundation Solana Dev Skill — GitHub](https://github.com/solana-foundation/solana-dev-skill)
