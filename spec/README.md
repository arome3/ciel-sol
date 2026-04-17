# Ciel PEA — Pre-Execution Attestation Standard

**Latest version:** [Ciel PEA / 1.0](./ciel-pea-1.0.md)
**Machine-readable schema:** [ciel-pea-1.0.schema.json](./ciel-pea-1.0.schema.json)
**Test vectors:** [ciel-pea-1.0-test-vectors.md](./ciel-pea-1.0-test-vectors.md)
**License:** Apache License 2.0 (see [LICENSE](./LICENSE))
**Governance:** [GOVERNANCE.md](./GOVERNANCE.md)
**Canonical URL:** `https://spec.ciel.xyz/ciel-pea-1.0`

---

## What is Pre-Execution Attestation?

A **Pre-Execution Attestation (PEA)** is a cryptographically signed verdict about a specific transaction, issued *before* the transaction is submitted to the network, that downstream consumers (wallets, agents, on-chain programs, policy engines) can verify without trusting the issuer's infrastructure.

PEA complements existing attestation services on a different axis:

| Primitive | What it attests | When it's issued | Example |
|---|---|---|---|
| [Solana Attestation Service (SAS)](https://attest.solana.com) | Static facts about an **account** (KYC, accreditation, eligibility) | Any time | "Wallet X is KYC-verified" |
| [Ethereum Attestation Service (EAS)](https://attest.org) | Arbitrary claims signed by an issuer | Any time | "This NFT was reviewed" |
| **Ciel PEA** | A safety verdict about a **specific transaction** | Before submission, scoped to a small slot window | "This TX is safe to execute; expires in 2 slots" |

PEA is the category; **Ciel PEA / 1.0** is the reference implementation. Other implementations are welcome to conform to the same wire format.

## Why a standard

Per-transaction safety verdicts are being produced by a growing set of providers (wallet-integrated scanners, policy engines, agent-security services). Without a common format, every consumer writes bespoke integration code for every issuer, and cross-issuer interoperability is impossible. A common versioned schema with pinned wire bytes removes that friction and enables:

- A wallet or agent runtime consuming PEAs from multiple issuers without per-issuer adapters
- On-chain programs verifying PEAs via a single canonical instruction
- Policy engines composing verdicts from multiple issuers under a uniform decision function
- Auditors replaying and verifying historical verdicts deterministically

## Quick start

```bash
# 1. Read the spec
open ciel-pea-1.0.md

# 2. Validate a sample against the schema (node example)
npx ajv-cli validate -s ciel-pea-1.0.schema.json -d examples/block.json

# 3. Follow the test vectors to verify a signature end-to-end
open ciel-pea-1.0-test-vectors.md
```

## Directory layout

```
spec/
├── README.md                        ← you are here
├── ciel-pea-1.0.md                  ← the specification (human-readable)
├── ciel-pea-1.0.schema.json         ← the specification (JSON Schema)
├── ciel-pea-1.0-test-vectors.md     ← step-by-step verification walkthrough
├── LICENSE                          ← Apache 2.0
├── GOVERNANCE.md                    ← how the spec evolves
├── test-public-key.txt              ← Ed25519 public key for verifying example signatures
└── examples/
    ├── approve.json                 ← verdict=APPROVE
    ├── warn.json                    ← verdict=WARN
    ├── block.json                   ← verdict=BLOCK (canonical test vector)
    └── timeout.json                 ← verdict=TIMEOUT
```

## Status

- **Specification:** Published 2026-04-17. Additive-only compatibility within the `1.x` series (see [GOVERNANCE.md](./GOVERNANCE.md)).
- **Reference implementation:** Ciel — [github.com/ciel](https://github.com/ciel) (adjust URL once org is claimed).
- **Conforming implementations:** Listed in [CONFORMANCE.md](./CONFORMANCE.md) once external adopters register.

## Contact

- Spec repository: `https://github.com/ciel/spec` (proposed)
- Proposal for changes: file an issue against the spec repository, or a PR with `[PEA/1.x]` in the title
- Contact for the reference implementation: see the Ciel project README
