# Ciel PEA — Governance

**Scope:** This document describes how the Ciel PEA specification evolves. The corresponding policy paragraph is duplicated inline in [ciel-pea-1.0.md §10](./ciel-pea-1.0.md) so that a reader of the spec alone has the complete compatibility contract without opening a second file.

---

## 1. Compatibility Policy (authoritative)

Within a major version (`1.x`, `2.x`, …):

1. Optional fields MAY be added at the top level of the envelope, inside the `flags` item schema, or inside the `intent` and `issuer` objects.
2. Required fields in §3.1 of the spec MUST NOT be removed or renamed.
3. The Borsh wire format (spec §4) MUST NOT change in any minor or patch release.
4. Verdict integer values (spec §5) MUST NOT be remapped.
5. Unknown top-level fields and unknown `flags[].code` values MUST be passed through by verifiers; rejection solely on unknown fields is non-conformant.

Any change that breaks rules 1–4 requires a major-version bump.

The `wire.fixture_version` pin (`"ciel_attestation_v1"` in `1.x`) is the single test each implementation must satisfy to claim conformance.

## 2. How changes happen

Until the spec is transferred to a foundation or working group, evolution proceeds as follows:

1. A change is proposed as a pull request against the spec repository with `[PEA/1.x]` or `[PEA/2.0]` in the title.
2. The PR MUST include:
   - A statement of whether the change is additive (rule 1) or breaking (rules 2–4).
   - Updated schema, test vectors, and at least one reference example if the wire or envelope changes.
   - Rationale and a list of conforming implementations that have signed off on the change (one is sufficient for additive; at least two are requested for breaking changes).
3. Review window: at least 10 calendar days for additive changes, 30 days for breaking changes.
4. Merges are performed by the repository maintainers. Current maintainers are listed at the end of this document.

## 3. Deprecation

Deprecation within a major version:

- Optional fields MAY be marked **deprecated** in the schema and human spec.
- Deprecated fields MUST continue to be accepted by verifiers until the next major version.
- Issuers are encouraged to stop emitting deprecated fields within 6 months of the deprecation notice but are not required to.

## 4. Disputes

Disputes about whether a proposed change is additive vs. breaking fall to the maintainers. If maintainers disagree, the change is treated as breaking (requires a major bump). This rule is conservative by design — it protects integrators who built against `1.x` from silent behavioral changes.

## 5. Relationship to the reference implementation

The Ciel project maintains the reference implementation at `crates/ciel-signer`. The reference implementation is NOT a normative source for the spec — the spec itself is normative. The reference implementation MUST pass all test vectors in [ciel-pea-1.0-test-vectors.md](./ciel-pea-1.0-test-vectors.md); divergence is a bug in the reference implementation, not a spec change.

External implementations are welcome and do not require coordination with the Ciel project beyond passing the test vectors. Implementers are encouraged to register conformance at [CONFORMANCE.md](./CONFORMANCE.md) (create on first external adopter).

## 6. Maintainers

- `@arome` — initial author, Ciel project maintainer.
- Additional maintainers will be listed here as the spec attracts external contributors. Governance transfer to a broader foundation or working group is explicitly anticipated and welcomed.

## 7. Licensing

This governance document, the specification it governs, and all example files are licensed under the Apache License 2.0 (see [LICENSE](./LICENSE)). The reference implementation is separately Apache 2.0 licensed in the Ciel repository.
