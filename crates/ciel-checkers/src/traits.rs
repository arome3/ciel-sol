// Checker trait, context, output types, and supporting enums.
// See spec Section 4.1 (Checker Plugin Interface).

use std::collections::HashMap;

use async_trait::async_trait;
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::Transaction;

use ciel_fork::SimulationTrace;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Per-checker timeout in milliseconds. See spec Section 4.2.
pub const CHECKER_DEADLINE_MS: u64 = 80;

// ---------------------------------------------------------------------------
// Checker trait
// ---------------------------------------------------------------------------

/// Every checker implements this trait. See spec Section 4.1.
///
/// Checkers must be deterministic: same `CheckerContext` → same `CheckerOutput`.
/// The `Send + Sync` bounds are required because checkers run concurrently
/// via `futures::join_all`.
#[async_trait]
pub trait Checker: Send + Sync {
    /// Unique identifier for this checker (e.g., "oracle_sanity").
    fn name(&self) -> &'static str;

    /// Run the check against a simulation trace.
    async fn check(&self, ctx: &CheckerContext) -> CheckerOutput;
}

// ---------------------------------------------------------------------------
// Stub types for upstream dependencies not yet implemented
// ---------------------------------------------------------------------------

/// Stub type for Intent. Real implementation lives in ciel-intent (Unit 10).
///
/// Carries both a free-text `description` (for display / LLM analysis) and an
/// optional structured `spec` (preferred by verification). This mirrors the
/// UniswapX / CoW Protocol / 1inch Fusion pattern of signed structured
/// intents with free-text reserved for rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub description: String,
    pub constraints: Vec<String>,
    /// Optional structured form. When present, the Intent Diff checker uses
    /// this directly and skips parsing `description`. SDKs should populate
    /// this whenever they have structured data available.
    #[serde(default)]
    pub spec: Option<IntentSpec>,
}

/// Structured intent spec consumed by the Intent Diff checker (Unit 12).
///
/// Amounts are raw token units (smallest denomination, not ui-amount). Mint
/// pubkeys are canonical SPL token mints; for SOL use the wSOL mint
/// (`So11111111111111111111111111111111111111112`) — the checker accepts both
/// native-lamport and wSOL deltas to handle Jupiter/Raydium end-of-tx unwraps.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IntentSpec {
    Swap {
        amount_in: u128,
        mint_in: Pubkey,
        mint_out: Pubkey,
        /// When supplied, the checker enforces `actual_out_delta >= min_amount_out`
        /// in addition to the direction-and-nonzero default.
        #[serde(default)]
        min_amount_out: Option<u128>,
    },
    Transfer {
        amount: u128,
        mint: Pubkey,
        recipient: Pubkey,
    },
    Deposit {
        amount: u128,
        mint: Pubkey,
        /// Program id of the target protocol.
        protocol: Pubkey,
    },
}

pub use crate::oracle_cache::OracleCache;
pub use crate::program_registry::ProgramRegistry;

// ---------------------------------------------------------------------------
// CheckerContext
// ---------------------------------------------------------------------------

/// Input provided to every checker. See spec Section 4.1.
///
/// Clone is required because each parallel checker task receives its own copy.
#[derive(Clone)]
pub struct CheckerContext {
    /// Balance deltas, CPI graph, account changes, logs.
    pub trace: SimulationTrace,
    /// The raw transaction being evaluated.
    pub original_tx: Transaction,
    /// Set if this was an intent-mode request.
    pub intent: Option<Intent>,
    /// Mainnet slot at fork time.
    pub slot: u64,
    /// Cached oracle prices for cross-reference.
    pub oracle_cache: OracleCache,
    /// Registry of known-good programs.
    pub known_programs: ProgramRegistry,
}

// ---------------------------------------------------------------------------
// Severity
// ---------------------------------------------------------------------------

/// Severity level for checker findings. See spec Section 4.1.
///
/// PartialOrd/Ord derived for downstream scorer comparisons.
/// Discriminants match the wire format used by ciel-signer's Verdict enum.
#[derive(
    BorshSerialize, BorshDeserialize, Serialize, Deserialize,
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord,
)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum Severity {
    None = 0,
    Low = 1,
    Medium = 2,
    High = 3,
    Critical = 4,
}

// ---------------------------------------------------------------------------
// Flag
// ---------------------------------------------------------------------------

/// A specific finding from a checker. See spec Section 4.1.
///
/// The `data` field uses `serde_json::Value` for arbitrary structured data.
/// Borsh serialization is handled via field-level `serialize_with`/`deserialize_with`
/// because `serde_json::Value` does not implement Borsh traits natively.
/// The JSON value is serialized as `Vec<u8>` containing canonical JSON bytes.
///
/// INVARIANT: Determinism depends on `serde_json` using `BTreeMap` for JSON objects
/// (the default when the `preserve_order` feature is NOT enabled).
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Flag {
    /// Machine-readable code, e.g. "ORACLE_DEVIATION_3_SIGMA".
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// Arbitrary structured data for this finding.
    #[borsh(
        serialize_with = "borsh_serialize_json_value",
        deserialize_with = "borsh_deserialize_json_value"
    )]
    pub data: serde_json::Value,
}

/// Borsh-serialize a `serde_json::Value` as its canonical JSON bytes (`Vec<u8>`).
fn borsh_serialize_json_value<W: borsh::io::Write>(
    val: &serde_json::Value,
    writer: &mut W,
) -> borsh::io::Result<()> {
    let bytes = serde_json::to_vec(val).map_err(borsh::io::Error::other)?;
    BorshSerialize::serialize(&bytes, writer)
}

/// Borsh-deserialize a `serde_json::Value` from its canonical JSON bytes.
fn borsh_deserialize_json_value<R: borsh::io::Read>(
    reader: &mut R,
) -> borsh::io::Result<serde_json::Value> {
    let bytes: Vec<u8> = BorshDeserialize::deserialize_reader(reader)?;
    serde_json::from_slice(&bytes)
        .map_err(|e| borsh::io::Error::new(borsh::io::ErrorKind::InvalidData, e))
}

// ---------------------------------------------------------------------------
// CheckerOutput
// ---------------------------------------------------------------------------

/// Output from every checker. Deterministic and serializable. See spec Section 4.1.
///
/// Derives BorshSerialize/BorshDeserialize for `checker_outputs_hash` computation
/// (see spec Section 7.1).
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CheckerOutput {
    /// Name of the checker that produced this output.
    pub checker_name: String,
    /// true = no issues found.
    pub passed: bool,
    /// Severity level of findings.
    pub severity: Severity,
    /// Specific findings.
    pub flags: Vec<Flag>,
    /// Human-readable explanation.
    pub details: String,
}

// ---------------------------------------------------------------------------
// CheckerStatus / CheckerResults
// ---------------------------------------------------------------------------

/// Result of a single checker execution within the parallel fan-out.
/// See spec Section 4.2.
#[derive(Clone, Debug)]
pub enum CheckerStatus {
    /// Checker completed within the deadline.
    Completed(CheckerOutput),
    /// Checker exceeded the per-checker timeout.
    TimedOut,
}

/// Aggregated results from running all checkers in parallel.
/// See spec Section 4.2.
#[derive(Clone, Debug)]
pub struct CheckerResults {
    /// Checker name → execution status.
    pub outputs: HashMap<String, CheckerStatus>,
    /// Wall-clock duration of the entire fan-out in milliseconds.
    pub total_duration_ms: u64,
}

impl CheckerResults {
    /// Returns only the completed checker outputs.
    pub fn completed(&self) -> Vec<&CheckerOutput> {
        self.outputs
            .values()
            .filter_map(|s| match s {
                CheckerStatus::Completed(output) => Some(output),
                CheckerStatus::TimedOut => None,
            })
            .collect()
    }

    /// Returns names of checkers that timed out.
    pub fn timed_out(&self) -> Vec<&str> {
        self.outputs
            .iter()
            .filter_map(|(name, s)| match s {
                CheckerStatus::TimedOut => Some(name.as_str()),
                CheckerStatus::Completed(_) => None,
            })
            .collect()
    }

    /// Returns true if any checker timed out.
    pub fn has_timeouts(&self) -> bool {
        self.outputs
            .values()
            .any(|s| matches!(s, CheckerStatus::TimedOut))
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors from the checker framework.
#[derive(Debug, thiserror::Error)]
pub enum CheckerError {
    #[error("Borsh serialization failed: {0}")]
    Serialization(String),

    #[error("hash computation failed: {0}")]
    Hash(String),
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use borsh::{to_vec, from_slice};
    use serde_json::json;

    #[test]
    fn test_severity_borsh_roundtrip() {
        let variants = [
            Severity::None,
            Severity::Low,
            Severity::Medium,
            Severity::High,
            Severity::Critical,
        ];
        for severity in &variants {
            let bytes = to_vec(severity).expect("serialize");
            let decoded: Severity = from_slice(&bytes).expect("deserialize");
            assert_eq!(*severity, decoded);
        }
    }

    #[test]
    fn test_severity_ordering() {
        assert!(Severity::None < Severity::Low);
        assert!(Severity::Low < Severity::Medium);
        assert!(Severity::Medium < Severity::High);
        assert!(Severity::High < Severity::Critical);
    }

    #[test]
    fn test_flag_borsh_roundtrip() {
        let flag = Flag {
            code: "ORACLE_DEVIATION_3_SIGMA".to_string(),
            message: "SOL/USD deviation 4.2 sigma".to_string(),
            data: json!({
                "asset": "SOL/USD",
                "deviation_sigma": 4.2,
                "pyth_price": 138.20,
                "switchboard_price": 142.50
            }),
        };
        let bytes = to_vec(&flag).expect("serialize");
        let decoded: Flag = from_slice(&bytes).expect("deserialize");
        assert_eq!(flag, decoded);
    }

    #[test]
    fn test_flag_borsh_roundtrip_null() {
        let flag = Flag {
            code: "NO_DATA".to_string(),
            message: "no additional data".to_string(),
            data: serde_json::Value::Null,
        };
        let bytes = to_vec(&flag).expect("serialize");
        let decoded: Flag = from_slice(&bytes).expect("deserialize");
        assert_eq!(flag, decoded);
    }

    #[test]
    fn test_checker_output_borsh_roundtrip() {
        let output = CheckerOutput {
            checker_name: "oracle_sanity".to_string(),
            passed: false,
            severity: Severity::Critical,
            flags: vec![
                Flag {
                    code: "ORACLE_DEVIATION_3_SIGMA".to_string(),
                    message: "price deviation detected".to_string(),
                    data: json!({"sigma": 4.2}),
                },
                Flag {
                    code: "STALE_FEED".to_string(),
                    message: "oracle feed older than 30s".to_string(),
                    data: json!({"age_seconds": 45}),
                },
            ],
            details: "Multiple oracle anomalies detected".to_string(),
        };
        let bytes = to_vec(&output).expect("serialize");
        let decoded: CheckerOutput = from_slice(&bytes).expect("deserialize");
        assert_eq!(output, decoded);
    }
}
