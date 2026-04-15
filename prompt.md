# Task: Generate a comprehensive technical specification for Ciel

You are acting as a Senior Staff Software Engineer / Principal Architect with deep expertise in Solana, distributed systems, cryptography, LLM orchestration, and high-throughput backend services. You are producing the technical specification that a small engineering team will use to build Ciel end-to-end.

## Input

The complete product specification is provided at `./ciel-product-spec.md`. Read it in full before doing anything else.

## Your output

Produce a single file: `./ciel-technical-spec.md`

This must be a complete, implementation-ready technical specification covering every feature, component, integration, and behavior described in the product spec. No feature may be summarized, deferred, or skipped. If the product spec mentions it, the technical spec must specify how it is built.

## Before writing, conduct web research

Use web search and web fetch to ground every technology choice in current 2026 reality. Do not rely on training-data assumptions — Solana tooling, LLM inference providers, RPC providers, and cryptographic libraries change rapidly. At minimum, research:

1. **Solana state forking and simulation** — current best options (solana-test-validator, Helius enhanced RPC with account hot-swap, LiteSVM, Surfpool, Foundry-style forking for Solana if it exists). Compare latency, account hot-swap fidelity, and production-readiness.
2. **Solana RPC providers** — Helius, Triton One, QuickNode, Shyft. Compare geyser/websocket support, enhanced APIs, rate limits, pricing, and sub-slot update delivery.
3. **Oracle integration** — Switchboard On-Demand vs Pyth Lazer/Pull feeds in 2026. Get current SDK names, deviation-check patterns, and cross-reference strategies.
4. **LLM inference for sub-100ms aggregation** — Groq, Cerebras, SambaNova, Together AI, Fireworks, local vLLM. Compare P50/P95 latency for small models (8B-class), structured output support, and cost per million tokens.
5. **LangGraph and agent orchestration** — current LangGraph version, best patterns for parallel tool execution with hard deadlines, and MCP server conventions for agent SDKs.
6. **Squads Protocol** — current Squads v4 SDK, policy hook / guard integration patterns, and programmatic proposal flows.
7. **Jito** — current bundle submission APIs, tip account mechanics, bundle precondition/validation patterns, and ShredStream.
8. **Lighthouse Protocol** — current Solana program ID, guard instruction types (assertion types), SDK, and integration examples.
9. **x402 protocol** — current x402 specification, Solana Foundation and QuickNode reference implementations, and SDK maturity.
10. **FROST threshold signatures on Solana** — Ed25519 FROST libraries (ZF FROST, frost-ed25519), signing round protocols, and on-chain verification cost on Solana.
11. **Ed25519 signing and verification on Solana** — native program support, precompile availability, and attestation payload encoding patterns.
12. **Account hot-swap and geyser** — Helius geyser gRPC vs Yellowstone, subscription patterns, and sub-slot account delivery guarantees.
13. **Simulation spoofing defenses** — current state of sandbox detection in Solana programs, known detection opcodes, and mitigation patterns.
14. **Agent / MCP SDK patterns** — current Anthropic MCP spec, tool-call signatures, and how to expose a verdict API as an MCP tool.

For every technology choice you make, cite the sources you consulted with links. Prefer primary sources (official docs, GitHub READMEs, maintainer blog posts) over aggregators. If a technology has changed significantly in the last 6 months, note it explicitly.

## Required structure for the technical spec

### 1. System Overview
- High-level architecture diagram (ASCII or Mermaid)
- Component inventory with responsibilities
- Data flow for each input type (raw transaction, structured intent, natural-language intent)
- Latency budget breakdown with P50 and P95 targets per component

### 2. Technology Stack and Rationale
For every technology chosen:
- What it is and the specific version
- Why it was chosen over alternatives (with at least 2 alternatives evaluated)
- Known limitations and mitigations
- Citation to primary source

### 3. Fork Simulator — detailed design
- Choice of forking engine with justification
- Account hot-swap strategy using geyser
- State parity guarantees (slot pinning, blockhash pinning, expiry window)
- RPC provider primary/fallback configuration
- Failover logic and timeout handling
- Anti-sandbox-detection measures for v1

### 4. Risk Graph and Checker Framework
For each of the seven checkers specified in the product spec (Oracle Sanity, Authority Diff, Intent Diff, Contagion Map, MEV/Sandwich, Approval Abuse, Sim-Spoof Detection):
- Exact detection algorithm
- Input data required
- Output schema
- Deterministic guarantees
- Known false-positive and false-negative modes
- Unit-test strategy

The checker framework itself:
- Plugin interface (traits/interfaces)
- Parallel execution model with hard deadlines
- How third parties contribute checkers (v2 path, described at design level)

### 5. LLM Orchestration Layer
- Model choice for intent compilation (small model) with latency and cost numbers
- Model choice for rationale aggregation with latency and cost numbers
- Structured output schemas (JSON Schema or equivalent)
- Prompt templates with full text
- Failure modes and fallback to deterministic rule engine
- Confirmation that LLM output is metadata only, not part of signed payload

### 6. Scorer
- `safety_score` calculation
- `optimality_score` calculation
- Combination rule and threshold logic
- Intent mode parallel candidate scoring design

### 7. Attestation and Signing
- Attestation payload schema (every field, every type)
- Canonical serialization (borsh, SSZ, or equivalent — choose and justify)
- Ed25519 signing for v1
- FROST threshold signing design for v2
- Expiry and slot-pinning semantics
- On-chain verification flow for each enforcement path

### 8. Enforcement Integrations
For each of Lighthouse, Squads, and Jito:
- Integration pattern with specific API calls
- How an attestation is presented to the enforcement surface
- Failure modes when attestation is rejected
- Code-level integration examples or pseudocode

### 9. Override with Time Delay
- `OVERRIDE_APPROVED` attestation type specification
- Time-delay enforcement mechanism per segment (treasuries, agents, users)
- On-chain recording of override events
- Data pipeline for override events into the training loop

### 10. Intent Layer
- NL-to-structured-intent compiler architecture
- Intent JSON schema
- Candidate plan generation for Demo 2 (for v1: hardcoded route generation logic is acceptable if documented)
- Parallel scoring architecture for candidates
- Jito bundle assembly for winning plan

### 11. x402 Monetization
- x402 endpoint specification
- Per-verdict pricing implementation
- Protocol SaaS tier metering
- Rate limiting and abuse prevention

### 12. Agent SDK
- SDK surface (function signatures)
- LangGraph integration pattern
- MCP server pattern
- Authentication and API key management

### 13. Learning Loop and Data Pipeline
- Verdict log schema (append-only)
- Storage choice (postgres, clickhouse, duckdb, parquet on S3 — choose and justify)
- Post-execution outcome capture
- Dataset product architecture for v3

### 14. Security and Threat Model
- Trust assumptions for v1 (single-signer Ciel node)
- Trust assumptions for v2 (Checker Provider + Validator split, FROST consensus)
- Known attack vectors: sim-spoofing, oracle manipulation, replay attacks, attestation forgery
- Mitigations for each
- Bootstrapping trust (code audit plan, bug bounty scope)

### 15. Observability
- Metrics to emit per verdict (latency per component, checker hit rates, verdict distribution)
- Logging strategy with structured fields
- Tracing (OpenTelemetry or equivalent)
- Dashboards for: operator health, checker performance, revenue per segment

### 16. Deployment Architecture
- Infrastructure choice (bare metal, cloud, hybrid) with justification for latency-sensitive workload
- Regions and RPC collocation strategy
- Container orchestration
- CI/CD pipeline
- Environment parity (dev, staging, production fork of mainnet)

### 17. Testing Strategy
- Unit tests per checker
- Integration tests against devnet
- End-to-end test: replay the Drift exploit transaction, assert BLOCK verdict
- Load testing: target RPS and latency under load
- Chaos testing: RPC failover, checker timeout, LLM provider outage

### 18. Five-Week Build Plan (Engineering View)
Map the product spec's Week 1–5 plan to engineering tasks:
- Specific tickets per week
- Dependencies and critical path
- Daily milestones for Week 1 (the load-bearing week)
- Demo rehearsal schedule for Week 5

### 19. Post-v1 Roadmap Engineering Notes
Engineering-level sketches (not full specs) for:
- v2 Checker Provider / Validator split with FROST
- v3 Cross-chain attestation bridge
- v4 Permissionless validator onboarding

### 20. Open Questions and Research Spikes
Any areas where research is inconclusive — document the uncertainty rather than fabricating a decision. Propose time-boxed research spikes for Week 1.

## Rules for the output

- Be an engineer, not a marketer. Technical precision over narrative polish.
- Every technology choice must have a justification and a citation.
- Every latency claim must have a source or a measured benchmark plan.
- Where the product spec says "we do X," specify *exactly how* X is implemented.
- Where the product spec is ambiguous, make the decision and document it — do not kick the ambiguity forward.
- No marketing language. No "we revolutionize." Plain technical prose.
- Call out known risks and unknowns explicitly. Honest limitations beat optimistic hand-waving.
- Target length: as long as it needs to be. If it ends up 8,000+ words, that's fine. Completeness over brevity.
- Use code blocks for schemas, pseudocode, and command examples.
- Use tables for comparative decisions (e.g., Helius vs Triton One).

## Sequence of work

1. Read `./ciel-product-spec.md` completely.
2. Produce a research plan: list every technology decision you need to make.
3. Conduct web research for each decision, citing sources.
4. Write the technical spec in the structure above.
5. Review: verify every feature in the product spec is covered. Any gap is a bug.
6. Write the final file to `./ciel-technical-spec.md`.

Begin.