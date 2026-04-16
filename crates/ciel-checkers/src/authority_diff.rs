// Authority Diff checker. See docs/11-authority-diff-checker.md (Unit 11) and
// spec Section 4.3.2.
//
// Scans the CPI call graph for hidden authority changes: SPL Token SetAuthority
// (disc 6), CloseAccount (disc 9), InitializeAccount (1/16/18); BPF Loader
// Upgradeable Upgrade (3), SetAuthority (4), Close (5), SetAuthorityChecked (7).
// Also flags Squads v4 CPIs whose accounts list carries a known protocol
// program ID — the fallback shape of the Apr 2026 Drift admin handoff.
//
// Flag code vocabulary aligns with SPL Token-2022 CPI Guard:
// `{PROG}_{OP}_IN_CPI`. See memory/project_cpi_guard_vocabulary.md.

use async_trait::async_trait;
use serde_json::json;

use crate::program_registry::{
    BPF_LOADER_UPGRADEABLE_PROGRAM_ID, SPL_TOKEN_2022_PROGRAM_ID, SPL_TOKEN_PROGRAM_ID,
    SQUADS_V4_PROGRAM_ID,
};
use crate::traits::{Checker, CheckerContext, CheckerOutput, Flag, ProgramRegistry, Severity};
use ciel_fork::trace::CpiCall;

// ---------------------------------------------------------------------------
// Flag codes
// ---------------------------------------------------------------------------

pub const FLAG_SPL_SET_AUTHORITY_IN_CPI: &str = "SPL_SET_AUTHORITY_IN_CPI";
pub const FLAG_SPL_CLOSE_ACCOUNT_IN_CPI: &str = "SPL_CLOSE_ACCOUNT_IN_CPI";
pub const FLAG_SPL_INIT_ACCOUNT_IN_CPI: &str = "SPL_INIT_ACCOUNT_IN_CPI";
pub const FLAG_BPF_UPGRADE_IN_CPI: &str = "BPF_UPGRADE_IN_CPI";
pub const FLAG_BPF_SET_AUTHORITY_IN_CPI: &str = "BPF_SET_AUTHORITY_IN_CPI";
pub const FLAG_BPF_CLOSE_IN_CPI: &str = "BPF_CLOSE_IN_CPI";
pub const FLAG_MULTISIG_ADMIN_HANDOFF_CANDIDATE: &str = "MULTISIG_ADMIN_HANDOFF_CANDIDATE";

// ---------------------------------------------------------------------------
// Instruction discriminators
// ---------------------------------------------------------------------------

// SPL Token / Token-2022 (implicit ordinals — verified against
// solana-program/token and solana-program/token-2022).
const SPL_TOKEN_DISC_INITIALIZE_ACCOUNT: u8 = 1;
const SPL_TOKEN_DISC_SET_AUTHORITY: u8 = 6;
const SPL_TOKEN_DISC_CLOSE_ACCOUNT: u8 = 9;
const SPL_TOKEN_DISC_INITIALIZE_ACCOUNT_2: u8 = 16;
const SPL_TOKEN_DISC_INITIALIZE_ACCOUNT_3: u8 = 18;

// BPF Loader Upgradeable. Wire format is bincode (u32 LE enum tag); for tags
// 0–7 the byte-0 check is sufficient and matches the upstream SDK helpers
// `is_upgrade_instruction` / `is_set_authority_instruction` / etc.
const BPF_DISC_UPGRADE: u8 = 3;
const BPF_DISC_SET_AUTHORITY: u8 = 4;
const BPF_DISC_CLOSE: u8 = 5;
const BPF_DISC_SET_AUTHORITY_CHECKED: u8 = 7;

// System program AdvanceNonceAccount (durable-nonce precursor).
const SYSTEM_DISC_ADVANCE_NONCE_ACCOUNT: u8 = 4;

// ---------------------------------------------------------------------------
// Checker
// ---------------------------------------------------------------------------

/// Authority Diff checker. See spec Section 4.3.2.
pub struct AuthorityDiffChecker;

impl AuthorityDiffChecker {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AuthorityDiffChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Checker for AuthorityDiffChecker {
    fn name(&self) -> &'static str {
        "authority_diff"
    }

    async fn check(&self, ctx: &CheckerContext) -> CheckerOutput {
        let cpi_graph = &ctx.trace.cpi_graph;
        if cpi_graph.is_empty() {
            return CheckerOutput {
                checker_name: "authority_diff".to_string(),
                passed: true,
                severity: Severity::None,
                flags: vec![],
                details: "No authority-change instructions detected".to_string(),
            };
        }

        let durable_nonce_precursor = has_durable_nonce_precursor(ctx);
        let mut flags: Vec<Flag> = Vec::new();

        for call in cpi_graph {
            if let Some(mut flag) = match_call(call, &ctx.known_programs) {
                let base = flag_severity(&flag.code);
                let mut sev = base;

                let known_protocol = lookup_known_protocol_in_call(call, &ctx.known_programs);
                if known_protocol.is_some() {
                    sev = Severity::Critical;
                }

                sev = modulate_by_stack_height(sev, call.stack_height);

                if durable_nonce_precursor {
                    sev = shift_severity(sev, 1);
                }

                if let Some(name) = known_protocol {
                    merge_field(&mut flag.data, "known_protocol_name", json!(name));
                }
                if durable_nonce_precursor {
                    merge_field(&mut flag.data, "durable_nonce_precursor", json!(true));
                }
                merge_field(&mut flag.data, "computed_severity", json!(severity_u8(sev)));

                flags.push(flag);
            }
        }

        // Intent acknowledgement pass.
        if let Some(intent) = ctx.intent.as_ref() {
            for flag in flags.iter_mut() {
                if intent_acknowledges(intent, &flag.code) {
                    let has_known_protocol = flag
                        .data
                        .get("known_protocol_name")
                        .map(|v| !v.is_null())
                        .unwrap_or(false);
                    let cur = recover_severity(&flag.data);
                    let floor = if has_known_protocol {
                        Severity::High
                    } else {
                        Severity::Low
                    };
                    let downgraded = shift_severity_with_floor(cur, -1, floor);
                    merge_field(
                        &mut flag.data,
                        "computed_severity",
                        json!(severity_u8(downgraded)),
                    );
                    merge_field(&mut flag.data, "intent_ack", json!(true));
                }
            }
        }

        let severity = flags
            .iter()
            .map(|f| recover_severity(&f.data))
            .max()
            .unwrap_or(Severity::None);

        // Strip the internal scratch field from public output.
        for flag in flags.iter_mut() {
            if let serde_json::Value::Object(map) = &mut flag.data {
                map.remove("computed_severity");
            }
        }

        let passed = flags.is_empty();
        let details = if passed {
            "No authority-change instructions detected".to_string()
        } else {
            format!("{} authority-change instruction(s) detected", flags.len())
        };

        CheckerOutput {
            checker_name: "authority_diff".to_string(),
            passed,
            severity,
            flags,
            details,
        }
    }
}

// ---------------------------------------------------------------------------
// Per-call matching
// ---------------------------------------------------------------------------

fn match_call(call: &CpiCall, registry: &ProgramRegistry) -> Option<Flag> {
    if call.program_id == SPL_TOKEN_PROGRAM_ID || call.program_id == SPL_TOKEN_2022_PROGRAM_ID {
        return match_spl_token(call);
    }
    if call.program_id == BPF_LOADER_UPGRADEABLE_PROGRAM_ID {
        return match_bpf_loader(call);
    }
    if call.program_id == SQUADS_V4_PROGRAM_ID {
        return match_squads_proxy(call, registry);
    }
    None
}

fn match_spl_token(call: &CpiCall) -> Option<Flag> {
    let disc = *call.data.first()?;
    match disc {
        SPL_TOKEN_DISC_SET_AUTHORITY => {
            let target = call.accounts.first().copied();
            let authority_type = call.data.get(1).copied();
            let authority_type_name = authority_type.map(authority_type_name);
            Some(Flag {
                code: FLAG_SPL_SET_AUTHORITY_IN_CPI.to_string(),
                message: format!(
                    "SPL Token SetAuthority at instruction {}, stack_height {}",
                    call.instruction_index, call.stack_height
                ),
                data: json!({
                    "instruction_index": call.instruction_index,
                    "stack_height": call.stack_height,
                    "program_id": call.program_id.to_string(),
                    "target_account": target.map(|p| p.to_string()),
                    "authority_type": authority_type,
                    "authority_type_name": authority_type_name,
                }),
            })
        }
        SPL_TOKEN_DISC_CLOSE_ACCOUNT => {
            let target = call.accounts.first().copied();
            Some(Flag {
                code: FLAG_SPL_CLOSE_ACCOUNT_IN_CPI.to_string(),
                message: format!(
                    "SPL Token CloseAccount at instruction {}, stack_height {}",
                    call.instruction_index, call.stack_height
                ),
                data: json!({
                    "instruction_index": call.instruction_index,
                    "stack_height": call.stack_height,
                    "program_id": call.program_id.to_string(),
                    "target_account": target.map(|p| p.to_string()),
                }),
            })
        }
        SPL_TOKEN_DISC_INITIALIZE_ACCOUNT
        | SPL_TOKEN_DISC_INITIALIZE_ACCOUNT_2
        | SPL_TOKEN_DISC_INITIALIZE_ACCOUNT_3 => {
            let target = call.accounts.first().copied();
            Some(Flag {
                code: FLAG_SPL_INIT_ACCOUNT_IN_CPI.to_string(),
                message: format!(
                    "SPL Token InitializeAccount (disc {}) at instruction {}, stack_height {}",
                    disc, call.instruction_index, call.stack_height
                ),
                data: json!({
                    "instruction_index": call.instruction_index,
                    "stack_height": call.stack_height,
                    "program_id": call.program_id.to_string(),
                    "target_account": target.map(|p| p.to_string()),
                }),
            })
        }
        _ => None,
    }
}

fn match_bpf_loader(call: &CpiCall) -> Option<Flag> {
    let disc = *call.data.first()?;
    match disc {
        BPF_DISC_UPGRADE => {
            let target = call.accounts.get(1).copied();
            Some(Flag {
                code: FLAG_BPF_UPGRADE_IN_CPI.to_string(),
                message: format!(
                    "BPF Loader Upgrade at instruction {}, stack_height {}",
                    call.instruction_index, call.stack_height
                ),
                data: json!({
                    "instruction_index": call.instruction_index,
                    "stack_height": call.stack_height,
                    "program_id": call.program_id.to_string(),
                    "target_account": target.map(|p| p.to_string()),
                }),
            })
        }
        BPF_DISC_SET_AUTHORITY | BPF_DISC_SET_AUTHORITY_CHECKED => {
            let target = call.accounts.first().copied();
            Some(Flag {
                code: FLAG_BPF_SET_AUTHORITY_IN_CPI.to_string(),
                message: format!(
                    "BPF Loader SetAuthority{} at instruction {}, stack_height {}",
                    if disc == BPF_DISC_SET_AUTHORITY_CHECKED {
                        "Checked"
                    } else {
                        ""
                    },
                    call.instruction_index,
                    call.stack_height
                ),
                data: json!({
                    "instruction_index": call.instruction_index,
                    "stack_height": call.stack_height,
                    "program_id": call.program_id.to_string(),
                    "target_account": target.map(|p| p.to_string()),
                    "checked": disc == BPF_DISC_SET_AUTHORITY_CHECKED,
                }),
            })
        }
        BPF_DISC_CLOSE => {
            let target = call.accounts.first().copied();
            Some(Flag {
                code: FLAG_BPF_CLOSE_IN_CPI.to_string(),
                message: format!(
                    "BPF Loader Close at instruction {}, stack_height {}",
                    call.instruction_index, call.stack_height
                ),
                data: json!({
                    "instruction_index": call.instruction_index,
                    "stack_height": call.stack_height,
                    "program_id": call.program_id.to_string(),
                    "target_account": target.map(|p| p.to_string()),
                }),
            })
        }
        _ => None,
    }
}

fn match_squads_proxy(call: &CpiCall, registry: &ProgramRegistry) -> Option<Flag> {
    // Squads v4 VaultTransactionExecute carries the inner program in its
    // accounts list (remaining_accounts after the fixed positions). If any
    // registry-known protocol appears there, flag the outer call as a
    // multisig admin-handoff candidate — the fallback path for the Apr 2026
    // Drift exploit where the inner UpdateAdmin CPI is Anchor-shaped and
    // not detectable by generic discriminator match. Squads itself is in
    // the registry; exclude it to avoid self-matching.
    let target = call
        .accounts
        .iter()
        .find(|pk| **pk != SQUADS_V4_PROGRAM_ID && registry.is_known_protocol(pk).is_some())
        .copied()?;
    let protocol_name = registry.is_known_protocol(&target)?;
    Some(Flag {
        code: FLAG_MULTISIG_ADMIN_HANDOFF_CANDIDATE.to_string(),
        message: format!(
            "Squads multisig CPI carries known protocol {} in accounts at instruction {}",
            protocol_name, call.instruction_index
        ),
        data: json!({
            "instruction_index": call.instruction_index,
            "stack_height": call.stack_height,
            "program_id": call.program_id.to_string(),
            "target_account": target.to_string(),
            "target_protocol_name": protocol_name,
        }),
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn flag_severity(code: &str) -> Severity {
    match code {
        FLAG_SPL_SET_AUTHORITY_IN_CPI => Severity::High,
        FLAG_SPL_CLOSE_ACCOUNT_IN_CPI => Severity::Medium,
        FLAG_SPL_INIT_ACCOUNT_IN_CPI => Severity::Low,
        FLAG_BPF_UPGRADE_IN_CPI => Severity::Critical,
        FLAG_BPF_SET_AUTHORITY_IN_CPI => Severity::High,
        FLAG_BPF_CLOSE_IN_CPI => Severity::High,
        FLAG_MULTISIG_ADMIN_HANDOFF_CANDIDATE => Severity::Medium,
        _ => Severity::Low,
    }
}

fn lookup_known_protocol_in_call<'r>(
    call: &CpiCall,
    registry: &'r ProgramRegistry,
) -> Option<&'r str> {
    // Search all accounts in the call — broader than "target only", catches
    // cases where the modified account is a PDA owned by a protocol but the
    // protocol program itself is also present in the call's account list.
    call.accounts
        .iter()
        .find_map(|pk| registry.is_known_protocol(pk))
}

fn modulate_by_stack_height(sev: Severity, stack_height: u8) -> Severity {
    match stack_height {
        0 | 1 => shift_severity_with_floor(sev, -1, Severity::Low),
        2 => sev,
        _ => shift_severity(sev, 1),
    }
}

fn severity_u8(s: Severity) -> u8 {
    match s {
        Severity::None => 0,
        Severity::Low => 1,
        Severity::Medium => 2,
        Severity::High => 3,
        Severity::Critical => 4,
    }
}

fn severity_from_u8(n: u8) -> Severity {
    match n {
        0 => Severity::None,
        1 => Severity::Low,
        2 => Severity::Medium,
        3 => Severity::High,
        _ => Severity::Critical,
    }
}

fn shift_severity(s: Severity, delta: i8) -> Severity {
    let n = severity_u8(s) as i8 + delta;
    let clamped = n.clamp(0, 4) as u8;
    severity_from_u8(clamped)
}

fn shift_severity_with_floor(s: Severity, delta: i8, floor: Severity) -> Severity {
    let shifted = shift_severity(s, delta);
    if severity_u8(shifted) < severity_u8(floor) {
        floor
    } else {
        shifted
    }
}

fn recover_severity(data: &serde_json::Value) -> Severity {
    data.get("computed_severity")
        .and_then(|v| v.as_u64())
        .map(|n| severity_from_u8(n as u8))
        .unwrap_or(Severity::None)
}

fn merge_field(data: &mut serde_json::Value, key: &str, value: serde_json::Value) {
    if let serde_json::Value::Object(map) = data {
        map.insert(key.to_string(), value);
    }
}

fn authority_type_name(byte: u8) -> String {
    match byte {
        0 => "MintTokens".to_string(),
        1 => "FreezeAccount".to_string(),
        2 => "AccountOwner".to_string(),
        3 => "CloseAccount".to_string(),
        // Token-2022 extensions.
        4 => "TransferFeeConfig".to_string(),
        5 => "WithheldWithdraw".to_string(),
        6 => "CloseMint".to_string(),
        7 => "InterestRate".to_string(),
        8 => "PermanentDelegate".to_string(),
        9 => "ConfidentialTransferMint".to_string(),
        10 => "TransferHookProgramId".to_string(),
        11 => "ConfidentialTransferFeeConfig".to_string(),
        12 => "MetadataPointer".to_string(),
        13 => "GroupPointer".to_string(),
        14 => "GroupMemberPointer".to_string(),
        15 => "ScaledUiAmount".to_string(),
        16 => "Pause".to_string(),
        17 => "PermissionedBurn".to_string(),
        n => format!("Unknown({n})"),
    }
}

fn has_durable_nonce_precursor(ctx: &CheckerContext) -> bool {
    let msg = &ctx.original_tx.message;
    let first = match msg.instructions.first() {
        Some(i) => i,
        None => return false,
    };
    let program_id = match msg.account_keys.get(first.program_id_index as usize) {
        Some(p) => p,
        None => return false,
    };
    *program_id == solana_sdk::system_program::ID
        && first.data.first() == Some(&SYSTEM_DISC_ADVANCE_NONCE_ACCOUNT)
}

// ---------------------------------------------------------------------------
// Intent heuristic (keyword-based, deterministic)
// ---------------------------------------------------------------------------
//
// Known risk (tracked for Unit 10 hardening): keyword-only acknowledgement
// lets a user type e.g. `"upgrade"` anywhere in their intent to silence every
// BPF_UPGRADE_IN_CPI flag. Intent downgrades are capped at one level and
// floored at High when a known protocol is involved — defense-in-depth until
// Intent carries structured `expected_authorities[]`.

fn intent_acknowledges(intent: &crate::traits::Intent, flag_code: &str) -> bool {
    let text = std::iter::once(intent.description.as_str())
        .chain(intent.constraints.iter().map(|s| s.as_str()))
        .collect::<Vec<_>>()
        .join(" ");
    let tokens = tokenize_lowercase(&text);
    keywords_for(flag_code)
        .iter()
        .any(|kw| all_tokens_present(kw, &tokens))
}

fn tokenize_lowercase(text: &str) -> Vec<String> {
    text.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .map(String::from)
        .collect()
}

fn all_tokens_present(keyword: &str, intent_tokens: &[String]) -> bool {
    let required: Vec<String> = keyword
        .split_whitespace()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    required
        .iter()
        .all(|r| intent_tokens.iter().any(|t| t == r))
}

fn keywords_for(flag_code: &str) -> &'static [&'static str] {
    match flag_code {
        FLAG_SPL_SET_AUTHORITY_IN_CPI | FLAG_BPF_SET_AUTHORITY_IN_CPI => &[
            "set authority",
            "setauthority",
            "authority change",
            "transfer authority",
            "update admin",
            "updateadmin",
            "rotate authority",
            "change authority",
        ],
        FLAG_BPF_UPGRADE_IN_CPI => &["upgrade", "deploy", "redeploy"],
        FLAG_SPL_CLOSE_ACCOUNT_IN_CPI | FLAG_BPF_CLOSE_IN_CPI => &["close account", "close"],
        FLAG_SPL_INIT_ACCOUNT_IN_CPI => &["initialize", "init account", "create account"],
        FLAG_MULTISIG_ADMIN_HANDOFF_CANDIDATE => &["multisig", "squads", "governance proposal"],
        _ => &[],
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle_cache::OracleCache;
    use crate::traits::Intent;
    use borsh::{from_slice, to_vec};
    use ciel_fork::SimulationTrace;
    use solana_sdk::hash::hash;
    use solana_sdk::instruction::{AccountMeta, Instruction};
    use solana_sdk::message::Message;
    use solana_sdk::pubkey::Pubkey;
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

    fn ctx_with_intent(trace: SimulationTrace, intent: Intent) -> CheckerContext {
        let mut c = ctx_with(trace);
        c.intent = Some(intent);
        c
    }

    fn ctx_with_durable_nonce(trace: SimulationTrace) -> CheckerContext {
        let mut c = ctx_with(trace);
        let nonce_account = Pubkey::new_unique();
        let authority = Pubkey::new_unique();
        let advance_nonce_ix = Instruction::new_with_bincode(
            solana_sdk::system_program::ID,
            &(SYSTEM_DISC_ADVANCE_NONCE_ACCOUNT as u32),
            vec![
                AccountMeta::new(nonce_account, false),
                AccountMeta::new_readonly(
                    solana_sdk::sysvar::recent_blockhashes::ID,
                    false,
                ),
                AccountMeta::new_readonly(authority, true),
            ],
        );
        let msg = Message::new(&[advance_nonce_ix], Some(&authority));
        c.original_tx = Transaction::new_unsigned(msg);
        c
    }

    fn spl_set_authority_call(
        stack_height: u8,
        target: Pubkey,
        extra_accounts: &[Pubkey],
        authority_type: u8,
    ) -> CpiCall {
        let mut accounts = vec![target];
        accounts.extend_from_slice(extra_accounts);
        // SPL Token SetAuthority data: disc(6) + authority_type(u8) + Option<Pubkey>(None=0 byte).
        let data = vec![SPL_TOKEN_DISC_SET_AUTHORITY, authority_type, 0];
        CpiCall {
            program_id: SPL_TOKEN_PROGRAM_ID,
            instruction_index: 0,
            stack_height,
            accounts,
            data,
        }
    }

    fn spl_close_call(stack_height: u8) -> CpiCall {
        CpiCall {
            program_id: SPL_TOKEN_PROGRAM_ID,
            instruction_index: 0,
            stack_height,
            accounts: vec![Pubkey::new_unique(), Pubkey::new_unique()],
            data: vec![SPL_TOKEN_DISC_CLOSE_ACCOUNT],
        }
    }

    fn spl_init_call(stack_height: u8, disc: u8) -> CpiCall {
        CpiCall {
            program_id: SPL_TOKEN_PROGRAM_ID,
            instruction_index: 0,
            stack_height,
            accounts: vec![Pubkey::new_unique()],
            data: vec![disc],
        }
    }

    fn bpf_call(stack_height: u8, disc: u8) -> CpiCall {
        CpiCall {
            program_id: BPF_LOADER_UPGRADEABLE_PROGRAM_ID,
            instruction_index: 0,
            stack_height,
            accounts: vec![Pubkey::new_unique(), Pubkey::new_unique()],
            data: vec![disc, 0, 0, 0],
        }
    }

    fn squads_proxy_call(inner_program: Pubkey) -> CpiCall {
        CpiCall {
            program_id: SQUADS_V4_PROGRAM_ID,
            instruction_index: 0,
            stack_height: 1,
            accounts: vec![
                Pubkey::new_unique(),
                Pubkey::new_unique(),
                inner_program,
                Pubkey::new_unique(),
            ],
            data: vec![0xaa, 0xbb],
        }
    }

    // -----------------------------------------------------------------------
    // 1. Core detection
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_clean_sol_transfer_passes() {
        let ctx = ctx_with(empty_trace());
        let out = AuthorityDiffChecker::new().check(&ctx).await;
        assert!(out.passed);
        assert_eq!(out.severity, Severity::None);
        assert!(out.flags.is_empty());
    }

    #[tokio::test]
    async fn test_hidden_set_authority_top_level() {
        let target = deterministic_pubkey("target-token-account");
        let ctx = ctx_with(trace_with_calls(vec![spl_set_authority_call(
            1,
            target,
            &[],
            2,
        )]));
        let out = AuthorityDiffChecker::new().check(&ctx).await;
        assert!(!out.passed);
        // High base − 1 (top-level modulation) = Medium.
        assert_eq!(out.severity, Severity::Medium);
        assert_eq!(out.flags.len(), 1);
        assert_eq!(out.flags[0].code, FLAG_SPL_SET_AUTHORITY_IN_CPI);
    }

    #[tokio::test]
    async fn test_hidden_set_authority_nested_cpi() {
        let target = deterministic_pubkey("nested-target");
        let ctx = ctx_with(trace_with_calls(vec![spl_set_authority_call(
            3,
            target,
            &[],
            2,
        )]));
        let out = AuthorityDiffChecker::new().check(&ctx).await;
        assert!(!out.passed);
        // High base + 1 (stack_height≥3) = Critical (cap).
        assert_eq!(out.severity, Severity::Critical);
    }

    #[tokio::test]
    async fn test_set_authority_on_known_protocol() {
        use crate::program_registry::DRIFT_V2_PROGRAM_ID;
        let target = deterministic_pubkey("drift-vault");
        let ctx = ctx_with(trace_with_calls(vec![spl_set_authority_call(
            2,
            target,
            &[DRIFT_V2_PROGRAM_ID],
            2,
        )]));
        let out = AuthorityDiffChecker::new().check(&ctx).await;
        assert!(!out.passed);
        assert_eq!(out.severity, Severity::Critical);
        let name = out.flags[0]
            .data
            .get("known_protocol_name")
            .and_then(|v| v.as_str())
            .unwrap();
        assert_eq!(name, "Drift v2");
    }

    #[tokio::test]
    async fn test_spl_token_close_account() {
        let ctx = ctx_with(trace_with_calls(vec![spl_close_call(2)]));
        let out = AuthorityDiffChecker::new().check(&ctx).await;
        assert!(!out.passed);
        assert_eq!(out.severity, Severity::Medium);
        assert_eq!(out.flags[0].code, FLAG_SPL_CLOSE_ACCOUNT_IN_CPI);
    }

    #[tokio::test]
    async fn test_spl_token_initialize_account_low() {
        let ctx = ctx_with(trace_with_calls(vec![spl_init_call(
            2,
            SPL_TOKEN_DISC_INITIALIZE_ACCOUNT,
        )]));
        let out = AuthorityDiffChecker::new().check(&ctx).await;
        assert!(!out.passed);
        assert_eq!(out.severity, Severity::Low);
        assert_eq!(out.flags[0].code, FLAG_SPL_INIT_ACCOUNT_IN_CPI);
    }

    #[tokio::test]
    async fn test_spl_token_non_matching_discriminator() {
        let transfer_call = CpiCall {
            program_id: SPL_TOKEN_PROGRAM_ID,
            instruction_index: 0,
            stack_height: 2,
            accounts: vec![Pubkey::new_unique(), Pubkey::new_unique()],
            data: vec![3, 0, 0, 0, 0, 0, 0, 0, 0], // Transfer
        };
        let ctx = ctx_with(trace_with_calls(vec![transfer_call]));
        let out = AuthorityDiffChecker::new().check(&ctx).await;
        assert!(out.passed);
        assert!(out.flags.is_empty());
    }

    // -----------------------------------------------------------------------
    // 2. Token-2022
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_token_2022_set_authority_extension_variant() {
        let target = deterministic_pubkey("t22-mint");
        let call = CpiCall {
            program_id: SPL_TOKEN_2022_PROGRAM_ID,
            instruction_index: 0,
            stack_height: 2,
            accounts: vec![target],
            // authority_type 8 = PermanentDelegate
            data: vec![SPL_TOKEN_DISC_SET_AUTHORITY, 8, 0],
        };
        let ctx = ctx_with(trace_with_calls(vec![call]));
        let out = AuthorityDiffChecker::new().check(&ctx).await;
        assert!(!out.passed);
        let name = out.flags[0]
            .data
            .get("authority_type_name")
            .and_then(|v| v.as_str())
            .unwrap();
        assert_eq!(name, "PermanentDelegate");
    }

    // -----------------------------------------------------------------------
    // 3. BPF Loader
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_bpf_upgrade_detected() {
        let ctx = ctx_with(trace_with_calls(vec![bpf_call(2, BPF_DISC_UPGRADE)]));
        let out = AuthorityDiffChecker::new().check(&ctx).await;
        assert!(!out.passed);
        assert_eq!(out.severity, Severity::Critical);
        assert_eq!(out.flags[0].code, FLAG_BPF_UPGRADE_IN_CPI);
    }

    #[tokio::test]
    async fn test_bpf_set_authority_checked_detected() {
        let ctx = ctx_with(trace_with_calls(vec![bpf_call(
            2,
            BPF_DISC_SET_AUTHORITY_CHECKED,
        )]));
        let out = AuthorityDiffChecker::new().check(&ctx).await;
        assert!(!out.passed);
        assert_eq!(out.flags[0].code, FLAG_BPF_SET_AUTHORITY_IN_CPI);
        assert_eq!(
            out.flags[0].data.get("checked").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    // -----------------------------------------------------------------------
    // 4. Squads proxy
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_squads_admin_proxy() {
        use crate::program_registry::DRIFT_V2_PROGRAM_ID;
        let ctx = ctx_with(trace_with_calls(vec![squads_proxy_call(
            DRIFT_V2_PROGRAM_ID,
        )]));
        let out = AuthorityDiffChecker::new().check(&ctx).await;
        assert!(!out.passed);
        assert_eq!(out.flags[0].code, FLAG_MULTISIG_ADMIN_HANDOFF_CANDIDATE);
        // stack_height=1 (top-level Squads call) + Drift in accounts:
        // Medium base → known-protocol bump → Critical → top-level modulation
        // -1 → High.
        assert_eq!(out.severity, Severity::High);
        let target_protocol = out.flags[0]
            .data
            .get("target_protocol_name")
            .and_then(|v| v.as_str())
            .unwrap();
        assert_eq!(target_protocol, "Drift v2");
    }

    #[tokio::test]
    async fn test_squads_without_known_protocol() {
        let unknown = Pubkey::new_unique();
        let ctx = ctx_with(trace_with_calls(vec![squads_proxy_call(unknown)]));
        let out = AuthorityDiffChecker::new().check(&ctx).await;
        assert!(out.passed);
        assert!(out.flags.is_empty());
    }

    // -----------------------------------------------------------------------
    // 5. Severity modulators
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_durable_nonce_precursor_bumps_severity() {
        let target = deterministic_pubkey("dn-target");
        let ctx = ctx_with_durable_nonce(trace_with_calls(vec![spl_set_authority_call(
            2, target, &[], 2,
        )]));
        let out = AuthorityDiffChecker::new().check(&ctx).await;
        assert!(!out.passed);
        // Base High + 1 (durable-nonce bump) = Critical.
        assert_eq!(out.severity, Severity::Critical);
        assert_eq!(
            out.flags[0]
                .data
                .get("durable_nonce_precursor")
                .and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    // -----------------------------------------------------------------------
    // 6. Defensive / guards
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_empty_instruction_data() {
        let call = CpiCall {
            program_id: SPL_TOKEN_PROGRAM_ID,
            instruction_index: 0,
            stack_height: 2,
            accounts: vec![Pubkey::new_unique()],
            data: vec![],
        };
        let ctx = ctx_with(trace_with_calls(vec![call]));
        let out = AuthorityDiffChecker::new().check(&ctx).await;
        assert!(out.passed);
    }

    #[tokio::test]
    async fn test_bpf_loader_short_data() {
        let call = CpiCall {
            program_id: BPF_LOADER_UPGRADEABLE_PROGRAM_ID,
            instruction_index: 0,
            stack_height: 2,
            accounts: vec![Pubkey::new_unique()],
            data: vec![BPF_DISC_UPGRADE], // 1 byte
        };
        let ctx = ctx_with(trace_with_calls(vec![call]));
        let out = AuthorityDiffChecker::new().check(&ctx).await;
        assert!(!out.passed);
        assert_eq!(out.flags[0].code, FLAG_BPF_UPGRADE_IN_CPI);
    }

    #[tokio::test]
    async fn test_multiple_set_authority_instructions() {
        let t1 = deterministic_pubkey("tgt1");
        let t2 = deterministic_pubkey("tgt2");
        let mut c1 = spl_set_authority_call(2, t1, &[], 2);
        c1.instruction_index = 0;
        let mut c2 = spl_set_authority_call(3, t2, &[], 2);
        c2.instruction_index = 1;
        let ctx = ctx_with(trace_with_calls(vec![c1, c2]));
        let out = AuthorityDiffChecker::new().check(&ctx).await;
        assert_eq!(out.flags.len(), 2);
        assert_eq!(
            out.flags[0]
                .data
                .get("instruction_index")
                .and_then(|v| v.as_u64()),
            Some(0)
        );
        assert_eq!(
            out.flags[1]
                .data
                .get("instruction_index")
                .and_then(|v| v.as_u64()),
            Some(1)
        );
        // c2 at stack_height=3 → Critical (max wins).
        assert_eq!(out.severity, Severity::Critical);
    }

    // -----------------------------------------------------------------------
    // 7. Intent downgrade
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_intent_keyword_downgrades() {
        let target = deterministic_pubkey("int-tgt");
        let intent = Intent {
            description: "rotate authority on admin account".to_string(),
            constraints: vec![],
        };
        let ctx = ctx_with_intent(
            trace_with_calls(vec![spl_set_authority_call(2, target, &[], 2)]),
            intent,
        );
        let out = AuthorityDiffChecker::new().check(&ctx).await;
        assert_eq!(out.severity, Severity::Medium);
        assert_eq!(
            out.flags[0].data.get("intent_ack").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[tokio::test]
    async fn test_intent_substring_does_not_falsely_downgrade() {
        let intent = Intent {
            description: "public disclosure of financials".to_string(),
            constraints: vec![],
        };
        let ctx = ctx_with_intent(trace_with_calls(vec![spl_close_call(2)]), intent);
        let out = AuthorityDiffChecker::new().check(&ctx).await;
        // "disclosure" must NOT match "close". Severity stays Medium, no ack.
        assert_eq!(out.severity, Severity::Medium);
        assert!(out.flags[0].data.get("intent_ack").is_none());
    }

    #[tokio::test]
    async fn test_intent_downgrade_floors_at_high_for_known_protocol() {
        use crate::program_registry::DRIFT_V2_PROGRAM_ID;
        let target = deterministic_pubkey("kp-tgt");
        let intent = Intent {
            description: "set authority on drift admin".to_string(),
            constraints: vec![],
        };
        let ctx = ctx_with_intent(
            trace_with_calls(vec![spl_set_authority_call(
                2,
                target,
                &[DRIFT_V2_PROGRAM_ID],
                2,
            )]),
            intent,
        );
        let out = AuthorityDiffChecker::new().check(&ctx).await;
        // Known protocol → Critical. Intent −1 would be High; floor is High
        // for known-protocol → severity = High.
        assert_eq!(out.severity, Severity::High);
        assert_eq!(
            out.flags[0].data.get("intent_ack").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    // -----------------------------------------------------------------------
    // 8. Contract / determinism
    // -----------------------------------------------------------------------

    #[test]
    fn test_checker_name_matches_stub() {
        assert_eq!(AuthorityDiffChecker::new().name(), "authority_diff");
    }

    #[tokio::test]
    async fn test_flag_data_shape_stable() {
        let target = deterministic_pubkey("shape-tgt");
        let ctx = ctx_with(trace_with_calls(vec![spl_set_authority_call(
            2, target, &[], 2,
        )]));
        let out = AuthorityDiffChecker::new().check(&ctx).await;
        let data = &out.flags[0].data;
        for required_key in ["instruction_index", "stack_height", "program_id", "target_account"] {
            assert!(
                data.get(required_key).is_some(),
                "missing required key: {required_key}"
            );
        }
        assert!(data.get("authority_type").is_some());
        assert!(data.get("authority_type_name").is_some());
        // No known protocol in this test → key should be absent.
        assert!(data.get("known_protocol_name").is_none());
        // Internal scratch field must be stripped.
        assert!(data.get("computed_severity").is_none());
    }

    #[tokio::test]
    async fn test_borsh_roundtrip_on_output() {
        let t1 = deterministic_pubkey("br1");
        let ctx = ctx_with(trace_with_calls(vec![
            spl_set_authority_call(2, t1, &[], 2),
            bpf_call(2, BPF_DISC_UPGRADE),
        ]));
        let out = AuthorityDiffChecker::new().check(&ctx).await;
        let bytes = to_vec(&out).expect("serialize");
        let decoded: CheckerOutput = from_slice(&bytes).expect("deserialize");
        assert_eq!(out, decoded);
    }

    #[tokio::test]
    async fn test_determinism_10_runs() {
        let t = deterministic_pubkey("det-tgt");
        let ctx = ctx_with(trace_with_calls(vec![spl_set_authority_call(
            2, t, &[], 2,
        )]));
        let checker = AuthorityDiffChecker::new();
        let first_bytes = to_vec(&checker.check(&ctx).await).expect("serialize");
        for _ in 0..9 {
            let bytes = to_vec(&checker.check(&ctx).await).expect("serialize");
            assert_eq!(first_bytes, bytes, "non-deterministic checker output");
        }
    }
}
