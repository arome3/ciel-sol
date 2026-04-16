// Ciel Verdict Pipeline
// See spec Section 1.4 (data flows), Section 6 (scorer), Section 9 (override), Section 13 (verdict store).

pub mod pipeline;
pub mod scorer_stub;
pub mod verdict_store;
pub mod override_handler;
pub mod pre_certified;
pub mod intent_pipeline;
pub mod outcome;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors from the verdict pipeline.
#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error("fork simulation failed: {0}")]
    ForkSim(#[from] ciel_fork::ForkError),

    #[error("signer error: {0}")]
    Signer(#[from] ciel_signer::SignerError),

    #[error("pipeline timed out after {elapsed_ms}ms")]
    Timeout { elapsed_ms: u64 },

    #[error("transaction serialization failed: {0}")]
    Deserialization(String),
}

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

pub use pipeline::{PipelineConfig, PipelineTiming, VerdictPipeline, VerdictResponse};
pub use scorer_stub::{compute_safety_score, encode_score_u16, score_to_verdict};
pub use verdict_store::{log_verdict, VerdictLogEntry};
pub use outcome::{spawn_outcome_capture, OutcomeCaptureConfig, OutcomeError};
