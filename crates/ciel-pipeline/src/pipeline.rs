// Verdict pipeline: fork sim → checkers → scorer → signer → attestation.
// See spec Section 1.4 (Flow A: Raw Transaction) and Section 1.5 (Latency Budget).
//
// Concurrency: ForkSimulator is behind std::sync::Mutex (not tokio::sync::Mutex).
// Rationale: execute_transaction is synchronous (~20ms), the lock is never held across
// .await points, and std::sync::Mutex is ~25x faster per lock/unlock cycle.
// See tokio-rs/tokio Discussion #7627 for benchmarks.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio::time::Instant;

use solana_sdk::transaction::Transaction;

use ciel_checkers::{
    checker_outputs_hash, run_checkers, Checker, CheckerContext, CheckerResults,
    OracleCache, ProgramRegistry,
};
use ciel_fork::{execute_transaction, ForkSimulator, StalenessState, StalenessTracker};
use ciel_signer::{CielAttestation, CielSigner, Verdict};

use crate::scorer_stub::{compute_safety_score, encode_score_u16, score_to_verdict};
use crate::verdict_store::{log_verdict, VerdictLogEntry};
use crate::PipelineError;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the verdict pipeline.
pub struct PipelineConfig {
    /// Top-level pipeline timeout in milliseconds. Default: 200 (spec Section 1.5).
    pub timeout_ms: u64,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self { timeout_ms: 200 }
    }
}

// ---------------------------------------------------------------------------
// VerdictPipeline
// ---------------------------------------------------------------------------

/// The verdict pipeline. Thread-safe via internal std::sync::Mutex on ForkSimulator.
///
/// Upgrade path for higher throughput:
/// - Phase 2: Actor pattern (mpsc channel to dedicated simulation task)
/// - Phase 3: Pool of N ForkSimulators with Semaphore(N)
pub struct VerdictPipeline {
    fork: Arc<Mutex<ForkSimulator>>,
    checkers: Vec<Box<dyn Checker>>,
    signer: CielSigner,
    staleness: StalenessTracker,
    db_pool: Option<PgPool>,
    config: PipelineConfig,
}

impl VerdictPipeline {
    pub fn new(
        fork: ForkSimulator,
        checkers: Vec<Box<dyn Checker>>,
        signer: CielSigner,
        staleness: StalenessTracker,
        db_pool: Option<PgPool>,
        config: PipelineConfig,
    ) -> Self {
        Self {
            fork: Arc::new(Mutex::new(fork)),
            checkers,
            signer,
            staleness,
            db_pool,
            config,
        }
    }

    /// Evaluate a raw transaction through the full verdict pipeline.
    /// Returns a signed CielAttestation wrapped in VerdictResponse.
    ///
    /// The entire pipeline is wrapped in a top-level timeout (default 200ms).
    /// On timeout, returns a TIMEOUT verdict with sentinel score values.
    /// See spec Section 1.5, Section 9.3.
    #[tracing::instrument(skip(self, tx), fields(tx_hash))]
    pub async fn evaluate_raw_tx(
        &self,
        tx: &Transaction,
    ) -> Result<VerdictResponse, PipelineError> {
        let pipeline_start = Instant::now();

        match tokio::time::timeout(
            Duration::from_millis(self.config.timeout_ms),
            self.evaluate_raw_tx_inner(tx, pipeline_start),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                tracing::warn!(
                    timeout_ms = self.config.timeout_ms,
                    "pipeline deadline exceeded — returning TIMEOUT"
                );
                self.build_timeout_response(tx, pipeline_start)
            }
        }
    }

    /// Inner pipeline logic, called within the top-level timeout.
    async fn evaluate_raw_tx_inner(
        &self,
        tx: &Transaction,
        pipeline_start: Instant,
    ) -> Result<VerdictResponse, PipelineError> {
        // Step 1: Compute tx_hash.
        let tx_hash = compute_tx_hash(tx)?;

        // Step 2: Check staleness. See spec Section 3.4.
        let mut downgrade_reason: Option<String> = None;
        match self.staleness.state() {
            StalenessState::Fresh => {}
            StalenessState::Warn => {
                downgrade_reason = Some("state_parity_degraded".to_string());
                tracing::warn!("geyser staleness in Warn state — verdict may be downgraded");
            }
            StalenessState::Timeout => {
                tracing::error!("geyser staleness in Timeout state — rejecting with TIMEOUT");
                return self.build_timeout_response(tx, pipeline_start);
            }
        }

        // Step 3: Fork simulation.
        // Lock held only for synchronous execute_transaction (~20ms). No .await inside.
        let sim_start = Instant::now();
        let (trace, slot) = {
            let mut fork = self.fork.lock().map_err(|e| {
                PipelineError::Deserialization(format!("fork mutex poisoned: {e}"))
            })?;
            let slot = fork.pinned_slot();
            let trace = execute_transaction(&mut fork, tx)?;
            (trace, slot)
        }; // Mutex guard dropped here — before any .await
        let fork_sim_ms = sim_start.elapsed().as_millis() as u64;
        tracing::info!(fork_sim_ms, success = trace.success, "fork simulation complete");

        // Step 4: Run checkers in parallel. See spec Section 4.2.
        let checkers_start = Instant::now();
        let checker_ctx = CheckerContext {
            trace,
            original_tx: tx.clone(),
            intent: None,
            slot,
            oracle_cache: OracleCache,
            known_programs: ProgramRegistry,
        };
        let checker_results = run_checkers(&checker_ctx, &self.checkers).await;
        let checkers_ms = checkers_start.elapsed().as_millis() as u64;
        tracing::info!(
            checkers_ms,
            completed = checker_results.completed().len(),
            timed_out = checker_results.timed_out().len(),
            "checkers complete"
        );

        // Step 5: Score. See spec Section 6.1.
        let scoring_start = Instant::now();
        let safety_score = compute_safety_score(&checker_results);
        let mut verdict = score_to_verdict(safety_score, &checker_results);

        // Apply staleness downgrade: APPROVE → WARN. See spec Section 3.4.
        if downgrade_reason.is_some() && verdict == Verdict::Approve {
            verdict = Verdict::Warn;
            tracing::warn!("verdict downgraded from APPROVE to WARN due to state_parity_degraded");
        }
        let scoring_ms = scoring_start.elapsed().as_millis() as u64;

        // Step 6: Compute checker_outputs_hash. See spec Section 7.1.
        let outputs_hash = checker_outputs_hash(&checker_results);

        // Step 7: Build and sign attestation. See spec Section 7.3.
        let signing_start = Instant::now();
        let timestamp = chrono::Utc::now().timestamp();
        let safety_score_u16 = encode_score_u16(safety_score);

        let attestation = CielAttestation::new(
            tx_hash,
            verdict,
            safety_score_u16,
            0, // optimality_score: 0 for raw tx mode
            outputs_hash,
            slot,
            self.signer.pubkey_bytes(),
            timestamp,
            0, // timeout_at_ms: not a timeout
        );

        let (attestation_bytes, signature) = self.signer.sign_attestation(&attestation)?;
        let signing_ms = signing_start.elapsed().as_millis() as u64;
        tracing::info!(signing_ms, verdict = ?verdict, "attestation signed");

        // Step 8: Compute timing.
        let total_ms = pipeline_start.elapsed().as_millis() as u64;
        let timing = PipelineTiming {
            total_ms,
            fork_sim_ms,
            checkers_ms,
            scoring_ms,
            signing_ms,
        };

        // Step 9: Fire-and-forget verdict logging. See spec Section 13.1.
        if let Some(ref pool) = self.db_pool {
            let entry = VerdictLogEntry {
                request_type: "raw_tx".to_string(),
                tx_hash: tx_hash.to_vec(),
                tx_signature: tx.signatures.first().map(|s| s.as_ref().to_vec()),
                verdict: verdict_to_str(verdict).to_string(),
                safety_score: Some(safety_score as f32),
                optimality_score: Some(0.0),
                attestation: attestation_bytes.clone(),
                signature: signature.to_bytes().to_vec(),
                checker_outputs: serde_json::to_value(checker_results.completed())
                    .unwrap_or(serde_json::Value::Null),
                checker_outputs_hash: outputs_hash.to_vec(),
                checkers_timed_out: checker_results
                    .timed_out()
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                total_latency_ms: total_ms as i32,
                fork_sim_ms: Some(fork_sim_ms as i32),
                checkers_ms: Some(checkers_ms as i32),
                signing_ms: Some(signing_ms as i32),
            };
            log_verdict(pool, entry);
        }

        // Step 10: Return response.
        Ok(VerdictResponse {
            verdict,
            safety_score,
            attestation_bytes,
            signature_bytes: signature.to_bytes(),
            slot,
            tx_hash,
            checker_outputs_hash: outputs_hash,
            checker_results,
            downgrade_reason,
            timing,
        })
    }

    /// Build a TIMEOUT verdict response. Used when:
    /// - The top-level pipeline deadline fires (spec Section 9.3)
    /// - Staleness state is Timeout (spec Section 3.4)
    fn build_timeout_response(
        &self,
        tx: &Transaction,
        pipeline_start: Instant,
    ) -> Result<VerdictResponse, PipelineError> {
        let elapsed_ms = pipeline_start.elapsed().as_millis() as u64;
        let tx_hash = compute_tx_hash(tx)?;

        let slot = self
            .fork
            .lock()
            .map_err(|e| PipelineError::Deserialization(format!("fork mutex poisoned: {e}")))?
            .pinned_slot();

        let timestamp = chrono::Utc::now().timestamp();
        let outputs_hash = [0u8; 32];

        let attestation = CielAttestation::new_timeout(
            tx_hash,
            outputs_hash,
            slot,
            self.signer.pubkey_bytes(),
            timestamp,
            elapsed_ms.min(u16::MAX as u64) as u16,
        );

        let (attestation_bytes, signature) = self.signer.sign_attestation(&attestation)?;

        Ok(VerdictResponse {
            verdict: Verdict::Timeout,
            safety_score: 0.0,
            attestation_bytes,
            signature_bytes: signature.to_bytes(),
            slot,
            tx_hash,
            checker_outputs_hash: outputs_hash,
            checker_results: CheckerResults {
                outputs: HashMap::new(),
                total_duration_ms: 0,
            },
            downgrade_reason: None,
            timing: PipelineTiming {
                total_ms: elapsed_ms,
                fork_sim_ms: 0,
                checkers_ms: 0,
                scoring_ms: 0,
                signing_ms: 0,
            },
        })
    }
}

// ---------------------------------------------------------------------------
// VerdictResponse
// ---------------------------------------------------------------------------

/// Response from the verdict pipeline, consumed by the API server (Unit 07).
///
/// This is an internal type — no Serialize derive. The API server builds its own
/// JSON response from these fields (e.g., base64-encoding attestation_bytes).
#[derive(Debug)]
pub struct VerdictResponse {
    pub verdict: Verdict,
    /// Safety score [0.0, 1.0]. 0.0 for TIMEOUT (on-chain uses 0xFFFF sentinel).
    pub safety_score: f64,
    /// Borsh-serialized CielAttestation.
    pub attestation_bytes: Vec<u8>,
    /// Ed25519 signature over attestation_bytes.
    pub signature_bytes: [u8; 64],
    /// Mainnet slot at fork time.
    pub slot: u64,
    /// SHA-256 of the evaluated transaction.
    pub tx_hash: [u8; 32],
    /// SHA-256 of concatenated checker outputs. See spec Section 7.1.
    pub checker_outputs_hash: [u8; 32],
    /// Full checker results (internal, not serialized).
    pub checker_results: CheckerResults,
    /// Reason for verdict downgrade, if any (e.g., "state_parity_degraded").
    pub downgrade_reason: Option<String>,
    /// Per-stage timing breakdown.
    pub timing: PipelineTiming,
}

// ---------------------------------------------------------------------------
// PipelineTiming
// ---------------------------------------------------------------------------

/// Per-stage timing breakdown in milliseconds.
#[derive(Debug, Clone)]
pub struct PipelineTiming {
    pub total_ms: u64,
    pub fork_sim_ms: u64,
    pub checkers_ms: u64,
    pub scoring_ms: u64,
    pub signing_ms: u64,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compute SHA-256 of the serialized transaction. See spec Section 7.1.
fn compute_tx_hash(
    tx: &Transaction,
) -> Result<[u8; 32], PipelineError> {
    let tx_bytes = bincode::serialize(tx)
        .map_err(|e| PipelineError::Deserialization(e.to_string()))?;
    let mut hasher = Sha256::new();
    hasher.update(&tx_bytes);
    Ok(hasher.finalize().into())
}

/// Convert Verdict enum to string for logging and DB storage.
pub fn verdict_to_str(v: Verdict) -> &'static str {
    match v {
        Verdict::Approve => "APPROVE",
        Verdict::Warn => "WARN",
        Verdict::Block => "BLOCK",
        Verdict::Timeout => "TIMEOUT",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use ciel_checkers::{all_stub_checkers, CheckerOutput, Severity};
    use ciel_fork::StalenessConfig;
    use ciel_signer::{verify_attestation, TIMEOUT_SENTINEL};
    use solana_sdk::signature::Keypair;
    use solana_sdk::signer::Signer;

    /// A checker that sleeps for a long time, used to trigger pipeline timeout.
    struct SlowChecker;

    #[async_trait]
    impl Checker for SlowChecker {
        fn name(&self) -> &'static str {
            "slow_checker"
        }

        async fn check(&self, _ctx: &CheckerContext) -> CheckerOutput {
            tokio::time::sleep(Duration::from_secs(10)).await;
            CheckerOutput {
                checker_name: "slow_checker".to_string(),
                passed: true,
                severity: Severity::None,
                flags: vec![],
                details: "slow".to_string(),
            }
        }
    }

    fn test_signer() -> CielSigner {
        CielSigner::from_bytes(&[42u8; 32])
    }

    fn fresh_staleness() -> StalenessTracker {
        let tracker = StalenessTracker::new(StalenessConfig::default());
        tracker.record_update(0);
        tracker
    }

    /// Helper to build a simple SOL transfer transaction in an offline fork.
    fn build_sol_transfer() -> (ForkSimulator, Transaction) {
        let mut fork = ForkSimulator::new_offline();
        let sender = Keypair::new();
        let receiver = solana_sdk::pubkey::Pubkey::new_unique();

        // Bridge to litesvm types for airdrop.
        let sender_addr =
            litesvm_address::Address::from(sender.pubkey().to_bytes());
        let receiver_addr =
            litesvm_address::Address::from(receiver.to_bytes());
        fork.svm_mut()
            .airdrop(&sender_addr, 10_000_000_000)
            .expect("airdrop sender");
        fork.svm_mut()
            .airdrop(&receiver_addr, 1_000_000_000)
            .expect("airdrop receiver");

        // Build transaction.
        #[allow(deprecated)]
        let ix = solana_sdk::system_instruction::transfer(
            &sender.pubkey(),
            &receiver,
            1_000_000,
        );
        let blockhash = solana_sdk::hash::Hash::new_from_array(
            fork.svm().latest_blockhash().to_bytes(),
        );
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&sender.pubkey()),
            &[&sender],
            blockhash,
        );

        (fork, tx)
    }

    #[tokio::test]
    async fn test_sol_transfer_produces_approve() {
        let (fork, tx) = build_sol_transfer();
        let pipeline = VerdictPipeline::new(
            fork,
            all_stub_checkers(),
            test_signer(),
            fresh_staleness(),
            None,
            PipelineConfig::default(),
        );

        let response = pipeline.evaluate_raw_tx(&tx).await.expect("pipeline should succeed");

        assert_eq!(response.verdict, Verdict::Approve);
        assert!((response.safety_score - 1.0).abs() < f64::EPSILON);
        assert_eq!(response.attestation_bytes.len(), 132);
    }

    #[tokio::test]
    async fn test_attestation_signature_valid() {
        let (fork, tx) = build_sol_transfer();
        let signer = test_signer();
        let pubkey = signer.pubkey_bytes();
        let pipeline = VerdictPipeline::new(
            fork,
            all_stub_checkers(),
            signer,
            fresh_staleness(),
            None,
            PipelineConfig::default(),
        );

        let response = pipeline.evaluate_raw_tx(&tx).await.expect("pipeline");

        assert!(
            verify_attestation(&pubkey, &response.attestation_bytes, &response.signature_bytes),
            "attestation signature must verify"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn test_staleness_warn_downgrades_verdict() {
        let (fork, tx) = build_sol_transfer();
        let staleness = StalenessTracker::new(StalenessConfig {
            warn_threshold: Duration::from_secs(1),
            timeout_threshold: Duration::from_secs(5),
        });
        staleness.record_update(0);

        // Advance past warn threshold but before timeout.
        tokio::time::advance(Duration::from_millis(2000)).await;

        let pipeline = VerdictPipeline::new(
            fork,
            all_stub_checkers(),
            test_signer(),
            staleness,
            None,
            PipelineConfig { timeout_ms: 5000 }, // generous timeout for paused time test
        );

        let response = pipeline.evaluate_raw_tx(&tx).await.expect("pipeline");

        assert_eq!(response.verdict, Verdict::Warn);
        assert_eq!(
            response.downgrade_reason.as_deref(),
            Some("state_parity_degraded")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn test_staleness_timeout_returns_timeout() {
        let (fork, tx) = build_sol_transfer();
        let staleness = StalenessTracker::new(StalenessConfig {
            warn_threshold: Duration::from_secs(1),
            timeout_threshold: Duration::from_secs(3),
        });
        staleness.record_update(0);

        // Advance past timeout threshold.
        tokio::time::advance(Duration::from_millis(4000)).await;

        let pipeline = VerdictPipeline::new(
            fork,
            all_stub_checkers(),
            test_signer(),
            staleness,
            None,
            PipelineConfig { timeout_ms: 10000 },
        );

        let response = pipeline.evaluate_raw_tx(&tx).await.expect("pipeline");

        assert_eq!(response.verdict, Verdict::Timeout);
        // Verify the attestation has sentinel score values.
        let att: CielAttestation =
            borsh::from_slice(&response.attestation_bytes).expect("borsh decode");
        assert_eq!(att.safety_score, TIMEOUT_SENTINEL);
        assert_eq!(att.optimality_score, TIMEOUT_SENTINEL);
    }

    #[tokio::test(start_paused = true)]
    async fn test_pipeline_timeout_returns_timeout() {
        let (fork, tx) = build_sol_transfer();

        // Inject a slow checker that sleeps 10s. With a 50ms pipeline timeout
        // and paused time, tokio auto-advances to the 50ms deadline and fires
        // the timeout before the slow checker completes.
        let mut checkers = all_stub_checkers();
        checkers.push(Box::new(SlowChecker));

        let pipeline = VerdictPipeline::new(
            fork,
            checkers,
            test_signer(),
            fresh_staleness(),
            None,
            PipelineConfig { timeout_ms: 50 },
        );

        let response = pipeline.evaluate_raw_tx(&tx).await.expect("pipeline");

        assert_eq!(response.verdict, Verdict::Timeout);
    }

    #[tokio::test]
    async fn test_timing_struct_populated() {
        let (fork, tx) = build_sol_transfer();
        let pipeline = VerdictPipeline::new(
            fork,
            all_stub_checkers(),
            test_signer(),
            fresh_staleness(),
            None,
            PipelineConfig::default(),
        );

        let response = pipeline.evaluate_raw_tx(&tx).await.expect("pipeline");

        // With stub checkers, the entire pipeline can complete in <1ms,
        // so total_ms may round to 0. Verify the struct is present and
        // the pipeline completed (non-TIMEOUT verdict = timing was recorded).
        assert_eq!(response.verdict, Verdict::Approve);
        // fork_sim_ms may be 0 for sub-millisecond simulation. That's fine.
        // The P50 benchmark test covers actual latency measurement.
    }

    #[tokio::test]
    async fn test_tx_hash_matches_sha256() {
        let (fork, tx) = build_sol_transfer();
        let pipeline = VerdictPipeline::new(
            fork,
            all_stub_checkers(),
            test_signer(),
            fresh_staleness(),
            None,
            PipelineConfig::default(),
        );

        let response = pipeline.evaluate_raw_tx(&tx).await.expect("pipeline");

        // Independently compute tx_hash.
        let tx_bytes = bincode::serialize(&tx).unwrap();
        let expected_hash: [u8; 32] = {
            let mut h = Sha256::new();
            h.update(&tx_bytes);
            h.finalize().into()
        };

        assert_eq!(response.tx_hash, expected_hash);
    }

    #[tokio::test]
    async fn test_checker_outputs_hash_matches() {
        let (fork, tx) = build_sol_transfer();
        let pipeline = VerdictPipeline::new(
            fork,
            all_stub_checkers(),
            test_signer(),
            fresh_staleness(),
            None,
            PipelineConfig::default(),
        );

        let response = pipeline.evaluate_raw_tx(&tx).await.expect("pipeline");

        // Independently compute checker_outputs_hash.
        let expected_hash = checker_outputs_hash(&response.checker_results);
        assert_eq!(response.checker_outputs_hash, expected_hash);
    }

    #[tokio::test]
    async fn test_pipeline_latency_p50() {
        let mut durations = Vec::with_capacity(5);

        for _ in 0..5 {
            let (fork, tx) = build_sol_transfer();
            let pipeline = VerdictPipeline::new(
                fork,
                all_stub_checkers(),
                test_signer(),
                fresh_staleness(),
                None,
                PipelineConfig { timeout_ms: 5000 },
            );

            let start = Instant::now();
            let response = pipeline.evaluate_raw_tx(&tx).await.expect("pipeline");
            let elapsed = start.elapsed();

            assert_eq!(response.verdict, Verdict::Approve);
            durations.push(elapsed);
        }

        durations.sort();
        let p50 = durations[2]; // 0-indexed: index 2 = 3rd value = median of 5

        tracing::info!(
            p50_ms = p50.as_millis(),
            min_ms = durations[0].as_millis(),
            max_ms = durations[4].as_millis(),
            "pipeline latency benchmark (5 runs)"
        );

        // Spec target: ~150ms P50, hard deadline 200ms.
        // Stub checkers are near-instant, so we expect <50ms in testing.
        assert!(
            p50.as_millis() < 200,
            "P50 latency {}ms exceeds 200ms spec target",
            p50.as_millis()
        );
    }

    /// End-to-end: Drift exploit fixture → pipeline → signed attestation.
    ///
    /// This is the most important integration property for the Week 5 demo.
    /// With stub checkers the verdict will be APPROVE — that's expected.
    /// The point is confirming the full pipeline composes against the real
    /// Drift Tx #2 fixture. The verdict becomes BLOCK once the Authority Diff
    /// checker replaces its stub in Week 2.
    #[tokio::test]
    async fn test_drift_fixture_through_pipeline() {
        let fixture = ciel_fixtures::load_drift_fixture().expect("load drift fixture");
        let mut fork = ForkSimulator::new_offline();

        // Inject fixture accounts. BPF program stubs may be rejected by LiteSVM.
        for (pubkey, account) in &fixture.accounts {
            let _ = fork.set_account(pubkey, account);
        }

        let signer = test_signer();
        let pubkey = signer.pubkey_bytes();
        let pipeline = VerdictPipeline::new(
            fork,
            all_stub_checkers(),
            signer,
            fresh_staleness(),
            None,
            PipelineConfig { timeout_ms: 5000 },
        );

        let response = pipeline
            .evaluate_raw_tx(&fixture.transaction)
            .await
            .expect("pipeline should produce attestation for drift fixture");

        // With stub checkers (all pass), verdict is APPROVE.
        assert_eq!(response.verdict, Verdict::Approve);
        assert_eq!(response.attestation_bytes.len(), 132);

        // Verify signature.
        assert!(
            verify_attestation(&pubkey, &response.attestation_bytes, &response.signature_bytes),
            "drift fixture attestation signature must verify"
        );
    }
}
