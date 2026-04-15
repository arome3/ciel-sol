// Helius LaserStream gRPC subscriber.
// Streams real-time Solana account updates into the fork simulator's cache.
// See spec Section 3.2 (cache architecture), 3.4 (reconnection/gap-fill).

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use futures::sink::SinkExt;
use futures::stream::StreamExt;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use yellowstone_grpc_client::GeyserGrpcClient;
use yellowstone_grpc_proto::prelude::{
    subscribe_update::UpdateOneof, CommitmentLevel, SubscribeRequest,
    SubscribeRequestFilterAccounts, SubscribeRequestPing, SubscribeUpdateAccount,
};

use crate::cache::AccountCache;
use crate::rpc::RpcManager;
use crate::staleness::StalenessTracker;
use crate::{ForkError, ForkResult};

/// V1 monitored programs. See spec Section 3.2.
pub const V1_MONITORED_PROGRAMS: &[&str] = &[
    "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",  // SPL Token
    "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL", // Associated Token
    "11111111111111111111111111111111",                // System Program
    "dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH",  // Drift v2
    "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4",  // Jupiter v6
    "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8", // Raydium AMM
    "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc",  // Orca Whirlpool
    "MarBmsSgKXdrN1egZf5sqe1TMai9K1rChYNDJgjq7aD",  // Marinade
    "SW1TCH7qEPTdLsDHRgPuMQjbQxKdH2aBStViMFnt64f",  // Switchboard v2
    "FsJ3A3u2vn5cTVofAjvy6y5kwABJAqYWpe4975bi2epH", // Pyth
];

/// Configuration for the geyser subscriber. See spec Section 3.5.
#[derive(Debug, Clone)]
pub struct GeyserConfig {
    /// LaserStream gRPC endpoint URL.
    pub endpoint: String,
    /// Helius API key (sent as x-token gRPC metadata).
    pub api_key: String,
    /// Base backoff duration for reconnection (spec: 100ms).
    pub backoff_base: Duration,
    /// Maximum backoff duration (spec: 5s).
    pub backoff_max: Duration,
    /// Initial set of account pubkeys to subscribe to.
    pub initial_accounts: Vec<Pubkey>,
}

impl GeyserConfig {
    /// Read configuration from environment variables.
    /// Requires `HELIUS_API_KEY`. `HELIUS_LASERSTREAM_URL` defaults to mainnet.
    pub fn from_env() -> ForkResult<Self> {
        let api_key = std::env::var("HELIUS_API_KEY")
            .map_err(|_| ForkError::Config("HELIUS_API_KEY not set".into()))?;

        let endpoint = std::env::var("HELIUS_LASERSTREAM_URL")
            .unwrap_or_else(|_| "https://laserstream.helius-rpc.com".to_string());

        let initial_accounts = V1_MONITORED_PROGRAMS
            .iter()
            .filter_map(|s| Pubkey::from_str(s).ok())
            .collect();

        Ok(Self {
            endpoint,
            api_key,
            backoff_base: Duration::from_millis(100),
            backoff_max: Duration::from_secs(5),
            initial_accounts,
        })
    }
}

/// Geyser subscriber that streams account updates from Helius LaserStream
/// into the shared AccountCache. Spawned as a background tokio task.
///
/// See spec Section 3.2 (cache architecture), 3.4 (reconnection/gap-fill).
pub struct GeyserSubscriber {
    config: GeyserConfig,
    cache: AccountCache,
    rpc: Arc<RpcManager>,
    staleness: StalenessTracker,
    /// Channel for dynamically adding accounts to the subscription.
    /// The verdict pipeline sends pubkeys here when evaluating transactions.
    account_tx: tokio::sync::mpsc::UnboundedSender<Vec<Pubkey>>,
    account_rx: Arc<tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<Vec<Pubkey>>>>,
}

impl GeyserSubscriber {
    /// Create a new subscriber.
    pub fn new(
        config: GeyserConfig,
        cache: AccountCache,
        rpc: Arc<RpcManager>,
        staleness: StalenessTracker,
    ) -> Self {
        let (account_tx, account_rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            config,
            cache,
            rpc,
            staleness,
            account_tx,
            account_rx: Arc::new(tokio::sync::Mutex::new(account_rx)),
        }
    }

    /// Convenience constructor from environment variables.
    pub fn from_env(
        cache: AccountCache,
        rpc: Arc<RpcManager>,
        staleness: StalenessTracker,
    ) -> ForkResult<Self> {
        Ok(Self::new(GeyserConfig::from_env()?, cache, rpc, staleness))
    }

    /// Returns a sender for dynamically adding accounts to the subscription.
    /// When the verdict pipeline evaluates a transaction, it sends all touched
    /// account pubkeys through this channel. See spec Section 3.2.
    pub fn account_sender(&self) -> tokio::sync::mpsc::UnboundedSender<Vec<Pubkey>> {
        self.account_tx.clone()
    }

    /// Spawn the subscriber as a background tokio task.
    /// The task runs for the process lifetime with automatic reconnection.
    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            self.run_loop().await;
        })
    }

    /// Outer reconnection loop. Runs forever.
    /// See spec Section 3.4 for backoff strategy: 100ms → 200ms → ... → 5s max.
    async fn run_loop(&self) {
        let mut backoff = self.config.backoff_base;

        loop {
            let from_slot = self.staleness.last_slot();

            match self.connect_and_stream(from_slot).await {
                Ok(()) => {
                    backoff = self.config.backoff_base;
                    tracing::info!("geyser stream ended cleanly, reconnecting");
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        backoff_ms = backoff.as_millis(),
                        "geyser stream error, reconnecting"
                    );
                }
            }

            // Fallback: RPC gap-fill if from_slot replay wasn't sufficient
            if let Err(e) = self.rpc_gap_fill().await {
                tracing::error!(error = %e, "RPC gap-fill failed");
            }

            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(self.config.backoff_max);
        }
    }

    /// Connect to LaserStream and stream account updates until error/EOF.
    /// Uses `from_slot` for gap recovery per Yellowstone protocol.
    async fn connect_and_stream(&self, from_slot: u64) -> ForkResult<()> {
        let mut client = GeyserGrpcClient::build_from_shared(self.config.endpoint.clone())
            .map_err(|e| ForkError::Geyser(format!("invalid endpoint: {e}")))?
            .x_token(Some(self.config.api_key.clone()))
            .map_err(|e| ForkError::Geyser(format!("invalid api key: {e}")))?
            .connect()
            .await
            .map_err(|e| ForkError::Geyser(format!("connect failed: {e}")))?;

        let request = self.build_subscribe_request(from_slot);

        let (mut subscribe_tx, mut stream) = client
            .subscribe_with_request(Some(request))
            .await
            .map_err(|e| ForkError::Geyser(format!("subscribe failed: {e}")))?;

        tracing::info!(
            from_slot = from_slot,
            endpoint = %self.config.endpoint,
            "geyser stream connected"
        );

        while let Some(msg) = stream.next().await {
            match msg {
                Ok(update) => {
                    match update.update_oneof {
                        Some(UpdateOneof::Account(acct_update)) => {
                            self.handle_account_update(acct_update);
                        }
                        Some(UpdateOneof::Ping(_)) => {
                            // Respond to keepalive pings. Cloud proxies close idle
                            // streams after ~30s if pongs are not sent.
                            // See ERPC best practices.
                            let pong = SubscribeRequest {
                                ping: Some(SubscribeRequestPing { id: 1 }),
                                ..Default::default()
                            };
                            if let Err(e) = subscribe_tx.send(pong).await {
                                tracing::warn!(error = %e, "failed to send pong");
                            }
                        }
                        _ => {} // Ignore slot/tx/block/entry updates
                    }
                }
                Err(e) => {
                    return Err(ForkError::Geyser(format!("stream error: {e}")));
                }
            }

            // Check for dynamic account additions (non-blocking)
            self.check_dynamic_accounts(&mut subscribe_tx).await;
        }

        // Stream ended (EOF)
        Ok(())
    }

    /// Process a single account update from the gRPC stream.
    fn handle_account_update(&self, update: SubscribeUpdateAccount) {
        let slot = update.slot;

        let Some(acct_info) = update.account else {
            return;
        };

        let Ok(pubkey) = Pubkey::try_from(acct_info.pubkey.as_slice()) else {
            tracing::warn!(len = acct_info.pubkey.len(), "invalid pubkey in geyser update");
            return;
        };

        let owner = Pubkey::try_from(acct_info.owner.as_slice()).unwrap_or_default();

        let account = Account {
            lamports: acct_info.lamports,
            data: acct_info.data,
            owner,
            executable: acct_info.executable,
            rent_epoch: acct_info.rent_epoch,
        };

        // Geyser-streamed accounts are always monitored (longer TTL)
        self.cache.insert(pubkey, account, true);
        self.staleness.record_update(slot);

        tracing::trace!(pubkey = %pubkey, slot = slot, "geyser account update cached");
    }

    /// Build a Yellowstone gRPC subscription request.
    ///
    /// IMPORTANT: `ping` must NOT be included in the initial SubscribeRequest.
    /// Per Helius docs, including ping disables all subscription filters.
    /// Pings are sent separately via `subscribe_tx.send()`.
    fn build_subscribe_request(&self, from_slot: u64) -> SubscribeRequest {
        let mut account_keys: Vec<String> = self
            .config
            .initial_accounts
            .iter()
            .map(|pk| pk.to_string())
            .collect();

        // Add dynamically monitored accounts from the cache
        for pk in self.cache.monitored_keys() {
            let s = pk.to_string();
            if !account_keys.contains(&s) {
                account_keys.push(s);
            }
        }

        let mut accounts = HashMap::new();
        accounts.insert(
            "monitored".to_string(),
            SubscribeRequestFilterAccounts {
                account: account_keys,
                owner: vec![],
                filters: vec![],
                nonempty_txn_signature: None,
            },
        );

        SubscribeRequest {
            accounts,
            commitment: Some(CommitmentLevel::Confirmed as i32),
            from_slot: if from_slot > 0 {
                Some(from_slot)
            } else {
                None
            },
            // DO NOT set ping here — disables filters per Helius docs
            ..Default::default()
        }
    }

    /// Check for dynamic account additions from the verdict pipeline.
    /// When new accounts arrive, send an updated SubscribeRequest.
    /// Yellowstone protocol overwrites the entire filter on each new request.
    async fn check_dynamic_accounts<S>(&self, subscribe_tx: &mut S)
    where
        S: SinkExt<SubscribeRequest> + Unpin,
        <S as futures::Sink<SubscribeRequest>>::Error: std::fmt::Display,
    {
        let mut rx = match self.account_rx.try_lock() {
            Ok(rx) => rx,
            Err(_) => return,
        };

        let mut added = false;
        while let Ok(pubkeys) = rx.try_recv() {
            for pk in pubkeys {
                self.cache.mark_monitored(&pk);
            }
            added = true;
        }

        if added {
            let updated = self.build_subscribe_request(0);
            if let Err(e) = subscribe_tx.send(updated).await {
                tracing::warn!("failed to update subscription filters: {e}");
            }
        }
    }

    /// Fetch all monitored accounts via RPC. Fallback for when `from_slot`
    /// replay is unavailable. See spec Section 3.4 gap-fill procedure.
    async fn rpc_gap_fill(&self) -> ForkResult<()> {
        let monitored = self.cache.monitored_keys();
        if monitored.is_empty() {
            tracing::debug!("no monitored accounts to gap-fill");
            return Ok(());
        }

        tracing::info!(
            count = monitored.len(),
            last_slot = self.staleness.last_slot(),
            "starting RPC gap-fill"
        );

        // getMultipleAccounts supports max 100 per batch
        for chunk in monitored.chunks(100) {
            let accounts = self.rpc.fetch_multiple_accounts(chunk).await?;
            for (pk, maybe_acct) in chunk.iter().zip(accounts) {
                if let Some(acct) = maybe_acct {
                    self.cache.insert(*pk, acct, true);
                }
            }
        }

        let current_slot = self.rpc.get_slot().await?;
        self.staleness.record_update(current_slot);

        tracing::info!(
            count = monitored.len(),
            slot = current_slot,
            "RPC gap-fill complete"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use yellowstone_grpc_proto::prelude::{SubscribeUpdateAccount, SubscribeUpdateAccountInfo};

    use crate::staleness::StalenessConfig;

    fn test_cache() -> AccountCache {
        AccountCache::new()
    }

    fn test_staleness() -> StalenessTracker {
        StalenessTracker::new(StalenessConfig::default())
    }

    fn make_account_update(pubkey: &Pubkey, lamports: u64, slot: u64) -> SubscribeUpdateAccount {
        SubscribeUpdateAccount {
            account: Some(SubscribeUpdateAccountInfo {
                pubkey: pubkey.to_bytes().to_vec(),
                lamports,
                owner: Pubkey::default().to_bytes().to_vec(),
                executable: false,
                rent_epoch: 0,
                data: vec![1, 2, 3],
                write_version: 1,
                txn_signature: None,
            }),
            slot,
            is_startup: false,
        }
    }

    #[test]
    fn test_handle_account_update_writes_to_cache() {
        let cache = test_cache();
        let staleness = test_staleness();
        let (account_tx, account_rx) = tokio::sync::mpsc::unbounded_channel();

        // Use a dummy RpcManager — won't be called in this test
        let rpc = RpcManager::new(
            "https://localhost:0".to_string(),
            "https://localhost:0".to_string(),
        );

        let subscriber = GeyserSubscriber {
            config: GeyserConfig {
                endpoint: "https://localhost:0".to_string(),
                api_key: "test".to_string(),
                backoff_base: Duration::from_millis(100),
                backoff_max: Duration::from_secs(5),
                initial_accounts: vec![],
            },
            cache: cache.clone(),
            rpc: Arc::new(rpc),
            staleness: staleness.clone(),
            account_tx,
            account_rx: Arc::new(tokio::sync::Mutex::new(account_rx)),
        };

        let pk = Pubkey::new_unique();
        let update = make_account_update(&pk, 42_000, 100);
        subscriber.handle_account_update(update);

        // Verify cache write
        let cached = cache.get(&pk).expect("account should be cached");
        assert_eq!(cached.lamports, 42_000);
        assert_eq!(cached.data, vec![1, 2, 3]);

        // Verify staleness update
        assert_eq!(staleness.last_slot(), 100);
    }

    #[test]
    fn test_build_subscribe_request_includes_accounts() {
        let cache = test_cache();
        let staleness = test_staleness();
        let (account_tx, account_rx) = tokio::sync::mpsc::unbounded_channel();

        // Pre-populate cache with a monitored account
        let monitored_pk = Pubkey::new_unique();
        cache.insert(
            monitored_pk,
            Account {
                lamports: 100,
                data: vec![],
                owner: Pubkey::default(),
                executable: false,
                rent_epoch: 0,
            },
            true,
        );

        let initial_pk = Pubkey::new_unique();
        let rpc = RpcManager::new(
            "https://localhost:0".to_string(),
            "https://localhost:0".to_string(),
        );

        let subscriber = GeyserSubscriber {
            config: GeyserConfig {
                endpoint: "https://localhost:0".to_string(),
                api_key: "test".to_string(),
                backoff_base: Duration::from_millis(100),
                backoff_max: Duration::from_secs(5),
                initial_accounts: vec![initial_pk],
            },
            cache,
            rpc: Arc::new(rpc),
            staleness,
            account_tx,
            account_rx: Arc::new(tokio::sync::Mutex::new(account_rx)),
        };

        let request = subscriber.build_subscribe_request(50);

        // Check account filter
        let filter = request.accounts.get("monitored").expect("should have filter");
        assert!(
            filter.account.contains(&initial_pk.to_string()),
            "should include initial accounts"
        );
        assert!(
            filter.account.contains(&monitored_pk.to_string()),
            "should include monitored cache accounts"
        );

        // Check from_slot
        assert_eq!(request.from_slot, Some(50));

        // Check commitment
        assert_eq!(
            request.commitment,
            Some(CommitmentLevel::Confirmed as i32)
        );

        // Ping must NOT be set (disables filters per Helius docs)
        assert!(request.ping.is_none());
    }

    #[test]
    fn test_backoff_progression() {
        let base = Duration::from_millis(100);
        let max = Duration::from_secs(5);

        let mut backoff = base;
        let expected = vec![100, 200, 400, 800, 1600, 3200, 5000, 5000];

        for expected_ms in expected {
            assert_eq!(
                backoff.as_millis(),
                expected_ms,
                "backoff should be {expected_ms}ms"
            );
            backoff = (backoff * 2).min(max);
        }
    }

    #[tokio::test]
    #[ignore] // Requires HELIUS_API_KEY env var
    async fn test_geyser_connects_to_laserstream() {
        let config = GeyserConfig::from_env().expect("HELIUS_API_KEY required");
        let cache = test_cache();
        let staleness = test_staleness();

        let mut client = GeyserGrpcClient::build_from_shared(config.endpoint.clone())
            .unwrap()
            .x_token(Some(config.api_key.clone()))
            .unwrap()
            .connect()
            .await
            .expect("should connect to LaserStream");

        let mut accounts = HashMap::new();
        accounts.insert(
            "test".to_string(),
            SubscribeRequestFilterAccounts {
                account: vec![
                    "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(), // USDC mint
                ],
                owner: vec![],
                filters: vec![],
                nonempty_txn_signature: None,
            },
        );

        let request = SubscribeRequest {
            accounts,
            commitment: Some(CommitmentLevel::Confirmed as i32),
            ..Default::default()
        };

        let (_tx, mut stream) = client
            .subscribe_with_request(Some(request))
            .await
            .expect("should subscribe");

        // Wait for at least one update (up to 30s)
        let timeout = tokio::time::timeout(Duration::from_secs(30), stream.next()).await;
        assert!(timeout.is_ok(), "should receive an update within 30s");
    }
}
