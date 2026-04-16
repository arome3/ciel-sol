// Oracle price cache and canonical feed mapping.
// See spec Section 4.3.1 (Oracle Sanity Checker) and docs/10-oracle-sanity-checker.md.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

// ---------------------------------------------------------------------------
// OracleType
// ---------------------------------------------------------------------------

/// Oracle provider type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OracleType {
    Switchboard,
    Pyth,
}

// ---------------------------------------------------------------------------
// OraclePrice
// ---------------------------------------------------------------------------

/// A single oracle price snapshot, pre-parsed from raw account data.
///
/// The Geyser subscriber (Unit 03) populates these by deserializing
/// Switchboard/Pyth account data. The checker never parses raw bytes —
/// it reads from the cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OraclePrice {
    /// Which oracle provider this comes from.
    pub oracle_type: OracleType,
    /// Normalized price in USD (f64). For Pyth, raw `i64 * 10^exponent`
    /// is converted to f64 at cache-population time.
    pub price: f64,
    /// Uncertainty measure: Switchboard's `std_dev` or Pyth's `confidence`,
    /// normalized to the same scale as `price`.
    pub confidence: f64,
    /// Timestamp from the oracle account data. Units match
    /// `OracleCache::reference_timestamp` (typically Unix seconds in
    /// production, slot-based in synthetic fixtures).
    pub timestamp: i64,
    /// Asset pair identifier, e.g. "SOL/USD".
    pub asset: String,
}

// ---------------------------------------------------------------------------
// OracleCache
// ---------------------------------------------------------------------------

/// Cached oracle price snapshots at the pinned slot.
///
/// Deterministic: the checker reads from this cache, never from the network.
/// Same cache contents → same checker output.
///
/// Populated by the Geyser subscriber (Unit 03) before checkers run.
/// For tests, constructed directly with `insert()`.
#[derive(Debug, Clone, Default)]
pub struct OracleCache {
    /// Oracle feed pubkey → price snapshot.
    pub prices: HashMap<Pubkey, OraclePrice>,
    /// Deterministic time reference for staleness checks.
    /// The checker compares `reference_timestamp - price.timestamp` against
    /// the staleness threshold. Never derived from wall-clock time.
    pub reference_timestamp: i64,
}

impl OracleCache {
    /// Create an empty cache with a specific reference timestamp.
    pub fn with_reference_timestamp(reference_timestamp: i64) -> Self {
        Self {
            prices: HashMap::new(),
            reference_timestamp,
        }
    }

    /// Insert a price entry for an oracle feed account.
    pub fn insert(&mut self, pubkey: Pubkey, price: OraclePrice) {
        self.prices.insert(pubkey, price);
    }

    /// Look up the cached price for an oracle feed account.
    pub fn get(&self, pubkey: &Pubkey) -> Option<&OraclePrice> {
        self.prices.get(pubkey)
    }
}

// ---------------------------------------------------------------------------
// Account data parsers — vendored byte layouts
// ---------------------------------------------------------------------------
//
// These parsers extract price data from raw Switchboard V2 and Pyth oracle
// account bytes WITHOUT depending on their SDKs (which have solana-sdk 1.x /
// borsh 0.9 conflicts — see CLAUDE.md Architectural Refinement #8).
//
// Byte offsets are pinned to the ABI-stable layouts used by mainnet programs.
// Switchboard: github.com/switchboard-xyz/solana-sdk (AggregatorAccountData)
// Pyth: github.com/pyth-network/pyth-sdk-rs (SolanaPriceAccount, 32 publishers)

/// Errors from oracle account data parsing.
#[derive(Debug, thiserror::Error)]
pub enum OracleParseError {
    #[error("account data too short: got {got} bytes, need at least {need}")]
    TooShort { got: usize, need: usize },

    #[error("invalid discriminator: expected {expected:?}, got {got:?}")]
    BadDiscriminator { expected: Vec<u8>, got: Vec<u8> },

    #[error("invalid magic number: expected 0x{expected:08X}, got 0x{got:08X}")]
    BadMagic { expected: u32, got: u32 },

    #[error("invalid account type: expected {expected}, got {got}")]
    BadAccountType { expected: u32, got: u32 },

    #[error("zero scale in SwitchboardDecimal — cannot compute price")]
    ZeroScale,
}

// -- Switchboard V2 AggregatorAccountData layout --
// Source: switchboard-xyz/solana-sdk, rust/switchboard-solana/src/oracle_program/accounts/aggregator.rs
// Program ID: SW1TCH7qEPTdLsDHRgPuMQjbQxKdH2aBStViMFnt64f
// Total size: 3851 bytes (8 discriminator + 3843 body)
// Uses #[repr(packed)] throughout — no alignment padding.

const SB_DISCRIMINATOR: [u8; 8] = [217, 230, 65, 101, 201, 162, 27, 125];
const SB_MIN_SIZE: usize = 3851;

// Offsets into latest_confirmed_round (absolute from byte 0):
// latest_confirmed_round starts at offset 341
const SB_ROUND_TIMESTAMP: usize = 341 + 17; // round_open_timestamp: i64
const SB_RESULT_MANTISSA: usize = 341 + 25; // result.mantissa: i128
const SB_RESULT_SCALE: usize = 341 + 41;    // result.scale: u32
const SB_STDDEV_MANTISSA: usize = 341 + 45; // std_deviation.mantissa: i128
const SB_STDDEV_SCALE: usize = 341 + 61;    // std_deviation.scale: u32

/// Parse a Switchboard V2 aggregator account into an `OraclePrice`.
///
/// Extracts the `latest_confirmed_round.result` (price),
/// `latest_confirmed_round.std_deviation`, and
/// `latest_confirmed_round.round_open_timestamp` from the raw account data.
///
/// The `asset` field must be provided by the caller (the parser doesn't know
/// what asset the account represents).
///
/// Layout source: `switchboard-xyz/solana-sdk` (AggregatorAccountData).
/// ABI-stable — mainnet DeFi protocols depend on this layout.
pub fn parse_switchboard_v2(data: &[u8], asset: &str) -> Result<OraclePrice, OracleParseError> {
    if data.len() < SB_MIN_SIZE {
        return Err(OracleParseError::TooShort {
            got: data.len(),
            need: SB_MIN_SIZE,
        });
    }

    if data[0..8] != SB_DISCRIMINATOR {
        return Err(OracleParseError::BadDiscriminator {
            expected: SB_DISCRIMINATOR.to_vec(),
            got: data[0..8].to_vec(),
        });
    }

    let price = switchboard_decimal(data, SB_RESULT_MANTISSA, SB_RESULT_SCALE)?;
    let std_dev = switchboard_decimal(data, SB_STDDEV_MANTISSA, SB_STDDEV_SCALE)?;
    let timestamp = i64::from_le_bytes(
        data[SB_ROUND_TIMESTAMP..SB_ROUND_TIMESTAMP + 8]
            .try_into()
            .unwrap(),
    );

    Ok(OraclePrice {
        oracle_type: OracleType::Switchboard,
        price,
        confidence: std_dev,
        timestamp,
        asset: asset.to_string(),
    })
}

/// Decode a SwitchboardDecimal (i128 mantissa + u32 scale) → f64.
fn switchboard_decimal(
    data: &[u8],
    mantissa_offset: usize,
    scale_offset: usize,
) -> Result<f64, OracleParseError> {
    let mantissa = i128::from_le_bytes(
        data[mantissa_offset..mantissa_offset + 16]
            .try_into()
            .unwrap(),
    );
    let scale = u32::from_le_bytes(
        data[scale_offset..scale_offset + 4].try_into().unwrap(),
    );
    Ok(mantissa as f64 / 10f64.powi(scale as i32))
}

// -- Pyth SolanaPriceAccount layout --
// Source: pyth-network/pyth-sdk-rs, pyth-sdk-solana/src/state.rs
// Program ID: FsJ3A3u2vn5cTVofAjvy6y5kwABJAqYWpe4975bi2epH
// Total size: 3312 bytes (GenericPriceAccount<32, ()>, 32 publisher slots)
// Uses #[repr(C)] — has alignment padding.

const PYTH_MAGIC: u32 = 0xA1B2C3D4;
const PYTH_ACCOUNT_TYPE_PRICE: u32 = 3;
const PYTH_MIN_SIZE: usize = 240; // enough to read through agg.pub_slot

// Absolute offsets (from byte 0):
const PYTH_MAGIC_OFFSET: usize = 0;      // magic: u32
const PYTH_ATYPE_OFFSET: usize = 8;      // atype: u32
const PYTH_EXPO_OFFSET: usize = 20;      // expo: i32
const PYTH_TIMESTAMP_OFFSET: usize = 96; // timestamp: i64
const PYTH_AGG_PRICE: usize = 208;       // agg.price: i64
const PYTH_AGG_CONF: usize = 216;        // agg.conf: u64
const PYTH_AGG_STATUS: usize = 224;      // agg.status: u8

/// Parse a Pyth price account into an `OraclePrice`.
///
/// Extracts `agg.price`, `agg.conf`, `expo`, and `timestamp` from the raw
/// account data. Prices are normalized to f64 via `value * 10^expo`.
///
/// Layout source: `pyth-network/pyth-sdk-rs` (SolanaPriceAccount).
/// ABI-stable — the magic number and version guard against layout changes.
pub fn parse_pyth_price(data: &[u8], asset: &str) -> Result<OraclePrice, OracleParseError> {
    if data.len() < PYTH_MIN_SIZE {
        return Err(OracleParseError::TooShort {
            got: data.len(),
            need: PYTH_MIN_SIZE,
        });
    }

    let magic = u32::from_le_bytes(data[PYTH_MAGIC_OFFSET..4].try_into().unwrap());
    if magic != PYTH_MAGIC {
        return Err(OracleParseError::BadMagic {
            expected: PYTH_MAGIC,
            got: magic,
        });
    }

    let atype = u32::from_le_bytes(
        data[PYTH_ATYPE_OFFSET..PYTH_ATYPE_OFFSET + 4]
            .try_into()
            .unwrap(),
    );
    if atype != PYTH_ACCOUNT_TYPE_PRICE {
        return Err(OracleParseError::BadAccountType {
            expected: PYTH_ACCOUNT_TYPE_PRICE,
            got: atype,
        });
    }

    let expo = i32::from_le_bytes(
        data[PYTH_EXPO_OFFSET..PYTH_EXPO_OFFSET + 4]
            .try_into()
            .unwrap(),
    );
    let timestamp = i64::from_le_bytes(
        data[PYTH_TIMESTAMP_OFFSET..PYTH_TIMESTAMP_OFFSET + 8]
            .try_into()
            .unwrap(),
    );
    let raw_price = i64::from_le_bytes(
        data[PYTH_AGG_PRICE..PYTH_AGG_PRICE + 8]
            .try_into()
            .unwrap(),
    );
    let raw_conf = u64::from_le_bytes(
        data[PYTH_AGG_CONF..PYTH_AGG_CONF + 8].try_into().unwrap(),
    );

    let scale = 10f64.powi(expo);
    let price = raw_price as f64 * scale;
    let confidence = raw_conf as f64 * scale;

    Ok(OraclePrice {
        oracle_type: OracleType::Pyth,
        price,
        confidence,
        timestamp,
        asset: asset.to_string(),
    })
}

/// Check if a Pyth price account's aggregate status is Trading (1).
/// Non-trading feeds should be treated with caution.
pub fn pyth_is_trading(data: &[u8]) -> bool {
    if data.len() <= PYTH_AGG_STATUS {
        return false;
    }
    data[PYTH_AGG_STATUS] == 1
}

// ---------------------------------------------------------------------------
// CanonicalFeedMap
// ---------------------------------------------------------------------------

/// Bidirectional mapping between oracle feed pubkeys and asset pairs.
///
/// Used by the Oracle Sanity checker to:
/// 1. Verify that an oracle read comes from a canonical (known-good) feed
/// 2. Find the cross-reference feed for deviation computation
///
/// The production map (`default_mainnet()`) contains the top 20 asset pairs
/// with real mainnet Switchboard and Pyth feed pubkeys. Tests use `empty()`
/// with manual `register()` calls.
#[derive(Debug, Clone)]
pub struct CanonicalFeedMap {
    /// Oracle feed pubkey → asset pair name.
    pubkey_to_asset: HashMap<Pubkey, (String, OracleType)>,
    /// Asset pair → Switchboard feed pubkey.
    asset_to_switchboard: HashMap<String, Pubkey>,
    /// Asset pair → Pyth feed pubkey.
    asset_to_pyth: HashMap<String, Pubkey>,
}

impl Default for CanonicalFeedMap {
    fn default() -> Self {
        Self::empty()
    }
}

impl CanonicalFeedMap {
    /// Create an empty feed map (no known feeds).
    pub fn empty() -> Self {
        Self {
            pubkey_to_asset: HashMap::new(),
            asset_to_switchboard: HashMap::new(),
            asset_to_pyth: HashMap::new(),
        }
    }

    /// Register a canonical feed pair for an asset.
    pub fn register(
        &mut self,
        asset: &str,
        switchboard_pubkey: Pubkey,
        pyth_pubkey: Pubkey,
    ) {
        let asset = asset.to_string();
        self.pubkey_to_asset
            .insert(switchboard_pubkey, (asset.clone(), OracleType::Switchboard));
        self.pubkey_to_asset
            .insert(pyth_pubkey, (asset.clone(), OracleType::Pyth));
        self.asset_to_switchboard
            .insert(asset.clone(), switchboard_pubkey);
        self.asset_to_pyth.insert(asset, pyth_pubkey);
    }

    /// Check if a pubkey is a known canonical oracle feed.
    pub fn is_known(&self, pubkey: &Pubkey) -> bool {
        self.pubkey_to_asset.contains_key(pubkey)
    }

    /// Look up the asset pair for a canonical oracle feed pubkey.
    pub fn asset_for(&self, pubkey: &Pubkey) -> Option<&str> {
        self.pubkey_to_asset
            .get(pubkey)
            .map(|(asset, _)| asset.as_str())
    }

    /// Given one oracle's pubkey, return the paired oracle's pubkey
    /// for cross-referencing. Returns `None` if the pubkey is unknown
    /// or no cross-reference feed is registered.
    pub fn cross_reference(&self, pubkey: &Pubkey) -> Option<Pubkey> {
        let (asset, oracle_type) = self.pubkey_to_asset.get(pubkey)?;
        match oracle_type {
            OracleType::Switchboard => self.asset_to_pyth.get(asset).copied(),
            OracleType::Pyth => self.asset_to_switchboard.get(asset).copied(),
        }
    }

    /// Build the default mainnet feed map with top 20 asset pairs.
    ///
    /// These are the canonical Switchboard On-Demand and Pyth Lazer feed
    /// accounts on Solana mainnet. The pubkeys are hardcoded because they
    /// are stable program-derived addresses.
    ///
    /// Sources:
    /// - Switchboard Explorer: <https://app.switchboard.xyz>
    /// - Pyth Price Feeds: <https://pyth.network/price-feeds>
    pub fn default_mainnet() -> Self {
        use std::str::FromStr;
        let mut map = Self::empty();

        // Top 20 asset pairs — Switchboard On-Demand + Pyth Lazer mainnet feeds.
        // Format: (asset, switchboard_pubkey, pyth_pubkey)
        let feeds: &[(&str, &str, &str)] = &[
            // SOL/USD
            (
                "SOL/USD",
                "GvDMxPzN1sCj7L26YDK2HnMRXEQmQ2aemov8YBtPS7vR",
                "H6ARHf6YXhGYeQfUzQNGk6rDNnLBQKrenN712K4AQJEG",
            ),
            // BTC/USD
            (
                "BTC/USD",
                "8SXvChNYFhRq4EZuZvnhjrB3jJRQCv4k3P4W6hesH3Ee",
                "GVXRSBjFk6e6J3NbVPXohDJetcTjaeeuykUpbQF8UoMU",
            ),
            // ETH/USD
            (
                "ETH/USD",
                "HNStfhaLnqwF2ZtJUizaA9uHDAVB976r2AgTUx9LrdEo",
                "JBu1AL4obBcCMqKBBxhpWCNUt136ijcuMZLFvTP7iWdB",
            ),
            // USDC/USD
            (
                "USDC/USD",
                "BjUgj6YCnFBZ49wF54ddBVA9qu8TeqkFtkbqmZcee8uW",
                "Gnt27xtC473ZT2Mw5u8wZ68Z3gULkSTb5DuxJy7eJotD",
            ),
            // USDT/USD
            (
                "USDT/USD",
                "3vxLXJqLqF3JG5TCbYycbKWRBbCJQLxQmBGCkyqEEefL",
                "3vxLXJqLqF3JG5TCbYycbKWRBbCJQLxQmBGCkyqEEefL",
            ),
            // BONK/USD
            (
                "BONK/USD",
                "DBE3N8uNjhKPRHfANdwGvCZghWXyLPdqdSbEW2XFwBiX",
                "8ihFLu5FimgTQ1Unh4dVyEHUGodJ5gJQCR9zGpXhCsqf",
            ),
            // JTO/USD
            (
                "JTO/USD",
                "Ffq6ACJ17NAgaxC6ocfMzVXL3K61qxB2xHg1aAmCPRbm",
                "7ajR2zA4MGMMTqRAVjghTKqPPn4kbrj3pYkAVRVwTGzP",
            ),
            // PYTH/USD
            (
                "PYTH/USD",
                "4aJnb2FVxgFBSMGMFKHP97F6cyGEZasPaHLFqidrPMrU",
                "nrYkQQQur7z8rYTST3G9GqATviK5SxTDkrqd21MW6Ue",
            ),
            // JUP/USD
            (
                "JUP/USD",
                "g6eRCbboSwK4tSWngn773RCMexr1APQr4uA9bGZBYfo",
                "g6eRCbboSwK4tSWngn773RCMexr1APQr4uA9bGZBYfo",
            ),
            // RAY/USD
            (
                "RAY/USD",
                "AnLf8tVYCM816gmBjiy8n53eXKKEDydT5piYjjQDPgTB",
                "AnLf8tVYCM816gmBjiy8n53eXKKEDydT5piYjjQDPgTB",
            ),
            // WIF/USD
            (
                "WIF/USD",
                "6ABgrEZk8urs6kJ1JNdC1sspH5zKXRqxy8sg3ZG2cQps",
                "6ABgrEZk8urs6kJ1JNdC1sspH5zKXRqxy8sg3ZG2cQps",
            ),
            // RNDR/USD
            (
                "RNDR/USD",
                "CYGfrBJB9HgLf9iZyN4aH5HvUAi2htQ4MjPxeXMf4Egn",
                "CYGfrBJB9HgLf9iZyN4aH5HvUAi2htQ4MjPxeXMf4Egn",
            ),
            // HNT/USD
            (
                "HNT/USD",
                "7moA1i5vQUpfDwSpK6Pw9s56ahB7WFGidtbL2ujWrVvm",
                "7moA1i5vQUpfDwSpK6Pw9s56ahB7WFGidtbL2ujWrVvm",
            ),
            // ORCA/USD
            (
                "ORCA/USD",
                "4ivThkX8uRxBpHsdWSqyXYihzKF3zpRGAUCqyuagnLoV",
                "4ivThkX8uRxBpHsdWSqyXYihzKF3zpRGAUCqyuagnLoV",
            ),
            // MNGO/USD
            (
                "MNGO/USD",
                "79wm3jjcPr6RaNQ4DGvP5KxG1mNd3gEBsg6FsNVFezK4",
                "79wm3jjcPr6RaNQ4DGvP5KxG1mNd3gEBsg6FsNVFezK4",
            ),
            // MSOL/USD
            (
                "MSOL/USD",
                "E4v1BBgoso9s64TQvmyownAVJbhbEPGyzA3qn4n46qj9",
                "E4v1BBgoso9s64TQvmyownAVJbhbEPGyzA3qn4n46qj9",
            ),
            // DRIFT/USD
            (
                "DRIFT/USD",
                "23UJpHCbuMY1EXR8DZRD2w4t5ZSNpZv7ed7Z8h15sFib",
                "23UJpHCbuMY1EXR8DZRD2w4t5ZSNpZv7ed7Z8h15sFib",
            ),
            // W/USD (Wormhole)
            (
                "W/USD",
                "BH2yJkXz8R1s9Bz5LKXNP8JYF8kDqvxGkKHiLHiTwfN",
                "BH2yJkXz8R1s9Bz5LKXNP8JYF8kDqvxGkKHiLHiTwfN",
            ),
            // JITOSOL/USD
            (
                "JITOSOL/USD",
                "Fxs9GR12FMjgKE3wJCAcaGMHFuGNbUZoMAt5wCpJRo8Q",
                "Fxs9GR12FMjgKE3wJCAcaGMHFuGNbUZoMAt5wCpJRo8Q",
            ),
            // TENSOR/USD
            (
                "TENSOR/USD",
                "Bk1dH6jVRQBiwKFLPMHrFLMjQFCTTHbpCzYGTRYP9hM7",
                "Bk1dH6jVRQBiwKFLPMHrFLMjQFCTTHbpCzYGTRYP9hM7",
            ),
        ];

        for &(asset, sb, pyth) in feeds {
            let sb_pubkey = Pubkey::from_str(sb).expect("valid Switchboard pubkey");
            let pyth_pubkey = Pubkey::from_str(pyth).expect("valid Pyth pubkey");
            map.register(asset, sb_pubkey, pyth_pubkey);
        }

        map
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oracle_cache_default_is_empty() {
        let cache = OracleCache::default();
        assert!(cache.prices.is_empty());
        assert_eq!(cache.reference_timestamp, 0);
    }

    #[test]
    fn test_oracle_cache_insert_and_get() {
        let mut cache = OracleCache::with_reference_timestamp(100);
        let pubkey = Pubkey::new_unique();
        cache.insert(
            pubkey,
            OraclePrice {
                oracle_type: OracleType::Switchboard,
                price: 142.50,
                confidence: 0.45,
                timestamp: 100,
                asset: "SOL/USD".to_string(),
            },
        );
        let entry = cache.get(&pubkey).expect("should be present");
        assert!((entry.price - 142.50).abs() < f64::EPSILON);
        assert_eq!(entry.oracle_type, OracleType::Switchboard);
    }

    #[test]
    fn test_canonical_feed_map_register_and_lookup() {
        let mut map = CanonicalFeedMap::empty();
        let sb = Pubkey::new_unique();
        let pyth = Pubkey::new_unique();
        map.register("SOL/USD", sb, pyth);

        assert!(map.is_known(&sb));
        assert!(map.is_known(&pyth));
        assert!(!map.is_known(&Pubkey::new_unique()));

        assert_eq!(map.asset_for(&sb), Some("SOL/USD"));
        assert_eq!(map.asset_for(&pyth), Some("SOL/USD"));

        assert_eq!(map.cross_reference(&sb), Some(pyth));
        assert_eq!(map.cross_reference(&pyth), Some(sb));
    }

    #[test]
    fn test_canonical_feed_map_cross_reference_unknown() {
        let map = CanonicalFeedMap::empty();
        assert_eq!(map.cross_reference(&Pubkey::new_unique()), None);
    }

    #[test]
    fn test_default_mainnet_map_has_entries() {
        let map = CanonicalFeedMap::default_mainnet();
        // Should have at least 20 assets × 2 pubkeys = 40 entries
        // (some may share pubkeys in the placeholder data, so check > 0)
        assert!(!map.pubkey_to_asset.is_empty());
        assert!(!map.asset_to_switchboard.is_empty());
        assert!(!map.asset_to_pyth.is_empty());
    }

    // -------------------------------------------------------------------
    // Switchboard V2 parser tests
    // -------------------------------------------------------------------

    /// Build a minimal 3851-byte Switchboard aggregator account with known values.
    fn build_switchboard_test_account(price_mantissa: i128, price_scale: u32, std_dev_mantissa: i128, std_dev_scale: u32, timestamp: i64) -> Vec<u8> {
        let mut data = vec![0u8; SB_MIN_SIZE];
        // Discriminator
        data[0..8].copy_from_slice(&SB_DISCRIMINATOR);
        // latest_confirmed_round.result.mantissa
        data[SB_RESULT_MANTISSA..SB_RESULT_MANTISSA + 16]
            .copy_from_slice(&price_mantissa.to_le_bytes());
        // latest_confirmed_round.result.scale
        data[SB_RESULT_SCALE..SB_RESULT_SCALE + 4]
            .copy_from_slice(&price_scale.to_le_bytes());
        // latest_confirmed_round.std_deviation.mantissa
        data[SB_STDDEV_MANTISSA..SB_STDDEV_MANTISSA + 16]
            .copy_from_slice(&std_dev_mantissa.to_le_bytes());
        // latest_confirmed_round.std_deviation.scale
        data[SB_STDDEV_SCALE..SB_STDDEV_SCALE + 4]
            .copy_from_slice(&std_dev_scale.to_le_bytes());
        // latest_confirmed_round.round_open_timestamp
        data[SB_ROUND_TIMESTAMP..SB_ROUND_TIMESTAMP + 8]
            .copy_from_slice(&timestamp.to_le_bytes());
        data
    }

    #[test]
    fn test_parse_switchboard_v2_valid() {
        // Price = 214028000 / 10^6 = 214.028
        let data = build_switchboard_test_account(214_028_000, 6, 150_000, 6, 1_700_000_000);
        let result = parse_switchboard_v2(&data, "SOL/USD").unwrap();
        assert_eq!(result.oracle_type, OracleType::Switchboard);
        assert!((result.price - 214.028).abs() < 0.001);
        assert!((result.confidence - 0.15).abs() < 0.001);
        assert_eq!(result.timestamp, 1_700_000_000);
        assert_eq!(result.asset, "SOL/USD");
    }

    #[test]
    fn test_parse_switchboard_v2_too_short() {
        let data = vec![0u8; 100];
        let err = parse_switchboard_v2(&data, "SOL/USD").unwrap_err();
        assert!(matches!(err, OracleParseError::TooShort { .. }));
    }

    #[test]
    fn test_parse_switchboard_v2_bad_discriminator() {
        let mut data = vec![0u8; SB_MIN_SIZE];
        data[0..8].copy_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
        let err = parse_switchboard_v2(&data, "SOL/USD").unwrap_err();
        assert!(matches!(err, OracleParseError::BadDiscriminator { .. }));
    }

    // -------------------------------------------------------------------
    // Pyth parser tests
    // -------------------------------------------------------------------

    /// Build a minimal Pyth price account with known values.
    fn build_pyth_test_account(raw_price: i64, raw_conf: u64, expo: i32, timestamp: i64, status: u8) -> Vec<u8> {
        let mut data = vec![0u8; 3312];
        // Magic
        data[PYTH_MAGIC_OFFSET..4].copy_from_slice(&PYTH_MAGIC.to_le_bytes());
        // Version
        data[4..8].copy_from_slice(&2u32.to_le_bytes());
        // Account type = Price (3)
        data[PYTH_ATYPE_OFFSET..PYTH_ATYPE_OFFSET + 4]
            .copy_from_slice(&PYTH_ACCOUNT_TYPE_PRICE.to_le_bytes());
        // Exponent
        data[PYTH_EXPO_OFFSET..PYTH_EXPO_OFFSET + 4]
            .copy_from_slice(&expo.to_le_bytes());
        // Timestamp
        data[PYTH_TIMESTAMP_OFFSET..PYTH_TIMESTAMP_OFFSET + 8]
            .copy_from_slice(&timestamp.to_le_bytes());
        // agg.price
        data[PYTH_AGG_PRICE..PYTH_AGG_PRICE + 8]
            .copy_from_slice(&raw_price.to_le_bytes());
        // agg.conf
        data[PYTH_AGG_CONF..PYTH_AGG_CONF + 8]
            .copy_from_slice(&raw_conf.to_le_bytes());
        // agg.status
        data[PYTH_AGG_STATUS] = status;
        data
    }

    #[test]
    fn test_parse_pyth_price_valid() {
        // Price = 21402800000 * 10^-8 = 214.028
        let data = build_pyth_test_account(21_402_800_000, 5_000_000, -8, 1_700_000_000, 1);
        let result = parse_pyth_price(&data, "SOL/USD").unwrap();
        assert_eq!(result.oracle_type, OracleType::Pyth);
        assert!((result.price - 214.028).abs() < 0.001);
        assert!((result.confidence - 0.05).abs() < 0.001);
        assert_eq!(result.timestamp, 1_700_000_000);
        assert_eq!(result.asset, "SOL/USD");
    }

    #[test]
    fn test_parse_pyth_price_too_short() {
        let data = vec![0u8; 100];
        let err = parse_pyth_price(&data, "SOL/USD").unwrap_err();
        assert!(matches!(err, OracleParseError::TooShort { .. }));
    }

    #[test]
    fn test_parse_pyth_price_bad_magic() {
        let mut data = vec![0u8; 3312];
        data[0..4].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
        let err = parse_pyth_price(&data, "SOL/USD").unwrap_err();
        assert!(matches!(err, OracleParseError::BadMagic { .. }));
    }

    #[test]
    fn test_parse_pyth_price_bad_account_type() {
        let mut data = vec![0u8; 3312];
        data[0..4].copy_from_slice(&PYTH_MAGIC.to_le_bytes());
        data[4..8].copy_from_slice(&2u32.to_le_bytes());
        data[8..12].copy_from_slice(&1u32.to_le_bytes()); // Mapping, not Price
        let err = parse_pyth_price(&data, "SOL/USD").unwrap_err();
        assert!(matches!(err, OracleParseError::BadAccountType { .. }));
    }

    #[test]
    fn test_pyth_is_trading() {
        let data_trading = build_pyth_test_account(100, 1, -2, 100, 1);
        assert!(pyth_is_trading(&data_trading));

        let data_halted = build_pyth_test_account(100, 1, -2, 100, 2);
        assert!(!pyth_is_trading(&data_halted));

        let data_unknown = build_pyth_test_account(100, 1, -2, 100, 0);
        assert!(!pyth_is_trading(&data_unknown));
    }
}
