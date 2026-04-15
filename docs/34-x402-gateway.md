# 34: x402 Gateway

## Overview

This unit implements the x402 payment gateway as a TypeScript Express middleware that sits in front of the Rust API server. Agents pay $0.002 in USDC per verdict via the HTTP 402 protocol. The gateway verifies payment before proxying the request to the Ciel API server.

> Authoritative reference: see [Section 11](../ciel-technical-spec.md#11-x402-monetization) of the technical spec for endpoint specification, pricing, and rate limiting.

## Technical Specifications

- **Protocol**: x402 HTTP 402 payment protocol. See [Section 11.1](../ciel-technical-spec.md#111-x402-endpoint-specification).
- **Pricing**: $0.002 per verdict (USDC). See [Section 11.2](../ciel-technical-spec.md#112-per-verdict-pricing).
- **SDK**: x402-solana-toolkit (Express middleware). See [Section 2.1](../ciel-technical-spec.md#21-core-technologies).
- **Rate limiting**: 100 req/s per payer, 500 req/s per API key. See [Section 11.4](../ciel-technical-spec.md#114-rate-limiting-and-abuse-prevention).
- **SaaS metering**: API key-based counter in PostgreSQL. See [Section 11.3](../ciel-technical-spec.md#113-protocol-saas-tier-metering).

## Key Capabilities

- [ ] Return HTTP 402 with payment requirements when no payment is included — verified by curl
- [ ] Accept x402 payment header and proxy to Ciel API — verified with test payment
- [ ] Rate limit per payer address — verified by exceeding limit
- [ ] SaaS tier: API key bypass of x402 for subscribed protocols — verified with valid API key

## Implementation Guide

1. **Set up Express app** at `gateway/`
2. **Integrate x402-solana-toolkit** Express middleware for payment verification
3. **Implement reverse proxy** to the Rust API server on localhost:8080
4. **Implement SaaS API key tier**: bypass x402 for valid API keys, meter usage in Postgres
5. **Implement rate limiting**: token bucket per payer address

**Files / modules to create**:
- `gateway/package.json`
- `gateway/src/index.ts` — Express app
- `gateway/src/x402.ts` — x402 middleware config
- `gateway/src/metering.ts` — SaaS tier metering
- `gateway/Dockerfile`

## Dependencies

### Upstream (units this depends on)
- `07-api-server` — the Rust API server this gateway proxies to

### Downstream (units that depend on this)
None (leaf unit — clients connect through this gateway).

## Prompt for Claude Code

```
Implement Unit 34: x402 Gateway

Context
You are implementing one unit of the Ciel project. The full technical specification is at ./ciel-technical-spec.md.

Required reading before you write any code
Read this unit doc first: ./docs/34-x402-gateway.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 11 (x402 Monetization): all subsections — endpoint spec, pricing, SaaS metering, rate limiting
- Section 2.1 (Technology Stack): x402 entry with SDK link
- Section 16.2 (Container Orchestration): the x402 gateway container config

Also read: ./docs/07-api-server.md — the upstream server this gateway proxies to

Scope: TypeScript Express app at gateway/, x402 middleware, reverse proxy, SaaS metering, rate limiting, Dockerfile.

Out of scope: Rust API server, agent SDK.

Implementation constraints
- Language: TypeScript
- Libraries: express, x402-solana-toolkit (or manual x402 implementation), http-proxy-middleware, pg (for metering)
- File location: gateway/
- USDC mint: EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v
- Treasury pubkey: configurable via CIEL_TREASURY_PUBKEY env var
- Upstream server: http://localhost:8080 (the Rust API server from unit 07)
- Rate limiting: use token bucket algorithm per payer address

Verification steps
Before declaring this unit complete, run and report results for every step:
1. Start the gateway + upstream Ciel API server
2. curl without payment → HTTP 402 with JSON body showing payment_required.amount, currency, recipient
3. curl with valid x402 payment header → request proxied to Ciel API, VerdictResponse returned
4. curl with valid API key (Bearer token) → bypasses x402 payment, proxied to API
5. Exceed rate limit (send 200 requests in 1 second) → HTTP 429 Too Many Requests
6. SaaS tier: send requests with API key, confirm usage count incremented in database
7. Verify the Dockerfile builds and the gateway starts correctly in a container

What to report when finished
- List of files created or modified with path
- Test results (pass/fail counts)
- Whether x402-solana-toolkit worked or if manual implementation was needed (resolves Open Question #7)
- x402 payment verification latency (should be <5ms)
- Estimated next unit to build: 35-agent-sdk

What NOT to do
- Do not implement the Rust API server (that is unit 07)
- Do not implement the agent SDK (that is unit 35)
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
- Do not declare the unit complete if any test fails
```
