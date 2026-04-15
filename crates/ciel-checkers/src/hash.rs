// Deterministic hash of checker outputs for CielAttestation.checker_outputs_hash.
// See spec Section 7.1.

use sha2::{Digest, Sha256};

use crate::traits::{CheckerResults, CheckerStatus};

/// Compute the SHA-256 hash of all completed checker outputs.
///
/// Algorithm:
/// 1. Filter to `Completed` outputs only (timed-out checkers excluded).
/// 2. Sort by `checker_name` (alphabetical) for deterministic ordering.
/// 3. Borsh-serialize each `CheckerOutput`.
/// 4. Concatenate serialized bytes and SHA-256 hash.
///
/// The resulting `[u8; 32]` is stored in `CielAttestation.checker_outputs_hash`.
/// See spec Section 7.1.
pub fn checker_outputs_hash(results: &CheckerResults) -> [u8; 32] {
    let mut completed: Vec<_> = results
        .outputs
        .iter()
        .filter_map(|(_, status)| match status {
            CheckerStatus::Completed(output) => Some(output),
            CheckerStatus::TimedOut => None,
        })
        .collect();

    // Sort by checker_name for deterministic ordering — HashMap iteration
    // order is randomized.
    completed.sort_by(|a, b| a.checker_name.cmp(&b.checker_name));

    let mut hasher = Sha256::new();
    for output in &completed {
        let bytes = borsh::to_vec(output)
            .expect("CheckerOutput Borsh serialization should not fail");
        hasher.update(&bytes);
    }

    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{CheckerOutput, Flag, Severity};
    use serde_json::json;
    use std::collections::HashMap;

    fn make_output(name: &str, passed: bool, severity: Severity) -> CheckerOutput {
        CheckerOutput {
            checker_name: name.to_string(),
            passed,
            severity,
            flags: vec![Flag {
                code: format!("{}_FLAG", name.to_uppercase()),
                message: format!("{} finding", name),
                data: json!({"checker": name}),
            }],
            details: format!("{} details", name),
        }
    }

    fn make_results(entries: Vec<(&str, CheckerStatus)>) -> CheckerResults {
        CheckerResults {
            outputs: entries
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
            total_duration_ms: 10,
        }
    }

    #[test]
    fn test_hash_determinism() {
        let results = make_results(vec![
            (
                "oracle_sanity",
                CheckerStatus::Completed(make_output("oracle_sanity", true, Severity::None)),
            ),
            (
                "authority_diff",
                CheckerStatus::Completed(make_output("authority_diff", false, Severity::High)),
            ),
            (
                "sim_spoof",
                CheckerStatus::Completed(make_output("sim_spoof", true, Severity::None)),
            ),
        ]);

        let hash1 = checker_outputs_hash(&results);
        let hash2 = checker_outputs_hash(&results);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_different_inputs() {
        let results_a = make_results(vec![(
            "oracle_sanity",
            CheckerStatus::Completed(make_output("oracle_sanity", true, Severity::None)),
        )]);
        let results_b = make_results(vec![(
            "oracle_sanity",
            CheckerStatus::Completed(make_output("oracle_sanity", false, Severity::Critical)),
        )]);

        let hash_a = checker_outputs_hash(&results_a);
        let hash_b = checker_outputs_hash(&results_b);
        assert_ne!(hash_a, hash_b);
    }

    #[test]
    fn test_hash_order_independence() {
        // Insert outputs in different orders into two HashMaps.
        let mut map_a = HashMap::new();
        map_a.insert(
            "alpha".to_string(),
            CheckerStatus::Completed(make_output("alpha", true, Severity::None)),
        );
        map_a.insert(
            "beta".to_string(),
            CheckerStatus::Completed(make_output("beta", true, Severity::Low)),
        );
        map_a.insert(
            "gamma".to_string(),
            CheckerStatus::Completed(make_output("gamma", false, Severity::High)),
        );

        let mut map_b = HashMap::new();
        map_b.insert(
            "gamma".to_string(),
            CheckerStatus::Completed(make_output("gamma", false, Severity::High)),
        );
        map_b.insert(
            "alpha".to_string(),
            CheckerStatus::Completed(make_output("alpha", true, Severity::None)),
        );
        map_b.insert(
            "beta".to_string(),
            CheckerStatus::Completed(make_output("beta", true, Severity::Low)),
        );

        let results_a = CheckerResults {
            outputs: map_a,
            total_duration_ms: 10,
        };
        let results_b = CheckerResults {
            outputs: map_b,
            total_duration_ms: 10,
        };

        assert_eq!(
            checker_outputs_hash(&results_a),
            checker_outputs_hash(&results_b)
        );
    }

    #[test]
    fn test_hash_excludes_timed_out() {
        // Results with 2 completed + 1 timed out.
        let with_timeout = make_results(vec![
            (
                "oracle_sanity",
                CheckerStatus::Completed(make_output("oracle_sanity", true, Severity::None)),
            ),
            (
                "authority_diff",
                CheckerStatus::Completed(make_output("authority_diff", true, Severity::None)),
            ),
            ("slow_checker", CheckerStatus::TimedOut),
        ]);

        // Same 2 completed, no timed out.
        let without_timeout = make_results(vec![
            (
                "oracle_sanity",
                CheckerStatus::Completed(make_output("oracle_sanity", true, Severity::None)),
            ),
            (
                "authority_diff",
                CheckerStatus::Completed(make_output("authority_diff", true, Severity::None)),
            ),
        ]);

        assert_eq!(
            checker_outputs_hash(&with_timeout),
            checker_outputs_hash(&without_timeout)
        );
    }
}
