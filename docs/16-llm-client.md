# 16: LLM Client

## Overview

This unit implements the async HTTP client for LLM inference, supporting Groq (primary) and Fireworks AI (fallback). It handles two roles: rationale aggregation (summarizing checker outputs into a human-readable explanation) and intent compilation (NL → structured JSON). The client includes timeout handling, provider failover, and structured JSON output parsing.

> Authoritative reference: see [Section 5](../ciel-technical-spec.md#5-llm-orchestration-layer) of the technical spec for model choices, prompt templates, structured output schemas, and failure modes.

## Technical Specifications

- **Primary provider**: Groq, Llama 3.x 8B, JSON mode. See [Section 5.1](../ciel-technical-spec.md#51-model-choices).
- **Fallback provider**: Fireworks AI. See [Section 5.4](../ciel-technical-spec.md#54-failure-modes-and-fallback).
- **Prompt templates**: rationale aggregation and intent compilation. See [Section 5.3](../ciel-technical-spec.md#53-prompt-templates).
- **Structured output schemas**: JSON Schema for both roles. See [Section 5.2](../ciel-technical-spec.md#52-structured-output-schemas).
- **Timeout**: 80ms Groq → fallback to Fireworks with 150ms. See [Section 5.4](../ciel-technical-spec.md#54-failure-modes-and-fallback).

## Key Capabilities

- [ ] Call Groq API with structured JSON output and receive a valid response — verified with a test prompt
- [ ] Fail over to Fireworks when Groq times out — verified by mocking a slow Groq response
- [ ] Parse rationale aggregation response into structured JSON — verified against the schema
- [ ] Parse intent compilation response into an Intent struct — verified against the schema
- [ ] Return None gracefully when both providers fail (deterministic-only mode) — verified by mocking both as failing

## Implementation Guide

1. **Create LlmClient struct**: holds provider configs (URL, API key, model name), reqwest::Client
2. **Implement `aggregate_rationale`**: sends checker outputs + prompt template, parses structured response
3. **Implement `compile_intent`**: sends NL string + prompt template, parses Intent struct
4. **Implement failover**: Groq with timeout → Fireworks with extended timeout → None
5. **Wire into the pipeline**: the pipeline calls aggregate_rationale after scoring (non-blocking)

**Key gotchas**:
- LLM response is NEVER part of the signed attestation — this is metadata only (Section 5.5)
- The rationale call can run concurrently with signing since it doesn't affect the attestation
- JSON mode may require `response_format: { "type": "json_object" }` in the API call — verify per provider

**Files / modules to create**:
- `crates/ciel-llm/Cargo.toml`
- `crates/ciel-llm/src/lib.rs`
- `crates/ciel-llm/src/client.rs` — LlmClient struct with provider failover
- `crates/ciel-llm/src/prompts.rs` — prompt templates from Section 5.3
- `crates/ciel-llm/src/schemas.rs` — response types (RationaleResponse, IntentResponse)

## Dependencies

### Upstream (units this depends on)
None. The LLM client is a standalone async HTTP library with no code dependencies on other Ciel crates.

### Downstream (units that depend on this)
- `12-intent-diff-checker` — optional LLM metadata enrichment uses this client
- `17-drift-replay-e2e` — the E2E test includes rationale generation
- `30-intent-compiler` — intent compilation uses compile_intent

## Prompt for Claude Code

```
Implement Unit 16: LLM Client

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

Required reading before you write any code
Read this unit doc first: ./docs/16-llm-client.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 5.1 (Model Choices): Groq primary, Fireworks fallback, model names, latency targets
- Section 5.2 (Structured Output Schemas): JSON Schema for both roles
- Section 5.3 (Prompt Templates): exact prompt text for rationale aggregation and intent compilation
- Section 5.4 (Failure Modes and Fallback): timeout handling, provider failover, deterministic fallback
- Section 5.5 (LLM Output Is Metadata Only): the invariant — LLM output never enters the signed attestation

No upstream unit docs to read — this is a standalone library with no Ciel crate dependencies.

Scope: what to build
In scope:
- Rust crate at crates/ciel-llm/
- LlmClient struct with Groq and Fireworks provider configs
- aggregate_rationale(checker_outputs, verdict, safety_score, tx_hash) -> Option<RationaleResponse>
- compile_intent(nl_intent: &str) -> Result<Intent>
- Provider failover: Groq (80ms timeout) → Fireworks (150ms) → None
- Prompt templates from Section 5.3
- Response parsing with serde_json
- Tests with mocked HTTP responses

Out of scope: scorer, checker logic, API server changes, intent candidate generation

Implementation constraints
- Language: Rust
- Libraries: reqwest (async HTTP), serde_json, tokio::time::timeout
- File location: crates/ciel-llm/
- API keys via environment: GROQ_API_KEY, FIREWORKS_API_KEY
- The LLM client returns Option — None means both providers failed and the caller uses deterministic-only mode

Verification steps
1. Run `cargo test --package ciel-llm` with mocked HTTP responses — all tests pass
2. With Groq API key: call aggregate_rationale with test checker outputs, confirm valid JSON response
3. Mock Groq timeout → confirm Fireworks fallback fires
4. Mock both failing → confirm None returned (deterministic fallback)
5. Call compile_intent with "swap 100 USDC for SOL" → confirm parsed Intent struct

What to report when finished
- Files created, test results
- Groq P50 latency if live testing was possible
- Estimated next unit: 17-drift-replay-e2e

What NOT to do
- Do not let LLM output enter the attestation payload
- Do not implement the scorer or checkers
- Do not modify ./ciel-technical-spec.md
```
