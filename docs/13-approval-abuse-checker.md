# 13: Approval Abuse Checker

## Overview

This unit implements the Approval Abuse checker, which detects unlimited token approvals (`u64::MAX`) granted to unknown or unregistered programs. It parses the CPI graph for `Approve` and `ApproveChecked` SPL Token instructions and cross-references delegates against the known-good program registry.

> Authoritative reference: see [Section 4.3.6](../ciel-technical-spec.md#436-approval-abuse-checker) of the technical spec for the full algorithm, output schema, and test strategy.

## Technical Specifications

- **Algorithm**: Parse CPI for Approve/ApproveChecked, check amount == u64::MAX, check delegate against registry. See [Section 4.3.6](../ciel-technical-spec.md#436-approval-abuse-checker).
- **Deterministic**: Yes — pure instruction parsing. See [Section 4.3.6](../ciel-technical-spec.md#436-approval-abuse-checker).

## Key Capabilities

- [ ] Detect unlimited token approvals (u64::MAX) — verified with a test trace
- [ ] Flag unknown delegate programs not in the registry — verified with an unregistered pubkey
- [ ] Pass when approvals are to known-good programs — verified with a registered program
- [ ] Pass when approval amount is limited (not u64::MAX) — verified with amount < MAX

## Implementation Guide

1. **Implement Checker trait** for ApprovalAbuseChecker
2. **Parse SPL Token Approve instructions**: discriminator byte 4 (Approve) and 13 (ApproveChecked)
3. **Extract amount and delegate from instruction data**
4. **Cross-reference delegate against ProgramRegistry** (shared with Authority Diff checker)

**Files / modules to create**:
- `crates/ciel-checkers/src/approval_abuse.rs`

## Dependencies

### Upstream (units this depends on)
- `05-checker-framework` — provides the Checker trait

### Downstream (units that depend on this)
- `15-scorer` — consumes this checker's output

## Prompt for Claude Code

```
Implement Unit 13: Approval Abuse Checker

Context
You are implementing one unit of the Ciel project — a pre-execution verdict layer for Solana submitted to the Colosseum Frontier Hackathon. The full technical specification is at ./ciel-technical-spec.md and is the authoritative source for all design decisions.

This prompt covers ONE unit. Do not implement adjacent units.

Required reading before you write any code
Read this unit doc first: ./docs/13-approval-abuse-checker.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 4.3.6 (Approval Abuse Checker): algorithm, output schema, test strategy
- Section 4.1 (Checker Plugin Interface): Checker trait and CheckerContext

Also read these unit docs for upstream dependencies:
- ./docs/05-checker-framework.md — Checker trait and CheckerOutput types
- ./docs/11-authority-diff-checker.md — shares the ProgramRegistry (reuse it)

Scope: what to build
In scope:
- ApprovalAbuseChecker struct implementing Checker trait
- SPL Token Approve/ApproveChecked instruction parsing
- u64::MAX detection for unlimited approvals
- Delegate cross-reference against ProgramRegistry
- Unit tests: unlimited approval to unknown program (flag), limited approval (pass), known program (pass)

Out of scope: other checkers, scorer

Implementation constraints
- Language: Rust
- Libraries: solana-sdk, spl-token
- File location: crates/ciel-checkers/src/approval_abuse.rs

Verification steps
1. Run `cargo test --package ciel-checkers` and confirm all approval_abuse tests pass
2. Test with Approve { amount: u64::MAX } to unknown program → assert passed: false, severity: High
3. Test with Approve { amount: 1000 } to unknown program → assert passed: true (limited approval is not abusive)
4. Test with Approve { amount: u64::MAX } to a registered program → assert passed: true

What to report when finished
- List of files created or modified with path
- Test results (pass/fail counts)
- Estimated next unit to build: 14-sim-spoof-checker

What NOT to do
- Do not implement other checkers
- Do not modify ./ciel-technical-spec.md
```
