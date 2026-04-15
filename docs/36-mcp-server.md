# 36: MCP Server

## Overview

This unit exposes the Ciel verdict API as an MCP (Model Context Protocol) tool server, allowing any MCP-compatible agent (Claude, GPT, etc.) to call `ciel_evaluate` as a tool. Uses Streamable HTTP transport per the 2025-11-25 MCP spec.

> Authoritative reference: see [Section 12.2](../ciel-technical-spec.md#122-mcp-server-pattern) of the technical spec for the tool definition and transport choice.

## Technical Specifications

- **MCP tool**: `ciel_evaluate` with transaction + optional intent inputs. See [Section 12.2](../ciel-technical-spec.md#122-mcp-server-pattern).
- **Transport**: Streamable HTTP. See [Section 12.2](../ciel-technical-spec.md#122-mcp-server-pattern).
- **Auth**: API key via MCP auth headers. See [Section 12.2](../ciel-technical-spec.md#122-mcp-server-pattern).
- **MCP spec version**: 2025-11-25. See [Section 2.1](../ciel-technical-spec.md#21-core-technologies).

## Key Capabilities

- [ ] Expose ciel_evaluate as an MCP tool via ListTools — verified by MCP client discovery
- [ ] Handle CallTool for ciel_evaluate and return a verdict — verified with a test call
- [ ] Authenticate via API key in MCP headers — verified with valid/invalid keys

## Implementation Guide

1. **Implement MCP server** using Streamable HTTP transport
2. **Register ciel_evaluate tool** with the input schema from Section 12.2
3. **Handle CallTool**: parse input, call CielClient (from unit 35), return formatted result
4. **Run as a standalone process** or embedded in the x402 gateway

**Files / modules to create**:
- `sdk/mcp-server/package.json`
- `sdk/mcp-server/src/index.ts` — MCP server entry point

## Dependencies

### Upstream (units this depends on)
- `35-agent-sdk` — the TypeScript CielClient the MCP server wraps

### Downstream (units that depend on this)
None (leaf unit — agents connect to this).

## Prompt for Claude Code

```
Implement Unit 36: MCP Server

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

This prompt covers ONE unit. Do not implement adjacent units. Do not restate or modify the technical spec.

Required reading before you write any code
Read this unit doc first: ./docs/36-mcp-server.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 12.2 (MCP Server Pattern): the ciel_evaluate tool definition JSON (name, description, inputSchema), Streamable HTTP transport, API key auth pattern
- Section 2.1 (Technology Stack): MCP spec version 2025-11-25, link to spec at modelcontextprotocol.io

Also read these unit docs for upstream dependencies:
- ./docs/35-agent-sdk.md — the TypeScript CielClient class that the MCP server wraps to make API calls

Scope: what to build
In scope:
- TypeScript MCP server at sdk/mcp-server/
- Implements MCP 2025-11-25 spec using Streamable HTTP transport
- Registers one tool: ciel_evaluate with the inputSchema from Section 12.2
- ListTools handler: returns the ciel_evaluate tool definition
- CallTool handler: parses input (transaction base64 + optional intent string), calls CielClient.evaluateTx or evaluateNl, returns formatted verdict
- API key authentication via MCP auth headers
- package.json with start script
- Unit tests with a mock MCP client

Out of scope (these belong to other units):
- Rust SDK — owned by ./docs/35-agent-sdk.md
- x402 gateway — owned by ./docs/34-x402-gateway.md
- API server — owned by ./docs/07-api-server.md

Implementation constraints
- Language: TypeScript
- Libraries: @modelcontextprotocol/sdk (if available), express (for HTTP transport), CielClient from sdk/typescript/
- File location: sdk/mcp-server/
- Transport: Streamable HTTP (default for network-accessible MCP servers)
- MCP spec: 2025-11-25 (JSON-RPC 2.0 messages)
- The tool inputSchema must match Section 12.2 exactly

Verification steps
Before declaring this unit complete, run and report results for every step:
1. Start the MCP server: `npm start` in sdk/mcp-server/
2. MCP client sends tools/list → receives ciel_evaluate with correct inputSchema
3. MCP client sends tools/call for ciel_evaluate with a base64 transaction → receives verdict JSON
4. Request with invalid API key → rejected with MCP error response
5. Request with missing required field (transaction) → rejected with schema validation error

What to report when finished
- List of files created or modified with path
- Test results (pass/fail counts)
- MCP SDK library used (official @modelcontextprotocol/sdk or custom implementation)
- Estimated next unit to build: 37-pre-certified-mode

What NOT to do
- Do not implement the Rust SDK or the x402 gateway
- Do not modify ./ciel-technical-spec.md
- Do not skip verification steps
- Do not declare the unit complete if any test fails
```
