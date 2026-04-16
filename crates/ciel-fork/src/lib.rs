// Ciel Fork Simulator
// Wraps LiteSVM with account caching, RPC failover, and sysvar initialization.
// See spec Section 3 for full design.

pub mod cache;
pub mod executor;
pub mod geyser;
pub mod rpc;
pub mod simulator;
pub mod staleness;
pub mod trace;

pub use cache::AccountCache;
pub use executor::execute_transaction;
pub use geyser::{GeyserConfig, GeyserSubscriber};
pub use rpc::{CircuitBreaker, RpcManager};
pub use simulator::ForkSimulator;
pub use staleness::{StalenessConfig, StalenessState, StalenessTracker};
pub use trace::{
    AccountChange, CpiCall, OracleRead, SimulationTrace, TokenApproval, TokenBalanceDelta,
};

/// Error type for fork simulator operations.
/// See spec Section 3.5 for RPC failover semantics.
#[derive(Debug, thiserror::Error)]
pub enum ForkError {
    #[error("RPC error: {0}")]
    Rpc(String),

    #[error("account not found: {pubkey}")]
    AccountNotFound { pubkey: String },

    #[error("LiteSVM error: {0}")]
    LiteSvm(String),

    #[error("primary RPC circuit breaker is open")]
    CircuitOpen,

    #[error("configuration error: {0}")]
    Config(String),

    #[error("RPC call timed out")]
    Timeout,

    #[error("all RPC providers failed")]
    AllProvidersDown,

    #[error("geyser stream error: {0}")]
    Geyser(String),
}

impl From<solana_client::client_error::ClientError> for ForkError {
    fn from(err: solana_client::client_error::ClientError) -> Self {
        ForkError::Rpc(err.to_string())
    }
}

pub type ForkResult<T> = Result<T, ForkError>;
