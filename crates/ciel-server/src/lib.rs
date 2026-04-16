// Ciel API Server
// See spec Sections 1.2, 1.3, 12.1, 15.1, 15.2, 16.2.

use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use metrics_exporter_prometheus::PrometheusHandle;

use ciel_pipeline::{PipelineError, VerdictPipeline};

pub mod grpc;
pub mod metrics;
pub mod rest;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// API server errors, mapped to HTTP status codes via IntoResponse.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("pipeline error: {0}")]
    Pipeline(#[from] PipelineError),

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("base64 decode error: {0}")]
    Base64Decode(String),

    #[error("not implemented: {0}")]
    NotImplemented(String),
}

pub type ServerResult<T> = Result<T, ServerError>;

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            ServerError::InvalidRequest(msg) => {
                (StatusCode::BAD_REQUEST, "INVALID_REQUEST", msg.clone())
            }
            ServerError::Base64Decode(msg) => {
                (StatusCode::BAD_REQUEST, "INVALID_REQUEST", msg.clone())
            }
            ServerError::Pipeline(PipelineError::Timeout { elapsed_ms }) => (
                StatusCode::GATEWAY_TIMEOUT,
                "TIMEOUT",
                format!("verdict pipeline timed out after {elapsed_ms}ms"),
            ),
            ServerError::Pipeline(e) => {
                tracing::error!(error = %e, "pipeline error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "PIPELINE_ERROR",
                    "verdict pipeline failed".to_string(),
                )
            }
            ServerError::NotImplemented(msg) => {
                (StatusCode::NOT_IMPLEMENTED, "NOT_IMPLEMENTED", msg.clone())
            }
        };
        let body = serde_json::json!({
            "error": { "code": code, "message": message }
        });
        (status, axum::Json(body)).into_response()
    }
}

// ---------------------------------------------------------------------------
// Shared application state
// ---------------------------------------------------------------------------

/// Shared state injected into both REST (axum State) and gRPC handlers.
pub struct AppState {
    pub pipeline: Arc<VerdictPipeline>,
    pub metrics_handle: PrometheusHandle,
}
