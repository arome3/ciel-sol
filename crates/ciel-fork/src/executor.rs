// Transaction executor: runs a transaction in the fork and captures the SimulationTrace.
// See spec Section 3.1 (engine choice) and Section 4.1 (CheckerContext.trace).
//
// Uses LiteSVM's simulate_transaction (readonly) rather than send_transaction,
// validated by industry practice: Jito simulateBundle, Surfpool profileTransaction,
// Phantom/Solflare wallet previews, and Solana RPC simulateTransaction all use
// readonly simulation. This allows fork reuse across multiple verdicts.

use std::collections::HashMap;

use litesvm::LiteSVM;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::Transaction;

use crate::simulator::{from_litesvm_account, from_litesvm_shared_account, to_litesvm_address};
use crate::trace::{
    AccountChange, CpiCall, OracleRead, SimulationTrace, TokenApproval,
};
use crate::{ForkError, ForkResult, ForkSimulator};

// Well-known program IDs for oracle and token detection.
const SPL_TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const SWITCHBOARD_V2_PROGRAM_ID: &str = "SW1TCH7qEPTdLsDHRgPuMQjbQxKdH2aBStViMFnt64f";
const PYTH_PROGRAM_ID: &str = "FsJ3A3u2vn5cTVofAjvy6y5kwABJAqYWpe4975bi2epH";

/// Lightweight pre-execution snapshot of one account's state.
struct AccountSnapshot {
    lamports: u64,
    owner: Pubkey,
    data_len: usize,
}

/// Execute a transaction in the fork simulator and capture a complete `SimulationTrace`.
///
/// Uses LiteSVM's `simulate_transaction` (readonly — does not mutate fork state).
/// The caller is responsible for pre-loading accounts into the fork before calling this.
///
/// # Arguments
/// * `fork` — mutable reference for flexibility (caller may pre-load accounts)
/// * `tx` — the solana-sdk 2.2 `Transaction` to simulate
///
/// # Returns
/// A `SimulationTrace` regardless of whether the transaction succeeds or fails.
/// On failure, `success=false`, `error` is set, and balance_deltas/account_changes are empty
/// (LiteSVM does not mutate state on failure). Logs and partial CPI data are still captured.
pub fn execute_transaction(
    fork: &mut ForkSimulator,
    tx: &Transaction,
) -> ForkResult<SimulationTrace> {
    let account_keys: Vec<Pubkey> = tx.message.account_keys.clone();

    // 1. Snapshot pre-execution account state.
    let pre_state = snapshot_accounts(fork.svm(), &account_keys);

    // 2. Convert Transaction v2 (solana-sdk 2.2) → VersionedTransaction v3 (litesvm)
    //    via bincode round-trip. Same pattern as tests/drift_fixture_smoke.rs:22-25.
    let tx_bytes = bincode::serialize(tx)
        .map_err(|e| ForkError::LiteSvm(format!("tx serialize failed: {e}")))?;
    let v3_tx: litesvm_transaction::Transaction = bincode::deserialize(&tx_bytes)
        .map_err(|e| ForkError::LiteSvm(format!("tx deserialize v2→v3 failed: {e}")))?;
    let versioned_tx = litesvm_transaction::versioned::VersionedTransaction::from(v3_tx);

    // 3. Simulate (readonly — does not mutate fork state).
    match fork.svm().simulate_transaction(versioned_tx) {
        Ok(sim_info) => {
            // Success path: we have metadata + post_accounts.
            let meta = &sim_info.meta;

            // Convert post_accounts from litesvm types to solana-sdk 2.2.
            let post_accounts: Vec<(Pubkey, Account)> = sim_info
                .post_accounts
                .into_iter()
                .map(|(addr, shared)| {
                    let pubkey = Pubkey::new_from_array(addr.to_bytes());
                    let account = from_litesvm_shared_account(shared);
                    (pubkey, account)
                })
                .collect();

            let balance_deltas = compute_balance_deltas(&pre_state, &post_accounts);
            let account_changes = compute_account_changes(&pre_state, &post_accounts);
            let cpi_graph = extract_cpi_graph(tx, &meta.inner_instructions, &account_keys);
            let oracle_reads = detect_oracle_reads(tx, &meta.logs);
            let token_approvals = detect_token_approvals(tx);

            Ok(SimulationTrace {
                success: true,
                error: None,
                balance_deltas,
                cpi_graph,
                account_changes,
                logs: meta.logs.clone(),
                oracle_reads,
                token_approvals,
                token_balance_deltas: Vec::new(),
                compute_units_consumed: meta.compute_units_consumed,
                fee: meta.fee,
            })
        }
        Err(failed) => {
            // Failure path: state not modified, but we still get metadata.
            let meta = &failed.meta;
            let cpi_graph = extract_cpi_graph(tx, &meta.inner_instructions, &account_keys);
            let oracle_reads = detect_oracle_reads(tx, &meta.logs);
            let token_approvals = detect_token_approvals(tx);

            Ok(SimulationTrace {
                success: false,
                error: Some(failed.err.to_string()),
                balance_deltas: HashMap::new(),
                cpi_graph,
                account_changes: Vec::new(),
                logs: meta.logs.clone(),
                oracle_reads,
                token_approvals,
                token_balance_deltas: Vec::new(),
                compute_units_consumed: meta.compute_units_consumed,
                fee: meta.fee,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Snapshot all accounts referenced by the transaction before simulation.
fn snapshot_accounts(svm: &LiteSVM, keys: &[Pubkey]) -> HashMap<Pubkey, AccountSnapshot> {
    let mut snapshots = HashMap::with_capacity(keys.len());
    for pubkey in keys {
        let addr = to_litesvm_address(pubkey);
        if let Some(acct) = svm.get_account(&addr) {
            let sdk_acct = from_litesvm_account(&acct);
            snapshots.insert(
                *pubkey,
                AccountSnapshot {
                    lamports: sdk_acct.lamports,
                    owner: sdk_acct.owner,
                    data_len: sdk_acct.data.len(),
                },
            );
        }
    }
    snapshots
}

/// Compute signed lamport deltas for each account.
fn compute_balance_deltas(
    pre: &HashMap<Pubkey, AccountSnapshot>,
    post_accounts: &[(Pubkey, Account)],
) -> HashMap<Pubkey, i64> {
    let mut deltas = HashMap::new();
    for (pubkey, post_acct) in post_accounts {
        let pre_lamports = pre.get(pubkey).map_or(0u64, |s| s.lamports);
        let delta = post_acct.lamports as i64 - pre_lamports as i64;
        if delta != 0 {
            deltas.insert(*pubkey, delta);
        }
    }
    deltas
}

/// Build account change records for all accounts that changed.
fn compute_account_changes(
    pre: &HashMap<Pubkey, AccountSnapshot>,
    post_accounts: &[(Pubkey, Account)],
) -> Vec<AccountChange> {
    let mut changes = Vec::new();
    for (pubkey, post_acct) in post_accounts {
        let (owner_before, lamports_before, data_len_before) = match pre.get(pubkey) {
            Some(snap) => (snap.owner, snap.lamports, snap.data_len),
            None => (Pubkey::default(), 0, 0),
        };
        let owner_after = post_acct.owner;
        let lamports_after = post_acct.lamports;
        let data_len_after = post_acct.data.len();

        // Only record accounts that actually changed.
        if owner_before != owner_after
            || lamports_before != lamports_after
            || data_len_before != data_len_after
        {
            changes.push(AccountChange {
                pubkey: *pubkey,
                owner_before,
                owner_after,
                lamports_before,
                lamports_after,
                data_len_before,
                data_len_after,
            });
        }
    }
    changes
}

/// Build the unified program-invocation call graph: top-level instructions
/// from the input transaction (Layer 1, stack_height=1) followed by inner
/// CPIs surfaced by execution metadata (Layer 2, stack_height>=2).
///
/// Layer 1 always populates — even when simulation fails before execution
/// (e.g., stale blockhash on a captured fixture). Without this, checkers
/// like Authority Diff would see an empty graph for the Drift exploit
/// fixture and produce false negatives. Same two-layer pattern as
/// `detect_oracle_reads`.
fn extract_cpi_graph(
    tx: &Transaction,
    inner_instructions: &litesvm_message::inner_instruction::InnerInstructionsList,
    account_keys: &[Pubkey],
) -> Vec<CpiCall> {
    let mut calls = Vec::new();

    // Layer 1: top-level instructions, derived from the input tx bytes alone.
    // These are the calls the user signed; they are part of the call graph
    // regardless of whether simulation makes any forward progress.
    for (ix_index, ix) in tx.message.instructions.iter().enumerate() {
        let program_id = account_keys
            .get(ix.program_id_index as usize)
            .copied()
            .unwrap_or_default();

        let accounts: Vec<Pubkey> = ix
            .accounts
            .iter()
            .map(|&idx| account_keys.get(idx as usize).copied().unwrap_or_default())
            .collect();

        calls.push(CpiCall {
            program_id,
            instruction_index: ix_index,
            stack_height: 1,
            accounts,
            data: ix.data.clone(),
        });
    }

    // Layer 2: inner CPIs from execution metadata. Only populated when
    // simulation runs at least far enough to invoke a CPI.
    for (ix_index, inner_ixs) in inner_instructions.iter().enumerate() {
        for inner in inner_ixs {
            let program_id_index = inner.instruction.program_id_index as usize;

            // Bounds-check: guard against ALT transactions or malformed data.
            let program_id = account_keys
                .get(program_id_index)
                .copied()
                .unwrap_or_default();

            let accounts: Vec<Pubkey> = inner
                .instruction
                .accounts
                .iter()
                .map(|&idx| account_keys.get(idx as usize).copied().unwrap_or_default())
                .collect();

            calls.push(CpiCall {
                program_id,
                instruction_index: ix_index,
                stack_height: inner.stack_height,
                accounts,
                data: inner.instruction.data.clone(),
            });
        }
    }

    calls
}

/// Detect oracle reads from both input instructions AND execution logs.
///
/// Two-layer detection ensures oracle reads are captured even when the transaction
/// fails before execution (e.g., blockhash mismatch). Without this, the Oracle
/// Sanity checker would see empty reads on blocked txs — a silent false negative.
///
/// Layer 1 (input-based): Scan tx.message.instructions for any instruction whose
/// program_id is Switchboard or Pyth. Accounts referenced by those instructions
/// (excluding the program itself) are oracle feed candidates.
///
/// Layer 2 (log-based): Confirm or discover additional oracle invocations from
/// execution logs (catches CPI-based oracle reads not visible in top-level ixs).
fn detect_oracle_reads(tx: &Transaction, logs: &[String]) -> Vec<OracleRead> {
    let switchboard_id: Pubkey = SWITCHBOARD_V2_PROGRAM_ID.parse().unwrap();
    let pyth_id: Pubkey = PYTH_PROGRAM_ID.parse().unwrap();
    let account_keys = &tx.message.account_keys;

    let mut reads = Vec::new();
    let mut seen_pubkeys = std::collections::HashSet::new();

    // Layer 1: Scan input instructions for direct oracle program invocations.
    for ix in &tx.message.instructions {
        let program_idx = ix.program_id_index as usize;
        let program_id = match account_keys.get(program_idx) {
            Some(id) => *id,
            None => continue,
        };

        let oracle_type = if program_id == switchboard_id {
            "switchboard"
        } else if program_id == pyth_id {
            "pyth"
        } else {
            continue;
        };

        // Every non-program account referenced by an oracle instruction
        // is a potential oracle feed account.
        for &acct_idx in &ix.accounts {
            if let Some(&pubkey) = account_keys.get(acct_idx as usize) {
                if pubkey != switchboard_id
                    && pubkey != pyth_id
                    && seen_pubkeys.insert(pubkey)
                {
                    reads.push(OracleRead {
                        oracle_pubkey: pubkey,
                        oracle_type: oracle_type.to_string(),
                    });
                }
            }
        }
    }

    // Layer 2: Check logs for CPI-based oracle invocations not visible in top-level ixs.
    let mut log_switchboard = false;
    let mut log_pyth = false;
    for log in logs {
        if !log_switchboard && log.contains(SWITCHBOARD_V2_PROGRAM_ID) {
            log_switchboard = true;
        }
        if !log_pyth && log.contains(PYTH_PROGRAM_ID) {
            log_pyth = true;
        }
    }

    // If logs show an oracle program that wasn't in top-level instructions,
    // add all non-program accounts as candidates (coarse — checkers refine).
    if log_switchboard && !reads.iter().any(|r| r.oracle_type == "switchboard") {
        for key in account_keys {
            if *key != switchboard_id && *key != pyth_id && seen_pubkeys.insert(*key) {
                reads.push(OracleRead {
                    oracle_pubkey: *key,
                    oracle_type: "switchboard".to_string(),
                });
            }
        }
    }
    if log_pyth && !reads.iter().any(|r| r.oracle_type == "pyth") {
        for key in account_keys {
            if *key != switchboard_id && *key != pyth_id && seen_pubkeys.insert(*key) {
                reads.push(OracleRead {
                    oracle_pubkey: *key,
                    oracle_type: "pyth".to_string(),
                });
            }
        }
    }

    reads
}

/// Detect SPL Token Approve instructions from the input transaction.
///
/// SPL Token Approve is instruction discriminator 4.
/// Layout: [4] [amount: u64 LE]
/// Accounts: [source, delegate, owner]
///
/// Scans input instructions, not execution results — detectable even if the tx fails.
pub(crate) fn detect_token_approvals(tx: &Transaction) -> Vec<TokenApproval> {
    let spl_token_id: Pubkey = SPL_TOKEN_PROGRAM_ID.parse().unwrap();
    let mut approvals = Vec::new();

    for ix in &tx.message.instructions {
        // Resolve program ID from index.
        let program_idx = ix.program_id_index as usize;
        let program_id = match tx.message.account_keys.get(program_idx) {
            Some(id) => id,
            None => continue,
        };

        if *program_id != spl_token_id {
            continue;
        }

        // SPL Token Approve: discriminator == 4, data length == 9 (1 + 8 bytes).
        if ix.data.len() == 9 && ix.data[0] == 4 {
            let amount = u64::from_le_bytes(
                ix.data[1..9].try_into().unwrap_or([0u8; 8]),
            );

            // Accounts: [source, delegate, owner, ...signers]
            if ix.accounts.len() >= 3 {
                let source = tx
                    .message
                    .account_keys
                    .get(ix.accounts[0] as usize)
                    .copied()
                    .unwrap_or_default();
                let delegate = tx
                    .message
                    .account_keys
                    .get(ix.accounts[1] as usize)
                    .copied()
                    .unwrap_or_default();
                let owner = tx
                    .message
                    .account_keys
                    .get(ix.accounts[2] as usize)
                    .copied()
                    .unwrap_or_default();

                approvals.push(TokenApproval {
                    source,
                    delegate,
                    amount,
                    owner,
                });
            }
        }
    }

    approvals
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(deprecated)] // solana-sdk 2.2 deprecates for 3.x migration; we stay on 2.2
    use solana_sdk::system_instruction;
    use solana_sdk::signature::Keypair;
    use solana_sdk::signer::Signer;

    /// Bridge LiteSVM's blockhash to solana-sdk 2.2 Hash.
    fn litesvm_blockhash_to_sdk(svm: &LiteSVM) -> solana_sdk::hash::Hash {
        solana_sdk::hash::Hash::new_from_array(svm.latest_blockhash().to_bytes())
    }

    /// Bridge solana-sdk 2.2 Pubkey → litesvm Address for airdrop.
    fn sdk_pubkey_to_litesvm(pubkey: &Pubkey) -> litesvm_address::Address {
        litesvm_address::Address::from(pubkey.to_bytes())
    }

    #[test]
    fn test_sol_transfer_balance_deltas() {
        let mut fork = ForkSimulator::new_offline();
        let sender = Keypair::new();
        let receiver = Pubkey::new_unique();
        let transfer_amount = 1_000_000u64; // 0.001 SOL

        // Airdrop to sender and receiver via LiteSVM.
        fork.svm_mut()
            .airdrop(&sdk_pubkey_to_litesvm(&sender.pubkey()), 10_000_000_000)
            .expect("airdrop sender");
        fork.svm_mut()
            .airdrop(&sdk_pubkey_to_litesvm(&receiver), 1_000_000_000)
            .expect("airdrop receiver");

        // Build a simple SOL transfer.
        let ix = system_instruction::transfer(&sender.pubkey(), &receiver, transfer_amount);
        let blockhash = litesvm_blockhash_to_sdk(fork.svm());
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&sender.pubkey()),
            &[&sender],
            blockhash,
        );

        let trace = execute_transaction(&mut fork, &tx).expect("execute should succeed");

        assert!(trace.success, "transaction should succeed");
        assert!(trace.error.is_none());
        assert!(!trace.logs.is_empty(), "should have program logs");
        assert!(trace.fee > 0, "should charge a fee");

        // Sender: lost transfer_amount + fees.
        let sender_delta = trace.balance_deltas.get(&sender.pubkey());
        assert!(
            sender_delta.is_some(),
            "sender should have a balance delta"
        );
        assert!(
            sender_delta.unwrap() < &-(transfer_amount as i64),
            "sender delta ({}) should be more negative than -{}",
            sender_delta.unwrap(),
            transfer_amount
        );

        // Receiver: gained exactly transfer_amount.
        let receiver_delta = trace.balance_deltas.get(&receiver);
        assert_eq!(
            receiver_delta,
            Some(&(transfer_amount as i64)),
            "receiver should gain exactly the transfer amount"
        );
    }

    #[test]
    fn test_failed_transaction_captures_error() {
        let mut fork = ForkSimulator::new_offline();
        let sender = Keypair::new();
        let receiver = Pubkey::new_unique();

        // Give sender enough for rent + fees but not for the large transfer.
        fork.svm_mut()
            .airdrop(&sdk_pubkey_to_litesvm(&sender.pubkey()), 1_000_000)
            .expect("airdrop");

        // Try to transfer far more than the sender has.
        let ix = system_instruction::transfer(&sender.pubkey(), &receiver, 1_000_000_000);
        let blockhash = litesvm_blockhash_to_sdk(fork.svm());
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&sender.pubkey()),
            &[&sender],
            blockhash,
        );

        let trace = execute_transaction(&mut fork, &tx).expect("should return trace even on failure");

        assert!(!trace.success, "transaction should fail");
        assert!(trace.error.is_some(), "error message should be set");
        assert!(
            trace.balance_deltas.is_empty(),
            "no balance changes on failed tx"
        );
    }

    #[test]
    fn test_drift_fixture_trace_capture() {
        let fixture = ciel_fixtures::load_drift_fixture().expect("load fixture");
        let mut fork = ForkSimulator::new_offline();

        // Inject fixture accounts — some may fail (LiteSVM rejects BPF program stubs).
        let mut loaded = 0usize;
        for (pubkey, account) in &fixture.accounts {
            match fork.set_account(pubkey, account) {
                Ok(()) => loaded += 1,
                Err(_) => {
                    // All rejected accounts must be executable (BPF stubs).
                    assert!(
                        account.executable,
                        "non-program account {pubkey} was rejected — fixture format issue"
                    );
                }
            }
        }
        assert!(loaded > 0, "at least some accounts should load");

        // Execute — fails (stale blockhash, missing BPF programs) but trace is captured.
        let trace =
            execute_transaction(&mut fork, &fixture.transaction).expect("should return trace");

        // On successful execution, verify we get real trace data.
        if trace.success {
            assert!(
                !trace.balance_deltas.is_empty(),
                "successful execution should show balance changes"
            );
        }

        // --- Critical assertion: input-derived fields must populate even on failure ---
        // The synthetic fixture has a Switchboard oracle update instruction, so
        // oracle_reads MUST detect the Switchboard feed accounts regardless of execution.
        assert!(
            !trace.oracle_reads.is_empty(),
            "oracle_reads must detect Switchboard references from input instructions \
             even when the transaction fails (blockhash mismatch). Empty oracle_reads \
             would cause false negatives in the Oracle Sanity checker."
        );
        assert!(
            trace.oracle_reads.iter().any(|r| r.oracle_type == "switchboard"),
            "should detect Switchboard oracle reads from the fixture's oracle update instruction"
        );

        // Token approvals: the synthetic fixture uses SetAuthority (discriminator 6),
        // not Approve (discriminator 4), so token_approvals is correctly empty here.
        // This is NOT a gap — the Authority Diff checker handles SetAuthority separately.

        // CPI graph Layer 1 must populate from input instructions even when the
        // tx fails. The synthetic fixture has 3 top-level instructions
        // (Switchboard update, SetAuthority, Transfer), so cpi_graph must have
        // at least 3 entries with stack_height=1.
        let top_level_count = trace
            .cpi_graph
            .iter()
            .filter(|c| c.stack_height == 1)
            .count();
        assert_eq!(
            top_level_count, 3,
            "Synthetic fixture has 3 top-level instructions; expected 3 \
             cpi_graph entries with stack_height=1, got {top_level_count}"
        );
    }

    #[test]
    fn test_real_drift_fixture_trace_capture() {
        // Real Drift exploit fixture: admin key transfer via Squads multisig (slot 410344009).
        // This is the actual transaction from the Drift exploit — more important for the demo
        // than the synthetic fixture.
        //
        // Tx structure (from real_exploit_metadata.json):
        //   ix[0]: System AdvanceNonce
        //   ix[1]: Squads ProposalApprove
        //   ix[2]: Squads VaultTransactionExecute → CPIs into Drift UpdateAdmin
        //
        // The CPI graph MUST capture the Squads-to-Drift call structure, or the
        // Authority Diff checker (Unit 11) will produce false negatives on the
        // single most important demo fixture.
        let fixture = match ciel_fixtures::load_drift_real_fixture() {
            Ok(f) => f,
            Err(e) => {
                println!("Skipping real fixture test (not on disk): {e}");
                return;
            }
        };

        let mut fork = ForkSimulator::new_offline();

        // Inject fixture accounts. Real fixture has no BPF stubs — all should load.
        for (pubkey, account) in &fixture.accounts {
            fork.set_account(pubkey, account)
                .unwrap_or_else(|e| panic!("real fixture account {pubkey} rejected: {e}"));
        }

        let trace =
            execute_transaction(&mut fork, &fixture.transaction).expect("should return trace");

        // Real fixture fails at blockhash check (account state was captured at
        // current slot, not slot 410344009). Trace must still populate from input.
        assert!(
            trace.error.is_some(),
            "real fixture tx should fail (stale blockhash); err was: {:?}",
            trace.error
        );

        let squads_pid = ciel_fixtures::drift::squads_program_id();
        let drift_pid = ciel_fixtures::drift::drift_program_id();

        // CPI graph must NOT be empty. Even though execution fails before any
        // CPI runs, Layer 1 (top-level instructions) must populate the graph.
        // An empty cpi_graph here would cause Unit 11's Authority Diff to say
        // "no SetAuthority/UpdateAdmin found" because the graph was empty,
        // not because the transaction was safe — a silent false negative.
        assert!(
            !trace.cpi_graph.is_empty(),
            "cpi_graph must not be empty; Layer 1 (top-level instructions) \
             should populate even when simulation fails before execution. \
             Empty cpi_graph causes Authority Diff false negatives."
        );

        // Squads must appear as a top-level program — the multisig executes the
        // attack via VaultTransactionExecute (ix[2]) and ProposalApprove (ix[1]).
        let squads_top_levels: Vec<&CpiCall> = trace
            .cpi_graph
            .iter()
            .filter(|c| c.program_id == squads_pid && c.stack_height == 1)
            .collect();
        assert!(
            !squads_top_levels.is_empty(),
            "Expected Squads ({squads_pid}) as a top-level program in cpi_graph; \
             got program_ids: {:?}",
            trace
                .cpi_graph
                .iter()
                .map(|c| c.program_id)
                .collect::<Vec<_>>()
        );

        // Drift must be reachable from the call graph. With a successful
        // execution, Drift would appear as its own cpi_graph entry with
        // stack_height >= 2 (the inner CPI from Squads VaultTransactionExecute).
        // With execution that fails at blockhash check (this fixture), the inner
        // CPI didn't run — but Drift is still discoverable as an account in the
        // Squads VaultTransactionExecute instruction's `accounts` list, because
        // Squads passes the inner program in `remaining_accounts`. The Authority
        // Diff checker (Unit 11) handles both shapes.
        let drift_as_program = trace.cpi_graph.iter().any(|c| c.program_id == drift_pid);
        let drift_in_accounts = trace
            .cpi_graph
            .iter()
            .any(|c| c.accounts.contains(&drift_pid));
        assert!(
            drift_as_program || drift_in_accounts,
            "Expected Drift ({drift_pid}) somewhere in cpi_graph: either as a \
             CPI program_id (successful execution) or in a top-level instruction's \
             accounts (Squads remaining_accounts). Found neither. cpi_graph: {:?}",
            trace
                .cpi_graph
                .iter()
                .map(|c| (c.program_id, c.stack_height, c.accounts.len()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_token_approval_detection() {
        // Hand-craft a transaction with an SPL Token Approve instruction.
        let payer = Keypair::new();
        let source = Pubkey::new_unique();
        let delegate = Pubkey::new_unique();
        let owner = payer.pubkey();
        let spl_token_id: Pubkey = SPL_TOKEN_PROGRAM_ID.parse().unwrap();

        // SPL Token Approve: discriminator 4, amount as u64 LE.
        let amount: u64 = 500_000;
        let mut data = vec![4u8];
        data.extend_from_slice(&amount.to_le_bytes());

        let ix = solana_sdk::instruction::Instruction {
            program_id: spl_token_id,
            accounts: vec![
                solana_sdk::instruction::AccountMeta::new(source, false),
                solana_sdk::instruction::AccountMeta::new_readonly(delegate, false),
                solana_sdk::instruction::AccountMeta::new_readonly(owner, true),
            ],
            data,
        };

        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&payer.pubkey()),
            &[&payer],
            solana_sdk::hash::Hash::default(),
        );

        let approvals = detect_token_approvals(&tx);
        assert_eq!(approvals.len(), 1, "should detect one approval");
        assert_eq!(approvals[0].source, source);
        assert_eq!(approvals[0].delegate, delegate);
        assert_eq!(approvals[0].amount, amount);
        assert_eq!(approvals[0].owner, owner);
    }

    #[test]
    fn test_simulation_latency_p50_under_40ms() {
        let mut fork = ForkSimulator::new_offline();
        let sender = Keypair::new();
        let receiver = Pubkey::new_unique();

        fork.svm_mut()
            .airdrop(&sdk_pubkey_to_litesvm(&sender.pubkey()), 10_000_000_000)
            .expect("airdrop");
        fork.svm_mut()
            .airdrop(&sdk_pubkey_to_litesvm(&receiver), 1_000_000_000)
            .expect("airdrop");

        let mut durations = Vec::with_capacity(10);

        for i in 0u64..10 {
            let ix = system_instruction::transfer(
                &sender.pubkey(),
                &receiver,
                1_000 + i, // vary amount to avoid tx dedup
            );
            let blockhash = litesvm_blockhash_to_sdk(fork.svm());
            let tx = Transaction::new_signed_with_payer(
                &[ix],
                Some(&sender.pubkey()),
                &[&sender],
                blockhash,
            );

            let start = tokio::time::Instant::now();
            let trace = execute_transaction(&mut fork, &tx);
            let elapsed = start.elapsed();
            durations.push(elapsed);

            // Simulation is readonly (simulate_transaction), so the sender balance
            // doesn't actually decrease. Each iteration starts from the same state.
            assert!(trace.is_ok(), "iteration {i} should not error");
        }

        durations.sort();
        let p50 = durations[4]; // 0-indexed: index 4 is the 5th value = P50

        // Relaxed target for early development: 40ms.
        // Production target is 20ms P50 (spec Section 1.5).
        assert!(
            p50.as_millis() < 40,
            "P50 latency {}ms exceeds 40ms target",
            p50.as_millis()
        );

        tracing::info!(
            p50_ms = p50.as_millis(),
            p95_ms = durations[9].as_millis(),
            min_ms = durations[0].as_millis(),
            "simulation latency benchmark"
        );
    }
}
