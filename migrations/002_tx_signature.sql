-- Add Solana transaction signature for post-execution outcome capture.
-- The tx_hash column is a SHA-256 of the serialized transaction, not the on-chain signature.
-- getSignatureStatuses requires the actual Solana signature (first signature in the tx).
-- See spec Section 13.3 and docs/09-outcome-capture.md.

ALTER TABLE verdict_log ADD COLUMN tx_signature BYTEA;

-- Index for the outcome capture background task: query APPROVE rows with NULL outcome.
CREATE INDEX idx_verdict_log_pending_outcome
    ON verdict_log (verdict, created_at)
    WHERE execution_outcome IS NULL;
