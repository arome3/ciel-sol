// Prometheus metrics setup and recording helpers.
// See spec Section 15.1 (Metrics Per Verdict).
//
// Uses the `metrics` facade crate with `metrics-exporter-prometheus` backend.
// Other crates can instrument with `metrics::counter!()` etc. without depending
// on the exporter — only ciel-server installs the Prometheus recorder.

use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};

use ciel_checkers::CheckerStatus;
use ciel_pipeline::pipeline::verdict_to_str;
use ciel_pipeline::VerdictResponse;

/// Latency histogram buckets in milliseconds, aligned with spec Section 1.5 targets.
const LATENCY_BUCKETS: &[f64] = &[1.0, 5.0, 10.0, 25.0, 50.0, 75.0, 100.0, 150.0, 200.0, 500.0];

/// Install the global metrics recorder and return a handle for rendering
/// Prometheus text exposition format on the `/metrics` endpoint.
pub fn setup_metrics() -> PrometheusHandle {
    PrometheusBuilder::new()
        .set_buckets_for_metric(
            Matcher::Full("ciel_verdict_latency_ms".to_string()),
            LATENCY_BUCKETS,
        )
        .expect("valid histogram matcher")
        .install_recorder()
        .expect("metrics recorder installed once")
}

/// Record metrics for a completed verdict. Called from both REST and gRPC handlers.
///
/// Metrics emitted (per spec Section 15.1):
/// - `ciel_verdict_total` — counter, labels: verdict, input_type
/// - `ciel_verdict_latency_ms` — histogram, labels: stage
/// - `ciel_checker_result` — counter, labels: checker, passed, timed_out
pub fn record_verdict(resp: &VerdictResponse, input_type: &str) {
    let verdict_str = verdict_to_str(resp.verdict).to_lowercase();

    // Verdict counter.
    metrics::counter!(
        "ciel_verdict_total",
        "verdict" => verdict_str.clone(),
        "input_type" => input_type.to_string(),
    )
    .increment(1);

    // Per-stage latency histograms.
    let timing = &resp.timing;
    metrics::histogram!("ciel_verdict_latency_ms", "stage" => "total")
        .record(timing.total_ms as f64);
    metrics::histogram!("ciel_verdict_latency_ms", "stage" => "fork_sim")
        .record(timing.fork_sim_ms as f64);
    metrics::histogram!("ciel_verdict_latency_ms", "stage" => "checkers")
        .record(timing.checkers_ms as f64);
    metrics::histogram!("ciel_verdict_latency_ms", "stage" => "scoring")
        .record(timing.scoring_ms as f64);
    metrics::histogram!("ciel_verdict_latency_ms", "stage" => "signing")
        .record(timing.signing_ms as f64);

    // Per-checker result counters.
    for (name, status) in &resp.checker_results.outputs {
        match status {
            CheckerStatus::Completed(output) => {
                metrics::counter!(
                    "ciel_checker_result",
                    "checker" => name.clone(),
                    "passed" => output.passed.to_string(),
                    "timed_out" => "false",
                )
                .increment(1);
            }
            CheckerStatus::TimedOut => {
                metrics::counter!(
                    "ciel_checker_result",
                    "checker" => name.clone(),
                    "passed" => "false",
                    "timed_out" => "true",
                )
                .increment(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prometheus_handle_renders() {
        // Build a recorder without installing it globally (avoids conflict
        // with other tests that may have already installed a recorder).
        let recorder = PrometheusBuilder::new()
            .set_buckets_for_metric(
                Matcher::Full("ciel_verdict_latency_ms".to_string()),
                LATENCY_BUCKETS,
            )
            .expect("valid matcher")
            .build_recorder();
        let handle = recorder.handle();

        // Empty registry is valid — just no metrics recorded yet.
        let output = handle.render();
        assert!(output.is_empty() || output.contains("# "));
    }
}
