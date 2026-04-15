# 43: Video and Submission

## Overview

This unit produces the final submission artifacts: a 3-minute pitch video, a 5-minute technical demo video, a polished README with reproducible setup instructions, and the Colosseum submission. This is the capstone unit — everything built in Weeks 1-4 converges here.

> Authoritative reference: see [Section 18 Week 5](../ciel-technical-spec.md#week-5--demo--submission) for the full submission schedule.

## Technical Specifications

- **Pitch video**: 3 minutes, covering problem (Drift $285M), solution (verdict layer), demo highlights. See [Section 18 Week 5](../ciel-technical-spec.md#week-5--demo--submission).
- **Technical demo video**: 5 minutes, showing Demo 1 and Demo 2 with narration. See [Section 18 Week 5](../ciel-technical-spec.md#week-5--demo--submission).
- **README architecture diagram**: Mermaid diagram from [Section 1.1](../ciel-technical-spec.md#11-architecture-diagram).

## Key Capabilities

- [ ] Pitch video (3 min) recorded and edited — verified by reviewing recording
- [ ] Technical demo video (5 min) showing Demo 1 and Demo 2 — verified by reviewing recording
- [ ] README.md with: project description, architecture diagram, setup instructions, demo replay instructions — verified by following setup on a clean machine
- [ ] GitHub repo is public, clean, and has reproducible setup — verified by cloning and running
- [ ] Colosseum submission is complete — verified by confirmation receipt

## Implementation Guide

1. **Record Demo 1 and Demo 2** using asciinema (terminal recording) + screen capture
2. **Record pitch video**: use the narration notes from units 41/42, focus on the 30-second story from the copilot deep dive
3. **Write README.md**: project overview, architecture diagram (from Section 1.1), prerequisites, setup, demo replay instructions
4. **Clean up the repo**: remove dead code, add .env.example, verify all tests pass
5. **Submit to Colosseum** before Saturday deadline

**Files / modules to create**:
- `README.md`
- `.env.example`
- `demos/record.sh` — recording automation script
- Video files (not committed; uploaded to submission platform)

## Dependencies

### Upstream (units this depends on)
- `41-demo1-drift-replay` — Demo 1 recording source
- `42-demo2-intent-flow` — Demo 2 recording source

### Downstream (units that depend on this)
None (final unit).

## Prompt for Claude Code

```
Implement Unit 43: Video and Submission

Context
You are implementing the final unit of the Ciel project. The full technical specification is at ./ciel-technical-spec.md.

Required reading before you write any code
Read this unit doc first: ./docs/43-video-and-submission.md — it contains the overview, capabilities checklist, implementation guide, and dependency graph for this unit.
Read these sections of ./ciel-technical-spec.md in order:
- Section 18 Week 5: full submission schedule (Thu-Sat)
- Section 1.1 (Architecture Diagram): the Mermaid diagram to include in the README

Also read:
- ./docs/41-demo1-drift-replay.md — Demo 1 script and narration
- ./docs/42-demo2-intent-flow.md — Demo 2 script and narration

Scope: README.md, .env.example, recording automation script, submission checklist.

In scope:
- README.md with: one-paragraph summary, architecture Mermaid diagram, prerequisites (Rust, Node.js, Postgres, Solana CLI), setup instructions, demo replay commands, links to tech spec and product spec
- .env.example with all required environment variables (HELIUS_API_KEY, TRITON_API_KEY, GROQ_API_KEY, etc.)
- demos/record.sh — asciinema recording script for both demos
- Submission checklist: verify GitHub repo is public, README is complete, videos are uploaded

Out of scope: actual video recording (requires human), Colosseum platform interaction.

Implementation constraints
- README should be under 500 lines — concise but complete
- .env.example must NOT contain real API keys — use placeholder values like YOUR_HELIUS_API_KEY_HERE
- Include a "Quick Demo" section that lets judges replay Demo 1 with a single command
- The recording script should use asciinema for terminal capture and optionally screen recording for the pitch
- All environment variables: HELIUS_API_KEY, TRITON_API_KEY, GROQ_API_KEY, FIREWORKS_API_KEY, CIEL_SIGNING_KEY_PATH, DATABASE_URL, CIEL_TREASURY_PUBKEY

Verification steps
Before declaring this unit complete, run and report results for every step:
1. README.md renders correctly on GitHub — Mermaid architecture diagram displays, all links work
2. .env.example lists ALL required environment variables with descriptions
3. `git clone → cp .env.example .env → fill in keys → docker compose up → ciel-demo replay fixtures/drift-exploit/` works on a fresh machine
4. `cargo test` passes with all tests green
5. demos/record.sh produces an asciinema recording file for Demo 1 and Demo 2
6. Submission checklist is complete:
   - [ ] GitHub repo is public
   - [ ] README has project description, architecture diagram, setup instructions, demo replay instructions
   - [ ] .env.example is present (no real keys)
   - [ ] All tests pass
   - [ ] Demo 1 and Demo 2 run successfully
   - [ ] Pitch video recorded (3 min)
   - [ ] Technical demo video recorded (5 min)
   - [ ] Colosseum submission form completed

What to report when finished
- List of files created or modified with path
- Submission readiness checklist status (all items checked or which are pending)
- README word count
- Any issues discovered during the clean-machine test

What NOT to do
- Do not include real API keys in any committed file
- Do not modify the technical specification or product specification
- Do not skip the clean-machine test — it catches setup instructions that assume local state
- Do not declare submission-ready if any checklist item is incomplete
```
