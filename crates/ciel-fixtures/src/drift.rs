// Drift Protocol exploit fixture — synthetic oracle manipulation + authority transfer.
// See spec Section 17.3 for what the fixture must contain.
// See docs/00-drift-exploit-fixture.md for the full implementation guide.

#[allow(deprecated)] // solana_sdk::system_program — stable re-export, replacement not yet in SDK 2.2
use solana_sdk::system_program;

use crate::{
    ExploitFixture, FixtureError, FixtureMetadata, SerializedAccount, SerializedTransaction,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use solana_sdk::{
    account::Account,
    bs58,
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    message::Message,
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
    transaction::Transaction,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

// ---------------------------------------------------------------------------
// Well-known program IDs
// ---------------------------------------------------------------------------

/// Drift Protocol v2 program.
pub fn drift_program_id() -> Pubkey {
    Pubkey::from_str("dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH").unwrap()
}

/// SPL Token program.
pub fn spl_token_program_id() -> Pubkey {
    Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap()
}

/// Switchboard V2 oracle program.
pub fn switchboard_v2_program_id() -> Pubkey {
    Pubkey::from_str("SW1TCH7qEPTdLsDHRgPuMQjbQxKdH2aBStViMFnt64f").unwrap()
}

/// Pyth oracle program.
pub fn pyth_program_id() -> Pubkey {
    Pubkey::from_str("FsJ3A3u2vn5cTVofAjvy6y5kwABJAqYWpe4975bi2epH").unwrap()
}

/// Squads V4 multisig program (used in the real Drift exploit).
pub fn squads_program_id() -> Pubkey {
    Pubkey::from_str("SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf").unwrap()
}

// ---------------------------------------------------------------------------
// Synthetic fixture metadata constants
// ---------------------------------------------------------------------------

const SYNTHETIC_SLOT: u64 = 350_000_000;
const DRAIN_AMOUNT: u64 = 285_000_000 * 1_000_000; // 285M USDC (6 decimals)

// Oracle prices for the synthetic exploit.
// Switchboard is manipulated to 200.00, Pyth reports the real price at 142.50.
// Deviation = |200.00 - 142.50| / max(0.45, 0.38) = 127.8 sigma (>>3 sigma).
// See spec Section 4.3.1 for the checker's sigma threshold.
const SWITCHBOARD_PRICE: f64 = 200.0;
const SWITCHBOARD_STD_DEV: f64 = 0.45;
const PYTH_PRICE: i64 = 14250; // $142.50 with exponent -2
const PYTH_CONFIDENCE: u64 = 38; // $0.38 with exponent -2
const PYTH_EXPONENT: i32 = -2;

// ---------------------------------------------------------------------------
// Deterministic keypair helpers
// ---------------------------------------------------------------------------

/// Derive a deterministic keypair from a human-readable seed string.
/// Uses SHA-256 to produce a 32-byte seed for Ed25519.
fn deterministic_keypair(seed_phrase: &str) -> Keypair {
    use solana_sdk::hash::hash;
    use solana_sdk::signer::keypair::keypair_from_seed;
    // SHA-256 hash produces a 32-byte deterministic seed from the phrase.
    let seed = hash(seed_phrase.as_bytes());
    keypair_from_seed(seed.as_ref()).expect("valid Ed25519 keypair from deterministic seed")
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Load the synthetic Drift exploit fixture from disk, falling back to
/// in-memory generation if fixture files are not present.
///
/// The synthetic fixture combines oracle manipulation + authority transfer
/// in a single transaction for demo convenience. In the real April 1, 2026
/// exploit, these are separate phases by different wallets: the admin transfer
/// (Phase 1, via Squads multisig) triggers Authority Diff, while the fake CVT
/// collateral deposit (Phase 3) triggers Oracle Sanity. Use `load_drift_real_fixture()`
/// for the actual on-chain admin transfer transaction.
/// See spec Section 17.3.
pub fn load_drift_fixture() -> Result<ExploitFixture, FixtureError> {
    let dir = fixture_dir();
    let tx_path = dir.join("transaction.json");
    let accounts_path = dir.join("accounts.json");
    let metadata_path = dir.join("metadata.json");

    if tx_path.exists() && accounts_path.exists() && metadata_path.exists() {
        tracing::info!(path = %dir.display(), "Loading synthetic Drift fixture from disk");
        load_from_disk(&tx_path, &accounts_path, &metadata_path)
    } else {
        tracing::warn!("Synthetic fixture files not found, generating in memory");
        Ok(generate_drift_fixture())
    }
}

/// Load the real Drift exploit admin transfer transaction (Tx #2).
///
/// This is the actual on-chain transaction from April 1, 2026 (slot 410344009)
/// where the attacker executed `UpdateAdmin` on Drift via a Squads multisig,
/// transferring protocol control. The Authority Diff checker fires on this.
///
/// Account state is captured at current slot (post-exploit), not at the
/// historical exploit slot. This is sufficient for checker demonstration
/// because the authority transfer instruction is in the transaction bytes.
pub fn load_drift_real_fixture() -> Result<ExploitFixture, FixtureError> {
    let dir = real_fixture_dir();
    let tx_path = dir.join("transaction.json");
    let accounts_path = dir.join("accounts.json");
    let metadata_path = dir.join("metadata.json");

    if tx_path.exists() && accounts_path.exists() && metadata_path.exists() {
        tracing::info!(path = %dir.display(), "Loading real Drift exploit fixture from disk");
        load_from_disk(&tx_path, &accounts_path, &metadata_path)
    } else {
        Err(FixtureError::FileNotFound {
            path: dir.display().to_string(),
        })
    }
}

/// Generate a synthetic Drift exploit fixture entirely in memory.
///
/// Combines two attack patterns from the real exploit into a single transaction
/// for testing convenience (the real exploit spreads these across separate
/// transactions and wallets):
/// 1. Oracle price manipulation (Switchboard feed diverges >3σ from Pyth)
/// 2. Hidden authority transfer (SPL Token SetAuthority on Drift vault)
///
/// All keypairs are derived deterministically so the output is reproducible.
pub fn generate_drift_fixture() -> ExploitFixture {
    // --- Deterministic keypairs ---
    let attacker = deterministic_keypair("ciel-drift-fixture-attacker");
    let oracle_authority = deterministic_keypair("ciel-drift-fixture-oracle-authority");
    let vault_authority = deterministic_keypair("ciel-drift-fixture-vault-authority");

    // --- Deterministic account addresses ---
    let switchboard_feed = deterministic_keypair("ciel-drift-fixture-switchboard-feed");
    let pyth_feed = deterministic_keypair("ciel-drift-fixture-pyth-feed");
    let usdc_mint = deterministic_keypair("ciel-drift-fixture-usdc-mint");
    let vault_token_account = deterministic_keypair("ciel-drift-fixture-vault-token");
    let attacker_token_account = deterministic_keypair("ciel-drift-fixture-attacker-token");

    // --- Build instructions ---

    // Instruction 0: Switchboard oracle update with manipulated price.
    // See spec Section 4.3.1 — Oracle Sanity Checker.
    let switchboard_update_ix = Instruction {
        program_id: switchboard_v2_program_id(),
        accounts: vec![
            AccountMeta::new(switchboard_feed.pubkey(), false),
            AccountMeta::new_readonly(oracle_authority.pubkey(), true),
        ],
        data: build_synthetic_switchboard_data(),
    };

    // Instruction 1: SPL Token SetAuthority — change vault authority from
    // vault_authority (Drift PDA stand-in) to attacker.
    // See spec Section 4.3.2 — Authority Diff Checker.
    // SPL Token SetAuthority encoding: [6, authority_type(1), has_new(1), new_authority(32)]
    let set_authority_ix = {
        let mut data = vec![6u8]; // SetAuthority discriminator
        data.push(2); // AuthorityType::AccountOwner
        data.push(1); // COption::Some — new authority present
        data.extend_from_slice(attacker.pubkey().as_ref());
        Instruction {
            program_id: spl_token_program_id(),
            accounts: vec![
                AccountMeta::new(vault_token_account.pubkey(), false),
                AccountMeta::new_readonly(vault_authority.pubkey(), true),
            ],
            data,
        }
    };

    // Instruction 2: SPL Token Transfer — drain vault to attacker.
    // SPL Token Transfer encoding: [3, amount(8 LE)]
    let transfer_ix = {
        let mut data = vec![3u8]; // Transfer discriminator
        data.extend_from_slice(&DRAIN_AMOUNT.to_le_bytes());
        Instruction {
            program_id: spl_token_program_id(),
            accounts: vec![
                AccountMeta::new(vault_token_account.pubkey(), false),
                AccountMeta::new(attacker_token_account.pubkey(), false),
                AccountMeta::new_readonly(attacker.pubkey(), true),
            ],
            data,
        }
    };

    // --- Build and sign the transaction ---
    let blockhash = Hash::new_from_array([1u8; 32]);
    let message = Message::new(
        &[switchboard_update_ix, set_authority_ix, transfer_ix],
        Some(&attacker.pubkey()),
    );
    let mut transaction = Transaction::new_unsigned(message);
    transaction.partial_sign(
        &[&attacker, &oracle_authority, &vault_authority],
        blockhash,
    );

    // --- Build account states ---
    let mut accounts: HashMap<Pubkey, Account> = HashMap::new();

    // Fee payer / attacker wallet
    accounts.insert(
        attacker.pubkey(),
        Account {
            lamports: 10_000_000_000, // 10 SOL
            data: vec![],
            owner: system_program::id(),
            executable: false,
            rent_epoch: 0,
        },
    );

    // Oracle authority
    accounts.insert(
        oracle_authority.pubkey(),
        Account {
            lamports: 1_000_000_000, // 1 SOL
            data: vec![],
            owner: system_program::id(),
            executable: false,
            rent_epoch: 0,
        },
    );

    // Vault authority (Drift PDA stand-in)
    accounts.insert(
        vault_authority.pubkey(),
        Account {
            lamports: 1_000_000_000,
            data: vec![],
            owner: system_program::id(),
            executable: false,
            rent_epoch: 0,
        },
    );

    // Switchboard oracle feed — manipulated price.
    // Layout: [price_f64_le(8) | std_dev_f64_le(8) | timestamp_i64_le(8) | padding(to 128 bytes)]
    accounts.insert(
        switchboard_feed.pubkey(),
        Account {
            lamports: 6_124_800,
            data: build_switchboard_account_data(),
            owner: switchboard_v2_program_id(),
            executable: false,
            rent_epoch: 0,
        },
    );

    // Pyth oracle feed — legitimate price.
    // Layout: [price_i64_le(8) | confidence_u64_le(8) | exponent_i32_le(4) | timestamp_i64_le(8) | padding(to 128 bytes)]
    accounts.insert(
        pyth_feed.pubkey(),
        Account {
            lamports: 6_124_800,
            data: build_pyth_account_data(),
            owner: pyth_program_id(),
            executable: false,
            rent_epoch: 0,
        },
    );

    // USDC mint — minimal SPL Mint data (82 bytes).
    // Layout: [mint_authority_option(4) | mint_authority(32) | supply(8) | decimals(1) | is_initialized(1) | freeze_authority_option(4) | freeze_authority(32)]
    accounts.insert(
        usdc_mint.pubkey(),
        Account {
            lamports: 1_461_600,
            data: build_mint_data(6), // 6 decimals for USDC
            owner: spl_token_program_id(),
            executable: false,
            rent_epoch: 0,
        },
    );

    // Drift vault token account — holds 285M USDC.
    // SPL Token Account layout (165 bytes).
    accounts.insert(
        vault_token_account.pubkey(),
        Account {
            lamports: 2_039_280,
            data: build_token_account_data(
                &usdc_mint.pubkey(),
                &vault_authority.pubkey(),
                DRAIN_AMOUNT,
            ),
            owner: spl_token_program_id(),
            executable: false,
            rent_epoch: 0,
        },
    );

    // Attacker token account — empty, will receive drained funds.
    accounts.insert(
        attacker_token_account.pubkey(),
        Account {
            lamports: 2_039_280,
            data: build_token_account_data(&usdc_mint.pubkey(), &attacker.pubkey(), 0),
            owner: spl_token_program_id(),
            executable: false,
            rent_epoch: 0,
        },
    );

    // Program accounts (executable).
    for (id, name) in [
        (spl_token_program_id(), "spl-token"),
        (switchboard_v2_program_id(), "switchboard-v2"),
        (pyth_program_id(), "pyth"),
        (drift_program_id(), "drift-v2"),
    ] {
        accounts.insert(
            id,
            Account {
                lamports: 1_141_440,
                data: name.as_bytes().to_vec(), // Minimal placeholder data
                owner: Pubkey::from_str("BPFLoaderUpgradeab1e11111111111111111111111")
                    .unwrap(),
                executable: true,
                rent_epoch: 0,
            },
        );
    }

    // System program
    accounts.insert(
        system_program::id(),
        Account {
            lamports: 1,
            data: vec![],
            owner: Pubkey::from_str("NativeLoader1111111111111111111111111111111").unwrap(),
            executable: true,
            rent_epoch: 0,
        },
    );

    // Pyth feed is referenced as a read-only account in the transaction's account
    // list (it appears in the Switchboard instruction as additional context).
    // To ensure it's included in account_keys, we added it above.
    // The transaction message compilation only includes accounts referenced by
    // instructions, so we need to also add the Pyth feed to an instruction.
    // We already have the Pyth feed in the accounts map; the transaction references
    // it implicitly through the account_keys list.
    //
    // Note: The Pyth feed is in the accounts map but may not be in the transaction's
    // account_keys (since no instruction references it). That's fine — the accounts
    // map is a superset. Checkers will look up oracle accounts by owner, not by
    // transaction account_keys.

    let metadata = FixtureMetadata {
        description: "Synthetic Drift Protocol exploit fixture — oracle price manipulation + \
                      admin key transfer (April 1, 2026 scenario)"
            .to_string(),
        slot: SYNTHETIC_SLOT,
        blockhash: bs58::encode(blockhash.as_ref()).into_string(),
        transaction_signature: None,
        exploit_type: "oracle_manipulation + authority_transfer".to_string(),
        expected_checkers: vec!["oracle_sanity".to_string(), "authority_diff".to_string()],
        expected_verdict: "BLOCK".to_string(),
        is_synthetic: true,
    };

    ExploitFixture {
        transaction,
        accounts,
        metadata,
    }
}

/// Write a fixture to disk as three JSON files (transaction.json, accounts.json,
/// metadata.json) in the given directory.
pub fn write_fixture_to_disk(fixture: &ExploitFixture, dir: &Path) -> Result<(), FixtureError> {
    std::fs::create_dir_all(dir)?;

    // Serialize transaction as base64-encoded bincode.
    let tx_bytes = bincode::serialize(&fixture.transaction)?;
    let serialized_tx = SerializedTransaction {
        encoding: "base64+bincode".to_string(),
        data: BASE64.encode(&tx_bytes),
    };
    let tx_json = serde_json::to_string_pretty(&serialized_tx)?;
    std::fs::write(dir.join("transaction.json"), tx_json)?;

    // Serialize accounts in Solana RPC format.
    let mut serialized_accounts: HashMap<String, SerializedAccount> = HashMap::new();
    for (pubkey, account) in &fixture.accounts {
        serialized_accounts.insert(
            pubkey.to_string(),
            SerializedAccount {
                lamports: account.lamports,
                data: (BASE64.encode(&account.data), "base64".to_string()),
                owner: account.owner.to_string(),
                executable: account.executable,
                rent_epoch: account.rent_epoch,
            },
        );
    }
    let accounts_json = serde_json::to_string_pretty(&serialized_accounts)?;
    std::fs::write(dir.join("accounts.json"), accounts_json)?;

    // Serialize metadata.
    let metadata_json = serde_json::to_string_pretty(&fixture.metadata)?;
    std::fs::write(dir.join("metadata.json"), metadata_json)?;

    tracing::info!(path = %dir.display(), "Wrote fixture to disk");
    Ok(())
}

/// Path to the synthetic fixture directory: `<workspace>/fixtures/drift-exploit/`.
pub fn fixture_dir() -> PathBuf {
    workspace_root().join("fixtures").join("drift-exploit")
}

/// Path to the real exploit fixture directory: `<workspace>/fixtures/drift-exploit-real/`.
pub fn real_fixture_dir() -> PathBuf {
    workspace_root().join("fixtures").join("drift-exploit-real")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent() // crates/
        .expect("crates/ parent")
        .parent() // workspace root
        .expect("workspace root")
        .to_path_buf()
}

// ---------------------------------------------------------------------------
// Disk loader internals
// ---------------------------------------------------------------------------

fn load_from_disk(
    tx_path: &Path,
    accounts_path: &Path,
    metadata_path: &Path,
) -> Result<ExploitFixture, FixtureError> {
    // Load and deserialize transaction.
    let tx_json = std::fs::read_to_string(tx_path)?;
    let serialized_tx: SerializedTransaction = serde_json::from_str(&tx_json)?;
    let tx_bytes = BASE64.decode(&serialized_tx.data)?;
    let transaction: Transaction = bincode::deserialize(&tx_bytes)?;

    // Load and deserialize accounts.
    let accounts_json = std::fs::read_to_string(accounts_path)?;
    let serialized_accounts: HashMap<String, SerializedAccount> =
        serde_json::from_str(&accounts_json)?;

    let mut accounts: HashMap<Pubkey, Account> = HashMap::new();
    for (key_str, ser_account) in &serialized_accounts {
        let pubkey = Pubkey::from_str(key_str).map_err(|_| FixtureError::MissingAccount {
            pubkey: key_str.clone(),
        })?;
        let data = BASE64.decode(&ser_account.data.0)?;
        let owner =
            Pubkey::from_str(&ser_account.owner).map_err(|_| FixtureError::MissingAccount {
                pubkey: ser_account.owner.clone(),
            })?;
        accounts.insert(
            pubkey,
            Account {
                lamports: ser_account.lamports,
                data,
                owner,
                executable: ser_account.executable,
                rent_epoch: ser_account.rent_epoch,
            },
        );
    }

    // Load metadata.
    let metadata_json = std::fs::read_to_string(metadata_path)?;
    let metadata: FixtureMetadata = serde_json::from_str(&metadata_json)?;

    // Validate: every account_key in the transaction must be present.
    for pubkey in &transaction.message.account_keys {
        if !accounts.contains_key(pubkey) {
            return Err(FixtureError::MissingAccount {
                pubkey: pubkey.to_string(),
            });
        }
    }

    Ok(ExploitFixture {
        transaction,
        accounts,
        metadata,
    })
}

// ---------------------------------------------------------------------------
// Synthetic account data builders
// ---------------------------------------------------------------------------

/// Build synthetic Switchboard oracle update instruction data.
/// This is a minimal representation — real Switchboard instructions are more complex.
fn build_synthetic_switchboard_data() -> Vec<u8> {
    // Discriminator (8 bytes) + price_f64_le (8 bytes)
    let mut data = vec![0u8; 8]; // Generic discriminator
    data.extend_from_slice(&SWITCHBOARD_PRICE.to_le_bytes());
    data
}

/// Build Switchboard aggregator account data with the manipulated price.
///
/// Synthetic layout (128 bytes):
///   [0..8]   price as f64 LE
///   [8..16]  std_dev as f64 LE
///   [16..24] timestamp as i64 LE (slot-based)
///   [24..128] padding
fn build_switchboard_account_data() -> Vec<u8> {
    let mut data = vec![0u8; 128];
    data[0..8].copy_from_slice(&SWITCHBOARD_PRICE.to_le_bytes());
    data[8..16].copy_from_slice(&SWITCHBOARD_STD_DEV.to_le_bytes());
    // Timestamp: use the synthetic slot as a proxy
    let timestamp = SYNTHETIC_SLOT as i64;
    data[16..24].copy_from_slice(&timestamp.to_le_bytes());
    data
}

/// Build Pyth price account data with the legitimate price.
///
/// Synthetic layout (128 bytes):
///   [0..8]   price as i64 LE (scaled by 10^|exponent|)
///   [8..16]  confidence as u64 LE
///   [16..20] exponent as i32 LE
///   [20..28] timestamp as i64 LE
///   [28..128] padding
fn build_pyth_account_data() -> Vec<u8> {
    let mut data = vec![0u8; 128];
    data[0..8].copy_from_slice(&PYTH_PRICE.to_le_bytes());
    data[8..16].copy_from_slice(&PYTH_CONFIDENCE.to_le_bytes());
    data[16..20].copy_from_slice(&PYTH_EXPONENT.to_le_bytes());
    let timestamp = SYNTHETIC_SLOT as i64;
    data[20..28].copy_from_slice(&timestamp.to_le_bytes());
    data
}

/// Build a minimal SPL Mint account (82 bytes).
///
/// Layout:
///   [0..4]   mint_authority COption tag (1 = Some)
///   [4..36]  mint_authority pubkey
///   [36..44] supply as u64 LE
///   [44]     decimals
///   [45]     is_initialized (1 = true)
///   [46..50] freeze_authority COption tag (0 = None)
///   [50..82] freeze_authority pubkey (zeroed)
fn build_mint_data(decimals: u8) -> Vec<u8> {
    let mut data = vec![0u8; 82];
    // mint_authority: None for simplicity
    data[0..4].copy_from_slice(&0u32.to_le_bytes()); // COption::None
    // supply: large number
    let supply: u64 = 10_000_000_000 * 10u64.pow(decimals as u32);
    data[36..44].copy_from_slice(&supply.to_le_bytes());
    data[44] = decimals;
    data[45] = 1; // is_initialized
    data
}

/// Build a minimal SPL Token Account (165 bytes).
///
/// Layout:
///   [0..32]   mint pubkey
///   [32..64]  owner pubkey
///   [64..72]  amount as u64 LE
///   [72..76]  delegate COption tag (0 = None)
///   [76..108] delegate pubkey (zeroed)
///   [108]     state (1 = Initialized)
///   [109..113] is_native COption tag (0 = None)
///   [113..121] is_native value (zeroed)
///   [121..125] delegated_amount COption tag (0 = None — actually just u64)
///   ... remainder zeroed up to 165 bytes
///   [157..161] close_authority COption tag (0 = None)
///   [161..165] padding
fn build_token_account_data(mint: &Pubkey, owner: &Pubkey, amount: u64) -> Vec<u8> {
    let mut data = vec![0u8; 165];
    data[0..32].copy_from_slice(mint.as_ref());
    data[32..64].copy_from_slice(owner.as_ref());
    data[64..72].copy_from_slice(&amount.to_le_bytes());
    data[108] = 1; // AccountState::Initialized
    data
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_drift_fixture() {
        let fixture = generate_drift_fixture();

        // Transaction should have 3 instructions.
        assert_eq!(
            fixture.transaction.message.instructions.len(),
            3,
            "Expected 3 instructions (switchboard update, set_authority, transfer)"
        );

        // Every account_key in the transaction must be in the accounts map.
        for pubkey in &fixture.transaction.message.account_keys {
            assert!(
                fixture.accounts.contains_key(pubkey),
                "Missing account for pubkey: {pubkey}"
            );
        }

        // Metadata checks.
        assert_eq!(fixture.metadata.expected_verdict, "BLOCK");
        assert!(fixture.metadata.expected_checkers.contains(&"oracle_sanity".to_string()));
        assert!(fixture.metadata.expected_checkers.contains(&"authority_diff".to_string()));
        assert!(fixture.metadata.is_synthetic);
        assert_eq!(fixture.metadata.slot, SYNTHETIC_SLOT);
    }

    #[test]
    fn test_fixture_round_trip_serialization() {
        let fixture = generate_drift_fixture();

        // Write to a temp directory.
        let tmp_dir = std::env::temp_dir().join("ciel-fixture-test-roundtrip");
        let _ = std::fs::remove_dir_all(&tmp_dir);
        write_fixture_to_disk(&fixture, &tmp_dir).expect("write should succeed");

        // Verify files exist.
        assert!(tmp_dir.join("transaction.json").exists());
        assert!(tmp_dir.join("accounts.json").exists());
        assert!(tmp_dir.join("metadata.json").exists());

        // Load back and compare.
        let loaded = load_from_disk(
            &tmp_dir.join("transaction.json"),
            &tmp_dir.join("accounts.json"),
            &tmp_dir.join("metadata.json"),
        )
        .expect("load should succeed");

        // Same number of instructions.
        assert_eq!(
            loaded.transaction.message.instructions.len(),
            fixture.transaction.message.instructions.len(),
        );

        // Same account keys in the transaction.
        assert_eq!(
            loaded.transaction.message.account_keys,
            fixture.transaction.message.account_keys,
        );

        // Same accounts map keys.
        let mut orig_keys: Vec<String> = fixture.accounts.keys().map(|k| k.to_string()).collect();
        let mut loaded_keys: Vec<String> =
            loaded.accounts.keys().map(|k| k.to_string()).collect();
        orig_keys.sort();
        loaded_keys.sort();
        assert_eq!(orig_keys, loaded_keys);

        // Same metadata.
        assert_eq!(loaded.metadata.slot, fixture.metadata.slot);
        assert_eq!(loaded.metadata.expected_verdict, fixture.metadata.expected_verdict);

        // Cleanup.
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn test_transaction_has_set_authority() {
        let fixture = generate_drift_fixture();
        let token_program = spl_token_program_id();

        let has_set_authority = fixture.transaction.message.instructions.iter().any(|ix| {
            let program_id =
                fixture.transaction.message.account_keys[ix.program_id_index as usize];
            // SetAuthority discriminator = 6
            program_id == token_program && !ix.data.is_empty() && ix.data[0] == 6
        });

        assert!(
            has_set_authority,
            "Transaction must contain an SPL Token SetAuthority instruction"
        );
    }

    #[test]
    fn test_oracle_accounts_present_with_data() {
        let fixture = generate_drift_fixture();
        let switchboard_pid = switchboard_v2_program_id();
        let pyth_pid = pyth_program_id();

        // Find Switchboard oracle account.
        let switchboard_accounts: Vec<_> = fixture
            .accounts
            .iter()
            .filter(|(_, acct)| acct.owner == switchboard_pid)
            .collect();
        assert!(
            !switchboard_accounts.is_empty(),
            "Must have at least one Switchboard oracle account"
        );
        for (_, acct) in &switchboard_accounts {
            assert!(!acct.data.is_empty(), "Switchboard account data must not be empty");
            // Verify the manipulated price is encoded in the first 8 bytes.
            let price = f64::from_le_bytes(acct.data[0..8].try_into().unwrap());
            assert!(
                (price - SWITCHBOARD_PRICE).abs() < f64::EPSILON,
                "Switchboard price should be {SWITCHBOARD_PRICE}, got {price}"
            );
        }

        // Find Pyth oracle account.
        let pyth_accounts: Vec<_> = fixture
            .accounts
            .iter()
            .filter(|(_, acct)| acct.owner == pyth_pid)
            .collect();
        assert!(
            !pyth_accounts.is_empty(),
            "Must have at least one Pyth oracle account"
        );
        for (_, acct) in &pyth_accounts {
            assert!(!acct.data.is_empty(), "Pyth account data must not be empty");
            // Verify the legitimate price is encoded.
            let price = i64::from_le_bytes(acct.data[0..8].try_into().unwrap());
            assert_eq!(price, PYTH_PRICE, "Pyth price should be {PYTH_PRICE}");
        }
    }

    #[test]
    fn test_load_drift_fixture_fallback() {
        // This exercises the fallback path (generate in-memory) since the
        // on-disk fixture files may or may not exist depending on test order.
        let fixture = load_drift_fixture().expect("load_drift_fixture should succeed");

        // Basic sanity checks.
        assert!(!fixture.transaction.message.instructions.is_empty());
        assert!(!fixture.accounts.is_empty());
        assert_eq!(fixture.metadata.expected_verdict, "BLOCK");
    }

    #[test]
    #[ignore] // Run manually: cargo test -- --ignored write_fixture_files
    fn write_fixture_files_to_disk() {
        let fixture = generate_drift_fixture();
        let dir = fixture_dir();
        write_fixture_to_disk(&fixture, &dir).expect("write to fixture dir should succeed");
        assert!(dir.join("transaction.json").exists());
        assert!(dir.join("accounts.json").exists());
        assert!(dir.join("metadata.json").exists());
        eprintln!("Fixture files written to {}", dir.display());
    }

    #[test]
    #[ignore] // Requires network — run with: cargo test -- --ignored capture
    fn test_capture_from_rpc() {
        // The real Drift exploit transaction signatures are known:
        //   Tx #1 (create+approve): 2HvMSgDEfKhNryYZKhjowrBY55rUx5MWtcWkG9hqxZCFBaTiahPwfynP1dxBSRk9s5UTVc8LFeS4Btvkm9pc2C4H
        //   Tx #2 (execute admin transfer): 4BKBmAJn6TdsENij7CsVbyMVLJU1tX27nfrMM1zgKv1bs2KJy6Am2NqdA3nJm4g9C6eC64UAf5sNs974ygB9RsN1
        //
        // Both are retrievable from the free public Solana RPC.
        // The real fixture at fixtures/drift-exploit-real/ was captured via
        // curl against api.mainnet-beta.solana.com.
        //
        // This test stub is for programmatic re-capture via solana_client:
        //   1. Use RpcClient::get_transaction() to fetch Tx #2
        //   2. Use RpcClient::get_multiple_accounts() for all referenced accounts
        //   3. Call write_fixture_to_disk() to persist
        //
        // Note: account state will be current (post-exploit), not historical.
        // Historical state at slot 410344009 is unavailable from any standard
        // Solana RPC — this is a fundamental limitation, not a provider issue.
        // See fixtures/drift-exploit-real/real_exploit_metadata.json for details.
        todo!("Implement programmatic re-capture via solana_client");
    }

    // -----------------------------------------------------------------------
    // Real exploit fixture tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_load_real_fixture() {
        let fixture = load_drift_real_fixture()
            .expect("real fixture should load from fixtures/drift-exploit-real/");

        // Must not be synthetic.
        assert!(!fixture.metadata.is_synthetic);

        // Real exploit was at slot 410344009.
        assert_eq!(fixture.metadata.slot, 410_344_009);

        // Expected verdict is BLOCK via authority_diff checker.
        assert_eq!(fixture.metadata.expected_verdict, "BLOCK");
        assert!(fixture.metadata.expected_checkers.contains(&"authority_diff".to_string()));

        // Transaction signature must match the known real signature.
        assert_eq!(
            fixture.metadata.transaction_signature.as_deref(),
            Some("4BKBmAJn6TdsENij7CsVbyMVLJU1tX27nfrMM1zgKv1bs2KJy6Am2NqdA3nJm4g9C6eC64UAf5sNs974ygB9RsN1"),
        );
    }

    #[test]
    fn test_real_fixture_account_keys_present() {
        let fixture = load_drift_real_fixture().expect("real fixture should load");

        // Every account_key in the transaction must be in the accounts map.
        for pubkey in &fixture.transaction.message.account_keys {
            assert!(
                fixture.accounts.contains_key(pubkey),
                "Missing account for pubkey: {pubkey}"
            );
        }

        // Must contain the Drift program and Squads program.
        assert!(
            fixture.accounts.contains_key(&drift_program_id()),
            "Real fixture must contain the Drift program account"
        );
        assert!(
            fixture.accounts.contains_key(&squads_program_id()),
            "Real fixture must contain the Squads program account"
        );
    }

    #[test]
    fn test_real_fixture_has_drift_cpi() {
        let fixture = load_drift_real_fixture().expect("real fixture should load");

        // The real transaction calls Squads, which CPIs into Drift's UpdateAdmin.
        // Verify Squads program is in the instruction list.
        let squads_pid = squads_program_id();
        let has_squads_ix = fixture.transaction.message.instructions.iter().any(|ix| {
            fixture.transaction.message.account_keys[ix.program_id_index as usize] == squads_pid
        });
        assert!(
            has_squads_ix,
            "Real transaction must contain a Squads program instruction"
        );

        // Verify Drift program is referenced as an account (target of CPI).
        let drift_pid = drift_program_id();
        assert!(
            fixture.transaction.message.account_keys.contains(&drift_pid),
            "Real transaction must reference the Drift program (CPI target)"
        );
    }
}
