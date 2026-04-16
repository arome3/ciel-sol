// tonic gRPC server: VerdictService.Evaluate on port 50051.
// See spec Section 1.2 (Component Inventory) and Section 12.1 (SDK Surface).

use std::sync::Arc;

use solana_sdk::transaction::Transaction;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use ciel_checkers::CheckerStatus;
use ciel_pipeline::pipeline::verdict_to_str;
use ciel_pipeline::VerdictResponse;

use crate::metrics::record_verdict;
use crate::AppState;

// ---------------------------------------------------------------------------
// Generated proto types
// ---------------------------------------------------------------------------

pub mod proto {
    tonic::include_proto!("ciel.v1");
}

use proto::verdict_service_server::{VerdictService, VerdictServiceServer};
use proto::verdict_request::Input;

// ---------------------------------------------------------------------------
// Service implementation
// ---------------------------------------------------------------------------

pub struct CielVerdictService {
    state: Arc<AppState>,
}

#[tonic::async_trait]
impl VerdictService for CielVerdictService {
    async fn evaluate(
        &self,
        request: Request<proto::VerdictRequest>,
    ) -> Result<Response<proto::VerdictResponse>, Status> {
        let verdict_id = uuid::Uuid::new_v4().to_string();
        let span = tracing::info_span!("grpc_verdict", verdict_id = %verdict_id);
        self.evaluate_inner(request, &verdict_id)
            .instrument(span)
            .await
    }
}

impl CielVerdictService {
    async fn evaluate_inner(
        &self,
        request: Request<proto::VerdictRequest>,
        verdict_id: &str,
    ) -> Result<Response<proto::VerdictResponse>, Status> {
        let input = request
            .into_inner()
            .input
            .ok_or_else(|| {
                Status::invalid_argument("request must contain raw_tx, intent, or nl_intent")
            })?;

        match input {
            Input::RawTx(bytes) => {
                let tx: Transaction = bincode::deserialize(&bytes).map_err(|e| {
                    Status::invalid_argument(format!("invalid transaction: {e}"))
                })?;

                let resp = self
                    .state
                    .pipeline
                    .evaluate_raw_tx(&tx)
                    .await
                    .map_err(|e| Status::internal(format!("pipeline error: {e}")))?;

                record_verdict(&resp, "raw_tx");

                tracing::info!(
                    verdict_id = %verdict_id,
                    verdict = verdict_to_str(resp.verdict),
                    safety_score = resp.safety_score,
                    total_ms = resp.timing.total_ms,
                    "grpc verdict complete"
                );

                Ok(Response::new(pipeline_to_proto(&resp)))
            }
            Input::Intent(_) => Err(Status::unimplemented(
                "intent evaluation not yet implemented",
            )),
            Input::NlIntent(_) => Err(Status::unimplemented(
                "natural language intent evaluation not yet implemented",
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Conversion: pipeline::VerdictResponse → proto::VerdictResponse
// ---------------------------------------------------------------------------

fn pipeline_to_proto(resp: &VerdictResponse) -> proto::VerdictResponse {
    let checker_details: Vec<proto::CheckerDetail> = resp
        .checker_results
        .outputs
        .iter()
        .map(|(name, status)| match status {
            CheckerStatus::Completed(output) => proto::CheckerDetail {
                checker_name: name.clone(),
                passed: output.passed,
                severity: format!("{:?}", output.severity),
                flags: output
                    .flags
                    .iter()
                    .map(|f| proto::Flag {
                        code: f.code.clone(),
                        message: f.message.clone(),
                        data_json: serde_json::to_string(&f.data).unwrap_or_default(),
                    })
                    .collect(),
                details: output.details.clone(),
            },
            CheckerStatus::TimedOut => proto::CheckerDetail {
                checker_name: name.clone(),
                passed: false,
                severity: "Critical".to_string(),
                flags: vec![],
                details: "checker timed out".to_string(),
            },
        })
        .collect();

    proto::VerdictResponse {
        attestation: resp.attestation_bytes.clone(),
        signature: resp.signature_bytes.to_vec(),
        verdict: verdict_to_str(resp.verdict).to_string(),
        safety_score: resp.safety_score as f32,
        optimality_score: 0.0, // Raw tx mode — no optimality score.
        rationale: None,       // No LLM rationale in raw tx mode.
        checker_details,
        latency_ms: resp.timing.total_ms,
    }
}

// ---------------------------------------------------------------------------
// Public constructor
// ---------------------------------------------------------------------------

/// Build the tonic gRPC service for VerdictService.
pub fn grpc_service(state: Arc<AppState>) -> VerdictServiceServer<CielVerdictService> {
    VerdictServiceServer::new(CielVerdictService { state })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use ciel_checkers::all_stub_checkers;
    use ciel_fork::{ForkSimulator, StalenessConfig, StalenessTracker};
    use ciel_pipeline::{PipelineConfig, VerdictPipeline};
    use ciel_signer::CielSigner;
    use solana_sdk::signature::Keypair;
    use solana_sdk::signer::Signer;

    fn test_app_state() -> Arc<AppState> {
        let handle = metrics_exporter_prometheus::PrometheusBuilder::new()
            .build_recorder()
            .handle();

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

    fn build_test_tx_bytes() -> Vec<u8> {
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

        bincode::serialize(&tx).expect("serialize tx")
    }

    #[tokio::test]
    async fn test_grpc_evaluate_raw_tx() {
        let state = test_app_state();
        let svc = CielVerdictService { state };

        let req = Request::new(proto::VerdictRequest {
            input: Some(Input::RawTx(build_test_tx_bytes())),
        });

        let resp = svc.evaluate(req).await.expect("evaluate should succeed");
        let inner = resp.into_inner();

        assert_eq!(inner.verdict, "APPROVE");
        assert!(inner.safety_score > 0.0);
        assert_eq!(inner.attestation.len(), 132);
        assert_eq!(inner.signature.len(), 64);
        assert!(inner.latency_ms < 5000);
    }

    #[tokio::test]
    async fn test_grpc_intent_unimplemented() {
        let state = test_app_state();
        let svc = CielVerdictService { state };

        let req = Request::new(proto::VerdictRequest {
            input: Some(Input::Intent(proto::IntentRequest {
                goal: "swap SOL for USDC".to_string(),
                constraints: None,
                budget: None,
                deadline: None,
            })),
        });

        let err = svc.evaluate(req).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unimplemented);
    }

    #[tokio::test]
    async fn test_grpc_nl_intent_unimplemented() {
        let state = test_app_state();
        let svc = CielVerdictService { state };

        let req = Request::new(proto::VerdictRequest {
            input: Some(Input::NlIntent("swap 10k USDC to SOL".to_string())),
        });

        let err = svc.evaluate(req).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unimplemented);
    }

    #[tokio::test]
    async fn test_grpc_missing_input() {
        let state = test_app_state();
        let svc = CielVerdictService { state };

        let req = Request::new(proto::VerdictRequest { input: None });
        let err = svc.evaluate(req).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }
}
