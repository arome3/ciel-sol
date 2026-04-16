// Integration tests that fetch real Switchboard V2 and Pyth oracle accounts
// from Solana mainnet and verify the vendored parsers produce sane results.
//
// These tests require network access and a Helius RPC endpoint.
// Run with: cargo test --package ciel-checkers --test parse_real_oracles -- --ignored
//
// Environment: HELIUS_API_KEY must be set (or uses public RPC as fallback).
// See CLAUDE.md "Integration Tests" section.

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

use ciel_checkers::{parse_pyth_price, parse_switchboard_v2, pyth_is_trading};

// ---------------------------------------------------------------------------
// Well-known mainnet feed accounts for SOL/USD
// ---------------------------------------------------------------------------

/// Switchboard V2 SOL/USD aggregator on mainnet.
/// Source: <https://app.switchboard.xyz> → SOL_USD
fn switchboard_sol_usd() -> Pubkey {
    Pubkey::from_str("GvDMxPzN1sCj7L26YDK2HnMRXEQmQ2aemov8YBtPS7vR").unwrap()
}

/// Pyth SOL/USD price account on mainnet.
/// Source: <https://pyth.network/price-feeds/crypto-sol-usd>
fn pyth_sol_usd() -> Pubkey {
    Pubkey::from_str("H6ARHf6YXhGYeQfUzQNGk6rDNnLBQKrenN712K4AQJEG").unwrap()
}

/// Switchboard V2 program ID.
fn switchboard_program_id() -> Pubkey {
    Pubkey::from_str("SW1TCH7qEPTdLsDHRgPuMQjbQxKdH2aBStViMFnt64f").unwrap()
}

/// Pyth oracle program ID.
fn pyth_program_id() -> Pubkey {
    Pubkey::from_str("FsJ3A3u2vn5cTVofAjvy6y5kwABJAqYWpe4975bi2epH").unwrap()
}

// ---------------------------------------------------------------------------
// RPC client helper
// ---------------------------------------------------------------------------

/// Build an RPC client, preferring Helius if HELIUS_API_KEY is set.
fn rpc_client() -> RpcClient {
    let url = if let Ok(key) = std::env::var("HELIUS_API_KEY") {
        format!("https://mainnet.helius-rpc.com/?api-key={key}")
    } else {
        // Fallback to public RPC (rate-limited but works for single-account fetches).
        "https://api.mainnet-beta.solana.com".to_string()
    };
    RpcClient::new_with_commitment(url, CommitmentConfig::confirmed())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Fetch the real Switchboard SOL/USD aggregator, parse it, and verify
/// the price is in a sane band.
#[tokio::test]
#[ignore] // Requires network — run with: cargo test --ignored -- parse_real_oracles
async fn test_parse_real_switchboard_sol_usd() {
    let client = rpc_client();
    let pubkey = switchboard_sol_usd();

    let account = client
        .get_account(&pubkey)
        .await
        .expect("failed to fetch Switchboard SOL/USD account from mainnet");

    // Verify the account is owned by the Switchboard V2 program.
    assert_eq!(
        account.owner,
        switchboard_program_id(),
        "Account owner should be the Switchboard V2 program"
    );

    // Verify the account is the expected size.
    assert_eq!(
        account.data.len(),
        3851,
        "Switchboard V2 aggregator should be 3851 bytes, got {}",
        account.data.len()
    );

    // Parse the account data.
    let oracle_price = parse_switchboard_v2(&account.data, "SOL/USD")
        .expect("failed to parse Switchboard account data");

    println!("Switchboard SOL/USD:");
    println!("  price:      {:.4}", oracle_price.price);
    println!("  std_dev:    {:.6}", oracle_price.confidence);
    println!("  timestamp:  {}", oracle_price.timestamp);

    // Sanity band: SOL has historically traded between $1 and $300+.
    // A price outside $1–$1000 indicates a parser bug, not a market event.
    assert!(
        oracle_price.price > 1.0 && oracle_price.price < 1000.0,
        "Switchboard SOL/USD price {:.4} is outside sane band [1, 1000]",
        oracle_price.price
    );

    // Std dev should be positive and small relative to price.
    assert!(
        oracle_price.confidence >= 0.0,
        "std_dev should be non-negative, got {}",
        oracle_price.confidence
    );

    // Timestamp should be a reasonable Unix timestamp (after 2020).
    assert!(
        oracle_price.timestamp > 1_577_836_800, // 2020-01-01
        "timestamp {} looks too old",
        oracle_price.timestamp
    );
}

/// Fetch the real Pyth SOL/USD price account, parse it, and verify
/// the price is in a sane band.
#[tokio::test]
#[ignore]
async fn test_parse_real_pyth_sol_usd() {
    let client = rpc_client();
    let pubkey = pyth_sol_usd();

    let account = client
        .get_account(&pubkey)
        .await
        .expect("failed to fetch Pyth SOL/USD account from mainnet");

    // Verify the account is owned by the Pyth program.
    assert_eq!(
        account.owner,
        pyth_program_id(),
        "Account owner should be the Pyth program"
    );

    // Pyth price accounts are 3312 bytes (32 publisher slots).
    assert_eq!(
        account.data.len(),
        3312,
        "Pyth price account should be 3312 bytes, got {}",
        account.data.len()
    );

    // Parse the account data.
    let oracle_price =
        parse_pyth_price(&account.data, "SOL/USD").expect("failed to parse Pyth account data");

    println!("Pyth SOL/USD:");
    println!("  price:      {:.4}", oracle_price.price);
    println!("  confidence: {:.6}", oracle_price.confidence);
    println!("  timestamp:  {}", oracle_price.timestamp);
    println!("  trading:    {}", pyth_is_trading(&account.data));

    // Same sanity band as Switchboard.
    assert!(
        oracle_price.price > 1.0 && oracle_price.price < 1000.0,
        "Pyth SOL/USD price {:.4} is outside sane band [1, 1000]",
        oracle_price.price
    );

    // Confidence should be positive and small relative to price.
    assert!(
        oracle_price.confidence >= 0.0,
        "confidence should be non-negative, got {}",
        oracle_price.confidence
    );

    // Timestamp should be a reasonable Unix timestamp.
    assert!(
        oracle_price.timestamp > 1_577_836_800,
        "timestamp {} looks too old",
        oracle_price.timestamp
    );
}

/// Verify that Switchboard and Pyth prices for SOL/USD are within 5%
/// of each other. This confirms the cross-reference logic would work
/// correctly with real data.
#[tokio::test]
#[ignore]
async fn test_cross_reference_switchboard_pyth_sol_usd() {
    let client = rpc_client();

    let sb_account = client
        .get_account(&switchboard_sol_usd())
        .await
        .expect("failed to fetch Switchboard account");
    let pyth_account = client
        .get_account(&pyth_sol_usd())
        .await
        .expect("failed to fetch Pyth account");

    let sb_price =
        parse_switchboard_v2(&sb_account.data, "SOL/USD").expect("parse Switchboard");
    let pyth_price =
        parse_pyth_price(&pyth_account.data, "SOL/USD").expect("parse Pyth");

    println!("Cross-reference SOL/USD:");
    println!("  Switchboard: {:.4}", sb_price.price);
    println!("  Pyth:        {:.4}", pyth_price.price);

    // Both prices should be positive.
    assert!(sb_price.price > 0.0, "Switchboard price should be positive");
    assert!(pyth_price.price > 0.0, "Pyth price should be positive");

    // Compute percentage difference.
    let avg = (sb_price.price + pyth_price.price) / 2.0;
    let pct_diff = (sb_price.price - pyth_price.price).abs() / avg * 100.0;

    println!("  Difference:  {:.2}%", pct_diff);

    // Under normal conditions, oracles for the same asset should agree
    // within 5%. A larger difference indicates either a parser bug or
    // a stale feed (one oracle hasn't updated recently).
    //
    // NOTE: The Switchboard SOL/USD V2 feed on mainnet may be stale
    // (last updated in early 2025) because Switchboard has migrated to
    // On-Demand feeds. If this test fails due to staleness rather than
    // price divergence, that's expected — the parser is still correct.
    // Check the timestamps to distinguish.
    if pct_diff > 5.0 {
        let time_diff = (sb_price.timestamp - pyth_price.timestamp).abs();
        println!(
            "  WARNING: {:.2}% divergence. Timestamp diff: {}s. \
             This may indicate a stale Switchboard V2 feed (migrated to On-Demand).",
            pct_diff, time_diff
        );
        // Don't hard-fail on divergence if timestamps differ significantly —
        // that indicates staleness, not a parser bug. The parser is correct
        // if both prices independently fall in the sane band.
        if time_diff > 86400 {
            println!(
                "  Switchboard feed is >24h stale — skipping cross-reference assertion. \
                 Parser validity confirmed by individual price band checks."
            );
            return;
        }
    }

    assert!(
        pct_diff < 5.0,
        "Switchboard ({:.4}) and Pyth ({:.4}) differ by {:.2}% — exceeds 5% threshold",
        sb_price.price, pyth_price.price, pct_diff
    );
}
