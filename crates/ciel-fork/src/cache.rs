// Account cache backed by DashMap with dual-TTL eviction.
// See spec Section 3.2 for cache architecture.

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use tokio::time::Instant;

/// Default TTL for non-monitored accounts (spec Section 3.2: 5 minutes).
const DEFAULT_TTL_SECS: u64 = 300;

/// Default TTL for monitored accounts (spec Section 3.2: 10 minutes).
const DEFAULT_MONITORED_TTL_SECS: u64 = 600;

/// An account with insertion metadata for TTL-based eviction.
#[derive(Debug, Clone)]
pub struct TimestampedAccount {
    pub account: Account,
    pub inserted_at: Instant,
    pub is_monitored: bool,
}

/// Thread-safe account cache with dual-TTL eviction.
///
/// Non-monitored accounts expire after 5 minutes (LRU eviction).
/// Monitored accounts (oracle feeds, DeFi programs) expire after 10 minutes.
/// See spec Section 3.2.
#[derive(Clone)]
pub struct AccountCache {
    inner: Arc<DashMap<Pubkey, TimestampedAccount>>,
    ttl: Duration,
    monitored_ttl: Duration,
}

impl AccountCache {
    /// Create a cache with default TTLs (5min / 10min per spec Section 3.2).
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
            ttl: Duration::from_secs(DEFAULT_TTL_SECS),
            monitored_ttl: Duration::from_secs(DEFAULT_MONITORED_TTL_SECS),
        }
    }

    /// Create a cache with custom TTLs (for testing).
    pub fn with_ttl(ttl_secs: u64, monitored_ttl_secs: u64) -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
            ttl: Duration::from_secs(ttl_secs),
            monitored_ttl: Duration::from_secs(monitored_ttl_secs),
        }
    }

    /// Get an account if it exists and is not expired.
    /// Performs lazy eviction: removes stale entries on access.
    pub fn get(&self, pubkey: &Pubkey) -> Option<Account> {
        let entry = self.inner.get(pubkey)?;
        let effective_ttl = if entry.is_monitored {
            self.monitored_ttl
        } else {
            self.ttl
        };

        if entry.inserted_at.elapsed() > effective_ttl {
            drop(entry);
            self.inner.remove(pubkey);
            return None;
        }

        Some(entry.account.clone())
    }

    /// Insert an account into the cache.
    pub fn insert(&self, pubkey: Pubkey, account: Account, monitored: bool) {
        self.inner.insert(
            pubkey,
            TimestampedAccount {
                account,
                inserted_at: Instant::now(),
                is_monitored: monitored,
            },
        );
    }

    /// Upgrade an existing entry to monitored status (extends its TTL).
    /// No-op if the entry doesn't exist.
    pub fn mark_monitored(&self, pubkey: &Pubkey) {
        if let Some(mut entry) = self.inner.get_mut(pubkey) {
            entry.is_monitored = true;
        }
    }

    /// Remove all expired entries. Returns the number of entries evicted.
    pub fn evict_stale(&self) -> usize {
        let before = self.inner.len();
        let ttl = self.ttl;
        let monitored_ttl = self.monitored_ttl;

        self.inner.retain(|_pubkey, entry| {
            let effective_ttl = if entry.is_monitored {
                monitored_ttl
            } else {
                ttl
            };
            entry.inserted_at.elapsed() <= effective_ttl
        });

        before - self.inner.len()
    }

    /// Number of entries currently in the cache (including potentially stale ones).
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Returns all pubkeys currently marked as monitored.
    /// Used by the geyser subscriber for subscription filters and gap-fill.
    pub fn monitored_keys(&self) -> Vec<Pubkey> {
        self.inner
            .iter()
            .filter(|entry| entry.value().is_monitored)
            .map(|entry| *entry.key())
            .collect()
    }
}

impl Default for AccountCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_account(lamports: u64) -> Account {
        Account {
            lamports,
            data: vec![1, 2, 3],
            owner: Pubkey::new_unique(),
            executable: false,
            rent_epoch: 0,
        }
    }

    #[test]
    fn test_insert_and_get() {
        let cache = AccountCache::new();
        let pubkey = Pubkey::new_unique();
        let account = make_test_account(1000);

        cache.insert(pubkey, account.clone(), false);

        let retrieved = cache.get(&pubkey).expect("should find cached account");
        assert_eq!(retrieved.lamports, 1000);
        assert_eq!(retrieved.data, account.data);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_cache_miss() {
        let cache = AccountCache::new();
        assert!(cache.get(&Pubkey::new_unique()).is_none());
        assert_eq!(cache.len(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn test_stale_entry_returns_none() {
        let cache = AccountCache::with_ttl(1, 1);
        let pubkey = Pubkey::new_unique();

        cache.insert(pubkey, make_test_account(500), false);
        assert!(cache.get(&pubkey).is_some(), "should exist before TTL");

        // Advance time past TTL
        tokio::time::advance(Duration::from_secs(2)).await;

        assert!(cache.get(&pubkey).is_none(), "should be evicted after TTL");
        assert_eq!(cache.len(), 0, "lazy eviction should have removed it");
    }

    #[tokio::test(start_paused = true)]
    async fn test_monitored_longer_ttl() {
        // Non-monitored: 1s TTL, monitored: 5s TTL
        let cache = AccountCache::with_ttl(1, 5);
        let pk_regular = Pubkey::new_unique();
        let pk_monitored = Pubkey::new_unique();

        cache.insert(pk_regular, make_test_account(100), false);
        cache.insert(pk_monitored, make_test_account(200), true);

        // Advance past non-monitored TTL but within monitored TTL
        tokio::time::advance(Duration::from_secs(2)).await;

        assert!(
            cache.get(&pk_regular).is_none(),
            "non-monitored should expire at 1s"
        );
        assert!(
            cache.get(&pk_monitored).is_some(),
            "monitored should survive past 1s"
        );

        // Advance past monitored TTL
        tokio::time::advance(Duration::from_secs(4)).await;

        assert!(
            cache.get(&pk_monitored).is_none(),
            "monitored should expire at 5s"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn test_evict_stale() {
        let cache = AccountCache::with_ttl(1, 5);

        // Insert 3 non-monitored and 1 monitored
        for _ in 0..3 {
            cache.insert(Pubkey::new_unique(), make_test_account(100), false);
        }
        cache.insert(Pubkey::new_unique(), make_test_account(200), true);
        assert_eq!(cache.len(), 4);

        // Advance past non-monitored TTL
        tokio::time::advance(Duration::from_secs(2)).await;

        let evicted = cache.evict_stale();
        assert_eq!(evicted, 3, "should evict 3 non-monitored entries");
        assert_eq!(cache.len(), 1, "only monitored entry should remain");
    }

    #[test]
    fn test_monitored_keys() {
        let cache = AccountCache::new();
        let pk_monitored1 = Pubkey::new_unique();
        let pk_monitored2 = Pubkey::new_unique();
        let pk_regular = Pubkey::new_unique();

        cache.insert(pk_monitored1, make_test_account(100), true);
        cache.insert(pk_monitored2, make_test_account(200), true);
        cache.insert(pk_regular, make_test_account(300), false);

        let mut keys = cache.monitored_keys();
        keys.sort();
        let mut expected = vec![pk_monitored1, pk_monitored2];
        expected.sort();

        assert_eq!(keys, expected);
        assert_eq!(keys.len(), 2, "should not include non-monitored account");
    }

    #[test]
    fn test_concurrent_access() {
        let cache = AccountCache::new();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let mut handles = Vec::new();

            for i in 0..100u64 {
                let cache = cache.clone();
                handles.push(tokio::spawn(async move {
                    let pk = Pubkey::new_unique();
                    cache.insert(pk, make_test_account(i), i % 2 == 0);
                    cache.get(&pk);
                }));
            }

            for handle in handles {
                handle.await.unwrap();
            }
        });

        assert_eq!(cache.len(), 100);
    }
}
