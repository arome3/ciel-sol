// Approval Abuse checker. See docs/13-approval-abuse-checker.md (Unit 13) and
// spec Section 4.3.6.
//
// Scans the CPI call graph for unlimited SPL Token / Token-2022 delegate
// approvals (Approve disc=4, ApproveChecked disc=13) granted to programs not
// present in the known-good ProgramRegistry. Suppresses flags when a matching
// Revoke (disc=5) on the same source token account appears later in the tx —
// the "approve-use-revoke" pattern common in legitimate DEX flows.
//
// v1 limitation: registry lookup checks the delegate pubkey directly, so a
// PDA delegate owned by a known protocol will not match. Resolving PDAs to
// owning programs via trace.account_changes is a v2 refinement.

use async_trait::async_trait;
use serde_json::json;
use solana_sdk::pubkey::Pubkey;

use crate::program_registry::{SPL_TOKEN_2022_PROGRAM_ID, SPL_TOKEN_PROGRAM_ID};
use crate::traits::{Checker, CheckerContext, CheckerOutput, Flag, ProgramRegistry, Severity};
use ciel_fork::trace::CpiCall;

// ---------------------------------------------------------------------------
// Flag codes
// ---------------------------------------------------------------------------

pub const FLAG_UNLIMITED_APPROVAL_UNKNOWN_PROGRAM: &str = "UNLIMITED_APPROVAL_UNKNOWN_PROGRAM";

// ---------------------------------------------------------------------------
// SPL Token instruction discriminators
// ---------------------------------------------------------------------------
// Shared between SPL Token and Token-2022. Ordinals verified against the
// upstream `spl_token::instruction::TokenInstruction` enum.

const SPL_TOKEN_DISC_APPROVE: u8 = 4;
const SPL_TOKEN_DISC_REVOKE: u8 = 5;
const SPL_TOKEN_DISC_APPROVE_CHECKED: u8 = 13;

// ---------------------------------------------------------------------------
// Checker
// ---------------------------------------------------------------------------

/// Approval Abuse checker. See spec Section 4.3.6.
pub struct ApprovalAbuseChecker;

impl ApprovalAbuseChecker {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ApprovalAbuseChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Checker for ApprovalAbuseChecker {
    fn name(&self) -> &'static str {
        "approval_abuse"
    }

    async fn check(&self, ctx: &CheckerContext) -> CheckerOutput {
        let graph = &ctx.trace.cpi_graph;
        let mut flags: Vec<Flag> = Vec::new();

        for (idx, call) in graph.iter().enumerate() {
            let Some(candidate) = inspect_approve(call, &ctx.known_programs) else {
                continue;
            };
            if revoked_after(graph, idx, &candidate.source) {
                // Spec 4.3.6 step 5 — benign approve-use-revoke pattern.
                continue;
            }
            flags.push(build_flag(call, &candidate));
        }

        let passed = flags.is_empty();
        let severity = if passed { Severity::None } else { Severity::High };
        let details = if passed {
            "No abusive token approvals detected".to_string()
        } else {
            format!(
                "{} unlimited approval(s) to unknown program(s) detected",
                flags.len()
            )
        };

        CheckerOutput {
            checker_name: "approval_abuse".to_string(),
            passed,
            severity,
            flags,
            details,
        }
    }
}

// ---------------------------------------------------------------------------
// Per-call inspection
// ---------------------------------------------------------------------------

struct ApproveCandidate {
    disc: u8,
    source: Pubkey,
    delegate: Pubkey,
    owner: Option<Pubkey>,
    mint: Option<Pubkey>,
    spl_program_name: &'static str,
}

fn inspect_approve(call: &CpiCall, registry: &ProgramRegistry) -> Option<ApproveCandidate> {
    let spl_program_name = if call.program_id == SPL_TOKEN_PROGRAM_ID {
        "SPL Token"
    } else if call.program_id == SPL_TOKEN_2022_PROGRAM_ID {
        "SPL Token-2022"
    } else {
        return None;
    };

    let disc = *call.data.first()?;
    let (amount, source, delegate, owner, mint) = match disc {
        SPL_TOKEN_DISC_APPROVE => {
            // data:     [4, amount_u64_le...]
            // accounts: [source, delegate, owner, ...signers]
            let amount = parse_amount_u64_le(&call.data)?;
            let source = call.accounts.first().copied()?;
            let delegate = call.accounts.get(1).copied()?;
            let owner = call.accounts.get(2).copied();
            (amount, source, delegate, owner, None)
        }
        SPL_TOKEN_DISC_APPROVE_CHECKED => {
            // data:     [13, amount_u64_le..., decimals_u8]
            // accounts: [source, mint, delegate, owner, ...signers]
            let amount = parse_amount_u64_le(&call.data)?;
            let source = call.accounts.first().copied()?;
            let mint = call.accounts.get(1).copied();
            let delegate = call.accounts.get(2).copied()?;
            let owner = call.accounts.get(3).copied();
            (amount, source, delegate, owner, mint)
        }
        _ => return None,
    };

    if amount != u64::MAX {
        return None;
    }
    if registry.is_known_protocol(&delegate).is_some() {
        return None;
    }

    Some(ApproveCandidate {
        disc,
        source,
        delegate,
        owner,
        mint,
        spl_program_name,
    })
}

fn revoked_after(graph: &[CpiCall], approve_idx: usize, source: &Pubkey) -> bool {
    graph.iter().skip(approve_idx + 1).any(|call| {
        (call.program_id == SPL_TOKEN_PROGRAM_ID || call.program_id == SPL_TOKEN_2022_PROGRAM_ID)
            && call.data.first() == Some(&SPL_TOKEN_DISC_REVOKE)
            && call.accounts.first() == Some(source)
    })
}

fn build_flag(call: &CpiCall, c: &ApproveCandidate) -> Flag {
    Flag {
        code: FLAG_UNLIMITED_APPROVAL_UNKNOWN_PROGRAM.to_string(),
        message: format!(
            "Unlimited token approval granted to unrecognized program {} at instruction {}, stack_height {}",
            c.delegate, call.instruction_index, call.stack_height
        ),
        data: json!({
            "token_mint":           c.mint.map(|p| p.to_string()),
            "delegate":             c.delegate.to_string(),
            "amount":               u64::MAX.to_string(),
            "in_known_registry":    false,
            "source_token_account": c.source.to_string(),
            "owner":                c.owner.map(|p| p.to_string()),
            "instruction_index":    call.instruction_index,
            "stack_height":         call.stack_height,
            "via_cpi":              call.stack_height >= 2,
            "spl_program_name":     c.spl_program_name,
            "discriminator":        c.disc,
        }),
    }
}

fn parse_amount_u64_le(data: &[u8]) -> Option<u64> {
    let bytes: [u8; 8] = data.get(1..9)?.try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle_cache::OracleCache;
    use crate::program_registry::JUPITER_V6_PROGRAM_ID;
    use borsh::to_vec;
    use ciel_fork::SimulationTrace;
    use solana_sdk::hash::hash;
    use solana_sdk::message::Message;
    use solana_sdk::signer::keypair::keypair_from_seed;
    use solana_sdk::signer::Signer;
    use solana_sdk::transaction::Transaction;
    use std::collections::HashMap;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn deterministic_pubkey(seed: &str) -> Pubkey {
        let h = hash(seed.as_bytes());
        keypair_from_seed(h.as_ref()).unwrap().pubkey()
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

    fn trace_with_calls(cpi_graph: Vec<CpiCall>) -> SimulationTrace {
        let mut t = empty_trace();
        t.cpi_graph = cpi_graph;
        t
    }

    fn ctx_with(trace: SimulationTrace) -> CheckerContext {
        CheckerContext {
            trace,
            original_tx: Transaction::new_unsigned(Message::new(&[], None)),
            intent: None,
            slot: 350_000_000,
            oracle_cache: OracleCache::default(),
            known_programs: ProgramRegistry::with_mainnet_defaults(),
        }
    }

    fn approve_call_on(
        program_id: Pubkey,
        stack_height: u8,
        instruction_index: usize,
        source: Pubkey,
        delegate: Pubkey,
        owner: Pubkey,
        amount: u64,
    ) -> CpiCall {
        let mut data = Vec::with_capacity(9);
        data.push(SPL_TOKEN_DISC_APPROVE);
        data.extend_from_slice(&amount.to_le_bytes());
        CpiCall {
            program_id,
            instruction_index,
            stack_height,
            accounts: vec![source, delegate, owner],
            data,
        }
    }

    fn approve_call(
        stack_height: u8,
        source: Pubkey,
        delegate: Pubkey,
        owner: Pubkey,
        amount: u64,
    ) -> CpiCall {
        approve_call_on(
            SPL_TOKEN_PROGRAM_ID,
            stack_height,
            0,
            source,
            delegate,
            owner,
            amount,
        )
    }

    fn approve_checked_call(
        stack_height: u8,
        source: Pubkey,
        mint: Pubkey,
        delegate: Pubkey,
        owner: Pubkey,
        amount: u64,
    ) -> CpiCall {
        let mut data = Vec::with_capacity(10);
        data.push(SPL_TOKEN_DISC_APPROVE_CHECKED);
        data.extend_from_slice(&amount.to_le_bytes());
        data.push(6); // decimals
        CpiCall {
            program_id: SPL_TOKEN_PROGRAM_ID,
            instruction_index: 0,
            stack_height,
            accounts: vec![source, mint, delegate, owner],
            data,
        }
    }

    fn revoke_call(
        stack_height: u8,
        instruction_index: usize,
        source: Pubkey,
        owner: Pubkey,
    ) -> CpiCall {
        CpiCall {
            program_id: SPL_TOKEN_PROGRAM_ID,
            instruction_index,
            stack_height,
            accounts: vec![source, owner],
            data: vec![SPL_TOKEN_DISC_REVOKE],
        }
    }

    // -----------------------------------------------------------------------
    // 1. Core detection — spec verification steps
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_no_approvals_passes() {
        let ctx = ctx_with(empty_trace());
        let out = ApprovalAbuseChecker::new().check(&ctx).await;
        assert!(out.passed);
        assert_eq!(out.severity, Severity::None);
        assert!(out.flags.is_empty());
        assert_eq!(out.checker_name, "approval_abuse");
    }

    #[tokio::test]
    async fn test_unlimited_approve_unknown_program_flags() {
        // Verification step 2.
        let source = deterministic_pubkey("source-ata-1");
        let delegate = deterministic_pubkey("malicious-drainer");
        let owner = deterministic_pubkey("user-wallet");
        let ctx = ctx_with(trace_with_calls(vec![approve_call(
            1, source, delegate, owner, u64::MAX,
        )]));
        let out = ApprovalAbuseChecker::new().check(&ctx).await;
        assert!(!out.passed);
        assert_eq!(out.severity, Severity::High);
        assert_eq!(out.flags.len(), 1);
        let flag = &out.flags[0];
        assert_eq!(flag.code, FLAG_UNLIMITED_APPROVAL_UNKNOWN_PROGRAM);
        assert_eq!(flag.data["in_known_registry"], json!(false));
        assert_eq!(flag.data["amount"], json!(u64::MAX.to_string()));
        assert_eq!(flag.data["delegate"], json!(delegate.to_string()));
        assert_eq!(flag.data["via_cpi"], json!(false));
        assert_eq!(flag.data["discriminator"], json!(SPL_TOKEN_DISC_APPROVE));
        assert_eq!(flag.data["spl_program_name"], json!("SPL Token"));
        // plain Approve has no mint -> null
        assert!(flag.data["token_mint"].is_null());
    }

    #[tokio::test]
    async fn test_limited_approve_unknown_program_passes() {
        // Verification step 3.
        let source = deterministic_pubkey("source-ata-2");
        let delegate = deterministic_pubkey("unknown-delegate-limited");
        let owner = deterministic_pubkey("user-wallet");
        let ctx = ctx_with(trace_with_calls(vec![approve_call(
            1, source, delegate, owner, 1000,
        )]));
        let out = ApprovalAbuseChecker::new().check(&ctx).await;
        assert!(out.passed);
        assert_eq!(out.severity, Severity::None);
        assert!(out.flags.is_empty());
    }

    #[tokio::test]
    async fn test_unlimited_approve_known_program_passes() {
        // Verification step 4.
        let source = deterministic_pubkey("source-ata-3");
        let owner = deterministic_pubkey("user-wallet");
        let ctx = ctx_with(trace_with_calls(vec![approve_call(
            1,
            source,
            JUPITER_V6_PROGRAM_ID,
            owner,
            u64::MAX,
        )]));
        let out = ApprovalAbuseChecker::new().check(&ctx).await;
        assert!(out.passed);
        assert_eq!(out.severity, Severity::None);
        assert!(out.flags.is_empty());
    }

    // -----------------------------------------------------------------------
    // 2. ApproveChecked path + Token-2022
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_approve_checked_unlimited_unknown_flags() {
        let source = deterministic_pubkey("source-ata-ac");
        let mint = deterministic_pubkey("usdc-mint");
        let delegate = deterministic_pubkey("unknown-drainer-ac");
        let owner = deterministic_pubkey("user-wallet");
        let ctx = ctx_with(trace_with_calls(vec![approve_checked_call(
            1,
            source,
            mint,
            delegate,
            owner,
            u64::MAX,
        )]));
        let out = ApprovalAbuseChecker::new().check(&ctx).await;
        assert!(!out.passed);
        assert_eq!(out.flags.len(), 1);
        let flag = &out.flags[0];
        assert_eq!(flag.data["discriminator"], json!(SPL_TOKEN_DISC_APPROVE_CHECKED));
        assert_eq!(flag.data["token_mint"], json!(mint.to_string()));
        assert_eq!(flag.data["delegate"], json!(delegate.to_string()));
    }

    #[tokio::test]
    async fn test_token_2022_approval_flagged() {
        let source = deterministic_pubkey("t22-source");
        let delegate = deterministic_pubkey("t22-drainer");
        let owner = deterministic_pubkey("t22-owner");
        let ctx = ctx_with(trace_with_calls(vec![approve_call_on(
            SPL_TOKEN_2022_PROGRAM_ID,
            1,
            0,
            source,
            delegate,
            owner,
            u64::MAX,
        )]));
        let out = ApprovalAbuseChecker::new().check(&ctx).await;
        assert!(!out.passed);
        assert_eq!(out.flags.len(), 1);
        assert_eq!(out.flags[0].data["spl_program_name"], json!("SPL Token-2022"));
    }

    // -----------------------------------------------------------------------
    // 3. Revoke-suppression (spec 4.3.6 step 5)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_approve_followed_by_revoke_suppresses_flag() {
        let source = deterministic_pubkey("rvk-source");
        let delegate = deterministic_pubkey("rvk-unknown-delegate");
        let owner = deterministic_pubkey("rvk-owner");
        let ctx = ctx_with(trace_with_calls(vec![
            approve_call(1, source, delegate, owner, u64::MAX),
            revoke_call(1, 1, source, owner),
        ]));
        let out = ApprovalAbuseChecker::new().check(&ctx).await;
        assert!(out.passed, "approve followed by revoke on same source should be suppressed");
        assert_eq!(out.severity, Severity::None);
        assert!(out.flags.is_empty());
    }

    #[tokio::test]
    async fn test_revoke_before_approve_does_not_suppress() {
        let source = deterministic_pubkey("rvk-before-source");
        let delegate = deterministic_pubkey("rvk-before-delegate");
        let owner = deterministic_pubkey("rvk-before-owner");
        let ctx = ctx_with(trace_with_calls(vec![
            revoke_call(1, 0, source, owner),
            approve_call_on(
                SPL_TOKEN_PROGRAM_ID,
                1,
                1,
                source,
                delegate,
                owner,
                u64::MAX,
            ),
        ]));
        let out = ApprovalAbuseChecker::new().check(&ctx).await;
        assert!(!out.passed);
        assert_eq!(out.flags.len(), 1);
    }

    #[tokio::test]
    async fn test_revoke_on_different_source_does_not_suppress() {
        let source_a = deterministic_pubkey("multi-src-A");
        let source_b = deterministic_pubkey("multi-src-B");
        let delegate = deterministic_pubkey("multi-delegate");
        let owner = deterministic_pubkey("multi-owner");
        let ctx = ctx_with(trace_with_calls(vec![
            approve_call(1, source_a, delegate, owner, u64::MAX),
            revoke_call(1, 1, source_b, owner),
        ]));
        let out = ApprovalAbuseChecker::new().check(&ctx).await;
        assert!(!out.passed);
        assert_eq!(out.flags.len(), 1);
    }

    // -----------------------------------------------------------------------
    // 4. CPI-Guard-aligned `via_cpi` tagging
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_via_cpi_flag_set_when_nested() {
        let source = deterministic_pubkey("cpi-source");
        let delegate = deterministic_pubkey("cpi-drainer");
        let owner = deterministic_pubkey("cpi-owner");
        let ctx = ctx_with(trace_with_calls(vec![approve_call(
            2, source, delegate, owner, u64::MAX,
        )]));
        let out = ApprovalAbuseChecker::new().check(&ctx).await;
        assert!(!out.passed);
        assert_eq!(out.flags.len(), 1);
        assert_eq!(out.flags[0].data["via_cpi"], json!(true));
        assert_eq!(out.flags[0].data["stack_height"], json!(2));
    }

    // -----------------------------------------------------------------------
    // 5. Robustness / negative paths
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_non_token_program_with_same_disc_ignored() {
        let source = deterministic_pubkey("nt-source");
        let delegate = deterministic_pubkey("nt-delegate");
        let owner = deterministic_pubkey("nt-owner");
        let random_program = deterministic_pubkey("some-random-program");
        let ctx = ctx_with(trace_with_calls(vec![approve_call_on(
            random_program,
            1,
            0,
            source,
            delegate,
            owner,
            u64::MAX,
        )]));
        let out = ApprovalAbuseChecker::new().check(&ctx).await;
        assert!(out.passed);
        assert!(out.flags.is_empty());
    }

    #[tokio::test]
    async fn test_malformed_data_does_not_panic() {
        // Approve discriminator with no amount bytes.
        let call = CpiCall {
            program_id: SPL_TOKEN_PROGRAM_ID,
            instruction_index: 0,
            stack_height: 1,
            accounts: vec![deterministic_pubkey("src"), deterministic_pubkey("dlg")],
            data: vec![SPL_TOKEN_DISC_APPROVE],
        };
        let ctx = ctx_with(trace_with_calls(vec![call]));
        let out = ApprovalAbuseChecker::new().check(&ctx).await;
        assert!(out.passed);
        assert!(out.flags.is_empty());
    }

    #[tokio::test]
    async fn test_multiple_approvals_only_unlimited_unknown_flagged() {
        let owner = deterministic_pubkey("multi-u-owner");
        let ctx = ctx_with(trace_with_calls(vec![
            approve_call(
                1,
                deterministic_pubkey("multi-u-srcA"),
                deterministic_pubkey("multi-u-unknownA"),
                owner,
                u64::MAX,
            ),
            approve_call(
                1,
                deterministic_pubkey("multi-u-srcB"),
                deterministic_pubkey("multi-u-unknownB"),
                owner,
                1000,
            ),
            approve_call(
                1,
                deterministic_pubkey("multi-u-srcC"),
                JUPITER_V6_PROGRAM_ID,
                owner,
                u64::MAX,
            ),
        ]));
        let out = ApprovalAbuseChecker::new().check(&ctx).await;
        assert!(!out.passed);
        assert_eq!(out.flags.len(), 1);
        assert_eq!(out.severity, Severity::High);
    }

    #[tokio::test]
    async fn test_deterministic_output() {
        let source = deterministic_pubkey("det-source");
        let delegate = deterministic_pubkey("det-delegate");
        let owner = deterministic_pubkey("det-owner");
        let trace = trace_with_calls(vec![approve_call(1, source, delegate, owner, u64::MAX)]);
        let ctx = ctx_with(trace);
        let checker = ApprovalAbuseChecker::new();

        let out1 = checker.check(&ctx).await;
        let out2 = checker.check(&ctx).await;

        let bytes1 = to_vec(&out1).expect("serialize out1");
        let bytes2 = to_vec(&out2).expect("serialize out2");
        assert_eq!(bytes1, bytes2, "Checker output must be byte-deterministic");
    }
}
