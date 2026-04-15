# 07: API Server

## Overview

This unit implements the axum/tonic API server that accepts verdict requests via REST (POST /v1/verdict) and gRPC, routes them to the VerdictPipeline, and returns signed attestations. It also serves health checks and exposes Prometheus metrics. Sunday deliverable of Week 1 — the final piece that makes the system callable from the outside.

> Authoritative reference: see [Section 1.2](../ciel-technical-spec.md#12-component-inventory) (API Server component), [Section 1.3](../ciel-technical-spec.md#13-process-boundary-decision-option-a--single-rust-process) (process boundary), [Section 1.4](../ciel-technical-spec.md#14-data-flows) (all three flows), and [Section 15.1](../ciel-technical-spec.md#151-metrics-per-verdict) (metrics).

## Technical Specifications

- **REST endpoint**: POST /v1/verdict accepting raw tx (base64), structured intent (JSON), or NL intent (string). See [Section 1.4](../ciel-technical-spec.md#14-data-flows).
- **gRPC endpoint**: VerdictService with Evaluate RPC. See [Section 1.2](../ciel-technical-spec.md#12-component-inventory).
- **Response**: VerdictResponse with attestation, signature, rationale, checker_details, latency_ms. See [Section 12.1](../ciel-technical-spec.md#121-sdk-surface).
- **Metrics**: ciel_verdict_total, ciel_verdict_latency_ms, etc. See [Section 15.1](../ciel-technical-spec.md#151-metrics-per-verdict).
- **Framework**: axum for REST, tonic for gRPC, tower middleware. See [Section 2.1](../ciel-technical-spec.md#21-core-technologies).

## Key Capabilities

- [ ] Accept POST /v1/verdict with `{ tx: "<base64>" }` and return a VerdictResponse — verified by curl
- [ ] Accept POST /v1/verdict with `{ intent: {...} }` (routes to stub for now) — verified by curl
- [ ] Emit Prometheus metrics on /metrics endpoint — verified by curling /metrics
- [ ] Health check on GET /health — verified by curl
- [ ] gRPC VerdictService.Evaluate works — verified by grpcurl
- [ ] Request tracing with unique verdict_id per request — verified in structured logs

## Implementation Guide

1. **Define request/response types**: VerdictRequest (enum: RawTx, Intent, NlIntent), VerdictResponse
2. **Implement axum router**: POST /v1/verdict, GET /health, GET /metrics
3. **Implement tonic gRPC service**: define .proto file, implement VerdictService
4. **Wire to VerdictPipeline**: the handler deserializes the request and calls pipeline.evaluate_raw_tx
5. **Add metrics middleware**: tower layer that increments counters and records histograms
6. **Add tracing middleware**: assigns a UUID verdict_id to each request for log correlation

**Key gotchas**:
- axum and tonic can share the same tokio runtime and TCP port (via tower layer multiplexing) but for simplicity in v1, run them on separate ports (8080 REST, 50051 gRPC)
- The VerdictResponse must include `latency_ms` measured server-side
- Intent mode routes to a stub until Week 4 — return a 501 Not Implemented for now

**Files / modules to create**:
- `crates/ciel-server/Cargo.toml`
- `crates/ciel-server/src/lib.rs`
- `crates/ciel-server/src/rest.rs` — axum routes
- `crates/ciel-server/src/grpc.rs` — tonic service
- `crates/ciel-server/src/metrics.rs` — Prometheus metrics
- `proto/ciel.proto` — gRPC service definition
- `src/main.rs` — binary entry point wiring everything together

## Dependencies

### Upstream (units this depends on)
- `06-pipeline-integration` — the VerdictPipeline that the server calls

### Downstream (units that depend on this)
- `22-squads-integration` — Squads webhook handler is added to the server
- `34-x402-gateway` — x402 proxy sits in front of this server
- `35-agent-sdk` — SDK clients call this server
- `40-demo-harness` — demo CLI calls this server

## Prompt for Claude Code

```
Implement Unit 07: API Server

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

This prompt covers ONE unit. Do not implement adjacent units. Do not restate or modify the technical spec.

Required reading before you write any code
Read this unit doc first: ./docs/07-api-server.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 1.2 (Component Inventory): the API Server row — its role, language, process
- Section 1.3 (Process Boundary): single Rust process, ports, deployment
- Section 1.4 (Data Flows): all three flows (raw tx, structured intent, NL intent) — the server routes to the correct pipeline
- Section 12.1 (SDK Surface): VerdictResponse struct — this is what the API returns
- Section 15.1 (Metrics Per Verdict): the metrics the server must emit
- Section 15.2 (Logging Strategy): structured JSON logging format
- Section 16.2 (Container Orchestration): port assignments (8080 REST, 50051 gRPC)

Also read these unit docs for upstream dependencies:
- ./docs/06-pipeline-integration.md — the VerdictPipeline API you'll call from request handlers

Scope: what to build
The HTTP and gRPC API server that exposes the verdict pipeline to external clients.

In scope:
- Rust crate at crates/ciel-server/
- axum REST server on port 8080 with POST /v1/verdict, GET /health, GET /metrics
- tonic gRPC server on port 50051 with VerdictService.Evaluate
- Proto file definition for the gRPC service
- VerdictRequest and VerdictResponse types (matching Section 12.1)
- Prometheus metrics middleware (tower layer)
- Structured JSON logging with tracing-subscriber
- Per-request tracing with unique verdict_id
- Binary entry point (src/main.rs) that initializes the pipeline and starts both servers
- Intent mode stub (return 501 for now)

Out of scope (these belong to other units):
- x402 payment gateway — owned by ./docs/34-x402-gateway.md
- Agent SDK client libraries — owned by ./docs/35-agent-sdk.md
- MCP server — owned by ./docs/36-mcp-server.md
- Real intent compilation — owned by ./docs/30-intent-compiler.md

Implementation constraints
- Language: Rust
- Libraries: axum 0.7+, tonic 0.12+, tower, tracing, tracing-subscriber (JSON), prometheus, serde_json
- File location: crates/ciel-server/ for the library, src/main.rs for the binary
- REST port: 8080, gRPC port: 50051 (configurable via env)
- Log format: JSON with fields matching Section 15.2
- Intent mode: return HTTP 501 / gRPC UNIMPLEMENTED until Week 4

Verification steps
Before declaring this unit complete, run and report results for every step:
1. Run `cargo test --package ciel-server` and confirm all tests pass
2. Start the server, send `curl -X POST localhost:8080/v1/verdict -d '{"tx":"<base64-encoded-test-tx>"}'` and confirm a VerdictResponse is returned
3. Confirm `curl localhost:8080/health` returns 200
4. Confirm `curl localhost:8080/metrics` returns Prometheus-format metrics including ciel_verdict_total
5. Confirm structured JSON appears in stdout logs with verdict_id field
6. If tonic/grpcurl is available: call VerdictService.Evaluate and confirm response

What to report when finished
- List of files created or modified with path
- Test results (pass/fail counts)
- API endpoint documentation (request/response formats)
- Any deviations from the technical spec, with justification
- Estimated next unit to build: 10-oracle-sanity-checker (start of Week 2)

What NOT to do
- Do not implement the x402 gateway, agent SDK, or MCP server
- Do not implement real intent compilation
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
- Do not declare the unit complete if any test fails
```
