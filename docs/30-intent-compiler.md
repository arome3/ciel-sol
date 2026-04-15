# 30: Intent Compiler

## Overview

This unit implements the NL-to-structured-intent compiler that converts natural language strings like "swap 10k USDC to SOL, minimize slippage" into structured Intent JSON objects via the Groq LLM client. This is the entry point for Flow C (natural language intent mode).

> Authoritative reference: see [Section 10.1](../ciel-technical-spec.md#101-nl-to-structured-intent-compiler) (compiler architecture), [Section 10.2](../ciel-technical-spec.md#102-intent-json-schema) (Intent schema), and [Section 5.3](../ciel-technical-spec.md#53-prompt-templates) (intent compilation prompt).

## Technical Specifications

- **Compiler**: LLM call (Groq) with the intent compilation prompt. See [Section 10.1](../ciel-technical-spec.md#101-nl-to-structured-intent-compiler).
- **Intent schema**: goal, constraints, budget, deadline. See [Section 10.2](../ciel-technical-spec.md#102-intent-json-schema).
- **Prompt**: exact text in Section 5.3. See [Section 5.3](../ciel-technical-spec.md#53-prompt-templates).

## Key Capabilities

- [ ] Compile "swap 10k USDC to SOL, minimize slippage" into a valid Intent struct — verified by parsing output
- [ ] Default slippage to 1% when not specified — verified by omitting slippage in input
- [ ] Return error on unparseable intents — verified with gibberish input
- [ ] Schema-validate the LLM output — verified by checking all required fields present

## Implementation Guide

1. **Implement `compile_intent`**: calls LlmClient.compile_intent from unit 16
2. **Implement schema validation**: validate the parsed Intent against required fields
3. **Wire into the API server**: add NL intent handling to the /v1/verdict endpoint

**Files / modules to create**:
- `crates/ciel-intent/Cargo.toml`
- `crates/ciel-intent/src/compiler.rs`

## Dependencies

### Upstream (units this depends on)
- `16-llm-client` — provides the compile_intent LLM function

### Downstream (units that depend on this)
- `31-candidate-generator` — consumes the compiled Intent struct

## Prompt for Claude Code

```
Implement Unit 30: Intent Compiler

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

This prompt covers ONE unit. Do not implement adjacent units. Do not restate or modify the technical spec.

Required reading before you write any code
Read this unit doc first: ./docs/30-intent-compiler.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 10.1 (NL-to-Structured-Intent Compiler): the compile_intent function signature and architecture
- Section 10.2 (Intent JSON Schema): the Intent struct with goal, constraints (max_slippage_pct, min_output_amount, preferred_dex, mev_protection), budget (input_token, input_amount, max_fee_lamports), and deadline
- Section 5.3 (Prompt Templates): the exact intent compilation prompt text — use this verbatim
- Section 5.2 (Structured Output Schemas): the Intent JSON Schema for LLM output validation
- Section 1.4 (Data Flows) Flow C: NL intent → LLM compilation → continues as Flow B

Also read these unit docs for upstream dependencies:
- ./docs/16-llm-client.md — the LlmClient.compile_intent API you will call

Scope: what to build
In scope:
- Rust crate at crates/ciel-intent/
- Intent struct definition matching Section 10.2 exactly (with Serialize, Deserialize, Clone derives)
- compile_intent(nl_intent: &str, llm_client: &LlmClient) -> Result<Intent> function per Section 10.1
- Schema validation: check all required fields are present, types are correct, defaults are applied
- Default application: slippage defaults to 1%, mev_protection defaults to true, deadline defaults to 30s from now
- Wire into API server: update /v1/verdict to handle the nl_intent field (Flow C)
- Unit tests: successful compilation, schema validation failure, default application, gibberish rejection

Out of scope (these belong to other units):
- Candidate generation from compiled intent — owned by ./docs/31-candidate-generator.md
- Parallel scoring — owned by ./docs/32-parallel-scoring.md
- LLM client implementation — owned by ./docs/16-llm-client.md (already built)

Implementation constraints
- Language: Rust
- Libraries: serde, serde_json, chrono (for deadline timestamps)
- File location: crates/ciel-intent/src/compiler.rs
- Intent struct must match Section 10.2 field-for-field
- Use the exact prompt template from Section 5.3 — do not modify it

Verification steps
Before declaring this unit complete, run and report results for every step:
1. Run `cargo test --package ciel-intent` and confirm all tests pass
2. "swap 10k USDC to SOL, minimize slippage" → valid Intent with goal="swap USDC for SOL", budget.input_amount=10000, constraints.max_slippage_pct=1.0 (default)
3. Gibberish input ("asdfghjkl") → returns Err with clear error message
4. "transfer 500 SOL to Abc123" → valid Intent with goal="transfer SOL", budget.input_amount=500
5. No slippage specified → defaults to 1.0%; no mev_protection specified → defaults to true

What to report when finished
- List of files created or modified with path
- Test results (pass/fail counts)
- Any deviations from the technical spec, with justification
- Estimated next unit to build: 31-candidate-generator

What NOT to do
- Do not implement candidate generation or scoring
- Do not modify the LLM client
- Do not modify the prompt template from Section 5.3
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
```
