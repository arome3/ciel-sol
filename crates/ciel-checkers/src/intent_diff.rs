// Intent Diff checker. See docs/12-intent-diff-checker.md (Unit 12) and spec
// Section 4.3.3.
//
// Verifies that a transaction's simulated balance deltas satisfy the user's
// stated intent. Three paths:
//
//   1. No intent supplied → deterministic no-op (passed: true).
//   2. Structured `Intent.spec` present → compare deltas against the spec
//      directly. This is the preferred path — mirrors how UniswapX / CoW /
//      1inch verify solver execution against signed numeric bounds.
//   3. Only free-text `Intent.description` → parse it via `intent_rules`;
//      Unrecognized shapes deterministically return INCONCLUSIVE.
//
// An optional LLM enrichment (Unit 16) runs in parallel as pure metadata.
// IMPORTANT: the return of `intent_diff_llm_analyze` NEVER enters CheckerOutput,
// Borsh hash input, or the signed attestation. See spec Section 5.5 and Key
// Invariant #1 in CLAUDE.md.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use solana_sdk::pubkey::Pubkey;

use ciel_fork::SimulationTrace;

use crate::intent_rules::{
    DEPOSIT_TOLERANCE_BPS, SWAP_TOLERANCE_BPS, TRANSFER_TOLERANCE_BPS, WSOL_MINT, IntentPattern,
    parse_intent_goal, token_info,
};
use crate::traits::{Checker, CheckerContext, CheckerOutput, Flag, Intent, IntentSpec, Severity};

// ---------------------------------------------------------------------------
// Flag codes
// ---------------------------------------------------------------------------

pub const FLAG_INTENT_BALANCE_MISMATCH: &str = "INTENT_BALANCE_MISMATCH";
pub const FLAG_INTENT_VERIFICATION_INCONCLUSIVE: &str = "INTENT_VERIFICATION_INCONCLUSIVE";

const CHECKER_NAME: &str = "intent_diff";

// ---------------------------------------------------------------------------
// Intent source label (for flag data)
// ---------------------------------------------------------------------------

enum IntentSource<'a> {
    Spec,
    ParsedGoal(&'a str),
}

impl<'a> IntentSource<'a> {
    fn label(&self) -> &'static str {
        match self {
            IntentSource::Spec => "spec",
            IntentSource::ParsedGoal(_) => "parsed_goal",
        }
    }

    fn goal_text(&self, fallback: &'a str) -> &'a str {
        match self {
            IntentSource::Spec => fallback,
            IntentSource::ParsedGoal(s) => s,
        }
    }
}

// ---------------------------------------------------------------------------
// Checker
// ---------------------------------------------------------------------------

pub struct IntentDiffChecker;

impl IntentDiffChecker {
    pub fn new() -> Self {
        Self
    }
}

impl Default for IntentDiffChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Checker for IntentDiffChecker {
    fn name(&self) -> &'static str {
        CHECKER_NAME
    }

    async fn check(&self, ctx: &CheckerContext) -> CheckerOutput {
        // Step 1: no intent → deterministic no-op (spec Section 4.3.3 step 1).
        let Some(intent) = &ctx.intent else {
            return passed_noop("intent_diff: no intent supplied (raw tx mode)");
        };

        // Step 2: structured spec takes precedence.
        if let Some(spec) = &intent.spec {
            return verify_spec(&ctx.trace, spec, &intent.description);
        }

        // Step 3: fall back to parsing the free-text description.
        match parse_intent_goal(&intent.description) {
            IntentPattern::Swap {
                amount,
                token_in,
                token_out,
            } => verify_swap_from_text(
                &ctx.trace,
                amount,
                &token_in,
                &token_out,
                &intent.description,
            ),
            IntentPattern::Transfer { amount, token, .. } => {
                verify_transfer_from_text(&ctx.trace, amount, &token, &intent.description)
            }
            IntentPattern::Deposit { amount, token, .. } => {
                verify_deposit_from_text(&ctx.trace, amount, &token, &intent.description)
            }
            IntentPattern::Unrecognized => inconclusive(
                &intent.description,
                "multi_leg_unrecognized_pattern",
                "Intent goal does not match a recognized verifiable pattern; balance-delta \
                 comparison cannot confirm or refute",
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Structured spec dispatch
// ---------------------------------------------------------------------------

fn verify_spec(trace: &SimulationTrace, spec: &IntentSpec, description: &str) -> CheckerOutput {
    match spec {
        IntentSpec::Swap {
            amount_in,
            mint_in,
            mint_out,
            min_amount_out,
        } => verify_swap(
            trace,
            *amount_in as i128,
            mint_in,
            mint_out,
            min_amount_out.map(|v| v as i128),
            SWAP_TOLERANCE_BPS,
            IntentSource::Spec,
            description,
        ),
        IntentSpec::Transfer { amount, mint, .. } => verify_transfer(
            trace,
            *amount as i128,
            mint,
            TRANSFER_TOLERANCE_BPS,
            IntentSource::Spec,
            description,
        ),
        IntentSpec::Deposit { amount, mint, .. } => verify_deposit(
            trace,
            *amount as i128,
            mint,
            DEPOSIT_TOLERANCE_BPS,
            IntentSource::Spec,
            description,
        ),
    }
}

// ---------------------------------------------------------------------------
// Free-text → structured bridging
// ---------------------------------------------------------------------------

fn verify_swap_from_text(
    trace: &SimulationTrace,
    amount: f64,
    token_in: &str,
    token_out: &str,
    goal: &str,
) -> CheckerOutput {
    let Some((mint_in, decimals_in)) = token_info(token_in) else {
        return inconclusive(
            goal,
            "unknown_token_symbol",
            &format!("token_in symbol '{}' not in registry", token_in),
        );
    };
    let Some((mint_out, _decimals_out)) = token_info(token_out) else {
        return inconclusive(
            goal,
            "unknown_token_symbol",
            &format!("token_out symbol '{}' not in registry", token_out),
        );
    };
    let amount_raw = to_raw_units(amount, decimals_in);
    verify_swap(
        trace,
        amount_raw,
        &mint_in,
        &mint_out,
        None,
        SWAP_TOLERANCE_BPS,
        IntentSource::ParsedGoal(goal),
        goal,
    )
}

fn verify_transfer_from_text(
    trace: &SimulationTrace,
    amount: f64,
    token: &str,
    goal: &str,
) -> CheckerOutput {
    let Some((mint, decimals)) = token_info(token) else {
        return inconclusive(
            goal,
            "unknown_token_symbol",
            &format!("token symbol '{}' not in registry", token),
        );
    };
    let amount_raw = to_raw_units(amount, decimals);
    verify_transfer(
        trace,
        amount_raw,
        &mint,
        TRANSFER_TOLERANCE_BPS,
        IntentSource::ParsedGoal(goal),
        goal,
    )
}

fn verify_deposit_from_text(
    trace: &SimulationTrace,
    amount: f64,
    token: &str,
    goal: &str,
) -> CheckerOutput {
    let Some((mint, decimals)) = token_info(token) else {
        return inconclusive(
            goal,
            "unknown_token_symbol",
            &format!("token symbol '{}' not in registry", token),
        );
    };
    let amount_raw = to_raw_units(amount, decimals);
    verify_deposit(
        trace,
        amount_raw,
        &mint,
        DEPOSIT_TOLERANCE_BPS,
        IntentSource::ParsedGoal(goal),
        goal,
    )
}

// ---------------------------------------------------------------------------
// Verifiers
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn verify_swap(
    trace: &SimulationTrace,
    expected_amount_in: i128,
    mint_in: &Pubkey,
    mint_out: &Pubkey,
    min_amount_out: Option<i128>,
    tolerance_bps: u16,
    source: IntentSource,
    description: &str,
) -> CheckerOutput {
    let actual_in = sum_mint_delta(trace, mint_in);
    let actual_out = sum_mint_delta(trace, mint_out);

    let expected_in = -expected_amount_in; // we expect a decrease
    let input_ok = within_tolerance(actual_in, expected_in, tolerance_bps);
    let output_direction_ok = actual_out > 0;
    let output_min_ok = min_amount_out
        .map(|min| actual_out >= min)
        .unwrap_or(true);

    let goal = source.goal_text(description);
    if input_ok && output_direction_ok && output_min_ok {
        let details = format!(
            "Intent '{}' matches simulation: {} in delta {}, {} out delta {}",
            goal,
            short_mint(mint_in),
            actual_in,
            short_mint(mint_out),
            actual_out,
        );
        return passed_noop(&details);
    }

    let reason = if !input_ok {
        "input_amount_outside_tolerance"
    } else if !output_direction_ok {
        "output_direction_or_token_mismatch"
    } else {
        "output_below_min_amount"
    };

    let data = json!({
        "expected_mint_in": mint_in.to_string(),
        "expected_amount_in": expected_amount_in.to_string(),
        "actual_in_delta": actual_in.to_string(),
        "expected_mint_out": mint_out.to_string(),
        "actual_out_delta": actual_out.to_string(),
        "min_amount_out": min_amount_out.map(|v| v.to_string()),
        "tolerance_bps": tolerance_bps,
        "intent_source": source.label(),
        "intent_goal": goal,
        "reason": reason,
    });

    let message = format!(
        "Intent expects swap {} → {} but simulation shows in-delta={}, out-delta={}",
        short_mint(mint_in),
        short_mint(mint_out),
        actual_in,
        actual_out,
    );
    let details = format!(
        "Intent '{}' does NOT match simulation ({})",
        goal, reason,
    );

    failed(
        FLAG_INTENT_BALANCE_MISMATCH,
        &message,
        data,
        Severity::High,
        details,
    )
}

fn verify_transfer(
    trace: &SimulationTrace,
    expected_amount: i128,
    mint: &Pubkey,
    tolerance_bps: u16,
    source: IntentSource,
    description: &str,
) -> CheckerOutput {
    let actual = sum_mint_delta(trace, mint);
    let expected = -expected_amount; // outflow

    let goal = source.goal_text(description);
    if within_tolerance(actual, expected, tolerance_bps) {
        let details = format!(
            "Intent '{}' matches simulation: {} delta {} (transfer outflow)",
            goal,
            short_mint(mint),
            actual,
        );
        return passed_noop(&details);
    }

    let data = json!({
        "expected_mint": mint.to_string(),
        "expected_amount": expected_amount.to_string(),
        "actual_delta": actual.to_string(),
        "tolerance_bps": tolerance_bps,
        "intent_source": source.label(),
        "intent_goal": goal,
        "reason": "transfer_amount_outside_tolerance",
    });

    failed(
        FLAG_INTENT_BALANCE_MISMATCH,
        &format!(
            "Intent expects transfer of {} {} but simulation shows delta {}",
            expected_amount,
            short_mint(mint),
            actual,
        ),
        data,
        Severity::High,
        format!("Intent '{}' does NOT match simulation (transfer)", goal),
    )
}

fn verify_deposit(
    trace: &SimulationTrace,
    expected_amount: i128,
    mint: &Pubkey,
    tolerance_bps: u16,
    source: IntentSource,
    description: &str,
) -> CheckerOutput {
    // v1 verifies the outflow leg only. Receipt-token verification requires a
    // protocol → receipt-mint registry (out of Unit 12 scope). We emit an
    // INCONCLUSIVE flag alongside a passing outflow check — still deterministic,
    // but honest about what was actually verified.
    let actual = sum_mint_delta(trace, mint);
    let expected = -expected_amount;

    let goal = source.goal_text(description);
    if !within_tolerance(actual, expected, tolerance_bps) {
        let data = json!({
            "expected_mint": mint.to_string(),
            "expected_amount": expected_amount.to_string(),
            "actual_delta": actual.to_string(),
            "tolerance_bps": tolerance_bps,
            "intent_source": source.label(),
            "intent_goal": goal,
            "reason": "deposit_outflow_outside_tolerance",
        });
        return failed(
            FLAG_INTENT_BALANCE_MISMATCH,
            &format!(
                "Intent expects deposit of {} {} but simulation shows outflow {}",
                expected_amount,
                short_mint(mint),
                actual,
            ),
            data,
            Severity::High,
            format!("Intent '{}' does NOT match simulation (deposit outflow)", goal),
        );
    }

    // Outflow verified; receipt token is unverified.
    let data = json!({
        "intent_goal": goal,
        "intent_source": source.label(),
        "reason": "deposit_receipt_unverified",
        "verified_outflow_mint": mint.to_string(),
        "verified_outflow_delta": actual.to_string(),
    });
    CheckerOutput {
        checker_name: CHECKER_NAME.to_string(),
        passed: true,
        severity: Severity::None,
        flags: vec![Flag {
            code: FLAG_INTENT_VERIFICATION_INCONCLUSIVE.to_string(),
            message: "Deposit outflow verified; receipt-token verification not in rule table"
                .to_string(),
            data,
        }],
        details: format!(
            "Intent '{}' outflow verified; receipt inconclusive (no protocol registry)",
            goal,
        ),
    }
}

// ---------------------------------------------------------------------------
// Delta summation
// ---------------------------------------------------------------------------

/// Sum the signed balance delta for a given mint across the trace.
///
/// For wSOL the function also folds in native-lamport deltas, because
/// Jupiter/Raydium routes typically unwrap at end-of-tx and surface the SOL
/// change on the user's lamport balance rather than as a wSOL token delta.
/// Summing all lamport deltas introduces ~5000-lamport fee noise per
/// transaction, which is orders of magnitude below any realistic swap-amount
/// tolerance and therefore safe to ignore.
fn sum_mint_delta(trace: &SimulationTrace, mint: &Pubkey) -> i128 {
    let token_sum: i128 = trace
        .token_balance_deltas
        .iter()
        .filter(|d| &d.mint == mint)
        .map(|d| d.delta)
        .sum();

    if mint.to_string() == WSOL_MINT {
        let lamport_sum: i128 = trace
            .balance_deltas
            .values()
            .copied()
            .map(i128::from)
            .sum();
        token_sum + lamport_sum
    } else {
        token_sum
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn to_raw_units(amount: f64, decimals: u8) -> i128 {
    // Scale to raw units. f64 appears only on the parse-time side — the result
    // of this function (i128) is what flows into Borsh-hashed comparisons.
    let scale = 10f64.powi(decimals as i32);
    (amount * scale).round() as i128
}

fn within_tolerance(actual: i128, expected: i128, tolerance_bps: u16) -> bool {
    // |actual - expected| * 10_000 <= |expected| * tolerance_bps
    let diff = (actual - expected).abs();
    let abs_expected = expected.abs();
    let lhs = diff.saturating_mul(10_000);
    let rhs = abs_expected.saturating_mul(tolerance_bps as i128);
    lhs <= rhs
}

fn short_mint(mint: &Pubkey) -> String {
    let s = mint.to_string();
    if s.len() > 8 {
        format!("{}…{}", &s[..4], &s[s.len() - 4..])
    } else {
        s
    }
}

fn passed_noop(details: &str) -> CheckerOutput {
    CheckerOutput {
        checker_name: CHECKER_NAME.to_string(),
        passed: true,
        severity: Severity::None,
        flags: vec![],
        details: details.to_string(),
    }
}

fn inconclusive(goal: &str, reason: &str, message: &str) -> CheckerOutput {
    let data = json!({
        "intent_goal": goal,
        "reason": reason,
    });
    CheckerOutput {
        checker_name: CHECKER_NAME.to_string(),
        passed: true,
        severity: Severity::None,
        flags: vec![Flag {
            code: FLAG_INTENT_VERIFICATION_INCONCLUSIVE.to_string(),
            message: message.to_string(),
            data,
        }],
        details: "Intent verification inconclusive — goal pattern not in deterministic rule table"
            .to_string(),
    }
}

fn failed(
    code: &str,
    message: &str,
    data: Value,
    severity: Severity,
    details: String,
) -> CheckerOutput {
    CheckerOutput {
        checker_name: CHECKER_NAME.to_string(),
        passed: false,
        severity,
        flags: vec![Flag {
            code: code.to_string(),
            message: message.to_string(),
            data,
        }],
        details,
    }
}

// ---------------------------------------------------------------------------
// LLM enrichment (metadata-only — NEVER enters CheckerOutput)
// ---------------------------------------------------------------------------

/// LLM-produced semantic comparison of intent vs simulation.
///
/// IMPORTANT: This type is a sibling of `CheckerOutput`, not a field of it. The
/// value never enters Borsh hash input, `checker_outputs_hash`, or the signed
/// attestation. See spec Sections 4.3.3 and 5.5, and Key Invariant #1 in
/// CLAUDE.md. Label is `analysis` (not `verdict`) so downstream consumers
/// don't treat it as authoritative.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentDiffLlmAnalysis {
    #[serde(rename = "match")]
    pub is_match: bool,
    pub confidence: f64,
    pub explanation: String,
}

/// Stub until Unit 16 wires a real LlmClient. Returns None, which the pipeline
/// treats as "no enrichment available."
pub async fn intent_diff_llm_analyze(
    _intent: &Intent,
    _trace: &SimulationTrace,
) -> Option<IntentDiffLlmAnalysis> {
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::str::FromStr;

    use borsh::to_vec;
    use ciel_fork::{SimulationTrace, TokenBalanceDelta};
    use solana_sdk::hash::hash;
    use solana_sdk::message::Message;
    use solana_sdk::signature::{Signer, keypair_from_seed};
    use solana_sdk::transaction::Transaction;

    use crate::oracle_cache::OracleCache;
    use crate::program_registry::ProgramRegistry;
    use crate::traits::{CheckerContext, Intent};

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn deterministic_pubkey(seed: &str) -> Pubkey {
        let h = hash(seed.as_bytes());
        keypair_from_seed(h.as_ref()).unwrap().pubkey()
    }

    fn wsol_mint() -> Pubkey {
        Pubkey::from_str(WSOL_MINT).unwrap()
    }

    fn usdc_mint() -> Pubkey {
        Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap()
    }

    fn eth_mint() -> Pubkey {
        Pubkey::from_str("7vfCXTUXx5WJV5JADk17DUJ4ksgau7utNKj4b963voxs").unwrap()
    }

    fn empty_trace() -> SimulationTrace {
        SimulationTrace {
            success: true,
            error: None,
            balance_deltas: HashMap::new(),
            cpi_graph: vec![],
            account_changes: vec![],
            logs: vec![],
            oracle_reads: vec![],
            token_approvals: vec![],
            token_balance_deltas: vec![],
            compute_units_consumed: 0,
            fee: 5000,
        }
    }

    fn trace_with_token_deltas(deltas: Vec<TokenBalanceDelta>) -> SimulationTrace {
        let mut t = empty_trace();
        t.token_balance_deltas = deltas;
        t
    }

    fn ctx_with(trace: SimulationTrace, intent: Option<Intent>) -> CheckerContext {
        CheckerContext {
            trace,
            original_tx: Transaction::new_unsigned(Message::new(&[], None)),
            intent,
            slot: 350_000_000,
            oracle_cache: OracleCache::default(),
            known_programs: ProgramRegistry::default(),
        }
    }

    fn intent_text(desc: &str) -> Intent {
        Intent {
            description: desc.to_string(),
            constraints: vec![],
            spec: None,
        }
    }

    fn intent_with_spec(desc: &str, spec: IntentSpec) -> Intent {
        Intent {
            description: desc.to_string(),
            constraints: vec![],
            spec: Some(spec),
        }
    }

    fn delta(owner_seed: &str, mint: Pubkey, amount: i128) -> TokenBalanceDelta {
        TokenBalanceDelta {
            owner: deterministic_pubkey(owner_seed),
            mint,
            delta: amount,
        }
    }

    // -----------------------------------------------------------------------
    // Swap happy path (spec test #1)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_swap_happy_path_parsed_goal() {
        let trace = trace_with_token_deltas(vec![
            delta("user", usdc_mint(), -100_000_000),
            delta("user", wsol_mint(), 670_000_000),
        ]);
        let intent = intent_text("swap 100 USDC for SOL");
        let out = IntentDiffChecker::new()
            .check(&ctx_with(trace, Some(intent)))
            .await;
        assert!(out.passed, "expected passed: {:?}", out);
        assert_eq!(out.severity, Severity::None);
        assert!(out.flags.is_empty(), "flags should be empty: {:?}", out.flags);
        assert!(out.details.contains("matches"));
    }

    #[tokio::test]
    async fn test_swap_happy_path_via_sol_native_lamports() {
        // Jupiter/Raydium end-of-tx unwrap: SOL surfaces in native lamport deltas,
        // not in wSOL token_balance_deltas. Checker must still accept.
        let mut trace = trace_with_token_deltas(vec![delta("user", usdc_mint(), -100_000_000)]);
        trace
            .balance_deltas
            .insert(deterministic_pubkey("user-wallet"), 670_000_000);

        let intent = intent_text("swap 100 USDC for SOL");
        let out = IntentDiffChecker::new()
            .check(&ctx_with(trace, Some(intent)))
            .await;
        assert!(out.passed, "native-lamport SOL path: {:?}", out);
        assert!(out.flags.is_empty());
    }

    // -----------------------------------------------------------------------
    // Swap mismatch (spec test #2)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_swap_mismatch_wrong_token() {
        let trace = trace_with_token_deltas(vec![
            delta("user", usdc_mint(), -100_000_000),
            delta("user", eth_mint(), 100_000_000), // wrong output token
        ]);
        let intent = intent_text("swap 100 USDC for SOL");
        let out = IntentDiffChecker::new()
            .check(&ctx_with(trace, Some(intent)))
            .await;
        assert!(!out.passed, "expected mismatch: {:?}", out);
        assert_eq!(out.severity, Severity::High);
        assert_eq!(out.flags.len(), 1);
        assert_eq!(out.flags[0].code, FLAG_INTENT_BALANCE_MISMATCH);
    }

    // -----------------------------------------------------------------------
    // Inconclusive (spec test #3)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_inconclusive_rebalance_pattern() {
        let trace = trace_with_token_deltas(vec![]);
        let intent = intent_text("rebalance portfolio to 60/30/10 split");
        let out = IntentDiffChecker::new()
            .check(&ctx_with(trace, Some(intent)))
            .await;
        assert!(out.passed);
        assert_eq!(out.severity, Severity::None);
        assert_eq!(out.flags.len(), 1);
        assert_eq!(out.flags[0].code, FLAG_INTENT_VERIFICATION_INCONCLUSIVE);
    }

    // -----------------------------------------------------------------------
    // Determinism (spec test #4) — the cryptographic determinism proof
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_determinism_same_input_two_runs() {
        let trace = trace_with_token_deltas(vec![
            delta("user", usdc_mint(), -100_000_000),
            delta("user", wsol_mint(), 670_000_000),
        ]);
        let intent = intent_text("swap 100 USDC for SOL");
        let ctx_a = ctx_with(trace.clone(), Some(intent.clone()));
        let ctx_b = ctx_with(trace, Some(intent));

        let out_a = IntentDiffChecker::new().check(&ctx_a).await;
        let out_b = IntentDiffChecker::new().check(&ctx_b).await;
        assert_eq!(out_a, out_b);

        // Byte-identical Borsh serialization — this is what `checker_outputs_hash`
        // actually hashes. PartialEq alone is not enough.
        let bytes_a = to_vec(&out_a).expect("borsh serialize");
        let bytes_b = to_vec(&out_b).expect("borsh serialize");
        assert_eq!(
            bytes_a, bytes_b,
            "Borsh bytes must be identical across runs"
        );
    }

    // -----------------------------------------------------------------------
    // LLM availability does not affect output (Invariant #1 proof)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_llm_availability_does_not_affect_output() {
        let trace = trace_with_token_deltas(vec![
            delta("user", usdc_mint(), -100_000_000),
            delta("user", wsol_mint(), 670_000_000),
        ]);
        let intent = intent_text("swap 100 USDC for SOL");
        let ctx_a = ctx_with(trace.clone(), Some(intent.clone()));
        let ctx_b = ctx_with(trace, Some(intent.clone()));

        let out_a = IntentDiffChecker::new().check(&ctx_a).await;
        // Concurrent LLM enrichment on ctx_b. Result is discarded —
        // it must not feed CheckerOutput.
        let _ = intent_diff_llm_analyze(&intent, &ctx_b.trace).await;
        let out_b = IntentDiffChecker::new().check(&ctx_b).await;

        assert_eq!(out_a, out_b);
        let bytes_a = to_vec(&out_a).expect("borsh serialize");
        let bytes_b = to_vec(&out_b).expect("borsh serialize");
        assert_eq!(bytes_a, bytes_b);
    }

    // -----------------------------------------------------------------------
    // No-intent no-op
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_no_intent_is_noop() {
        let trace = empty_trace();
        let out = IntentDiffChecker::new().check(&ctx_with(trace, None)).await;
        assert!(out.passed);
        assert_eq!(out.severity, Severity::None);
        assert!(out.flags.is_empty());
        assert!(out.details.contains("raw tx mode"));
    }

    // -----------------------------------------------------------------------
    // Structured spec preferred over free-text
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_swap_structured_spec_preferred_over_description() {
        let trace = trace_with_token_deltas(vec![
            delta("user", usdc_mint(), -100_000_000),
            delta("user", wsol_mint(), 670_000_000),
        ]);
        // Description is gibberish that would otherwise parse to Unrecognized.
        let intent = intent_with_spec(
            "rebalance portfolio to 60/30/10 split",
            IntentSpec::Swap {
                amount_in: 100_000_000,
                mint_in: usdc_mint(),
                mint_out: wsol_mint(),
                min_amount_out: None,
            },
        );
        let out = IntentDiffChecker::new()
            .check(&ctx_with(trace, Some(intent)))
            .await;
        assert!(out.passed, "spec path should verify: {:?}", out);
        assert!(out.flags.is_empty());
    }

    #[tokio::test]
    async fn test_swap_min_amount_out_violated() {
        let trace = trace_with_token_deltas(vec![
            delta("user", usdc_mint(), -100_000_000),
            delta("user", wsol_mint(), 670_000_000),
        ]);
        let intent = intent_with_spec(
            "swap 100 USDC for SOL",
            IntentSpec::Swap {
                amount_in: 100_000_000,
                mint_in: usdc_mint(),
                mint_out: wsol_mint(),
                min_amount_out: Some(700_000_000), // demand more than actual 670M
            },
        );
        let out = IntentDiffChecker::new()
            .check(&ctx_with(trace, Some(intent)))
            .await;
        assert!(!out.passed);
        assert_eq!(out.flags[0].code, FLAG_INTENT_BALANCE_MISMATCH);
    }

    // -----------------------------------------------------------------------
    // Tolerance edges
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_swap_within_1pct_tolerance() {
        // -99.5 USDC (within 1% of -100): should pass.
        let trace = trace_with_token_deltas(vec![
            delta("user", usdc_mint(), -99_500_000),
            delta("user", wsol_mint(), 670_000_000),
        ]);
        let intent = intent_text("swap 100 USDC for SOL");
        let out = IntentDiffChecker::new()
            .check(&ctx_with(trace, Some(intent)))
            .await;
        assert!(out.passed, "within-tolerance should pass: {:?}", out);
    }

    #[tokio::test]
    async fn test_swap_outside_1pct_tolerance() {
        // -98 USDC (2% off): should fail.
        let trace = trace_with_token_deltas(vec![
            delta("user", usdc_mint(), -98_000_000),
            delta("user", wsol_mint(), 670_000_000),
        ]);
        let intent = intent_text("swap 100 USDC for SOL");
        let out = IntentDiffChecker::new()
            .check(&ctx_with(trace, Some(intent)))
            .await;
        assert!(!out.passed);
        assert_eq!(out.flags[0].code, FLAG_INTENT_BALANCE_MISMATCH);
    }

    #[tokio::test]
    async fn test_swap_wrong_direction() {
        // +100 USDC (wrong direction entirely): should fail.
        let trace = trace_with_token_deltas(vec![
            delta("user", usdc_mint(), 100_000_000),
            delta("user", wsol_mint(), 670_000_000),
        ]);
        let intent = intent_text("swap 100 USDC for SOL");
        let out = IntentDiffChecker::new()
            .check(&ctx_with(trace, Some(intent)))
            .await;
        assert!(!out.passed);
    }

    // -----------------------------------------------------------------------
    // Transfer / Deposit
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_transfer_happy_path() {
        let trace = trace_with_token_deltas(vec![delta("sender", usdc_mint(), -5_000_000)]);
        let intent = intent_text("transfer 5 USDC to alice.sol");
        let out = IntentDiffChecker::new()
            .check(&ctx_with(trace, Some(intent)))
            .await;
        assert!(out.passed, "{:?}", out);
        assert!(out.flags.is_empty());
    }

    #[tokio::test]
    async fn test_deposit_outflow_verified_receipt_inconclusive() {
        // Outflow check passes; receipt verification is deliberately out of scope
        // so the checker emits INCONCLUSIVE alongside passed: true.
        let trace = trace_with_token_deltas(vec![delta("user", wsol_mint(), -10_000_000_000)]);
        let intent = intent_text("deposit 10 SOL into marinade");
        let out = IntentDiffChecker::new()
            .check(&ctx_with(trace, Some(intent)))
            .await;
        assert!(out.passed);
        assert_eq!(out.severity, Severity::None);
        assert_eq!(out.flags.len(), 1);
        assert_eq!(out.flags[0].code, FLAG_INTENT_VERIFICATION_INCONCLUSIVE);
        assert_eq!(
            out.flags[0].data.get("reason").and_then(|v| v.as_str()),
            Some("deposit_receipt_unverified")
        );
    }

    // -----------------------------------------------------------------------
    // LLM stub
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_llm_stub_returns_none() {
        let intent = intent_text("swap 100 USDC for SOL");
        let trace = empty_trace();
        let result = intent_diff_llm_analyze(&intent, &trace).await;
        assert!(result.is_none());
    }
}
