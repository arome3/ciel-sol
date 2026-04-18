// Ciel Checker Framework and Implementations
// See spec Section 4 for trait design, Section 6 for scorer.

pub mod traits;
pub mod runner;
pub mod stubs;
pub mod hash;
pub mod scorer;
pub mod program_registry;
pub mod oracle_cache;

// Individual checkers
pub mod oracle_sanity;
pub mod authority_diff;
pub mod intent_diff;
pub mod intent_rules;
pub mod approval_abuse;
pub mod sim_spoof;
pub mod sim_patterns;
pub mod contagion_map;
pub mod dependency_graph;
pub mod event_cache;
pub mod mev_sandwich;

// Re-export public API
pub use traits::{
    Checker, CheckerContext, CheckerError, CheckerOutput, CheckerResults, CheckerStatus, Flag,
    Intent, IntentSpec, OracleCache, ProgramRegistry, Severity, CHECKER_DEADLINE_MS,
};
pub use oracle_cache::{
    CanonicalFeedMap, OracleParseError, OraclePrice, OracleType,
    parse_switchboard_v2, parse_pyth_price, pyth_is_trading,
};
pub use oracle_sanity::OracleSanityChecker;
pub use authority_diff::AuthorityDiffChecker;
pub use approval_abuse::{ApprovalAbuseChecker, FLAG_UNLIMITED_APPROVAL_UNKNOWN_PROGRAM};
pub use intent_diff::{
    FLAG_INTENT_BALANCE_MISMATCH, FLAG_INTENT_VERIFICATION_INCONCLUSIVE, IntentDiffChecker,
    IntentDiffLlmAnalysis, intent_diff_llm_analyze,
};
pub use intent_rules::{
    DEPOSIT_TOLERANCE_BPS, IntentPattern, RULE_TABLE_VERSION, SWAP_TOLERANCE_BPS,
    TRANSFER_TOLERANCE_BPS, parse_intent_goal, token_info,
};
pub use runner::run_checkers;
pub use stubs::all_stub_checkers;
pub use hash::checker_outputs_hash;
