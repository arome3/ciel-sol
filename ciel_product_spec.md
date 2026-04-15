# Ciel — Product Specification v1.0

**The verdict layer for Solana's agent economy.**

Prepared for: Colosseum Frontier Hackathon (April 6 – May 11, 2026)
Author: Abraham Arome Onoja
Status: Pre-build, submission-ready thesis

---

## 1. Executive Summary

Ciel is the pre-execution verdict layer for Solana — **the last line of defense at the transaction layer.** Every transaction or intent submitted to Ciel is simulated against live mainnet state, judged on both **safety** and **optimality** dimensions by an LLM-orchestrated risk graph, and returned as a signed attestation — APPROVE, WARN, or BLOCK — enforceable on-chain via Lighthouse guard instructions, Squads policy hooks, or Jito bundle preconditions. Calling parties retain final authority: a BLOCK verdict can be overridden with explicit additional approval plus a time delay, preserving protocol sovereignty while adding a required pause for thought.

Ciel accepts two input types:
- **Raw transactions** (defensive mode): "Is this safe to sign?"
- **Natural-language or structured intents** (offensive mode): "What's the safest optimal way to achieve this outcome?"

In offensive mode, multiple candidate execution paths are scored in parallel on `optimality × safety_multiplier`; unsafe paths are eliminated before ranking, not after.

The business is monetized via x402 per-verdict micropayments from agents, SaaS subscriptions for protocols and treasuries, and a slashable verifier insurance pool (v2).

---

## 2. The Problem

### 2.1 Three converging crises on Solana in 2026

**Crisis 1: DeFi security contagion.** On April 1, 2026, Drift Protocol was drained of approximately $285M after an attacker compromised an admin key and manipulated oracle data. Twenty downstream protocols that had integrated Drift's vaults were exposed through composability. The exploit happened five days before the Frontier Hackathon opened.

**Crisis 2: Trust-driven capital flight.** Solana stablecoin supply dropped 17% ($2.7B) in 30 days leading into the hackathon. No competing chain captured the outflow, suggesting capital is exiting due to fear, not competition.

**Crisis 3: The agent economy has no brakes.** AI agents are now generating measurable onchain economic output on Solana. Realms shipped an MCP giving agents DAO governance powers. Helius released a CLI letting agents programmatically generate and fund their own API keys. Solana's sub-cent fees make continuous agent micropayment loops economically viable where legacy chains cannot. But agents operate with no shared risk infrastructure — every exploit is a fresh disaster.

### 2.2 The Solana Foundation's response — and what it explicitly does not solve

Five days after the Drift exploit, the Solana Foundation announced **STRIDE** (Solana Trust, Resilience and Infrastructure for DeFi Enterprises) and **SIRN** (Solana Incident Response Network). STRIDE is a human-assessor program led by Asymmetric Research that evaluates protocols across eight security pillars, providing ongoing operational security and formal verification for protocols above $100M TVL. SIRN is a membership network coordinating threat intelligence and incident response across firms like OtterSec, Neodyme, Squads, and ZeroShadow.

Both are meaningful and necessary. Neither addresses the Drift attack class. The Foundation's own announcement states:

> *"The initiatives address real gaps, but not the mechanics behind the Drift loss itself. Drift's smart contracts were not compromised and its code passed audits. Neither formal verification nor onchain monitoring would have caught the attack, since the transactions were valid by design."*

And further:

> *"These programs do not transfer the underlying responsibility away from the protocols themselves."*

This is the Foundation confirming, unprompted, that there is a runtime gap between annual audits (STRIDE) and post-hoc coordination (SIRN). That gap is exactly where Ciel operates: per-transaction, pre-execution, automated, producing signed attestations enforceable at runtime. STRIDE audits protocols annually. SIRN coordinates response after a loss. Ciel runs on every transaction before it lands. A properly defended protocol needs all three.

### 2.3 Why existing solutions don't solve this

The transaction-simulation category exists but is consumer-wallet-shaped:

- **Blowfish** (acquired by Phantom for ~$55M) provides scan APIs that wallets call to show warnings before a user signs. Consumer popups, not enforceable policy.
- **Pocket Universe** (acquired by Kerberus, 2025) is a browser extension that intercepts transactions before wallet signing. Human-in-the-loop only.
- **Blockaid** provides similar simulation-and-validation APIs for wallet integrations.
- **Lighthouse Protocol** provides on-chain guard instructions that verify state changes and revert transactions if outcomes diverge from previews — but it's a primitive, not a decision layer. Ciel consumes Lighthouse; Lighthouse is not a competitor.

None of these products:
- Serve **autonomous agents** as first-class clients (agents don't see popups)
- Enforce verdicts via **protocol-level policy** (Squads treasury rules, Jito bundle preconditions)
- Model **contagion risk** across composable protocols
- Score **optimality alongside safety** in a single pass
- Accept **intents** as an input type, not just raw transactions
- Resist **simulation spoofing** attacks where malicious contracts detect sandboxed environments and fake good behavior

Ciel addresses all six gaps.

---

## 3. Product Architecture

### 3.1 The verdict pipeline (≤200ms target per verdict)

```
Input → Fork Simulator → Risk Graph (parallel) → Scorer → Signer → Enforcement
```

**3.1.1 Input Layer.** Accepts two payload types:
- `RawTransaction`: base64-encoded unsigned Solana transaction
- `Intent`: structured JSON (`{goal, constraints, budget, deadline}`) or natural-language string compiled to structured intent via a dedicated small model

**3.1.2 Fork Simulator.** Forks current Solana mainnet state using a modified solana-test-validator with account hot-swapping from **Helius enhanced RPC (primary) + Triton One public RPC (fallback)** for redundancy. Executes the candidate transaction against the forked state. Captures: balance deltas, authority changes, CPI call graph, oracle reads, program upgrades, token approvals.

**State parity is non-negotiable.** If the fork diverges from mainnet by even one slot, the Ed25519 attestation will be rejected by Squads policy hooks and Jito bundle preconditions — dropped transactions destroy trust. Mitigation: (a) attestations are pinned to a specific `slot` + `blockhash` with a short expiry window (≤2 slots / ~800ms); (b) enforcement contracts verify the attestation's slot is within the current confirmed window before accepting; (c) the fork validator subscribes to Helius geyser for sub-slot account updates rather than polling. We over-engineer this before anything else — it's the load-bearing wall of the entire product.

**Anti-spoofing:** the fork exposes production RPC endpoints and real slot/blockhash values, making sandbox detection harder than naive simulators. For v1 hackathon scope, we rely on a single hardened primary fork (differential execution across two fork implementations is v2) — this is a deliberate latency-budget tradeoff we defend in Section 8.

**3.1.3 Risk Graph.** An LLM orchestrator (LangGraph) coordinates deterministic checkers that run in parallel:

| Checker | What it catches | Data source |
|---|---|---|
| Oracle Sanity | Drift-style oracle manipulation | Cross-reference Switchboard + Pyth feeds, flag deviations >N σ |
| Authority Diff | Upgrade-authority transfers hidden in "deposit" calls | Parse CPI graph for `SetAuthority`, `Upgrade`, `CloseAccount` |
| Intent Diff | Transaction outcome diverges from stated intent | Semantic compare of intent vs simulated state delta (LLM judge) |
| Contagion Map | Target protocol's dependencies are behaving anomalously | Graph DB of protocol dependencies + real-time behavior baseline |
| MEV/Sandwich | Transaction is being front-run or sandwiched | Jito bundle mempool introspection |
| Approval Abuse | Unlimited token approvals to unknown programs | Static analysis + known-good program registry |
| Sim-Spoof Detection | Malicious contract detects sandbox and fakes behavior | Differential execution across two fork implementations |

All deterministic checkers are open-source and pluggable; third-party checkers can be contributed via PRs, laying the foundation for a v2 decentralized verification network where independent node operators run checker sets. This turns Ciel from middleware into a network-effect platform: the more checkers contributed, the broader the coverage, the stronger the moat.

**Ciel attests only to deterministic outcomes.** Given the same transaction data and the same checker set, every honest verifier reaches the same conclusion — this is the precondition for the v2 threshold-signature network (see Section 10). The LLM's role is not to "decide" safety and is not part of the signed attestation payload. It **weighs and combines** deterministic checker outputs into a human-readable rationale string that is logged as metadata but is cryptographically separate from the verdict itself. A verdict can be independently reproduced from the fork snapshot and checker outputs by anyone; the rationale is a convenience for auditors and human operators, not a trust assumption. This separation is critical for the "LLMs are non-deterministic, how do you audit" objection: the checkers are deterministic, the aggregation is LLM-judged but logged, and every verdict is reproducible from the fork snapshot.

**3.1.4 Scorer.** Produces `(safety_score, optimality_score)` where:
- `safety_score ∈ [0, 1]` — below threshold → BLOCK
- `optimality_score ∈ ℝ⁺` — only computed if safety passes (intent mode only)
- Combined: `final_score = optimality_score × safety_multiplier`, where `safety_multiplier = 0` on fail

In intent mode, multiple candidate plans are scored in parallel and the highest `final_score` wins. **Safety is an auction dimension, not a post-hoc gate.**

**3.1.5 Signer.** Produces an Ed25519-signed attestation over `(tx_hash, verdict, safety_score, optimality_score, checker_outputs_hash, slot, expiry)`. The attestation is the product — small, portable, verifiable on-chain.

**3.1.6 Enforcement.** The attestation is consumed by one of three enforcement paths:
- **Lighthouse guard instructions** (on-chain state verification) for end users
- **Squads policy hook** for multisig treasuries — a Squads transaction won't execute without a valid Ciel attestation in the proposal
- **Jito bundle precondition** — searchers include the attestation in their bundle, Jito validates before inclusion

**Override with time delay.** A BLOCK verdict is not a veto. Calling parties can override any Ciel verdict with explicit additional approval plus a time delay calibrated to context: 24 hours for institutional treasuries, 1 hour for autonomous agents under a Squads policy, configurable per integration. The override itself is a signed attestation of type `OVERRIDE_APPROVED` and is recorded onchain. This preserves protocol sovereignty — Ciel is a required-consideration layer, not a control layer. It also generates valuable training data: every override is a human signal that the checker set missed context, and that signal feeds the risk graph.

### 3.2 The learning loop

Every verdict + post-execution outcome is written to an append-only dataset. When a new exploit occurs, the post-mortem is ingested and new detection primitives are added to the risk graph. The Drift post-mortem becomes a training example; the next exploit feeds the next model revision. This is the compounding moat.

### 3.3 What Ciel is NOT (scope discipline)

- Not a wallet, browser extension, or consumer UI
- Not a token (v1)
- Not a reputation/slashing marketplace (v2 roadmap only)
- Not an intent router that competes with Jupiter — Ciel verifies intent execution; it does not run its own DEX aggregation
- Not a replacement for protocol audits

---

## 4. Go-to-Market

### 4.1 Three customer segments, one product

**Segment A — Autonomous Agents.** Pays per-verdict via x402. Integrates via a LangGraph/MCP-compatible SDK that wraps any agent's `execute_transaction()` call. Cold-start: ship the SDK with three flagship examples (yield optimizer, treasury rebalancer, NFT sniper).

**Segment B — Protocols.** Flat SaaS subscription. Integrates via Lighthouse guard instructions or direct API. Beachhead: **new protocol launches** ("Ciel Launch Shield") — launching protocols have budget, fear, and no legacy integrations to unwind.

**Segment C — Institutional treasuries.** Enterprise contract. Integrates via Squads policy hook. Unlocks Goldman's $108M SOL position and BlackRock BUIDL's $550M on Solana — these need exactly this before they scale further.

### 4.2 Design partner strategy

Target three design partners during the hackathon:
1. **Mercantill** — already building agent spending controls on Squads Grid for enterprise banking. Shares the agent-safety thesis, has no Drift-style PR trauma to navigate, and is the most technically aligned first integration. Ciel attestations feed directly into Mercantill's spending policy engine — a natural, low-friction partnership. This is the realistic first "yes."
2. **Drift Protocol** — strongest possible incentive post-$285M exploit; reputational recovery benefits more from integrating a public safety layer than any other protocol on Solana. Longer sales cycle but highest-signal logo for the submission.
3. **One new-launch protocol** — Launch Shield beachhead proof.

**SIRN-registered protocols are the ideal GTM lead list.** These protocols have publicly self-identified as security-conscious, have internal budget for security tooling, and have existing relationships with the Foundation — the three qualifying signals we'd build a sales pipeline around anyway. Post-hackathon, SIRN membership becomes our pre-qualified outbound list.

### 4.3 Pricing (v1)

- x402 agent verdict: $0.002 per call (economically viable only on Solana)
- Protocol SaaS: $2,500/month flat + $0.0005 per verdict above 1M/month
- Institutional: negotiated, starting $10K/month

---

## 5. Competitive Positioning

| | Blowfish / Blockaid | Pocket Universe | Lighthouse | STRIDE / SIRN | **Ciel** |
|---|---|---|---|---|---|
| Operating layer | Wallet popup | Browser extension | On-chain revert | Protocol program + response network | **Per-transaction runtime** |
| Cadence | Per-tx (human) | Per-tx (human) | Per-tx (on-chain) | Annual audit + post-hoc | **Per-tx (automated, pre-execution)** |
| Client | Wallets | Humans | Protocols | Established protocols (>$10M TVL) | **Agents + protocols + treasuries** |
| Output | Warning string | Popup | On-chain revert | Assessment + monitoring + incident coord | **Signed attestation** |
| Catches Drift-class attacks | ✗ | ✗ | ✗ | ✗ (Foundation's own admission) | **✓** |
| Intent support | ✗ | ✗ | ✗ | ✗ | **✓** |
| Contagion graph | ✗ | ✗ | ✗ | Partial (via SIRN) | **✓** |
| Optimality scoring | ✗ | ✗ | ✗ | ✗ | **✓** |
| Squads / Jito policy enforcement | ✗ | ✗ | ✗ | ✗ | **✓** |
| Anti-simulation-spoofing | Partial | ✗ | N/A | N/A | **✓ (v2 differential fork)** |

Ciel is the first verdict layer built for machine clients, operating at the runtime layer that STRIDE/SIRN explicitly do not address. It is complementary to every tool in this table — a properly defended protocol runs STRIDE for governance, registers with SIRN for incident coordination, integrates Lighthouse for on-chain enforcement, and calls Ciel on every transaction for pre-execution verdict.

---

## 6. The Data Moat

Ciel sees every intent, every candidate execution path, every safety verdict, and every outcome across all users. This is the only dataset of its kind on Solana, because it requires sitting at the pre-execution chokepoint.

- **Year 1**: Train risk graph on exploit corpus
- **Year 2**: Dataset becomes the ground truth for onchain behavioral analytics — the Solana equivalent of order-flow data, which in TradFi is the most valuable dataset in finance
- **Year 3**: License the dataset to researchers, auditors, and institutional risk teams; become the standard attestation format every Solana integration references

---

## 7. Hackathon Build Plan (5 weeks)

### Week 1 — Foundation
- Fork simulator: solana-test-validator + Helius account hot-swap
- Verdict schema + Ed25519 signing infra
- Deterministic checker framework (pluggable)
- Deliverable: raw tx in → simulation trace out

### Week 2 — Risk Graph v1
- Oracle Sanity checker (Switchboard + Pyth cross-reference)
- Authority Diff checker
- Intent Diff checker (LLM judge with structured output)
- Training corpus: Drift post-mortem + 5 prior Solana exploits
- Deliverable: tx in → APPROVE/WARN/BLOCK verdict out with rationale

### Week 3 — Enforcement + Design Partner
- Lighthouse guard instruction integration
- Squads policy hook prototype
- Jito bundle precondition path
- Outreach to Drift team; secure one design partner
- Deliverable: end-to-end enforcement demo

### Week 4 — Intent Layer + Monetization
- Intent compiler (NL → structured intent)
- Parallel candidate scoring (optimality × safety_multiplier) — PoIN-style agent competition stub
- x402 metering per verdict (integrate the open-source x402 SDK from Solana Foundation / QuickNode)
- Agent SDK (LangGraph/MCP-compatible)
- Deliverable: natural-language intent in → safe optimal execution out

### Week 5 — Demo + Submission
- Demo 1: Replay the Drift exploit through Ciel, show it blocking in real time
- Demo 2: Submit "swap 10k USDC → SOL, minimize slippage, MEV-protected" → **PoIN-style agent competition** where multiple agents submit execution plans in parallel → Ciel scores each on `optimality × safety_multiplier` → winner executes under Jito bundle
- Pitch video, technical demo video, GitHub repo with reproducible setup
- Submit to Colosseum

---

## 8. Pressure Test: Technical, Judge & Investor Objections

### 8.1 Technical objections

**Q: "Can you really hit 200ms with LangGraph + RPC + LLM + signing in the loop?"**
A: The 200ms target is for the *happy path* with a warm fork and cached oracle reads. Honest budget breakdown:
- Fork state hot-swap (warm cache): ~40ms
- Parallel deterministic checkers: ~80ms
- LLM aggregation (Llama 3 8B via Groq or local vLLM): ~60ms
- Signing + serialization: <10ms
- **P50 budget: ~190ms. P95 realistic: 350–450ms.**

For high-frequency searchers where even 200ms is too slow, Ciel offers a **pre-certified mode**: the agent submits a policy template ahead of time, Ciel pre-signs a verdict class, and runtime checks become a <20ms lookup rather than a full simulation. This is how we avoid becoming a bottleneck for Jito searchers while still providing safety for the treasury and agent segments where 200–400ms is acceptable.

**Timeout handling:** every verdict request has a hard deadline. On timeout, Ciel returns `WARN: verdict_incomplete` rather than failing open or failing closed — the downstream enforcement contract decides its own policy (Squads treasuries typically require APPROVE; Jito searchers may proceed on WARN).

**Q: "Is the LLM actually needed? Couldn't this be fully deterministic?"**
A: For v1, the LLM is load-bearing in exactly two places: (a) compiling natural-language intents into structured form, and (b) producing auditable rationale strings. The **verdict itself can be reduced to a deterministic rule engine** — and we will ship both paths. Infra-heavy judges who distrust LLMs can use the deterministic-only mode; agent builders who want intent compilation get the LLM path. This is not a retreat; it's product segmentation that strengthens the pitch.

**Q: "Hook and bundle injection is unforgiving — one state mismatch and the attestation is rejected, transaction dropped, user trust destroyed. How do you guarantee state parity?"**
A: This is the load-bearing risk of the entire product and we treat it that way. Four mitigations stacked:
1. Attestations are pinned to a specific `slot` + `blockhash` with a ≤2-slot expiry (~800ms)
2. Enforcement contracts verify the attestation's slot is within the confirmed window before accepting
3. The fork validator subscribes to Helius **geyser** for sub-slot account updates rather than polling RPC
4. Primary RPC (Helius) + fallback RPC (Triton One) with sub-100ms failover
If state parity fails anyway, the attestation simply isn't accepted downstream and the agent retries with a fresh verdict — the failure mode is "slower," not "funds lost." We will dedicate Week 1 entirely to this problem.

**Q: "Sim-spoofing — malicious contracts can detect your sandbox and fake good behavior. How do you defend?"**
A: Honest answer for the hackathon: **differential execution across two fork implementations doubles compute and blows the latency budget, so v1 ships with a single hardened primary fork.** Our v1 defenses are: (a) the fork exposes real production RPC endpoints, real slot values, and real blockhashes, making naive detection harder; (b) we maintain a registry of known sim-detection patterns and flag programs that call suspicious opcodes; (c) for high-value transactions above a threshold, we fall back to differential execution and accept the latency hit. **Full sim-spoof defense is v2.** We document this limitation transparently rather than hand-wave it — judges reward honesty about scope.

### 8.2 Judge objections

**Q: "Isn't this just Blowfish for agents?"**
A: Blowfish is a wallet warning API — a string consumed by a human popup. Ciel is a signed attestation consumed by Squads, Jito, and Lighthouse for on-chain policy enforcement. Different customer (agents and protocols, not humans), different output (signed attestation, not warning string), different enforcement primitive (on-chain policy, not UI popup). The competitive table in Section 5 makes this explicit.

**Q: "Cold start — who integrates first, and why would Drift trust you immediately?"**
A: Drift specifically has the strongest possible incentive: they just lost $285M. Their reputational recovery benefits more from integrating a public safety layer than any other protocol on Solana. We secure the conversation during Week 3 of the hackathon with a clear ask: "be the first design partner, keep your name on our submission, help us harden the risk graph against the exact exploit you suffered." If Drift declines, the fallback is Squads (distribution wedge to every major treasury) or one new-launch protocol (Launch Shield beachhead). **Cold start is a cold-call problem, not a product problem.**

**Q: "Why not just improve Lighthouse?"**
A: Lighthouse is an excellent on-chain enforcement primitive — we integrate with it, not against it. Lighthouse verifies that observed state matches predicted state. Ciel decides whether the predicted state is safe and optimal in the first place. Complementary, not competitive. Our pitch explicitly credits Lighthouse in the enforcement section.

**Q: "Didn't the Solana Foundation already announce STRIDE and SIRN? Isn't this solved?"**
A: Read the Foundation's own announcement: *"Neither formal verification nor onchain monitoring would have caught the attack, since the transactions were valid by design."* STRIDE is annual human assessment for established protocols. SIRN is post-hoc incident coordination. Neither runs on a transaction at runtime, neither serves agents, neither produces enforceable attestations. The Foundation has publicly confirmed the gap Ciel fills. A well-defended protocol in 2026 runs STRIDE for governance, SIRN for response coordination, and Ciel for per-transaction runtime verdict. We're the third leg of a stool the Foundation itself acknowledged is incomplete.

**Q: "Isn't this just Safenet(https://safefoundation.org/safenet) for Solana?"**
A: Architecturally, yes — and that's the point. Safe Foundation, operators of the largest multisig platform in crypto, launched Safenet on EVM with this exact thesis: validator consensus on transaction safety before execution. The pattern is proven at Safe's scale. Ciel brings it to Solana — where no equivalent exists — and extends it in four directions Safenet doesn't cover: (1) agents as first-class clients, not just human wallet users; (2) intents alongside raw transactions, with optimality scoring; (3) tokenless v1 economics using x402 micropayments and SOL staking instead of a new token; (4) native integration with Squads, Jito, and Lighthouse — the Solana primitives Safenet's EVM-centric design doesn't touch. Safenet validates the category. Ciel owns it on Solana.

**Q: "LLMs are non-deterministic. How do you audit a verdict?"**
A: The LLM is not part of the signed attestation. Ciel attests only to **deterministic checker outputs** — given the same transaction data and the same checker set, every honest verifier reaches the same conclusion. This is the precondition for v2's threshold-signature network: you cannot run BFT consensus over a non-deterministic LLM output, and we don't try. The LLM generates a human-readable rationale string that is logged as metadata but is cryptographically separate from the verdict. Any party can reproduce a verdict from the fork snapshot and checker outputs without trusting the LLM at all. This is the same design principle Safe Foundation's Safenet uses on EVM.

This approach is directly validated by a16z's December 2025 research paper *"Runtime Enforcement: A New Line of Defense Against Subtle Numerical Exploits,"* which argues for enforcing key safety properties in deterministic code that auto-reverts violating transactions. The paper states: *"In practice, almost every exploit to date would have tripped one of these checks during execution."* Ciel operationalizes exactly this thesis — deterministic checkers, runtime enforcement, auditable verdicts — at the pre-execution layer, three months after a16z published the intellectual foundation.

### 8.3 Investor objections

**Q: "Will protocols actually outsource risk decisions to a third party?"**
A: Protocols don't outsource *decisions* to Ciel — they outsource the *simulation and checking work* under policies they own. And critically, **Ciel verdicts are overridable.** A BLOCK can be overridden with explicit additional approval plus a context-calibrated time delay (24h for treasuries, 1h for agents under policy). The override is itself a signed attestation recorded onchain. This is the Safenet pattern, adapted: we add a required pause for thought, we don't take final authority. Combined with the fact that (a) policies are user-configured, (b) the checker set is open-source and pluggable, and (c) v2 decentralizes verdict production across independent validators running FROST threshold consensus — there is no single party with unilateral control over anyone's transactions. Ciel is a required-consideration layer, not a control layer.

**Q: "Is this middleware or a platform? Middleware has no network effects."**
A: v1 is middleware; v2 is a platform. The transition happens through the open checker framework: once third parties can contribute detection primitives and run verifier nodes, Ciel becomes a two-sided network — more checkers attract more clients, more clients attract more checkers, and the exploit dataset compounds across all participants. The roadmap in Section 10 is explicit about this transition. We are deliberately shipping middleware first because it's the fastest path to paying customers and real exploit data.

**Q: "Does this introduce latency or friction to Solana execution? Solana prioritizes speed above all else."**
A: Ciel is opt-in per transaction. Agents and protocols that don't need safety don't call it. For those that do, the latency is front-loaded into the *verdict request*, not injected into consensus — the transaction itself lands on mainnet at normal Solana speed once the attestation is obtained. We are not slowing Solana down; we are adding a sub-second pre-flight check that the caller chooses to run. The searchers who need sub-50ms get the pre-certified mode. The treasuries and agents who can absorb 200–400ms get the full risk graph. **Friction is segmented, not imposed.**

**Q: "What's the defensible moat beyond the brand?"**
A: Three layers. (1) The exploit dataset — every verdict and outcome feeds a training corpus no competitor can replicate without sitting at the same chokepoint. (2) The enforcement integrations — once Squads, Jito, and Lighthouse consume Ciel attestations natively, replacing us requires replacing three integrations simultaneously. (3) In v2, the network of independent verifier nodes and contributed checkers becomes a standards play, similar to how Chainlink became the default oracle not because it was technically best but because it was universally integrated.

---


## 9. Why This Wins Frontier

- **Urgency**: answers the $285M Drift exploit five weeks after it happened
- **Last line of defense at the transaction layer**: complements STRIDE (annual governance), SIRN (post-hoc response), and Lighthouse (on-chain enforcement primitive); closes the runtime gap the Foundation itself acknowledged
- **Pattern validated on EVM**: Safe Foundation's Safenet ships the same thesis (validator consensus on transaction safety before execution) on EVM. The pattern is proven; Ciel owns it on Solana, extended to agents and intents
- **Horizontal infra**: every protocol in the Colosseum portfolio becomes a design partner, not a competitor
- **Fits both prize tracks**: Grand Champion (transformative infra) and Public Goods Award (protects the ecosystem)
- **AI-native in a way that matters**: LLM is a judge in a verifiable risk graph, not a chat wrapper; attestations are deterministic, not LLM-dependent
- **Institutional unlock**: Goldman, BlackRock BUIDL, Western Union stablecoin all need exactly this
- **Compounding moat**: exploit dataset grows with every incident; overrides generate negative-labeled training data
- **Buildable in 5 weeks**: scoped to attestation layer + three enforcement paths + one intent demo

---

## 10. Post-Hackathon Roadmap

### v1 — Hackathon (May 2026)
Verdict layer + three enforcement paths (Lighthouse, Squads, Jito) + intent demo. Single-signer attestations from a Ciel-operated node. Checker Providers and Validators collapsed into one role. Tokenless. x402 monetization live.

### v2 — Decentralized Verifier Network (Q3 2026)
Split the monolithic node into two distinct roles, each open to third parties:

- **Checker Providers** — independent security firms (OtterSec, Neodyme, Asymmetric Research, Sec3, and others, including SIRN members) contribute detection logic. Each checker is open-source, versioned, and reputation-scored. Checker Providers earn fees proportional to the verdicts their checkers contribute to.
- **Validators** — independent operators run the consensus layer. Validators execute the full checker set against the fork, coordinate to produce a **FROST threshold signature** over the verdict, and publish attestations onchain. Validators stake **native SOL** (not a new token) and can be slashed for dishonest attestations. Staking yields are paid in USDC from x402 fees.

This split turns Ciel from middleware into a two-sided platform. Checker Providers compete on detection quality; Validators compete on reliability and latency. The network tolerates up to 1/3 Byzantine validators while still producing correct attestations. All attestations are onchain-visible and independently verifiable.

### v3 — Standards and Data (Q4 2026)
Permissionless validator onboarding with SOL staking minimums. Launch the **onchain behavioral dataset product** — aggregated, anonymized intent-and-outcome data licensed to institutional risk teams, auditors, and researchers. This is the "order-flow data for Solana" thesis from Section 6.

### v4 — Cross-chain and Horizontal Expansion (2027)
Bridge Ciel attestations to EVM chains (Ethereum, Base) via a cross-chain attestation protocol. Open the checker framework to third-party contributions beyond curated security firms. Become the standard attestation format for the agent economy across multiple L1s.

**Design inspiration.** The v2 network draws architectural patterns from Safe Foundation's Safenet (threshold signatures, deterministic attestations, Checker/Validator role separation) while adapting them to Solana: native SOL staking instead of a new token, Lighthouse guard instructions instead of Safe Guard, and agent-first rather than wallet-first positioning. The pattern of decentralized onchain enforcement is validated by Safenet's EVM deployment; Ciel is its Solana-native equivalent.

---

## 11. Team Fit

This product is a direct application of the author's prior work on Meridian (AI-native SOC replacement platform) and SecGate (outcome-based monetization for cybersecurity AI). Ciel is Meridian + SecGate compiled to Solana. The author's LangGraph multi-agent orchestration experience, ERC-8004/ScopeGuard verifiable-intent work, and prior OpenTender.AI verdict-engine patterns all port directly.

---

## 12. Pitch Line

> Solana's throughput made the agent economy possible. Drift proved it isn't safe. Ciel is the verdict layer every agent and every protocol calls before a transaction lands — a compounding risk graph that turns every exploit into the next one's detection rule, and every intent into a safe optimal execution.