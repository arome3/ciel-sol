// Geyser stream staleness tracking.
// See spec Section 3.4 for staleness thresholds:
//   - 3s  -> WARN (downgrade in-flight verdicts)
//   - 10s -> TIMEOUT (reject new verdict requests)

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::time::Instant;

/// Staleness state exposed to the verdict pipeline.
/// See spec Section 3.4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StalenessState {
    /// Stream is healthy, data is fresh.
    Fresh,
    /// Gap exceeds warn threshold (default 3s).
    /// In-flight verdicts should be downgraded to WARN with reason "state_parity_degraded".
    Warn,
    /// Gap exceeds timeout threshold (default 10s).
    /// New verdict requests should receive TIMEOUT until cache is refreshed.
    Timeout,
}

/// Configuration for staleness thresholds. See spec Section 3.5.
#[derive(Debug, Clone)]
pub struct StalenessConfig {
    /// Duration before downgrading verdicts to WARN (default 3s, ~7-8 slots).
    pub warn_threshold: Duration,
    /// Duration before rejecting new requests with TIMEOUT (default 10s).
    pub timeout_threshold: Duration,
}

impl Default for StalenessConfig {
    fn default() -> Self {
        Self {
            warn_threshold: Duration::from_secs(3),
            timeout_threshold: Duration::from_secs(10),
        }
    }
}

/// Tracks geyser stream freshness. Thread-safe, cheap to clone.
///
/// Updated by the geyser subscriber on every received account update.
/// **Must be consumed by the verdict pipeline (Unit 06)** to gate verdicts:
///
/// ```ignore
/// match staleness.state() {
///     StalenessState::Fresh   => { /* proceed normally */ }
///     StalenessState::Warn    => { verdict = WARN; reason = "state_parity_degraded"; }
///     StalenessState::Timeout => { return TIMEOUT; }
/// }
/// ```
///
/// Without this wiring, Ciel will produce APPROVE attestations against stale
/// fork state during outages. See spec Section 3.4.
///
/// Uses `Mutex<Instant>` over `AtomicU64` for time tracking because
/// `tokio::time::Instant` is compatible with `tokio::test(start_paused = true)`
/// for deterministic testing. The mutex critical section is a single write.
#[derive(Clone)]
pub struct StalenessTracker {
    last_update: Arc<Mutex<Instant>>,
    last_slot: Arc<AtomicU64>,
    config: StalenessConfig,
}

impl StalenessTracker {
    /// Create a new tracker. `last_update` is initialized to now.
    pub fn new(config: StalenessConfig) -> Self {
        Self {
            last_update: Arc::new(Mutex::new(Instant::now())),
            last_slot: Arc::new(AtomicU64::new(0)),
            config,
        }
    }

    /// Called by the geyser subscriber on every successful account update.
    pub fn record_update(&self, slot: u64) {
        *self.last_update.lock().unwrap() = Instant::now();
        self.last_slot.store(slot, Ordering::Release);
    }

    /// Current staleness state based on time since last update.
    pub fn state(&self) -> StalenessState {
        let elapsed = self.time_since_last_update();
        if elapsed > self.config.timeout_threshold {
            StalenessState::Timeout
        } else if elapsed > self.config.warn_threshold {
            StalenessState::Warn
        } else {
            StalenessState::Fresh
        }
    }

    /// Last slot successfully received from the geyser stream.
    pub fn last_slot(&self) -> u64 {
        self.last_slot.load(Ordering::Acquire)
    }

    /// Duration since the last geyser update was received.
    pub fn time_since_last_update(&self) -> Duration {
        self.last_update.lock().unwrap().elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> StalenessConfig {
        StalenessConfig {
            warn_threshold: Duration::from_secs(1),
            timeout_threshold: Duration::from_secs(3),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn test_staleness_fresh_after_update() {
        let tracker = StalenessTracker::new(test_config());
        tracker.record_update(100);

        assert_eq!(tracker.state(), StalenessState::Fresh);
        assert_eq!(tracker.last_slot(), 100);
        assert!(tracker.time_since_last_update() < Duration::from_millis(50));
    }

    #[tokio::test(start_paused = true)]
    async fn test_staleness_warn_after_threshold() {
        let tracker = StalenessTracker::new(test_config());
        tracker.record_update(100);

        // Advance past warn threshold (1s) but before timeout (3s)
        tokio::time::advance(Duration::from_millis(1500)).await;

        assert_eq!(tracker.state(), StalenessState::Warn);
    }

    #[tokio::test(start_paused = true)]
    async fn test_staleness_timeout_after_threshold() {
        let tracker = StalenessTracker::new(test_config());
        tracker.record_update(100);

        // Advance past timeout threshold (3s)
        tokio::time::advance(Duration::from_millis(4000)).await;

        assert_eq!(tracker.state(), StalenessState::Timeout);
    }

    #[tokio::test(start_paused = true)]
    async fn test_staleness_recovers_after_update() {
        let tracker = StalenessTracker::new(test_config());
        tracker.record_update(100);

        // Enter Warn state
        tokio::time::advance(Duration::from_millis(1500)).await;
        assert_eq!(tracker.state(), StalenessState::Warn);

        // New update brings us back to Fresh
        tracker.record_update(200);
        assert_eq!(tracker.state(), StalenessState::Fresh);
        assert_eq!(tracker.last_slot(), 200);
    }
}
