# Ciel — Project Conventions

Pre-execution verdict layer for Solana. Colosseum Frontier Hackathon (April 6 – May 11, 2026).

## Architecture Reference

The authoritative technical specification is `./ciel-technical-spec.md`. Read the relevant sections before writing code — never guess at schemas, algorithms, or API designs. Section numbers are stable; reference them in code comments as `// See spec Section X.Y` when the logic is non-obvious.

The implementation is decomposed into unit docs at `./docs/`. Each unit doc has a "Prompt for Claude Code" block at the bottom. If you are building a specific unit, read that doc first.

## Language and Framework Rules

- **Rust** for all latency-critical code: fork simulator, checkers, scorer, signer, API server, LLM client, pipeline.
- **TypeScript** only for: x402 gateway (`gateway/`), Agent SDK client (`sdk/typescript/`), MCP server (`sdk/mcp-server/`).
- **No orchestration frameworks.** No LangGraph, CrewAI, Temporal, Restate. All parallel execution uses `tokio::join_all` / `futures::join_all` with `tokio::time::timeout`. See spec Section 2.2.
- **Anchor** for the on-chain CielAssert program (`programs/ciel-assert/`).

## Crate Structure

All Rust crates live under `crates/`. The workspace root is `./Cargo.toml`.

| Crate | Purpose | Key types |
|-------|---------|-----------|
| `ciel-fork` | Fork simulator, account cache, geyser subscriber | `ForkSimulator`, `SimulationTrace`, `GeyserSubscriber` |
| `ciel-signer` | Attestation schemas, Ed25519 signing | `CielAttestation`, `PolicyAttestation`, `OverrideAttestation`, `CielSigner` |
| `ciel-checkers` | Checker trait, all 7 checkers, scorer, parallel runner | `Checker` trait, `CheckerOutput`, `run_checkers()`, `compute_safety_score()` |
| `ciel-llm` | Groq/Fireworks async HTTP client | `LlmClient`, `RationaleResponse`, `IntentResponse` |
| `ciel-pipeline` | Verdict pipeline, override handler, pre-certified mode | `VerdictPipeline`, `VerdictResponse` |
| `ciel-intent` | Intent compiler, candidate generator, bundle assembly | `Intent`, `compile_intent()`, `generate_candidates()` |
| `ciel-enforcement` | Lighthouse, Squads, Jito integration | `build_lighthouse_guarded_tx()`, `build_jito_bundle()` |
| `ciel-server` | axum REST + tonic gRPC server | API routes, metrics, tracing |
| `ciel-sdk` | Rust client library | `CielClient` |
| `ciel-fixtures` | Test fixture loaders | `load_drift_fixture()` |
| `ciel-demo` | Demo CLI harness | `main()` with clap subcommands |

## Coding Conventions

- Use `thiserror` for error types. Each crate has its own error enum.
- Use `tracing` for logging (not `log` or `println!`). Structured fields, JSON output.
- Use `tokio::time::Instant` for latency measurement, not `std::time`.
- Derive `Serialize, Deserialize, Clone` on all public types. Add `BorshSerialize, BorshDeserialize` on types that cross the on-chain boundary.
- Config via environment variables. Use `std::env::var` directly — no config crate needed for hackathon scope.
- Tests go in the same file (`#[cfg(test)] mod tests`) for unit tests, or `tests/` for integration tests.

## Key Invariants (violating these is a bug)

1. **LLM output is metadata only.** It never enters `CielAttestation`, `checker_outputs_hash`, or any field that affects the verdict. See spec Section 5.5.
2. **All checkers are deterministic.** Same `CheckerContext` → same `CheckerOutput`, always. The Intent Diff checker's LLM enrichment writes to a separate field outside `CheckerOutput`. See spec Section 4.3.3.
3. **TIMEOUT is not WARN.** TIMEOUT (verdict=3) is a distinct verdict indicating infrastructure failure. It is not overridable. See spec Section 9.3.
4. **Attestations expire in 2 slots (~800ms).** Enforcement contracts verify `current_slot <= expiry_slot`. See spec Section 7.6.
5. **Staleness gates verdicts.** `StalenessTracker::state()` must be checked before every verdict: `Warn` → downgrade to WARN with `reason: "state_parity_degraded"`, `Timeout` → reject with TIMEOUT. Without this, stale fork state produces invalid APPROVEs. See spec Section 3.4. Wiring happens in `VerdictPipeline` (Unit 06).
6. **Off-chain and on-chain Borsh serialization must agree on the wire bytes for CielAttestation.** A committed fixture at `crates/ciel-signer/fixtures/ciel_attestation_v1.bin` (132 bytes) pins the exact wire format. Both `ciel-signer` (off-chain, borsh 1.x) and `ciel-assert` (on-chain, Anchor/borsh) must produce and consume identical bytes. The `test_ciel_attestation_wire_fixture` test catches drift. When implementing Unit 20 (CielAssert program), add a matching test that deserializes this same fixture file.
7. **Checker implementations must use async I/O exclusively.** Any blocking call (`std::thread::sleep`, blocking file I/O, synchronous HTTP) inside a `Checker::check()` implementation will hold a tokio worker thread and block all other concurrent verdicts sharing that thread. Use `tokio::time::sleep`, `tokio::fs`, and async HTTP clients (e.g., `reqwest` with its default async API) instead. The `test_concurrent_runs_do_not_block_on_slow_checker` test in `crates/ciel-checkers/src/runner.rs` verifies this property — if a checker introduces a blocking call, this test will fail by exceeding 200ms for 10 concurrent runs.

## Build Prerequisites

### macOS: C++ SDK headers for `protobuf-src`

`yellowstone-grpc-proto` compiles protobuf from source via `protobuf-src`, which needs C++ standard library headers. On macOS 15+, the headers live exclusively in the Xcode SDK but the default compiler search path misses them. Set these before any `cargo build/test/clippy`:

```bash
export CXXFLAGS="-isystem $(xcrun --show-sdk-path)/usr/include/c++/v1 -isysroot $(xcrun --show-sdk-path)"
export CFLAGS="-isysroot $(xcrun --show-sdk-path)"
```

Add to your shell profile to avoid re-discovering this. Symptoms without it: `fatal error: 'cstdlib' file not found` during `protobuf-src` build.

## Testing

- `cargo test` must pass before committing.
- `cargo clippy -- -D warnings` must pass (warnings are errors).
- Integration tests that need network (RPC, LLM APIs) are marked `#[ignore]` and run separately.
- The Drift exploit replay E2E test (`crates/ciel-pipeline/tests/drift_replay_e2e.rs`) is the most important test. If it doesn't produce BLOCK, something is wrong.

## Commit Style

- Conventional commits: `feat:`, `fix:`, `test:`, `refactor:`, `docs:`, `chore:`
- Reference the unit number: `feat(unit-05): implement checker framework with parallel fan-out`
- Keep commits atomic — one unit's work per commit where possible.

## Environment Variables

Required (see `.env.example`):
- `HELIUS_API_KEY` — Helius RPC and LaserStream
- `TRITON_API_KEY` — Triton One fallback RPC
- `GROQ_API_KEY` — Groq LLM inference
- `FIREWORKS_API_KEY` — Fireworks fallback LLM
- `DATABASE_URL` — PostgreSQL connection string
- `CIEL_SIGNING_KEY_PATH` — path to Ed25519 signing key JSON file

Optional:
- `JITO_BLOCK_ENGINE_URL` — Jito endpoint (default: mainnet)
- `HELIUS_LASERSTREAM_URL` — LaserStream gRPC endpoint
- `CIEL_TREASURY_PUBKEY` — x402 payment recipient
