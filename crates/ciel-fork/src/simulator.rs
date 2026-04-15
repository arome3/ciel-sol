// Fork simulator wrapping LiteSVM with account caching and sysvar initialization.
// See spec Section 3.1 for engine choice, Section 3.6 for anti-sandbox-detection.

use std::sync::Arc;

use litesvm::LiteSVM;
use solana_sdk::account::Account;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::sysvar;

use crate::cache::AccountCache;
use crate::rpc::RpcManager;
use crate::{ForkError, ForkResult};

/// Sysvar accounts to load from mainnet for anti-sandbox-detection.
/// See spec Section 3.6: programs can detect simulation by checking these.
const SYSVAR_IDS: &[(&str, Pubkey)] = &[
    ("Clock", sysvar::clock::ID),
    ("Rent", sysvar::rent::ID),
    ("SlotHashes", sysvar::slot_hashes::ID),
    ("RecentBlockhashes", sysvar::recent_blockhashes::ID),
    ("EpochSchedule", sysvar::epoch_schedule::ID),
    ("StakeHistory", sysvar::stake_history::ID),
];

// --- Type conversion functions (solana-sdk 2.2 ↔ litesvm's solana 3.x types) ---

/// Convert solana-sdk 2.2 Pubkey → litesvm's solana-address 2.6 Address.
/// Both are [u8; 32] under the hood.
pub(crate) fn to_litesvm_address(pubkey: &Pubkey) -> litesvm_address::Address {
    litesvm_address::Address::from(pubkey.to_bytes())
}

/// Convert solana-sdk 2.2 Account → litesvm's solana-account 3.4 Account.
/// Same-shaped structs, different crate versions.
pub(crate) fn to_litesvm_account(account: &Account) -> litesvm_account::Account {
    litesvm_account::Account {
        lamports: account.lamports,
        data: account.data.clone(),
        owner: litesvm_address::Address::from(account.owner.to_bytes()),
        executable: account.executable,
        rent_epoch: account.rent_epoch,
    }
}

/// Convert litesvm's solana-account 3.4 Account → solana-sdk 2.2 Account.
/// Used by unit 02 (executor) for reading simulation results back from LiteSVM.
#[allow(dead_code)]
pub(crate) fn from_litesvm_account(account: &litesvm_account::Account) -> Account {
    Account {
        lamports: account.lamports,
        data: account.data.clone(),
        owner: Pubkey::new_from_array(account.owner.to_bytes()),
        executable: account.executable,
        rent_epoch: account.rent_epoch,
    }
}

/// Convert litesvm's solana-account 3.4 AccountSharedData → solana-sdk 2.2 Account.
/// Used by executor to process simulate_transaction post_accounts results.
/// See spec Section 3.1 — simulate_transaction returns AccountSharedData, not Account.
pub(crate) fn from_litesvm_shared_account(shared: litesvm_account::AccountSharedData) -> Account {
    let litesvm_acct: litesvm_account::Account = shared.into();
    from_litesvm_account(&litesvm_acct)
}

/// LiteSVM-based fork simulator with lazy account loading and caching.
///
/// Wraps an in-process LiteSVM instance with:
/// - DashMap account cache with TTL-based eviction
/// - RPC failover (Helius primary → Triton One fallback, 100ms timeout)
/// - Real mainnet sysvar initialization for anti-sandbox-detection
///
/// See spec Section 3.1.
pub struct ForkSimulator {
    svm: LiteSVM,
    cache: AccountCache,
    rpc: Option<Arc<RpcManager>>,
    pinned_slot: u64,
    pinned_blockhash: Hash,
}

impl ForkSimulator {
    /// Create a new ForkSimulator, initializing LiteSVM and loading mainnet sysvars.
    /// Reads HELIUS_API_KEY and TRITON_API_KEY from environment.
    pub async fn new() -> ForkResult<Self> {
        let rpc = RpcManager::from_env()?.into_arc();
        Self::with_rpc(rpc).await
    }

    /// Create a ForkSimulator with a pre-built RpcManager.
    /// Useful for sharing an RpcManager across multiple simulator instances.
    pub async fn with_rpc(rpc: Arc<RpcManager>) -> ForkResult<Self> {
        // Initialize LiteSVM with builtins and default sysvars.
        // We overwrite sysvars with real mainnet data in init_sysvars().
        // See Surfpool's initialization pattern.
        let svm = LiteSVM::new()
            .with_builtins()
            .with_sysvars();

        let cache = AccountCache::new();

        let mut simulator = Self {
            svm,
            cache,
            rpc: Some(rpc),
            pinned_slot: 0,
            pinned_blockhash: Hash::default(),
        };

        simulator.init_sysvars().await?;

        Ok(simulator)
    }

    /// Create an offline ForkSimulator without RPC connectivity.
    /// Uses LiteSVM's synthetic default sysvars — suitable for fixture-based
    /// testing where accounts are injected directly via `set_account()`.
    pub fn new_offline() -> Self {
        let svm = LiteSVM::new()
            .with_builtins()
            .with_sysvars();

        Self {
            svm,
            cache: AccountCache::new(),
            rpc: None,
            pinned_slot: 0,
            pinned_blockhash: Hash::default(),
        }
    }

    /// Directly inject an account into the fork (bypasses cache and RPC).
    /// Used for loading fixture data or pre-populating state.
    pub fn set_account(&mut self, pubkey: &Pubkey, account: &Account) -> ForkResult<()> {
        let addr = to_litesvm_address(pubkey);
        let svm_account = to_litesvm_account(account);
        self.svm
            .set_account(addr, svm_account)
            .map_err(|e| ForkError::LiteSvm(e.to_string()))?;
        self.cache.insert(*pubkey, account.clone(), false);
        Ok(())
    }

    /// Load real mainnet sysvar accounts into the fork.
    /// See spec Section 3.6: pre-load all sysvar accounts to defeat sandbox detection.
    async fn init_sysvars(&mut self) -> ForkResult<()> {
        let rpc = self
            .rpc
            .as_ref()
            .ok_or_else(|| ForkError::Config("RPC not configured (offline mode)".into()))?;

        // Pin to the current mainnet slot and blockhash
        self.pinned_slot = rpc.get_slot().await?;
        self.pinned_blockhash = rpc.get_latest_blockhash().await?;

        tracing::info!(
            slot = self.pinned_slot,
            blockhash = %self.pinned_blockhash,
            "fork pinned to mainnet state"
        );

        let mut loaded = 0usize;

        for (name, sysvar_id) in SYSVAR_IDS {
            match rpc.fetch_account(sysvar_id).await {
                Ok(account) => {
                    let addr = to_litesvm_address(sysvar_id);
                    let svm_account = to_litesvm_account(&account);

                    self.svm.set_account(addr, svm_account).map_err(|e| {
                        ForkError::LiteSvm(format!("failed to set sysvar {name}: {e}"))
                    })?;

                    // Cache the sysvar as monitored (longer TTL)
                    self.cache.insert(*sysvar_id, account, true);
                    loaded += 1;

                    tracing::debug!(sysvar = name, "loaded mainnet sysvar");
                }
                Err(ForkError::AccountNotFound { .. }) => {
                    // Some sysvars (e.g., deprecated Fees) may not exist — skip with warning
                    tracing::warn!(sysvar = name, "sysvar not found on mainnet, using LiteSVM default");
                }
                Err(e) => {
                    tracing::warn!(sysvar = name, error = %e, "failed to fetch sysvar, using LiteSVM default");
                }
            }
        }

        tracing::info!(
            count = loaded,
            total = SYSVAR_IDS.len(),
            "mainnet sysvars loaded into fork"
        );

        Ok(())
    }

    /// Lazy-load an account into the fork.
    /// Cache hit → inject into SVM and return.
    /// Cache miss → fetch from RPC, cache, inject into SVM, return.
    pub async fn load_account(&mut self, pubkey: &Pubkey) -> ForkResult<Account> {
        // Check cache first
        if let Some(account) = self.cache.get(pubkey) {
            // Inject into SVM (may already be there, but set_account is idempotent)
            let addr = to_litesvm_address(pubkey);
            let svm_account = to_litesvm_account(&account);
            self.svm
                .set_account(addr, svm_account)
                .map_err(|e| ForkError::LiteSvm(e.to_string()))?;

            tracing::debug!(pubkey = %pubkey, "account loaded from cache");
            return Ok(account);
        }

        // Cache miss — fetch from RPC
        let rpc = self
            .rpc
            .as_ref()
            .ok_or_else(|| ForkError::Config("RPC not configured (offline mode)".into()))?;
        let account = rpc.fetch_account(pubkey).await?;

        // Cache as non-monitored (lazy-fetched accounts per spec Section 3.2)
        self.cache.insert(*pubkey, account.clone(), false);

        // Inject into SVM
        let addr = to_litesvm_address(pubkey);
        let svm_account = to_litesvm_account(&account);
        self.svm
            .set_account(addr, svm_account)
            .map_err(|e| ForkError::LiteSvm(e.to_string()))?;

        tracing::debug!(pubkey = %pubkey, "account fetched from RPC and cached");
        Ok(account)
    }

    /// Batch-load multiple accounts into the fork.
    /// Partitions into cached and uncached, batch-fetches uncached from RPC.
    pub async fn load_accounts(&mut self, pubkeys: &[Pubkey]) -> ForkResult<Vec<Account>> {
        let mut results = Vec::with_capacity(pubkeys.len());
        let mut uncached_indices = Vec::new();
        let mut uncached_pubkeys = Vec::new();

        // Partition: check cache for each pubkey
        for (i, pubkey) in pubkeys.iter().enumerate() {
            if let Some(account) = self.cache.get(pubkey) {
                results.push(Some(account));
            } else {
                results.push(None);
                uncached_indices.push(i);
                uncached_pubkeys.push(*pubkey);
            }
        }

        // Batch-fetch uncached accounts
        if !uncached_pubkeys.is_empty() {
            let rpc = self
                .rpc
                .as_ref()
                .ok_or_else(|| ForkError::Config("RPC not configured (offline mode)".into()))?;
            let fetched = rpc.fetch_multiple_accounts(&uncached_pubkeys).await?;

            for (idx_pos, fetched_account) in fetched.into_iter().enumerate() {
                let original_idx = uncached_indices[idx_pos];
                let pubkey = &pubkeys[original_idx];

                match fetched_account {
                    Some(account) => {
                        self.cache.insert(*pubkey, account.clone(), false);
                        results[original_idx] = Some(account);
                    }
                    None => {
                        return Err(ForkError::AccountNotFound {
                            pubkey: pubkey.to_string(),
                        });
                    }
                }
            }
        }

        // Inject all accounts into SVM
        for (pubkey, account_opt) in pubkeys.iter().zip(results.iter()) {
            if let Some(account) = account_opt {
                let addr = to_litesvm_address(pubkey);
                let svm_account = to_litesvm_account(account);
                self.svm
                    .set_account(addr, svm_account)
                    .map_err(|e| ForkError::LiteSvm(e.to_string()))?;
            }
        }

        tracing::debug!(
            total = pubkeys.len(),
            from_cache = pubkeys.len() - uncached_pubkeys.len(),
            from_rpc = uncached_pubkeys.len(),
            "batch account load complete"
        );

        // Unwrap all Options — we've ensured all are Some above
        Ok(results.into_iter().map(|opt| opt.unwrap()).collect())
    }

    /// The slot this fork is pinned to.
    pub fn pinned_slot(&self) -> u64 {
        self.pinned_slot
    }

    /// The blockhash this fork is pinned to.
    pub fn pinned_blockhash(&self) -> Hash {
        self.pinned_blockhash
    }

    /// Access the shared account cache.
    pub fn cache(&self) -> &AccountCache {
        &self.cache
    }

    /// Read-only access to the underlying LiteSVM instance.
    pub fn svm(&self) -> &LiteSVM {
        &self.svm
    }

    /// Mutable access to the underlying LiteSVM instance (for unit 02: executor).
    pub fn svm_mut(&mut self) -> &mut LiteSVM {
        &mut self.svm
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_account(lamports: u64) -> Account {
        Account {
            lamports,
            data: vec![10, 20, 30, 40],
            owner: solana_sdk::system_program::id(),
            executable: false,
            rent_epoch: 42,
        }
    }

    #[test]
    fn test_to_litesvm_address_roundtrip() {
        let pubkey = Pubkey::new_unique();
        let addr = to_litesvm_address(&pubkey);
        assert_eq!(pubkey.to_bytes(), addr.to_bytes());
    }

    #[test]
    fn test_to_litesvm_account_preserves_fields() {
        let account = make_test_account(1234);
        let converted = to_litesvm_account(&account);

        assert_eq!(converted.lamports, 1234);
        assert_eq!(converted.data, vec![10, 20, 30, 40]);
        assert_eq!(converted.owner.to_bytes(), account.owner.to_bytes());
        assert!(!converted.executable);
        assert_eq!(converted.rent_epoch, 42);
    }

    #[test]
    fn test_account_conversion_roundtrip() {
        let original = make_test_account(999);
        let litesvm_acct = to_litesvm_account(&original);
        let roundtripped = from_litesvm_account(&litesvm_acct);

        assert_eq!(roundtripped.lamports, original.lamports);
        assert_eq!(roundtripped.data, original.data);
        assert_eq!(roundtripped.owner, original.owner);
        assert_eq!(roundtripped.executable, original.executable);
        assert_eq!(roundtripped.rent_epoch, original.rent_epoch);
    }

    #[test]
    fn test_litesvm_set_account_roundtrip() {
        let mut svm = LiteSVM::new().with_builtins().with_sysvars();
        let pubkey = Pubkey::new_unique();
        let account = make_test_account(500);

        let addr = to_litesvm_address(&pubkey);
        let svm_account = to_litesvm_account(&account);

        svm.set_account(addr, svm_account)
            .expect("set_account should succeed");

        let retrieved = svm.get_account(&addr).expect("account should exist in SVM");
        let retrieved_v2 = from_litesvm_account(&retrieved);

        assert_eq!(retrieved_v2.lamports, 500);
        assert_eq!(retrieved_v2.data, account.data);
        assert_eq!(retrieved_v2.owner, account.owner);
        assert_eq!(retrieved_v2.executable, account.executable);
    }

    #[tokio::test]
    #[ignore] // Requires HELIUS_API_KEY and TRITON_API_KEY env vars
    async fn test_init_sysvars_loads_clock() {
        let sim = ForkSimulator::new().await.expect("should initialize");

        assert!(sim.pinned_slot() > 0, "should be pinned to a real slot");
        assert_ne!(
            sim.pinned_blockhash(),
            Hash::default(),
            "should have a real blockhash"
        );

        // Verify Clock sysvar was loaded
        let clock_addr = to_litesvm_address(&sysvar::clock::ID);
        let clock_account = sim.svm().get_account(&clock_addr);
        assert!(clock_account.is_some(), "Clock sysvar should be loaded");
    }

    #[tokio::test]
    #[ignore] // Requires HELIUS_API_KEY and TRITON_API_KEY env vars
    async fn test_load_account_caches() {
        let mut sim = ForkSimulator::new().await.expect("should initialize");
        let usdc_mint: Pubkey = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
            .parse()
            .unwrap();

        // First load — fetches from RPC
        let account1 = sim.load_account(&usdc_mint).await.expect("should fetch");
        assert!(account1.lamports > 0);
        assert!(!account1.data.is_empty());

        // Second load — should hit cache
        let account2 = sim.load_account(&usdc_mint).await.expect("should cache hit");
        assert_eq!(account1.data, account2.data);
        assert_eq!(account1.lamports, account2.lamports);
    }

    #[tokio::test]
    #[ignore] // Requires HELIUS_API_KEY and TRITON_API_KEY env vars
    async fn test_load_account_data_correct() {
        let mut sim = ForkSimulator::new().await.expect("should initialize");
        let usdc_mint: Pubkey = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
            .parse()
            .unwrap();

        let account = sim.load_account(&usdc_mint).await.expect("should fetch");

        // USDC mint is owned by the Token Program
        let token_program: Pubkey = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
            .parse()
            .unwrap();
        assert_eq!(account.owner, token_program, "USDC mint should be owned by Token Program");
        assert!(!account.executable, "mint accounts are not executable");
    }
}
