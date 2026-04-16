// Simulation trace types consumed by every checker.
// See spec Section 4.1 (CheckerContext.trace field).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

/// Complete trace of a single transaction simulation in the fork.
///
/// Produced by `execute_transaction()` and consumed by all 7 checkers via `CheckerContext`.
/// Contains balance deltas, CPI call graph, account state changes, program logs,
/// oracle reads, and token approvals.
///
/// Fields beyond the spec (`success`, `error`, `compute_units_consumed`, `fee`) are
/// included because LiteSVM provides them directly and every production simulation
/// tool (Jito, Helius, Solana RPC) surfaces them. See spec Section 4.1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationTrace {
    /// Whether the transaction executed successfully (no error).
    pub success: bool,
    /// Error message if the transaction failed. None on success.
    pub error: Option<String>,
    /// SOL balance changes per account in lamports (signed).
    /// Positive = received lamports, negative = sent or paid fees.
    pub balance_deltas: HashMap<Pubkey, i64>,
    /// Complete program-invocation call graph as a flat list, ordered top-level
    /// instructions first (stack_height=1) followed by inner CPIs from execution
    /// (stack_height>=2). Including top-level entries — even when execution fails
    /// before any CPIs run (e.g., stale blockhash in a captured fixture) — is
    /// what lets checkers like Authority Diff find admin transfers from input
    /// bytes alone. Without that Layer-1 baseline, every captured-fixture verdict
    /// would be a false negative. Same pattern as `oracle_reads`. See `CpiCall`.
    pub cpi_graph: Vec<CpiCall>,
    /// Account state changes: owner, lamports, and data length diffs.
    pub account_changes: Vec<AccountChange>,
    /// Program logs emitted during execution.
    pub logs: Vec<String>,
    /// Oracle feeds accessed during execution (Switchboard/Pyth).
    pub oracle_reads: Vec<OracleRead>,
    /// SPL Token Approve instructions in the transaction.
    pub token_approvals: Vec<TokenApproval>,
    /// Compute units consumed by the transaction.
    pub compute_units_consumed: u64,
    /// Transaction fee in lamports.
    pub fee: u64,
}

/// A single program invocation in the transaction's call graph. Despite the
/// name, this represents BOTH top-level instructions (`stack_height = 1`) and
/// actual cross-program invocations made during execution (`stack_height >= 2`).
/// The flat list in `SimulationTrace.cpi_graph` is the unified call graph;
/// checkers iterate it without caring whether a call was top-level or nested.
///
/// Top-level entries are derived from `Transaction.message.instructions` and
/// always populate, even when simulation fails before any code runs. Inner
/// entries (`stack_height >= 2`) are derived from LiteSVM's `inner_instructions`
/// and only populate when execution makes progress.
///
/// For Squads `VaultTransactionExecute`-style indirection (where the inner
/// program — e.g., Drift `UpdateAdmin` — is reached via a CPI that didn't run
/// in a captured fixture), the inner program ID is still discoverable via the
/// outer call's `accounts` field (Squads passes the inner program in
/// `remaining_accounts`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpiCall {
    /// The program being invoked.
    pub program_id: Pubkey,
    /// Index of the top-level instruction (`tx.message.instructions[i]`) this
    /// call belongs to. For top-level calls, this is the call's own index.
    pub instruction_index: usize,
    /// Call depth: 1 = top-level instruction, 2 = direct CPI, 3 = nested CPI, …
    pub stack_height: u8,
    /// Accounts passed to this invocation (resolved from indices).
    pub accounts: Vec<Pubkey>,
    /// Instruction data passed to the program.
    pub data: Vec<u8>,
}

/// Pre/post diff of a single account's state across the simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountChange {
    pub pubkey: Pubkey,
    pub owner_before: Pubkey,
    pub owner_after: Pubkey,
    pub lamports_before: u64,
    pub lamports_after: u64,
    pub data_len_before: usize,
    pub data_len_after: usize,
}

/// An oracle feed accessed during the transaction.
/// Detected heuristically from program logs and account keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleRead {
    /// The oracle feed account pubkey.
    pub oracle_pubkey: Pubkey,
    /// Oracle provider: "switchboard" or "pyth".
    pub oracle_type: String,
}

/// An SPL Token Approve instruction detected in the transaction.
/// Scanned from input instructions (detectable even if the tx fails).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenApproval {
    /// The token account granting the approval.
    pub source: Pubkey,
    /// The delegate receiving approval.
    pub delegate: Pubkey,
    /// Amount of tokens approved.
    pub amount: u64,
    /// Owner of the source token account.
    pub owner: Pubkey,
}
