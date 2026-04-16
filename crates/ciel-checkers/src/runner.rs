// Parallel checker fan-out runner.
// See spec Section 4.2 (Parallel Execution Model) and Section 2.2 (Tokio pattern).

use std::time::Duration;

use futures::future::join_all;
use tokio::time::timeout;

use crate::traits::{
    Checker, CheckerContext, CheckerResults, CheckerStatus, CHECKER_DEADLINE_MS,
};

/// Run all checkers in parallel with a per-checker timeout.
///
/// Each checker gets its own clone of `ctx` and runs concurrently via
/// `futures::join_all`. A checker that exceeds `CHECKER_DEADLINE_MS` produces
/// `CheckerStatus::TimedOut` — it does not fail the entire fan-out.
///
/// See spec Section 4.2.
pub async fn run_checkers(
    ctx: &CheckerContext,
    checkers: &[Box<dyn Checker>],
) -> CheckerResults {
    let start = tokio::time::Instant::now();
    let deadline = Duration::from_millis(CHECKER_DEADLINE_MS);

    let futures: Vec<_> = checkers
        .iter()
        .map(|c| {
            let ctx = ctx.clone();
            let name = c.name().to_string();
            async move {
                match timeout(deadline, c.check(&ctx)).await {
                    Ok(output) => (name, CheckerStatus::Completed(output)),
                    Err(_) => {
                        tracing::warn!(
                            checker = %name,
                            deadline_ms = CHECKER_DEADLINE_MS,
                            "checker timed out"
                        );
                        (name, CheckerStatus::TimedOut)
                    }
                }
            }
        })
        .collect();

    let results = join_all(futures).await;
    CheckerResults {
        outputs: results.into_iter().collect(),
        total_duration_ms: start.elapsed().as_millis() as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stubs::all_stub_checkers;
    use crate::traits::{CheckerOutput, OracleCache, ProgramRegistry, Severity};
    use async_trait::async_trait;
    use ciel_fork::SimulationTrace;
    use solana_sdk::message::Message;
    use solana_sdk::transaction::Transaction;
    use std::collections::HashMap;

    /// Build a minimal CheckerContext for testing.
    fn test_context() -> CheckerContext {
        CheckerContext {
            trace: SimulationTrace {
                success: true,
                error: None,
                balance_deltas: HashMap::new(),
                cpi_graph: vec![],
                account_changes: vec![],
                logs: vec![],
                oracle_reads: vec![],
                token_approvals: vec![],
                compute_units_consumed: 0,
                fee: 5000,
            },
            original_tx: Transaction::new_unsigned(Message::new(&[], None)),
            intent: None,
            slot: 100,
            oracle_cache: OracleCache::default(),
            known_programs: ProgramRegistry,
        }
    }

    /// A checker that sleeps for a configurable duration, used to test timeout.
    struct SlowStub {
        delay: Duration,
    }

    #[async_trait]
    impl Checker for SlowStub {
        fn name(&self) -> &'static str {
            "slow_stub"
        }

        async fn check(&self, _ctx: &CheckerContext) -> CheckerOutput {
            tokio::time::sleep(self.delay).await;
            CheckerOutput {
                checker_name: "slow_stub".to_string(),
                passed: true,
                severity: Severity::None,
                flags: vec![],
                details: "slow".to_string(),
            }
        }
    }

    #[tokio::test]
    async fn test_run_all_stubs_complete() {
        let ctx = test_context();
        let checkers = all_stub_checkers();
        let results = run_checkers(&ctx, &checkers).await;

        assert_eq!(results.outputs.len(), 7);
        assert!(!results.has_timeouts());

        for status in results.outputs.values() {
            match status {
                CheckerStatus::Completed(output) => {
                    assert!(output.passed);
                    assert_eq!(output.severity, Severity::None);
                }
                CheckerStatus::TimedOut => panic!("no checker should time out"),
            }
        }
    }

    #[tokio::test]
    async fn test_slow_checker_times_out() {
        let ctx = test_context();
        let mut checkers = all_stub_checkers();
        checkers.push(Box::new(SlowStub {
            delay: Duration::from_millis(200),
        }));

        let results = run_checkers(&ctx, &checkers).await;

        assert_eq!(results.outputs.len(), 8);
        assert!(results.has_timeouts());

        // The slow stub should have timed out.
        let slow_status = results.outputs.get("slow_stub").expect("slow_stub entry");
        assert!(matches!(slow_status, CheckerStatus::TimedOut));

        // The other 7 should have completed.
        let completed_count = results
            .outputs
            .values()
            .filter(|s| matches!(s, CheckerStatus::Completed(_)))
            .count();
        assert_eq!(completed_count, 7);
    }

    #[tokio::test]
    async fn test_run_empty_checkers() {
        let ctx = test_context();
        let checkers: Vec<Box<dyn Checker>> = vec![];
        let results = run_checkers(&ctx, &checkers).await;

        assert_eq!(results.outputs.len(), 0);
        assert!(!results.has_timeouts());
    }

    /// Verify that 10 concurrent run_checkers calls — each containing a slow
    /// checker — complete in roughly one timeout window, not 10x.
    /// This confirms the slow checker's sleep yields to the runtime and doesn't
    /// starve other concurrent verdict requests.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_concurrent_runs_do_not_block_on_slow_checker() {
        let ctx = test_context();
        let mut checkers: Vec<Box<dyn Checker>> = all_stub_checkers();
        checkers.push(Box::new(SlowStub {
            delay: Duration::from_millis(200),
        }));

        let start = tokio::time::Instant::now();
        let all_results: Vec<_> = join_all(
            (0..10).map(|_| run_checkers(&ctx, &checkers)),
        )
        .await;
        let elapsed = start.elapsed().as_millis();

        // All 10 runs should complete in ~80-100ms (one timeout window),
        // not 800ms+ (sequential).
        assert!(
            elapsed < 200,
            "10 concurrent runs took {elapsed}ms — expected < 200ms"
        );

        // Each run should have 7 completed + 1 timed out.
        for results in &all_results {
            assert_eq!(results.outputs.len(), 8);
            assert!(results.has_timeouts());
            assert!(matches!(
                results.outputs.get("slow_stub"),
                Some(CheckerStatus::TimedOut)
            ));
        }
    }
}
