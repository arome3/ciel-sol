// RPC failover manager with circuit breaker.
// See spec Section 3.5 for failover logic and circuit breaker thresholds.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::account::Account;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use tokio::time::Instant;

use crate::{ForkError, ForkResult};

// Circuit breaker defaults from spec Section 3.5
const FAILURE_THRESHOLD: usize = 5;
const FAILURE_WINDOW: Duration = Duration::from_secs(10);
const OPEN_DURATION: Duration = Duration::from_secs(30);
const FAILOVER_TIMEOUT: Duration = Duration::from_millis(100);

/// Circuit breaker state. See spec Section 3.5.
#[derive(Debug, Clone, PartialEq)]
pub enum CircuitState {
    /// Normal operation — requests flow to primary.
    Closed,
    /// Primary is unhealthy — skip directly to fallback.
    Open,
    /// Tentatively allowing one request to test recovery.
    HalfOpen,
}

/// Sliding-window circuit breaker for RPC provider health tracking.
///
/// Opens after `failure_threshold` failures within `failure_window`,
/// then transitions to HalfOpen after `open_duration` to probe recovery.
/// See spec Section 3.5.
pub struct CircuitBreaker {
    state: CircuitState,
    failures: VecDeque<Instant>,
    failure_threshold: usize,
    failure_window: Duration,
    open_duration: Duration,
    opened_at: Option<Instant>,
}

impl CircuitBreaker {
    pub fn new() -> Self {
        Self {
            state: CircuitState::Closed,
            failures: VecDeque::new(),
            failure_threshold: FAILURE_THRESHOLD,
            failure_window: FAILURE_WINDOW,
            open_duration: OPEN_DURATION,
            opened_at: None,
        }
    }

    /// Check if the primary provider should be tried.
    /// Handles Open → HalfOpen transition when open_duration has elapsed.
    pub fn is_available(&mut self) -> bool {
        match &self.state {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => true,
            CircuitState::Open => {
                if let Some(opened_at) = self.opened_at {
                    if opened_at.elapsed() >= self.open_duration {
                        self.state = CircuitState::HalfOpen;
                        tracing::info!("circuit breaker transitioning to HalfOpen");
                        return true;
                    }
                }
                false
            }
        }
    }

    /// Record a successful request. Resets the circuit to Closed.
    pub fn record_success(&mut self) {
        if self.state == CircuitState::HalfOpen {
            tracing::info!("circuit breaker recovered, transitioning to Closed");
        }
        self.state = CircuitState::Closed;
        self.failures.clear();
        self.opened_at = None;
    }

    /// Record a failed request. May transition Closed → Open or HalfOpen → Open.
    pub fn record_failure(&mut self) {
        let now = Instant::now();

        match self.state {
            CircuitState::HalfOpen => {
                // Probe failed — re-open
                self.state = CircuitState::Open;
                self.opened_at = Some(now);
                tracing::warn!("circuit breaker probe failed, re-opening");
            }
            CircuitState::Closed => {
                // Prune failures outside the sliding window
                while self
                    .failures
                    .front()
                    .is_some_and(|t| t.elapsed() > self.failure_window)
                {
                    self.failures.pop_front();
                }

                self.failures.push_back(now);

                if self.failures.len() >= self.failure_threshold {
                    self.state = CircuitState::Open;
                    self.opened_at = Some(now);
                    tracing::warn!(
                        failures = self.failures.len(),
                        window_secs = self.failure_window.as_secs(),
                        "circuit breaker opened"
                    );
                }
            }
            CircuitState::Open => {
                // Already open, just update the timestamp
                self.opened_at = Some(now);
            }
        }
    }

    /// Current circuit state.
    pub fn state(&self) -> CircuitState {
        self.state.clone()
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

/// RPC manager with primary/fallback failover and circuit breaker.
/// See spec Section 3.5.
pub struct RpcManager {
    primary: RpcClient,
    fallback: RpcClient,
    circuit_breaker: tokio::sync::Mutex<CircuitBreaker>,
    failover_timeout: Duration,
}

impl RpcManager {
    /// Create an RpcManager from environment variables.
    /// Requires HELIUS_API_KEY. TRITON_API_KEY is optional — falls back to
    /// public Solana mainnet RPC if not set.
    pub fn from_env() -> ForkResult<Self> {
        let helius_key = std::env::var("HELIUS_API_KEY")
            .map_err(|_| ForkError::Config("HELIUS_API_KEY not set".into()))?;

        let primary_url = format!("https://mainnet.helius-rpc.com/?api-key={helius_key}");

        let fallback_url = match std::env::var("TRITON_API_KEY") {
            Ok(triton_key) => format!("https://api.triton.one/{triton_key}"),
            Err(_) => {
                tracing::warn!("TRITON_API_KEY not set, using public Solana RPC as fallback");
                "https://api.mainnet-beta.solana.com".to_string()
            }
        };

        Ok(Self::new(primary_url, fallback_url))
    }

    /// Create an RpcManager with explicit URLs.
    pub fn new(primary_url: String, fallback_url: String) -> Self {
        tracing::info!(
            primary = %primary_url.split('?').next().unwrap_or(&primary_url),
            fallback = %fallback_url.split('/').take(3).collect::<Vec<_>>().join("/"),
            "RPC manager initialized"
        );

        Self {
            primary: RpcClient::new(primary_url),
            fallback: RpcClient::new(fallback_url),
            circuit_breaker: tokio::sync::Mutex::new(CircuitBreaker::new()),
            failover_timeout: FAILOVER_TIMEOUT,
        }
    }

    /// Fetch a single account with failover.
    /// Primary (100ms timeout) → fallback → AllProvidersDown.
    /// See spec Section 3.5.
    pub async fn fetch_account(&self, pubkey: &Pubkey) -> ForkResult<Account> {
        let commitment = CommitmentConfig::confirmed();

        // Step 1: Check circuit breaker
        let try_primary = {
            let mut cb = self.circuit_breaker.lock().await;
            cb.is_available()
        };

        // Step 2: Try primary with timeout
        if try_primary {
            match tokio::time::timeout(
                self.failover_timeout,
                self.primary.get_account_with_commitment(pubkey, commitment),
            )
            .await
            {
                Ok(Ok(response)) => {
                    if let Some(account) = response.value {
                        let mut cb = self.circuit_breaker.lock().await;
                        cb.record_success();
                        return Ok(account);
                    }
                    // Account doesn't exist — still a successful RPC call
                    let mut cb = self.circuit_breaker.lock().await;
                    cb.record_success();
                    return Err(ForkError::AccountNotFound {
                        pubkey: pubkey.to_string(),
                    });
                }
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "primary RPC failed, trying fallback");
                    let mut cb = self.circuit_breaker.lock().await;
                    cb.record_failure();
                }
                Err(_) => {
                    tracing::warn!("primary RPC timed out ({}ms), trying fallback", self.failover_timeout.as_millis());
                    let mut cb = self.circuit_breaker.lock().await;
                    cb.record_failure();
                }
            }
        }

        // Step 3: Try fallback
        match self
            .fallback
            .get_account_with_commitment(pubkey, commitment)
            .await
        {
            Ok(response) => match response.value {
                Some(account) => Ok(account),
                None => Err(ForkError::AccountNotFound {
                    pubkey: pubkey.to_string(),
                }),
            },
            Err(e) => {
                tracing::error!(error = %e, "fallback RPC also failed");
                Err(ForkError::AllProvidersDown)
            }
        }
    }

    /// Fetch multiple accounts with failover.
    /// Uses getMultipleAccounts RPC (up to 100 per batch).
    pub async fn fetch_multiple_accounts(
        &self,
        pubkeys: &[Pubkey],
    ) -> ForkResult<Vec<Option<Account>>> {
        let commitment = CommitmentConfig::confirmed();

        // Try primary with timeout
        let try_primary = {
            let mut cb = self.circuit_breaker.lock().await;
            cb.is_available()
        };

        if try_primary {
            match tokio::time::timeout(
                self.failover_timeout,
                self.primary
                    .get_multiple_accounts_with_commitment(pubkeys, commitment),
            )
            .await
            {
                Ok(Ok(response)) => {
                    let mut cb = self.circuit_breaker.lock().await;
                    cb.record_success();
                    return Ok(response.value);
                }
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "primary RPC batch failed, trying fallback");
                    let mut cb = self.circuit_breaker.lock().await;
                    cb.record_failure();
                }
                Err(_) => {
                    tracing::warn!("primary RPC batch timed out, trying fallback");
                    let mut cb = self.circuit_breaker.lock().await;
                    cb.record_failure();
                }
            }
        }

        // Fallback
        match self
            .fallback
            .get_multiple_accounts_with_commitment(pubkeys, commitment)
            .await
        {
            Ok(response) => Ok(response.value),
            Err(e) => {
                tracing::error!(error = %e, "fallback RPC batch also failed");
                Err(ForkError::AllProvidersDown)
            }
        }
    }

    /// Get the current confirmed slot.
    pub async fn get_slot(&self) -> ForkResult<u64> {
        let commitment = CommitmentConfig::confirmed();

        let try_primary = {
            let mut cb = self.circuit_breaker.lock().await;
            cb.is_available()
        };

        if try_primary {
            match tokio::time::timeout(
                self.failover_timeout,
                self.primary.get_slot_with_commitment(commitment),
            )
            .await
            {
                Ok(Ok(slot)) => {
                    let mut cb = self.circuit_breaker.lock().await;
                    cb.record_success();
                    return Ok(slot);
                }
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "primary get_slot failed");
                    let mut cb = self.circuit_breaker.lock().await;
                    cb.record_failure();
                }
                Err(_) => {
                    tracing::warn!("primary get_slot timed out");
                    let mut cb = self.circuit_breaker.lock().await;
                    cb.record_failure();
                }
            }
        }

        self.fallback
            .get_slot_with_commitment(commitment)
            .await
            .map_err(|e| ForkError::Rpc(e.to_string()))
    }

    /// Get the latest blockhash.
    pub async fn get_latest_blockhash(&self) -> ForkResult<Hash> {
        let commitment = CommitmentConfig::confirmed();

        let try_primary = {
            let mut cb = self.circuit_breaker.lock().await;
            cb.is_available()
        };

        if try_primary {
            match tokio::time::timeout(
                self.failover_timeout,
                self.primary.get_latest_blockhash_with_commitment(commitment),
            )
            .await
            {
                Ok(Ok((blockhash, _))) => {
                    let mut cb = self.circuit_breaker.lock().await;
                    cb.record_success();
                    return Ok(blockhash);
                }
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "primary get_latest_blockhash failed");
                    let mut cb = self.circuit_breaker.lock().await;
                    cb.record_failure();
                }
                Err(_) => {
                    tracing::warn!("primary get_latest_blockhash timed out");
                    let mut cb = self.circuit_breaker.lock().await;
                    cb.record_failure();
                }
            }
        }

        self.fallback
            .get_latest_blockhash_with_commitment(commitment)
            .await
            .map(|(hash, _)| hash)
            .map_err(|e| ForkError::Rpc(e.to_string()))
    }

    /// Access the circuit breaker (for testing/monitoring).
    pub fn circuit_breaker(&self) -> &tokio::sync::Mutex<CircuitBreaker> {
        &self.circuit_breaker
    }
}

/// Wrap RpcManager in Arc for sharing across ForkSimulator instances.
impl RpcManager {
    pub fn into_arc(self) -> Arc<Self> {
        Arc::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Circuit breaker unit tests (hermetic, no network) ---

    #[tokio::test(start_paused = true)]
    async fn test_circuit_breaker_starts_closed() {
        let mut cb = CircuitBreaker::new();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.is_available());
    }

    #[tokio::test(start_paused = true)]
    async fn test_circuit_breaker_opens_after_threshold() {
        let mut cb = CircuitBreaker::new();

        // Record 4 failures — should stay closed
        for _ in 0..4 {
            cb.record_failure();
        }
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.is_available());

        // 5th failure triggers open
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.is_available());
    }

    #[tokio::test(start_paused = true)]
    async fn test_circuit_breaker_window_expiry() {
        let mut cb = CircuitBreaker::new();

        // Record 4 failures
        for _ in 0..4 {
            cb.record_failure();
        }

        // Advance past the failure window (10s)
        tokio::time::advance(Duration::from_secs(11)).await;

        // 5th failure is now the only one in the window — should stay closed
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[tokio::test(start_paused = true)]
    async fn test_circuit_breaker_recovery() {
        let mut cb = CircuitBreaker::new();

        // Open the circuit
        for _ in 0..5 {
            cb.record_failure();
        }
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.is_available());

        // Advance past open_duration (30s)
        tokio::time::advance(Duration::from_secs(31)).await;

        // Should transition to HalfOpen on next availability check
        assert!(cb.is_available());
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        // Successful probe → Closed
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.is_available());
    }

    #[tokio::test(start_paused = true)]
    async fn test_circuit_breaker_half_open_failure() {
        let mut cb = CircuitBreaker::new();

        // Open the circuit
        for _ in 0..5 {
            cb.record_failure();
        }

        // Transition to HalfOpen
        tokio::time::advance(Duration::from_secs(31)).await;
        assert!(cb.is_available()); // triggers HalfOpen
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        // Failed probe → re-open
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.is_available());
    }

    // --- RPC Manager tests ---

    #[test]
    fn test_rpc_manager_from_env_missing_keys() {
        // Ensure keys are not set
        std::env::remove_var("HELIUS_API_KEY");
        std::env::remove_var("TRITON_API_KEY");

        let result = RpcManager::from_env();
        assert!(result.is_err());
        match result {
            Err(ForkError::Config(msg)) => assert!(msg.contains("HELIUS_API_KEY")),
            Err(other) => panic!("expected Config error, got: {other:?}"),
            Ok(_) => panic!("expected error when env vars are not set"),
        }
    }

    #[tokio::test]
    #[ignore] // Requires HELIUS_API_KEY env var
    async fn test_fetch_account_primary() {
        let rpc = RpcManager::from_env().expect("RPC keys required");
        let usdc_mint: Pubkey = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
            .parse()
            .unwrap();

        let account = rpc.fetch_account(&usdc_mint).await.expect("should fetch USDC mint");
        assert!(account.lamports > 0);
        assert!(!account.data.is_empty());
    }

    #[tokio::test]
    #[ignore] // Integration test for failover
    async fn test_failover_to_fallback() {
        // Use an invalid primary to force failover to public Solana RPC
        let rpc = RpcManager::new(
            "https://invalid-endpoint.example.com".into(),
            "https://api.mainnet-beta.solana.com".into(),
        );

        let usdc_mint: Pubkey = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
            .parse()
            .unwrap();

        let account = rpc
            .fetch_account(&usdc_mint)
            .await
            .expect("should fall back to Triton One");
        assert!(account.lamports > 0);
    }
}
