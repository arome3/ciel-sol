// axum REST server: POST /v1/verdict, GET /health, GET /metrics.
// See spec Section 1.4 (data flows), Section 12.1 (VerdictResponse), Section 16.2 (port 8080).

use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderValue, Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use solana_sdk::transaction::Transaction;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

use ciel_checkers::CheckerStatus;
use ciel_pipeline::pipeline::verdict_to_str;
use ciel_pipeline::VerdictResponse;

use crate::metrics::record_verdict;
use crate::{AppState, ServerError};

// ---------------------------------------------------------------------------
// JSON request / response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct VerdictRequestJson {
    #[serde(default)]
    pub tx: Option<String>,
    #[serde(default)]
    pub intent: Option<serde_json::Value>,
    #[serde(default)]
    pub nl_intent: Option<String>,
}

#[derive(Serialize)]
pub struct VerdictResponseJson {
    pub verdict: String,
    pub safety_score: f64,
    pub attestation: String,
    pub signature: String,
    pub slot: u64,
    pub tx_hash: String,
    pub checker_outputs_hash: String,
    pub checker_details: Vec<CheckerDetailJson>,
    pub latency_ms: u64,
    pub timing: TimingJson,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downgrade_reason: Option<String>,
}

#[derive(Serialize)]
pub struct CheckerDetailJson {
    pub checker_name: String,
    pub passed: bool,
    pub severity: String,
    pub flags: Vec<FlagJson>,
    pub details: String,
}

#[derive(Serialize)]
pub struct FlagJson {
    pub code: String,
    pub message: String,
    pub data: serde_json::Value,
}

#[derive(Serialize)]
pub struct TimingJson {
    pub total_ms: u64,
    pub fork_sim_ms: u64,
    pub checkers_ms: u64,
    pub scoring_ms: u64,
    pub signing_ms: u64,
}

// ---------------------------------------------------------------------------
// Conversion: pipeline::VerdictResponse → JSON
// ---------------------------------------------------------------------------

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn pipeline_response_to_json(resp: &VerdictResponse) -> VerdictResponseJson {
    let checker_details: Vec<CheckerDetailJson> = resp
        .checker_results
        .outputs
        .iter()
        .map(|(name, status)| match status {
            CheckerStatus::Completed(output) => CheckerDetailJson {
                checker_name: name.clone(),
                passed: output.passed,
                severity: format!("{:?}", output.severity),
                flags: output
                    .flags
                    .iter()
                    .map(|f| FlagJson {
                        code: f.code.clone(),
                        message: f.message.clone(),
                        data: f.data.clone(),
                    })
                    .collect(),
                details: output.details.clone(),
            },
            CheckerStatus::TimedOut => CheckerDetailJson {
                checker_name: name.clone(),
                passed: false,
                severity: "Critical".to_string(),
                flags: vec![],
                details: "checker timed out".to_string(),
            },
        })
        .collect();

    VerdictResponseJson {
        verdict: verdict_to_str(resp.verdict).to_string(),
        safety_score: resp.safety_score,
        attestation: BASE64.encode(&resp.attestation_bytes),
        signature: BASE64.encode(resp.signature_bytes),
        slot: resp.slot,
        tx_hash: to_hex(&resp.tx_hash),
        checker_outputs_hash: to_hex(&resp.checker_outputs_hash),
        checker_details,
        latency_ms: resp.timing.total_ms,
        timing: TimingJson {
            total_ms: resp.timing.total_ms,
            fork_sim_ms: resp.timing.fork_sim_ms,
            checkers_ms: resp.timing.checkers_ms,
            scoring_ms: resp.timing.scoring_ms,
            signing_ms: resp.timing.signing_ms,
        },
        downgrade_reason: resp.downgrade_reason.clone(),
    }
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the axum REST router with tower-http tracing middleware.
pub fn rest_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/verdict", post(handle_verdict))
        .route("/health", get(handle_health))
        .route("/metrics", get(handle_metrics))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &Request<_>| {
                let verdict_id = request
                    .headers()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("unknown");
                tracing::info_span!(
                    "verdict_request",
                    verdict_id = %verdict_id,
                    method = %request.method(),
                    uri = %request.uri(),
                )
            }),
        )
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn handle_verdict(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<VerdictRequestJson>,
) -> Result<impl IntoResponse, ServerError> {
    let verdict_id = headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    // Dispatch on request type. See spec Section 1.4 data flows.
    if let Some(ref tx_b64) = req.tx {
        // Flow A: Raw Transaction
        let tx_bytes = BASE64
            .decode(tx_b64)
            .map_err(|e| ServerError::Base64Decode(e.to_string()))?;

        let tx: Transaction = bincode::deserialize(&tx_bytes)
            .map_err(|e| ServerError::InvalidRequest(format!("invalid transaction: {e}")))?;

        let resp = state.pipeline.evaluate_raw_tx(&tx).await?;

        // Record metrics.
        record_verdict(&resp, "raw_tx");

        // Log structured verdict info. See spec Section 15.2.
        let passed: Vec<&str> = resp.checker_results.completed().iter().filter(|o| o.passed).map(|o| o.checker_name.as_str()).collect();
        let flagged: Vec<&str> = resp.checker_results.completed().iter().filter(|o| !o.passed).map(|o| o.checker_name.as_str()).collect();
        let timed_out = resp.checker_results.timed_out();

        tracing::info!(
            verdict_id = %verdict_id,
            verdict = verdict_to_str(resp.verdict),
            safety_score = resp.safety_score,
            total_ms = resp.timing.total_ms,
            fork_sim_ms = resp.timing.fork_sim_ms,
            checkers_ms = resp.timing.checkers_ms,
            signing_ms = resp.timing.signing_ms,
            checkers_passed = ?passed,
            checkers_flagged = ?flagged,
            checkers_timed_out = ?timed_out,
            "verdict complete"
        );

        let json_resp = pipeline_response_to_json(&resp);
        Ok(Json(json_resp))
    } else if req.intent.is_some() {
        // Flow B: Structured Intent — stub until Week 4.
        Err(ServerError::NotImplemented(
            "intent evaluation not yet implemented".to_string(),
        ))
    } else if req.nl_intent.is_some() {
        // Flow C: Natural Language Intent — stub until Week 4.
        Err(ServerError::NotImplemented(
            "natural language intent evaluation not yet implemented".to_string(),
        ))
    } else {
        Err(ServerError::InvalidRequest(
            "request must contain 'tx', 'intent', or 'nl_intent'".to_string(),
        ))
    }
}

async fn handle_health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn handle_metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let body = state.metrics_handle.render();
    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
        )],
        body,
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use ciel_checkers::all_stub_checkers;
    use ciel_fork::{ForkSimulator, StalenessConfig, StalenessTracker};
    use ciel_pipeline::{PipelineConfig, VerdictPipeline};
    use ciel_signer::CielSigner;
    use solana_sdk::signature::Keypair;
    use solana_sdk::signer::Signer;

    fn test_app_state() -> Arc<AppState> {
        let handle = metrics_exporter_prometheus::PrometheusBuilder::new()
            .install_recorder()
            .unwrap_or_else(|_| {
                // Recorder already installed by another test — get a noop handle.
                // This is safe: metrics just won't record to this handle.
                metrics_exporter_prometheus::PrometheusBuilder::new()
                    .build_recorder()
                    .handle()
            });

        let fork = ForkSimulator::new_offline();
        let signer = CielSigner::from_bytes(&[42u8; 32]);
        let staleness = StalenessTracker::new(StalenessConfig::default());
        staleness.record_update(0);
        let pipeline = VerdictPipeline::new(
            fork,
            all_stub_checkers(),
            signer,
            staleness,
            None,
            PipelineConfig::default(),
        );
        Arc::new(AppState {
            pipeline: Arc::new(pipeline),
            metrics_handle: handle,
        })
    }

    /// Build a signed SOL transfer and return its base64-encoded bincode bytes.
    /// Same pattern as ciel-pipeline/src/pipeline.rs tests.
    fn build_test_tx_base64() -> String {
        let mut fork = ForkSimulator::new_offline();
        let sender = Keypair::new();
        let receiver = solana_sdk::pubkey::Pubkey::new_unique();

        let sender_addr = litesvm_address::Address::from(sender.pubkey().to_bytes());
        let receiver_addr = litesvm_address::Address::from(receiver.to_bytes());
        fork.svm_mut()
            .airdrop(&sender_addr, 10_000_000_000)
            .expect("airdrop sender");
        fork.svm_mut()
            .airdrop(&receiver_addr, 1_000_000_000)
            .expect("airdrop receiver");

        #[allow(deprecated)]
        let ix = solana_sdk::system_instruction::transfer(&sender.pubkey(), &receiver, 1_000_000);
        let blockhash = solana_sdk::hash::Hash::new_from_array(
            fork.svm().latest_blockhash().to_bytes(),
        );
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&sender.pubkey()),
            &[&sender],
            blockhash,
        );

        let tx_bytes = bincode::serialize(&tx).expect("serialize tx");
        BASE64.encode(&tx_bytes)
    }

    fn post_verdict(body: &str) -> Request<Body> {
        Request::builder()
            .uri("/v1/verdict")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn test_health_returns_200() {
        let state = test_app_state();
        let app = rest_router(state);

        let resp = app
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
    }

    #[tokio::test]
    async fn test_verdict_raw_tx_returns_approve() {
        let state = test_app_state();
        let app = rest_router(state);

        let tx_b64 = build_test_tx_base64();
        let body = serde_json::json!({ "tx": tx_b64 }).to_string();

        let resp = app.oneshot(post_verdict(&body)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(json["verdict"], "APPROVE");
        assert!(json["safety_score"].as_f64().unwrap() > 0.0);
        assert!(json["attestation"].as_str().unwrap().len() > 0);
        assert!(json["signature"].as_str().unwrap().len() > 0);
        assert!(json["latency_ms"].is_number());
        assert!(json["timing"]["total_ms"].is_number());
        assert!(json["tx_hash"].as_str().unwrap().len() == 64); // 32 bytes hex
        assert!(json["checker_outputs_hash"].as_str().unwrap().len() == 64);
    }

    #[tokio::test]
    async fn test_verdict_intent_returns_501() {
        let state = test_app_state();
        let app = rest_router(state);

        let body = serde_json::json!({ "intent": { "goal": "swap" } }).to_string();
        let resp = app.oneshot(post_verdict(&body)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
    }

    #[tokio::test]
    async fn test_verdict_nl_intent_returns_501() {
        let state = test_app_state();
        let app = rest_router(state);

        let body = serde_json::json!({ "nl_intent": "swap 10k USDC to SOL" }).to_string();
        let resp = app.oneshot(post_verdict(&body)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
    }

    #[tokio::test]
    async fn test_verdict_empty_body_returns_400() {
        let state = test_app_state();
        let app = rest_router(state);

        let resp = app
            .oneshot(post_verdict("{}"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_verdict_invalid_base64_returns_400() {
        let state = test_app_state();
        let app = rest_router(state);

        let body = serde_json::json!({ "tx": "not-valid-base64!!!" }).to_string();
        let resp = app.oneshot(post_verdict(&body)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_verdict_invalid_tx_returns_400() {
        let state = test_app_state();
        let app = rest_router(state);

        // Valid base64 but not a valid serialized Transaction.
        let body = serde_json::json!({ "tx": "AAAA" }).to_string();
        let resp = app.oneshot(post_verdict(&body)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_metrics_endpoint() {
        let state = test_app_state();
        let app = rest_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp.headers().get("content-type").unwrap().to_str().unwrap();
        assert!(ct.contains("text/plain"));
    }
}
