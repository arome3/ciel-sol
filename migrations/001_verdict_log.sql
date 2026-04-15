-- Verdict Log Schema
-- See spec Section 13.1 for full schema design.
-- Applied by: sqlx migrate run (or manually via psql)

CREATE EXTENSION IF NOT EXISTS "pgcrypto";

CREATE TABLE verdict_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Request
    request_type VARCHAR(20) NOT NULL,  -- 'raw_tx', 'intent', 'nl_intent'
    tx_hash BYTEA NOT NULL,
    intent JSONB,
    nl_intent TEXT,

    -- Verdict
    verdict VARCHAR(10) NOT NULL,       -- 'APPROVE', 'WARN', 'BLOCK', 'TIMEOUT'
    safety_score REAL,
    optimality_score REAL,
    attestation BYTEA NOT NULL,         -- full Borsh-serialized attestation
    signature BYTEA NOT NULL,           -- Ed25519 signature

    -- Checker details
    checker_outputs JSONB NOT NULL,     -- array of CheckerOutput
    checker_outputs_hash BYTEA NOT NULL,
    checkers_timed_out TEXT[],          -- names of checkers that timed out

    -- LLM
    rationale TEXT,
    rationale_model VARCHAR(50),
    intent_diff_llm_analysis JSONB,    -- optional LLM enrichment (metadata only)

    -- Timing
    total_latency_ms INTEGER NOT NULL,
    fork_sim_ms INTEGER,
    checkers_ms INTEGER,
    llm_ms INTEGER,
    signing_ms INTEGER,

    -- Post-execution (filled later)
    execution_outcome VARCHAR(20),      -- 'landed', 'reverted', 'expired', 'overridden'
    execution_slot BIGINT,
    actual_state_delta JSONB,

    -- Override
    is_override BOOLEAN DEFAULT FALSE,
    override_reason TEXT,
    original_verdict_id UUID REFERENCES verdict_log(id)
);

CREATE INDEX idx_verdict_log_created ON verdict_log(created_at);
CREATE INDEX idx_verdict_log_verdict ON verdict_log(verdict);
CREATE INDEX idx_verdict_log_tx_hash ON verdict_log(tx_hash);

-- API key metering for SaaS tier (see spec Section 11.3)
CREATE TABLE api_key_usage (
    api_key_id UUID,
    month DATE,
    verdict_count BIGINT DEFAULT 0,
    PRIMARY KEY (api_key_id, month)
);
