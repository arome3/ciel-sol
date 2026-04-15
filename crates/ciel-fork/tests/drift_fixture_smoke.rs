// Smoke test: load real Drift exploit fixture into ForkSimulator.
// Validates that Unit 00.5 (fixtures) and Unit 01 (simulator) integrate correctly.

use ciel_fixtures::load_drift_real_fixture;
use ciel_fork::ForkSimulator;

#[tokio::test]
#[ignore] // hermetic (no network), but slow; run explicitly with --ignored
async fn simulator_loads_real_drift_fixture() {
    let fixture = load_drift_real_fixture().expect("load fixture");
    let mut sim = ForkSimulator::new_offline();

    // Inject all 11 accounts from the real exploit fixture
    for (pubkey, account) in &fixture.accounts {
        sim.set_account(pubkey, account).expect("set account");
    }

    assert_eq!(fixture.accounts.len(), 11, "fixture should have 11 accounts");
    assert_eq!(sim.cache().len(), 11, "all accounts should be cached");

    // Bincode roundtrip: v2 Transaction → bytes → v3 Transaction → VersionedTransaction
    let tx_bytes = bincode::serialize(&fixture.transaction).expect("serialize v2 tx");
    let v3_tx: litesvm_transaction::Transaction =
        bincode::deserialize(&tx_bytes).expect("v2/v3 Transaction bincode-compatible");
    let versioned_tx = litesvm_transaction::versioned::VersionedTransaction::from(v3_tx);

    // The transaction was signed with an old blockhash (slot 410344009).
    // A fresh LiteSVM fork won't accept it — send_transaction will fail.
    // What we're testing: set_account + send_transaction don't panic,
    // and the fixture format is compatible with the simulator.
    let result = sim.svm_mut().send_transaction(versioned_tx);
    // Transaction failure is expected (blockhash mismatch, missing programs, etc.)
    // Success OR failure is fine — a panic is not.
    match &result {
        Ok(meta) => println!("tx succeeded (unexpected): sig={}", meta.signature),
        Err(failed) => println!("tx failed (expected): {:?}", failed.err),
    }
}
