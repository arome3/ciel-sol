// Intent rules engine. See docs/12-intent-diff-checker.md (Unit 12) and spec
// Section 4.3.3.
//
// Two concerns live here:
//   1. A versioned static `TOKEN_REGISTRY` mapping symbols → (mint, decimals).
//   2. A deterministic free-text parser for `intent.description` that classifies
//      goals into `IntentPattern::{Swap, Transfer, Deposit, Unrecognized}`.
//
// The parser is the FALLBACK path. When `Intent.spec` is supplied, the Intent
// Diff checker consumes structured data directly and does NOT consult this
// parser — same as UniswapX / CoW, where free text is display-only.
//
// Versioning: expanding TOKEN_REGISTRY or changing the parser shape is an
// observable output change (previously-INCONCLUSIVE intents become verifiable),
// so both bumps require updating `RULE_TABLE_VERSION`. Two verifiers running
// different versions would disagree — that's a determinism-invariant violation.

use std::str::FromStr;

use solana_sdk::pubkey::Pubkey;

// ---------------------------------------------------------------------------
// Versioning
// ---------------------------------------------------------------------------

/// Version of the rule-table. Increment on any change to `TOKEN_REGISTRY`, the
/// parser shape, or the recognized-verb set.
pub const RULE_TABLE_VERSION: &str = "v1";

// ---------------------------------------------------------------------------
// Per-rule tolerances (basis points)
// ---------------------------------------------------------------------------

/// Swap input-side tolerance. Jupiter's default slippage is 50 bps; 100 bps is
/// intentionally looser to absorb DEX fees, rounding, and multi-hop routes.
pub const SWAP_TOLERANCE_BPS: u16 = 100;

/// Transfer tolerance. Transfers should be near-exact modulo rounding.
pub const TRANSFER_TOLERANCE_BPS: u16 = 10;

/// Deposit outflow tolerance.
pub const DEPOSIT_TOLERANCE_BPS: u16 = 100;

// ---------------------------------------------------------------------------
// Token registry
// ---------------------------------------------------------------------------

/// Symbol → (primary mint base58, decimals). The "primary" mint is the one the
/// Intent Diff checker compares against; multi-bridge variants (e.g. native-USDC
/// vs Wormhole-USDC) are intentionally out of scope until the demand surfaces.
///
/// SOL maps to the wSOL mint, but the Intent Diff checker also reads native
/// lamport deltas when verifying a "SOL" intent — Jupiter/Raydium routes
/// unwrap at end-of-tx and surface SOL changes in native lamports.
pub const TOKEN_REGISTRY: &[(&str, &str, u8)] = &[
    ("SOL", "So11111111111111111111111111111111111111112", 9),
    ("USDC", "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", 6),
    ("USDT", "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB", 6),
    ("ETH", "7vfCXTUXx5WJV5JADk17DUJ4ksgau7utNKj4b963voxs", 8),
    ("BTC", "9n4nbM75f5Ui33ZbPYXn59EwSgE8CGsHtAeTH5YFeJ9E", 8),
    ("BONK", "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263", 5),
];

/// wSOL mint. The Intent Diff checker treats this as an alias for native SOL
/// in balance-delta comparisons.
pub const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";

/// Case-insensitive lookup. Returns `(mint, decimals)` for registered symbols.
pub fn token_info(symbol: &str) -> Option<(Pubkey, u8)> {
    let sym_upper = symbol.to_uppercase();
    TOKEN_REGISTRY
        .iter()
        .find(|(s, _, _)| *s == sym_upper)
        .and_then(|(_, mint_str, decimals)| {
            Pubkey::from_str(mint_str).ok().map(|p| (p, *decimals))
        })
}

// ---------------------------------------------------------------------------
// Intent pattern
// ---------------------------------------------------------------------------

/// Pattern classification produced by `parse_intent_goal`. Unrecognized is the
/// deterministic fallback — the checker converts it into
/// `INTENT_VERIFICATION_INCONCLUSIVE`.
#[derive(Debug, Clone, PartialEq)]
pub enum IntentPattern {
    Swap {
        amount: f64,
        token_in: String,
        token_out: String,
    },
    Transfer {
        amount: f64,
        token: String,
        recipient: String,
    },
    Deposit {
        amount: f64,
        token: String,
        protocol: String,
    },
    Unrecognized,
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parse a free-text intent goal into an `IntentPattern`. Deterministic —
/// identical input always produces identical output.
///
/// Recognized shapes (case-insensitive, whitespace-tolerant):
///   - `swap {amount} {token_in} for {token_out}`
///   - `transfer {amount} {token} to {recipient...}`
///   - `send {amount} {token} to {recipient...}`
///   - `deposit {amount} {token} into {protocol...}`
///
/// Any deviation returns `IntentPattern::Unrecognized`.
pub fn parse_intent_goal(goal: &str) -> IntentPattern {
    let tokens: Vec<String> = goal
        .split_whitespace()
        .map(|s| s.to_lowercase())
        .collect();

    if tokens.is_empty() {
        return IntentPattern::Unrecognized;
    }

    match tokens[0].as_str() {
        "swap" => parse_swap(&tokens),
        "transfer" | "send" => parse_transfer(&tokens),
        "deposit" => parse_deposit(&tokens),
        _ => IntentPattern::Unrecognized,
    }
}

fn parse_amount(s: &str) -> Option<f64> {
    let v = f64::from_str(s).ok()?;
    if v.is_finite() && v > 0.0 {
        Some(v)
    } else {
        None
    }
}

fn parse_swap(tokens: &[String]) -> IntentPattern {
    // Exactly: ["swap", amount, token_in, "for", token_out]
    if tokens.len() != 5 || tokens[3] != "for" {
        return IntentPattern::Unrecognized;
    }
    let Some(amount) = parse_amount(&tokens[1]) else {
        return IntentPattern::Unrecognized;
    };
    IntentPattern::Swap {
        amount,
        token_in: tokens[2].to_uppercase(),
        token_out: tokens[4].to_uppercase(),
    }
}

fn parse_transfer(tokens: &[String]) -> IntentPattern {
    // ["transfer" | "send", amount, token, "to", recipient...]
    if tokens.len() < 5 || tokens[3] != "to" {
        return IntentPattern::Unrecognized;
    }
    let Some(amount) = parse_amount(&tokens[1]) else {
        return IntentPattern::Unrecognized;
    };
    IntentPattern::Transfer {
        amount,
        token: tokens[2].to_uppercase(),
        recipient: tokens[4..].join(" "),
    }
}

fn parse_deposit(tokens: &[String]) -> IntentPattern {
    // ["deposit", amount, token, "into", protocol...]
    if tokens.len() < 5 || tokens[3] != "into" {
        return IntentPattern::Unrecognized;
    }
    let Some(amount) = parse_amount(&tokens[1]) else {
        return IntentPattern::Unrecognized;
    };
    IntentPattern::Deposit {
        amount,
        token: tokens[2].to_uppercase(),
        protocol: tokens[4..].join(" "),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Swap parsing ---

    #[test]
    fn parse_swap_basic() {
        assert_eq!(
            parse_intent_goal("swap 100 USDC for SOL"),
            IntentPattern::Swap {
                amount: 100.0,
                token_in: "USDC".to_string(),
                token_out: "SOL".to_string(),
            }
        );
    }

    #[test]
    fn parse_swap_case_insensitive() {
        assert_eq!(
            parse_intent_goal("SWAP 100 usdc FOR Sol"),
            IntentPattern::Swap {
                amount: 100.0,
                token_in: "USDC".to_string(),
                token_out: "SOL".to_string(),
            }
        );
    }

    #[test]
    fn parse_swap_fractional_amount() {
        assert_eq!(
            parse_intent_goal("swap 0.5 SOL for USDC"),
            IntentPattern::Swap {
                amount: 0.5,
                token_in: "SOL".to_string(),
                token_out: "USDC".to_string(),
            }
        );
    }

    // --- Transfer parsing ---

    #[test]
    fn parse_transfer_basic() {
        assert_eq!(
            parse_intent_goal("transfer 5 USDC to alice.sol"),
            IntentPattern::Transfer {
                amount: 5.0,
                token: "USDC".to_string(),
                recipient: "alice.sol".to_string(),
            }
        );
    }

    #[test]
    fn parse_transfer_send_alias() {
        assert_eq!(
            parse_intent_goal("send 5 USDC to bob"),
            IntentPattern::Transfer {
                amount: 5.0,
                token: "USDC".to_string(),
                recipient: "bob".to_string(),
            }
        );
    }

    #[test]
    fn parse_transfer_multi_word_recipient() {
        assert_eq!(
            parse_intent_goal("transfer 1 SOL to 7xkM...r2jq"),
            IntentPattern::Transfer {
                amount: 1.0,
                token: "SOL".to_string(),
                recipient: "7xkm...r2jq".to_string(),
            }
        );
    }

    // --- Deposit parsing ---

    #[test]
    fn parse_deposit_basic() {
        assert_eq!(
            parse_intent_goal("deposit 10 SOL into marinade"),
            IntentPattern::Deposit {
                amount: 10.0,
                token: "SOL".to_string(),
                protocol: "marinade".to_string(),
            }
        );
    }

    // --- Unrecognized paths ---

    #[test]
    fn parse_unrecognized_rebalance_spec_string() {
        assert_eq!(
            parse_intent_goal("rebalance portfolio to 60/30/10 split"),
            IntentPattern::Unrecognized
        );
    }

    #[test]
    fn parse_unrecognized_empty() {
        assert_eq!(parse_intent_goal(""), IntentPattern::Unrecognized);
        assert_eq!(parse_intent_goal("   "), IntentPattern::Unrecognized);
    }

    #[test]
    fn parse_unrecognized_malformed_amount() {
        assert_eq!(
            parse_intent_goal("swap foo USDC for SOL"),
            IntentPattern::Unrecognized
        );
    }

    #[test]
    fn parse_unrecognized_zero_amount() {
        assert_eq!(
            parse_intent_goal("swap 0 USDC for SOL"),
            IntentPattern::Unrecognized
        );
    }

    #[test]
    fn parse_unrecognized_wrong_connector() {
        // "into" instead of "for"
        assert_eq!(
            parse_intent_goal("swap 100 USDC into SOL"),
            IntentPattern::Unrecognized
        );
    }

    #[test]
    fn parse_unrecognized_missing_tokens() {
        assert_eq!(
            parse_intent_goal("swap 100 USDC"),
            IntentPattern::Unrecognized
        );
        assert_eq!(parse_intent_goal("transfer 5"), IntentPattern::Unrecognized);
    }

    #[test]
    fn parse_unrecognized_extra_words() {
        assert_eq!(
            parse_intent_goal("please swap 100 USDC for SOL"),
            IntentPattern::Unrecognized
        );
        assert_eq!(
            parse_intent_goal("swap 100 USDC for SOL immediately"),
            IntentPattern::Unrecognized
        );
    }

    // --- Token registry ---

    #[test]
    fn token_info_registered() {
        let (mint, decimals) = token_info("USDC").expect("USDC registered");
        assert_eq!(
            mint,
            Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap()
        );
        assert_eq!(decimals, 6);
    }

    #[test]
    fn token_info_case_insensitive() {
        assert!(token_info("usdc").is_some());
        assert!(token_info("Usdc").is_some());
    }

    #[test]
    fn token_info_unknown() {
        assert!(token_info("XYZFAKE").is_none());
    }

    #[test]
    fn token_info_sol_maps_to_wsol() {
        let (mint, decimals) = token_info("SOL").expect("SOL registered");
        assert_eq!(mint.to_string(), WSOL_MINT);
        assert_eq!(decimals, 9);
    }

    // --- Determinism ---

    #[test]
    fn parse_is_deterministic() {
        for _ in 0..100 {
            assert_eq!(
                parse_intent_goal("swap 100 USDC for SOL"),
                IntentPattern::Swap {
                    amount: 100.0,
                    token_in: "USDC".to_string(),
                    token_out: "SOL".to_string(),
                }
            );
        }
    }
}
