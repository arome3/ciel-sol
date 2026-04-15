// Ciel Test Fixtures
// See spec Section 17.3 for Drift exploit replay requirements.
// See docs/00-drift-exploit-fixture.md for implementation guide.

pub mod drift;

use serde::{Deserialize, Serialize};
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::Transaction;
use std::collections::HashMap;

pub use drift::{load_drift_fixture, load_drift_real_fixture};

/// Error type for fixture loading and generation.
#[derive(Debug, thiserror::Error)]
pub enum FixtureError {
    #[error("fixture file not found: {path}")]
    FileNotFound { path: String },

    #[error("JSON deserialization failed: {0}")]
    Deserialization(#[from] serde_json::Error),

    #[error("base64 decode failed: {0}")]
    Base64Decode(#[from] base64::DecodeError),

    #[error("bincode decode failed: {0}")]
    BincodeDecode(#[from] bincode::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("missing account in fixture: {pubkey}")]
    MissingAccount { pubkey: String },
}

/// A complete self-contained fixture for replaying an exploit.
/// See spec Section 17.3.
#[derive(Debug, Clone)]
pub struct ExploitFixture {
    pub transaction: Transaction,
    pub accounts: HashMap<Pubkey, Account>,
    pub metadata: FixtureMetadata,
}

/// Metadata about the captured exploit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureMetadata {
    pub description: String,
    pub slot: u64,
    pub blockhash: String,
    pub transaction_signature: Option<String>,
    pub exploit_type: String,
    pub expected_checkers: Vec<String>,
    pub expected_verdict: String,
    pub is_synthetic: bool,
}

/// On-disk JSON representation of a serialized transaction.
/// Uses base64-encoded bincode, matching Solana RPC's getTransaction format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedTransaction {
    pub encoding: String,
    pub data: String,
}

/// On-disk JSON representation of an account, mirroring Solana RPC format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedAccount {
    pub lamports: u64,
    /// Tuple of (base64_data, "base64") matching Solana RPC convention.
    pub data: (String, String),
    /// Base58-encoded owner pubkey.
    pub owner: String,
    pub executable: bool,
    pub rent_epoch: u64,
}
