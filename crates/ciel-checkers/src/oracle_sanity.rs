// Oracle Sanity checker — cross-references Switchboard and Pyth price feeds
// to detect oracle manipulation (Drift-class attacks).
// See spec Section 4.3.1 and docs/10-oracle-sanity-checker.md.

use std::collections::HashSet;

use async_trait::async_trait;
use serde_json::json;
use solana_sdk::pubkey::Pubkey;

use crate::oracle_cache::{CanonicalFeedMap, OracleType};
use crate::traits::{Checker, CheckerContext, CheckerOutput, Flag, Severity};

// ---------------------------------------------------------------------------
// Flag codes
// ---------------------------------------------------------------------------

const FLAG_DEVIATION: &str = "ORACLE_DEVIATION_3_SIGMA";
const FLAG_UNKNOWN: &str = "UNKNOWN_ORACLE_ACCOUNT";
const FLAG_STALE: &str = "STALE_ORACLE_FEED";
const FLAG_WIDE_CONF: &str = "WIDE_CONFIDENCE_INTERVAL";

// ---------------------------------------------------------------------------
// OracleSanityChecker
// ---------------------------------------------------------------------------

/// Oracle Sanity checker. See spec Section 4.3.1.
///
/// Cross-references Switchboard On-Demand and Pyth Lazer price feeds for
/// the same asset and flags deviations exceeding a configurable sigma
/// threshold (default 3.0). Also detects non-canonical oracle accounts,
/// stale feeds, and abnormally wide confidence intervals.
///
/// Fully deterministic given the same `OracleCache` snapshot.
pub struct OracleSanityChecker {
    /// Sigma deviation threshold. Default 3.0. See spec Section 4.3.1.
    pub sigma_threshold: f64,
    /// Maximum age (in the same units as OracleCache timestamps) before
    /// a feed is considered stale. Default 30.
    pub staleness_threshold_secs: i64,
    /// Maximum confidence/price ratio before flagging as abnormally wide.
    /// Default 0.05 (5% of price).
    pub wide_confidence_ratio: f64,
    /// Canonical feed mapping for cross-referencing and verification.
    pub feed_map: CanonicalFeedMap,
}

impl OracleSanityChecker {
    /// Create a new checker with the given feed map and default thresholds.
    pub fn new(feed_map: CanonicalFeedMap) -> Self {
        Self {
            sigma_threshold: 3.0,
            staleness_threshold_secs: 30,
            wide_confidence_ratio: 0.05,
            feed_map,
        }
    }

    /// Override the sigma deviation threshold.
    pub fn with_sigma_threshold(mut self, threshold: f64) -> Self {
        self.sigma_threshold = threshold;
        self
    }

    /// Override the staleness threshold.
    pub fn with_staleness_threshold(mut self, secs: i64) -> Self {
        self.staleness_threshold_secs = secs;
        self
    }

    /// Override the wide-confidence ratio.
    pub fn with_wide_confidence_ratio(mut self, ratio: f64) -> Self {
        self.wide_confidence_ratio = ratio;
        self
    }
}

#[async_trait]
impl Checker for OracleSanityChecker {
    fn name(&self) -> &'static str {
        "oracle_sanity"
    }

    async fn check(&self, ctx: &CheckerContext) -> CheckerOutput {
        let oracle_reads = &ctx.trace.oracle_reads;

        // Early exit: no oracle reads → nothing to check.
        if oracle_reads.is_empty() {
            return CheckerOutput {
                checker_name: "oracle_sanity".to_string(),
                passed: true,
                severity: Severity::None,
                flags: vec![],
                details: "No oracle reads in transaction".to_string(),
            };
        }

        let mut flags: Vec<Flag> = Vec::new();
        // Track checked pairs to avoid duplicate deviation flags when both
        // feeds for the same asset appear in oracle_reads.
        let mut checked_pairs: HashSet<(Pubkey, Pubkey)> = HashSet::new();

        for read in oracle_reads {
            let pubkey = read.oracle_pubkey;

            // Step 1: Look up in OracleCache.
            let cached = ctx.oracle_cache.get(&pubkey);

            match cached {
                None => {
                    // Not in cache — check if it's even a known canonical feed.
                    if !self.feed_map.is_known(&pubkey) {
                        flags.push(Flag {
                            code: FLAG_UNKNOWN.to_string(),
                            message: format!(
                                "Oracle account {} (type: {}) is not a known canonical feed",
                                pubkey, read.oracle_type
                            ),
                            data: json!({
                                "oracle_pubkey": pubkey.to_string(),
                                "oracle_type": read.oracle_type,
                            }),
                        });
                    }
                    // Known but not cached → skip (no data to cross-reference).
                    continue;
                }
                Some(price) => {
                    // Step 2: Staleness check.
                    let age = ctx.oracle_cache.reference_timestamp - price.timestamp;
                    if age > self.staleness_threshold_secs {
                        flags.push(Flag {
                            code: FLAG_STALE.to_string(),
                            message: format!(
                                "{} feed for {} is stale (age: {}s, threshold: {}s)",
                                format_oracle_type(price.oracle_type),
                                price.asset,
                                age,
                                self.staleness_threshold_secs,
                            ),
                            data: json!({
                                "oracle_pubkey": pubkey.to_string(),
                                "oracle_type": format_oracle_type(price.oracle_type),
                                "asset": price.asset,
                                "age_seconds": age,
                                "threshold_seconds": self.staleness_threshold_secs,
                            }),
                        });
                    }

                    // Step 3: Wide confidence interval check.
                    if price.price > 0.0 {
                        let ratio = price.confidence / price.price;
                        if ratio > self.wide_confidence_ratio {
                            flags.push(Flag {
                                code: FLAG_WIDE_CONF.to_string(),
                                message: format!(
                                    "{} feed for {} has abnormally wide confidence ({:.2}% of price)",
                                    format_oracle_type(price.oracle_type),
                                    price.asset,
                                    ratio * 100.0,
                                ),
                                data: json!({
                                    "oracle_pubkey": pubkey.to_string(),
                                    "oracle_type": format_oracle_type(price.oracle_type),
                                    "asset": price.asset,
                                    "confidence": price.confidence,
                                    "price": price.price,
                                    "ratio": ratio,
                                }),
                            });
                        }
                    }

                    // Step 4: Cross-reference deviation check.
                    if let Some(cross_pubkey) = self.feed_map.cross_reference(&pubkey) {
                        // Normalize pair for deduplication: (min, max).
                        let pair = if pubkey < cross_pubkey {
                            (pubkey, cross_pubkey)
                        } else {
                            (cross_pubkey, pubkey)
                        };

                        if !checked_pairs.contains(&pair) {
                            checked_pairs.insert(pair);

                            if let Some(cross_price) = ctx.oracle_cache.get(&cross_pubkey) {
                                let deviation = (price.price - cross_price.price).abs();
                                let max_uncertainty =
                                    price.confidence.max(cross_price.confidence);

                                if max_uncertainty > 0.0 {
                                    let sigma = deviation / max_uncertainty;
                                    if sigma > self.sigma_threshold {
                                        // Determine which is Switchboard and which is Pyth
                                        // for the output schema. See spec Section 4.3.1.
                                        let (sb_price, pyth_price, sb_conf, pyth_conf) =
                                            match price.oracle_type {
                                                OracleType::Switchboard => (
                                                    price.price,
                                                    cross_price.price,
                                                    price.confidence,
                                                    cross_price.confidence,
                                                ),
                                                OracleType::Pyth => (
                                                    cross_price.price,
                                                    price.price,
                                                    cross_price.confidence,
                                                    price.confidence,
                                                ),
                                            };

                                        flags.push(Flag {
                                            code: FLAG_DEVIATION.to_string(),
                                            message: format!(
                                                "{} price deviation {:.1} sigma between Switchboard and Pyth",
                                                price.asset, sigma,
                                            ),
                                            data: json!({
                                                "asset": price.asset,
                                                "switchboard_price": sb_price,
                                                "pyth_price": pyth_price,
                                                "deviation_sigma": sigma,
                                                "switchboard_std_dev": sb_conf,
                                                "pyth_confidence": pyth_conf,
                                            }),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Aggregate: severity = max of all flag severities.
        let severity = flags
            .iter()
            .map(|f| flag_severity(&f.code))
            .max()
            .unwrap_or(Severity::None);
        let passed = flags.is_empty();
        let details = if passed {
            "All oracle reads passed sanity checks".to_string()
        } else {
            format!("{} oracle anomalies detected", flags.len())
        };

        CheckerOutput {
            checker_name: "oracle_sanity".to_string(),
            passed,
            severity,
            flags,
            details,
        }
    }
}

/// Map flag code → severity.
fn flag_severity(code: &str) -> Severity {
    match code {
        FLAG_DEVIATION => Severity::Critical,
        FLAG_UNKNOWN => Severity::High,
        FLAG_STALE => Severity::High,
        FLAG_WIDE_CONF => Severity::Medium,
        _ => Severity::Low,
    }
}

fn format_oracle_type(t: OracleType) -> &'static str {
    match t {
        OracleType::Switchboard => "Switchboard",
        OracleType::Pyth => "Pyth",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle_cache::{OracleCache, OraclePrice, OracleType};
    use crate::traits::ProgramRegistry;
    use ciel_fork::trace::OracleRead;
    use ciel_fork::SimulationTrace;
    use solana_sdk::message::Message;
    use solana_sdk::transaction::Transaction;
    use std::collections::HashMap;

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    /// Derive a deterministic pubkey from a seed, matching the pattern in
    /// ciel-fixtures/src/drift.rs.
    fn deterministic_pubkey(seed: &str) -> Pubkey {
        use solana_sdk::hash::hash;
        use solana_sdk::signer::keypair::keypair_from_seed;
        use solana_sdk::signer::Signer;
        let h = hash(seed.as_bytes());
        keypair_from_seed(h.as_ref()).unwrap().pubkey()
    }

    fn switchboard_feed_pubkey() -> Pubkey {
        deterministic_pubkey("ciel-drift-fixture-switchboard-feed")
    }

    fn pyth_feed_pubkey() -> Pubkey {
        deterministic_pubkey("ciel-drift-fixture-pyth-feed")
    }

    /// Build a test CanonicalFeedMap with the Drift fixture's SOL/USD feeds.
    fn test_feed_map() -> CanonicalFeedMap {
        let mut map = CanonicalFeedMap::empty();
        map.register("SOL/USD", switchboard_feed_pubkey(), pyth_feed_pubkey());
        map
    }

    /// Build an OracleCache with the given Switchboard and Pyth prices.
    fn test_cache(
        sb_price: f64,
        sb_confidence: f64,
        pyth_price: f64,
        pyth_confidence: f64,
        timestamp: i64,
        reference_timestamp: i64,
    ) -> OracleCache {
        let mut cache = OracleCache::with_reference_timestamp(reference_timestamp);
        cache.insert(
            switchboard_feed_pubkey(),
            OraclePrice {
                oracle_type: OracleType::Switchboard,
                price: sb_price,
                confidence: sb_confidence,
                timestamp,
                asset: "SOL/USD".to_string(),
            },
        );
        cache.insert(
            pyth_feed_pubkey(),
            OraclePrice {
                oracle_type: OracleType::Pyth,
                price: pyth_price,
                confidence: pyth_confidence,
                timestamp,
                asset: "SOL/USD".to_string(),
            },
        );
        cache
    }

    /// Build a minimal CheckerContext with the given OracleCache and reads.
    fn test_context(cache: OracleCache, reads: Vec<OracleRead>) -> CheckerContext {
        CheckerContext {
            trace: SimulationTrace {
                success: true,
                error: None,
                balance_deltas: HashMap::new(),
                cpi_graph: vec![],
                account_changes: vec![],
                logs: vec![],
                oracle_reads: reads,
                token_approvals: vec![],
                compute_units_consumed: 0,
                fee: 5000,
            },
            original_tx: Transaction::new_unsigned(Message::new(&[], None)),
            intent: None,
            slot: 350_000_000,
            oracle_cache: cache,
            known_programs: ProgramRegistry,
        }
    }

    fn switchboard_read() -> OracleRead {
        OracleRead {
            oracle_pubkey: switchboard_feed_pubkey(),
            oracle_type: "switchboard".to_string(),
        }
    }

    fn pyth_read() -> OracleRead {
        OracleRead {
            oracle_pubkey: pyth_feed_pubkey(),
            oracle_type: "pyth".to_string(),
        }
    }

    // -----------------------------------------------------------------------
    // Core deviation tests — spec Section 4.3.1 test strategy
    // -----------------------------------------------------------------------

    /// 2.9σ deviation → should PASS (below default 3.0 threshold).
    #[tokio::test]
    async fn test_2_9_sigma_passes() {
        // max(confidence) = max(0.45, 0.38) = 0.45
        // deviation = 2.9 * 0.45 = 1.305
        // sb_price = 142.50 + 1.305 = 143.805
        let cache = test_cache(143.805, 0.45, 142.50, 0.38, 100, 100);
        let ctx = test_context(cache, vec![switchboard_read()]);
        let checker = OracleSanityChecker::new(test_feed_map());

        let output = checker.check(&ctx).await;

        assert!(output.passed, "2.9σ should pass: {output:?}");
        assert!(!output.flags.iter().any(|f| f.code == FLAG_DEVIATION));
    }

    /// 3.1σ deviation → should FLAG as Critical.
    #[tokio::test]
    async fn test_3_1_sigma_flags() {
        // max(confidence) = 0.45
        // deviation = 3.1 * 0.45 = 1.395
        // sb_price = 142.50 + 1.395 = 143.895
        let cache = test_cache(143.895, 0.45, 142.50, 0.38, 100, 100);
        let ctx = test_context(cache, vec![switchboard_read()]);
        let checker = OracleSanityChecker::new(test_feed_map());

        let output = checker.check(&ctx).await;

        assert!(!output.passed, "3.1σ should flag: {output:?}");
        assert_eq!(output.severity, Severity::Critical);
        let deviation_flag = output
            .flags
            .iter()
            .find(|f| f.code == FLAG_DEVIATION)
            .expect("should have ORACLE_DEVIATION_3_SIGMA flag");
        let sigma = deviation_flag.data["deviation_sigma"]
            .as_f64()
            .expect("deviation_sigma should be f64");
        assert!(
            sigma > 3.0 && sigma < 3.2,
            "sigma should be ~3.1, got {sigma}"
        );
    }

    /// Full Drift fixture deviation (127.8σ) → should FLAG as Critical.
    #[tokio::test]
    async fn test_drift_fixture_deviation() {
        // Switchboard: 200.00 (manipulated), std_dev: 0.45
        // Pyth: 142.50 (real), confidence: 0.38
        // deviation = |200.00 - 142.50| / max(0.45, 0.38) = 57.5 / 0.45 = 127.8σ
        let cache = test_cache(200.0, 0.45, 142.50, 0.38, 350_000_000, 350_000_000);
        let ctx = test_context(cache, vec![switchboard_read(), pyth_read()]);
        let checker = OracleSanityChecker::new(test_feed_map());

        let output = checker.check(&ctx).await;

        assert!(!output.passed, "Drift exploit should flag: {output:?}");
        assert_eq!(output.severity, Severity::Critical);
        let deviation_flag = output
            .flags
            .iter()
            .find(|f| f.code == FLAG_DEVIATION)
            .expect("should have ORACLE_DEVIATION_3_SIGMA flag");
        let sigma = deviation_flag.data["deviation_sigma"]
            .as_f64()
            .expect("deviation_sigma");
        assert!(sigma > 120.0, "Drift exploit sigma should be ~127.8, got {sigma}");

        // Verify output schema matches spec Section 4.3.1.
        assert_eq!(deviation_flag.data["asset"], "SOL/USD");
        assert!(deviation_flag.data["switchboard_price"].as_f64().is_some());
        assert!(deviation_flag.data["pyth_price"].as_f64().is_some());
        assert!(deviation_flag.data["switchboard_std_dev"].as_f64().is_some());
        assert!(deviation_flag.data["pyth_confidence"].as_f64().is_some());
    }

    // -----------------------------------------------------------------------
    // Non-canonical oracle account
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_unknown_oracle_account() {
        let unknown_pubkey = Pubkey::new_unique();
        let cache = OracleCache::default();
        let reads = vec![OracleRead {
            oracle_pubkey: unknown_pubkey,
            oracle_type: "switchboard".to_string(),
        }];
        let ctx = test_context(cache, reads);
        let checker = OracleSanityChecker::new(test_feed_map());

        let output = checker.check(&ctx).await;

        assert!(!output.passed, "Unknown oracle should flag: {output:?}");
        assert!(output.flags.iter().any(|f| f.code == FLAG_UNKNOWN));
    }

    // -----------------------------------------------------------------------
    // Stale feed
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_stale_feed() {
        // Oracle timestamp = 100, reference = 160 → age = 60 > 30s threshold.
        let cache = test_cache(142.50, 0.45, 142.50, 0.38, 100, 160);
        let ctx = test_context(cache, vec![switchboard_read()]);
        let checker = OracleSanityChecker::new(test_feed_map());

        let output = checker.check(&ctx).await;

        assert!(!output.passed, "Stale feed should flag: {output:?}");
        let stale_flags: Vec<_> = output
            .flags
            .iter()
            .filter(|f| f.code == FLAG_STALE)
            .collect();
        assert!(
            !stale_flags.is_empty(),
            "Should have STALE_ORACLE_FEED flag"
        );
        let age = stale_flags[0].data["age_seconds"]
            .as_i64()
            .expect("age_seconds");
        assert_eq!(age, 60);
    }

    // -----------------------------------------------------------------------
    // Wide confidence interval
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_wide_confidence_interval() {
        // confidence = 15.0, price = 142.50 → ratio = 0.105 > 0.05 threshold.
        let cache = test_cache(142.50, 15.0, 142.50, 0.38, 100, 100);
        let ctx = test_context(cache, vec![switchboard_read()]);
        let checker = OracleSanityChecker::new(test_feed_map());

        let output = checker.check(&ctx).await;

        assert!(!output.passed, "Wide confidence should flag: {output:?}");
        assert!(output.flags.iter().any(|f| f.code == FLAG_WIDE_CONF));
    }

    // -----------------------------------------------------------------------
    // Edge cases
    // -----------------------------------------------------------------------

    /// No oracle reads → pass immediately.
    #[tokio::test]
    async fn test_no_oracle_reads_passes() {
        let cache = OracleCache::default();
        let ctx = test_context(cache, vec![]);
        let checker = OracleSanityChecker::new(test_feed_map());

        let output = checker.check(&ctx).await;

        assert!(output.passed);
        assert!(output.flags.is_empty());
        assert_eq!(output.severity, Severity::None);
    }

    /// Only one oracle source cached → no deviation flag (can't cross-reference).
    #[tokio::test]
    async fn test_one_source_only_no_deviation() {
        let mut cache = OracleCache::with_reference_timestamp(100);
        cache.insert(
            switchboard_feed_pubkey(),
            OraclePrice {
                oracle_type: OracleType::Switchboard,
                price: 200.0, // Manipulated — but no Pyth to cross-reference.
                confidence: 0.45,
                timestamp: 100,
                asset: "SOL/USD".to_string(),
            },
        );
        let ctx = test_context(cache, vec![switchboard_read()]);
        let checker = OracleSanityChecker::new(test_feed_map());

        let output = checker.check(&ctx).await;

        assert!(
            !output.flags.iter().any(|f| f.code == FLAG_DEVIATION),
            "Should not flag deviation with only one source"
        );
    }

    /// Both feeds in oracle_reads → deviation flagged only once (deduplication).
    #[tokio::test]
    async fn test_deduplication_both_feeds() {
        let cache = test_cache(200.0, 0.45, 142.50, 0.38, 100, 100);
        let ctx = test_context(cache, vec![switchboard_read(), pyth_read()]);
        let checker = OracleSanityChecker::new(test_feed_map());

        let output = checker.check(&ctx).await;

        let deviation_count = output
            .flags
            .iter()
            .filter(|f| f.code == FLAG_DEVIATION)
            .count();
        assert_eq!(
            deviation_count, 1,
            "Deviation should be flagged exactly once, got {deviation_count}"
        );
    }

    /// Deterministic: same input twice → same output.
    #[tokio::test]
    async fn test_deterministic_output() {
        let cache = test_cache(200.0, 0.45, 142.50, 0.38, 100, 100);
        let reads = vec![switchboard_read(), pyth_read()];
        let ctx = test_context(cache, reads);
        let checker = OracleSanityChecker::new(test_feed_map());

        let output1 = checker.check(&ctx).await;
        let output2 = checker.check(&ctx).await;

        let bytes1 = borsh::to_vec(&output1).expect("serialize output1");
        let bytes2 = borsh::to_vec(&output2).expect("serialize output2");
        assert_eq!(bytes1, bytes2, "Outputs should be byte-identical");
    }

    /// Oracle read for a known feed but not in cache → no flag (graceful skip).
    #[tokio::test]
    async fn test_known_feed_not_cached_no_flag() {
        // Feed is registered in the map but not in the cache.
        let cache = OracleCache::default();
        let ctx = test_context(cache, vec![switchboard_read()]);
        let checker = OracleSanityChecker::new(test_feed_map());

        let output = checker.check(&ctx).await;

        // Should NOT flag as UNKNOWN_ORACLE_ACCOUNT because it IS known.
        assert!(
            !output.flags.iter().any(|f| f.code == FLAG_UNKNOWN),
            "Known feed not in cache should not be flagged as unknown"
        );
    }

    /// Custom sigma threshold: 5.0 → a 4.0σ deviation should pass.
    #[tokio::test]
    async fn test_custom_sigma_threshold() {
        // deviation = 4.0 * 0.45 = 1.8
        // sb_price = 142.50 + 1.8 = 144.30
        let cache = test_cache(144.30, 0.45, 142.50, 0.38, 100, 100);
        let ctx = test_context(cache, vec![switchboard_read()]);
        let checker = OracleSanityChecker::new(test_feed_map()).with_sigma_threshold(5.0);

        let output = checker.check(&ctx).await;

        // 4.0σ is below the 5.0 threshold → should pass.
        assert!(
            !output.flags.iter().any(|f| f.code == FLAG_DEVIATION),
            "4.0σ should pass with 5.0 threshold"
        );
    }
}
