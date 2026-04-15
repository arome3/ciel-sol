// Verdict logging to PostgreSQL.
// See spec Section 13.1 for the verdict_log table schema.
// See migrations/001_verdict_log.sql for the CREATE TABLE statement.
//
// Design: fire-and-forget via tokio::spawn. DB errors are logged via tracing,
// never propagated to the verdict caller. The signed attestation is the source
// of truth — the database record is observability, not authority.
//
// Known limitations (v1):
// - No retry on DB failure. Extended Postgres outage silently drops log entries.
//   Acceptable for hackathon; production should add bounded retry with backoff
//   or a local WAL buffer that flushes on reconnect.
// - No explicit schema version column. The checker_outputs JSONB is unversioned.
//   If CheckerOutput fields change, queries over historical data must handle both
//   shapes. Mitigated by the fact that CheckerOutput is Borsh-versioned upstream.
//
// Timestamp safety: created_at uses TIMESTAMPTZ (UTC internally in Postgres).
// Pipeline timestamps use chrono::Utc::now(). No server-local-time ambiguity.

use sqlx::PgPool;

/// Data needed to log a verdict. Constructed by the pipeline after signing.
pub struct VerdictLogEntry {
    pub request_type: String,
    pub tx_hash: Vec<u8>,
    pub verdict: String,
    pub safety_score: Option<f32>,
    pub optimality_score: Option<f32>,
    pub attestation: Vec<u8>,
    pub signature: Vec<u8>,
    pub checker_outputs: serde_json::Value,
    pub checker_outputs_hash: Vec<u8>,
    pub checkers_timed_out: Vec<String>,
    pub total_latency_ms: i32,
    pub fork_sim_ms: Option<i32>,
    pub checkers_ms: Option<i32>,
    pub signing_ms: Option<i32>,
}

/// Log a verdict to PostgreSQL. Non-blocking: spawns the insert as a background task.
/// Errors are logged via tracing, not propagated — the pipeline response is not
/// affected by DB failures. See spec Section 13.1.
pub fn log_verdict(pool: &PgPool, entry: VerdictLogEntry) {
    let pool = pool.clone();
    tokio::spawn(async move {
        if let Err(e) = insert_verdict(&pool, &entry).await {
            tracing::error!(
                error = %e,
                verdict = %entry.verdict,
                "failed to log verdict to database"
            );
        }
    });
}

/// Perform the actual SQL INSERT into verdict_log.
/// Uses runtime-checked sqlx::query() to avoid compile-time DATABASE_URL requirement.
async fn insert_verdict(pool: &PgPool, entry: &VerdictLogEntry) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO verdict_log (
            request_type, tx_hash, verdict, safety_score, optimality_score,
            attestation, signature, checker_outputs, checker_outputs_hash,
            checkers_timed_out, total_latency_ms, fork_sim_ms, checkers_ms, signing_ms
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)"#,
    )
    .bind(&entry.request_type)
    .bind(&entry.tx_hash)
    .bind(&entry.verdict)
    .bind(entry.safety_score)
    .bind(entry.optimality_score)
    .bind(&entry.attestation)
    .bind(&entry.signature)
    .bind(&entry.checker_outputs)
    .bind(&entry.checker_outputs_hash)
    .bind(&entry.checkers_timed_out)
    .bind(entry.total_latency_ms)
    .bind(entry.fork_sim_ms)
    .bind(entry.checkers_ms)
    .bind(entry.signing_ms)
    .execute(pool)
    .await?;
    Ok(())
}
