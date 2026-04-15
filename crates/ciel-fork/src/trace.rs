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
    /// CPI call graph — program invocations with depth, accounts, and data.
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

/// A single CPI (Cross-Program Invocation) call within the transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpiCall {
    /// The program being invoked.
    pub program_id: Pubkey,
    /// Index of the top-level instruction that triggered this CPI.
    pub instruction_index: usize,
    /// Call depth: 2 = direct CPI from top-level, 3 = nested, etc.
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
