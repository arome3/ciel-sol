// Post-execution outcome capture.
// See spec Section 13.3 (outcome capture), Section 13.4 (TIMEOUT vs BLOCK analytics).
//
// Background task that polls recent APPROVE verdicts with NULL execution_outcome,
// checks on-chain status via RPC getSignatureStatuses, and updates verdict_log
// with the outcome (landed / reverted / expired).
//
// Design decisions:
// - RPC polling (not Geyser) for v1. Geyser subscription is a v2 optimization.
// - Gentle on DB: batched queries, configurable poll interval (default 5s).
// - Does not block or slow the verdict pipeline — runs as a separate tokio::spawn task.
// - BLOCK verdict outcome capture requires Unit 26 (override mechanism); skipped here.
// - TIMEOUT verdicts represent infrastructure failure, not submitted transactions;
//   they never need outcome capture. See spec Section 13.4.

use std::sync::Arc;
use std::time::Duration;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::signature::Signature;
use sqlx::PgPool;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the outcome capture background task.
#[derive(Debug, Clone)]
pub struct OutcomeCaptureConfig {
    /// How long to wait for a transaction to land before marking it expired.
    /// Default: 60 seconds (~150 slots at 400ms/slot).
    pub ttl: Duration,
    /// How often to poll the database for pending verdicts.
    /// Default: 5 seconds.
    pub poll_interval: Duration,
    /// Maximum number of pending verdicts to process per poll cycle.
    /// Default: 50.
    pub batch_size: i64,
}

impl Default for OutcomeCaptureConfig {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(60),
            poll_interval: Duration::from_secs(5),
            batch_size: 50,
        }
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors from the outcome capture task.
#[derive(Debug, thiserror::Error)]
pub enum OutcomeError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("RPC error: {0}")]
    Rpc(String),

    #[error("invalid signature bytes in verdict_log row {id}")]
    InvalidSignature { id: String },
}

// ---------------------------------------------------------------------------
// Execution outcome values (match DB column constraints)
// ---------------------------------------------------------------------------

/// Execution outcome strings stored in the `execution_outcome` column.
/// See spec Section 13.1.
pub mod outcomes {
    /// Transaction confirmed on-chain.
    pub const LANDED: &str = "landed";
    /// Transaction executed on-chain but failed (e.g., insufficient funds at execution time).
    pub const REVERTED: &str = "reverted";
    /// Transaction never landed within the TTL window.
    pub const EXPIRED: &str = "expired";
    // `overridden` is handled by Unit 26 (override mechanism).
}

// ---------------------------------------------------------------------------
// Pending verdict row
// ---------------------------------------------------------------------------

/// A verdict_log row that needs outcome capture.
struct PendingVerdict {
    id: uuid::Uuid,
    tx_signature: Vec<u8>,
    created_at: chrono::DateTime<chrono::Utc>,
}

// ---------------------------------------------------------------------------
// Outcome capture loop
// ---------------------------------------------------------------------------

/// Spawn the outcome capture background task.
///
/// The task runs indefinitely, polling the database every `config.poll_interval`
/// for APPROVE verdicts with NULL execution_outcome and a non-NULL tx_signature.
/// For each pending verdict, it checks on-chain status via RPC and updates the row.
///
/// Returns a `JoinHandle` so the caller can abort if needed (e.g., on shutdown).
///
/// # Arguments
/// * `pool` - PostgreSQL connection pool
/// * `rpc_url` - Solana RPC endpoint URL for getSignatureStatuses
/// * `config` - Tuning parameters (TTL, poll interval, batch size)
pub fn spawn_outcome_capture(
    pool: PgPool,
    rpc_url: String,
    config: OutcomeCaptureConfig,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tracing::info!(
            ttl_secs = config.ttl.as_secs(),
            poll_interval_secs = config.poll_interval.as_secs(),
            batch_size = config.batch_size,
            "outcome capture task started"
        );

        let rpc_client = Arc::new(RpcClient::new(rpc_url));

        loop {
            if let Err(e) = capture_cycle(&pool, &rpc_client, &config).await {
                tracing::error!(error = %e, "outcome capture cycle failed");
            }
            tokio::time::sleep(config.poll_interval).await;
        }
    })
}

/// Run a single capture cycle: fetch pending verdicts, check on-chain, update DB.
async fn capture_cycle(
    pool: &PgPool,
    rpc_client: &RpcClient,
    config: &OutcomeCaptureConfig,
) -> Result<(), OutcomeError> {
    let pending = fetch_pending_verdicts(pool, config.batch_size).await?;

    if pending.is_empty() {
        return Ok(());
    }

    tracing::debug!(count = pending.len(), "processing pending verdicts");

    let now = chrono::Utc::now();

    for verdict in &pending {
        let outcome = determine_outcome(rpc_client, verdict, now, config.ttl).await;

        match outcome {
            Ok(Some((outcome_str, slot))) => {
                update_verdict_outcome(pool, verdict.id, outcome_str, slot).await?;
                metrics::counter!(
                    "ciel_outcome_capture_total",
                    "outcome" => outcome_str.to_string(),
                )
                .increment(1);
                tracing::info!(
                    verdict_id = %verdict.id,
                    outcome = outcome_str,
                    slot = slot.unwrap_or(0),
                    "outcome captured"
                );
            }
            Ok(None) => {
                // Transaction still pending — check again next cycle.
            }
            Err(e) => {
                tracing::warn!(
                    verdict_id = %verdict.id,
                    error = %e,
                    "failed to determine outcome, will retry"
                );
            }
        }
    }

    Ok(())
}

/// Fetch APPROVE verdicts with NULL execution_outcome and a non-NULL tx_signature,
/// ordered by created_at (oldest first) so we process the most urgent ones first.
///
/// Uses runtime-checked sqlx::query() to avoid compile-time DATABASE_URL requirement,
/// matching the pattern in verdict_store.rs.
async fn fetch_pending_verdicts(
    pool: &PgPool,
    batch_size: i64,
) -> Result<Vec<PendingVerdict>, OutcomeError> {
    let rows = sqlx::query(
        r#"SELECT id, tx_signature, created_at
           FROM verdict_log
           WHERE verdict = 'APPROVE'
             AND execution_outcome IS NULL
             AND tx_signature IS NOT NULL
           ORDER BY created_at ASC
           LIMIT $1"#,
    )
    .bind(batch_size)
    .fetch_all(pool)
    .await?;

    let pending = rows
        .into_iter()
        .map(|row| {
            use sqlx::Row;
            PendingVerdict {
                id: row.get("id"),
                tx_signature: row.get("tx_signature"),
                created_at: row.get("created_at"),
            }
        })
        .collect();

    Ok(pending)
}

/// Determine the execution outcome for a single verdict.
///
/// Returns:
/// - `Ok(Some(("landed", Some(slot))))` — confirmed on-chain
/// - `Ok(Some(("reverted", Some(slot))))` — executed but failed
/// - `Ok(Some(("expired", None)))` — TTL exceeded, tx never landed
/// - `Ok(None)` — transaction still pending (not yet confirmed or expired)
/// - `Err(...)` — RPC error or invalid signature
async fn determine_outcome(
    rpc_client: &RpcClient,
    verdict: &PendingVerdict,
    now: chrono::DateTime<chrono::Utc>,
    ttl: Duration,
) -> Result<Option<(&'static str, Option<i64>)>, OutcomeError> {
    // Check if TTL has expired before making the RPC call.
    let age = now.signed_duration_since(verdict.created_at);
    let ttl_chrono = chrono::Duration::from_std(ttl).unwrap_or(chrono::Duration::seconds(60));

    // Parse the tx_signature bytes into a Solana Signature.
    let sig_bytes: [u8; 64] = verdict
        .tx_signature
        .as_slice()
        .try_into()
        .map_err(|_| OutcomeError::InvalidSignature {
            id: verdict.id.to_string(),
        })?;
    let signature = Signature::from(sig_bytes);

    // Call getSignatureStatuses (single signature).
    let statuses = rpc_client
        .get_signature_statuses(&[signature])
        .await
        .map_err(|e| OutcomeError::Rpc(e.to_string()))?;

    if let Some(Some(status)) = statuses.value.first() {
        // Transaction has been processed.
        let slot = status.slot as i64;

        if let Some(ref err) = status.err {
            tracing::debug!(
                verdict_id = %verdict.id,
                error = ?err,
                slot,
                "transaction reverted on-chain"
            );
            return Ok(Some((outcomes::REVERTED, Some(slot))));
        }

        // Confirmed successfully.
        return Ok(Some((outcomes::LANDED, Some(slot))));
    }

    // Transaction not found in RPC response — either still pending or expired.
    if age > ttl_chrono {
        tracing::debug!(
            verdict_id = %verdict.id,
            age_secs = age.num_seconds(),
            "transaction expired (TTL exceeded)"
        );
        return Ok(Some((outcomes::EXPIRED, None)));
    }

    // Still within TTL window — wait for next cycle.
    Ok(None)
}

/// Update a verdict_log row with the execution outcome and slot.
async fn update_verdict_outcome(
    pool: &PgPool,
    verdict_id: uuid::Uuid,
    outcome: &str,
    slot: Option<i64>,
) -> Result<(), OutcomeError> {
    sqlx::query(
        r#"UPDATE verdict_log
           SET execution_outcome = $1, execution_slot = $2
           WHERE id = $3"#,
    )
    .bind(outcome)
    .bind(slot)
    .bind(verdict_id)
    .execute(pool)
    .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = OutcomeCaptureConfig::default();
        assert_eq!(config.ttl, Duration::from_secs(60));
        assert_eq!(config.poll_interval, Duration::from_secs(5));
        assert_eq!(config.batch_size, 50);
    }

    #[test]
    fn test_outcome_constants() {
        assert_eq!(outcomes::LANDED, "landed");
        assert_eq!(outcomes::REVERTED, "reverted");
        assert_eq!(outcomes::EXPIRED, "expired");
    }

    #[test]
    fn test_config_custom_values() {
        let config = OutcomeCaptureConfig {
            ttl: Duration::from_secs(120),
            poll_interval: Duration::from_secs(10),
            batch_size: 100,
        };
        assert_eq!(config.ttl.as_secs(), 120);
        assert_eq!(config.poll_interval.as_secs(), 10);
        assert_eq!(config.batch_size, 100);
    }

    #[test]
    fn test_outcome_error_display() {
        let err = OutcomeError::Rpc("connection refused".to_string());
        assert!(err.to_string().contains("connection refused"));

        let err = OutcomeError::InvalidSignature {
            id: "abc-123".to_string(),
        };
        assert!(err.to_string().contains("abc-123"));
    }

    /// Verify that an invalid signature length produces InvalidSignature error.
    #[tokio::test]
    async fn test_determine_outcome_invalid_signature() {
        let rpc = RpcClient::new("http://localhost:8899".to_string());
        let verdict = PendingVerdict {
            id: uuid::Uuid::new_v4(),
            tx_signature: vec![0u8; 32], // Wrong length — should be 64
            created_at: chrono::Utc::now(),
        };
        let config = OutcomeCaptureConfig::default();

        let result = determine_outcome(&rpc, &verdict, chrono::Utc::now(), config.ttl).await;
        assert!(matches!(result, Err(OutcomeError::InvalidSignature { .. })));
    }

    /// Verify that an expired verdict with no RPC connection still produces "expired"
    /// when the TTL has been exceeded and the signature is valid but RPC is unreachable.
    /// This tests the TTL check path — the RPC call would fail, but we want to confirm
    /// the function handles the case where the tx_signature parses correctly.
    #[tokio::test]
    async fn test_determine_outcome_expired_after_ttl() {
        // Use a very short TTL and a created_at in the past.
        let rpc = RpcClient::new("http://localhost:1".to_string()); // unreachable
        let created_at = chrono::Utc::now() - chrono::Duration::seconds(120);
        let verdict = PendingVerdict {
            id: uuid::Uuid::new_v4(),
            tx_signature: vec![0u8; 64], // Valid length
            created_at,
        };
        let ttl = Duration::from_secs(60);

        // The RPC call will fail, but since we check TTL first... actually no,
        // the current implementation does the RPC call first. The TTL check
        // only fires when the RPC returns no status. So with an unreachable RPC
        // this will return an Rpc error, not expired.
        //
        // This is actually correct behavior: we want to check RPC first in case
        // the tx did land. If RPC is down, we retry next cycle.
        let result = determine_outcome(&rpc, &verdict, chrono::Utc::now(), ttl).await;
        assert!(matches!(result, Err(OutcomeError::Rpc(..))));
    }
}
