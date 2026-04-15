// Stub scorer implementing the penalty-based safety_score from spec Section 6.1.
// This is a temporary implementation — Unit 15 will replace it with the real scorer
// that includes optimality_score and more sophisticated penalty curves.

use ciel_checkers::{CheckerResults, CheckerStatus, Severity};
use ciel_signer::Verdict;

/// Compute safety score from checker results using the penalty-based algorithm.
/// See spec Section 6.1.
///
/// Algorithm: start at 1.0, subtract a penalty per checker output based on severity.
/// Timed-out checkers incur a mild 0.10 penalty (uncertain = conservative).
/// Result clamped to [0.0, 1.0].
pub fn compute_safety_score(results: &CheckerResults) -> f64 {
    let mut score = 1.0_f64;

    for status in results.outputs.values() {
        match status {
            CheckerStatus::Completed(output) => {
                let penalty = match output.severity {
                    Severity::None => 0.0,
                    Severity::Low => 0.05,
                    Severity::Medium => 0.15,
                    Severity::High => 0.40,
                    Severity::Critical => 1.0,
                };
                score -= penalty;
            }
            CheckerStatus::TimedOut => {
                score -= 0.10;
            }
        }
    }

    score.clamp(0.0, 1.0)
}

/// Convert a safety score to a verdict, applying the Critical short-circuit rule.
/// See spec Section 6.1 thresholds.
///
/// - Any single `Critical` severity → immediate `Block` (regardless of score)
/// - score >= 0.7 → `Approve`
/// - 0.4 <= score < 0.7 → `Warn`
/// - score < 0.4 → `Block`
pub fn score_to_verdict(score: f64, results: &CheckerResults) -> Verdict {
    // Short-circuit: any Critical finding → immediate BLOCK.
    let has_critical = results.completed().iter().any(|o| o.severity == Severity::Critical);
    if has_critical {
        return Verdict::Block;
    }

    if score >= 0.7 {
        Verdict::Approve
    } else if score >= 0.4 {
        Verdict::Warn
    } else {
        Verdict::Block
    }
}

/// Encode a float safety_score [0.0, 1.0] as fixed-point u16 [0, 10000].
/// See spec Section 7.1: `safety_score` field is `score * 10000`.
pub fn encode_score_u16(score: f64) -> u16 {
    (score.clamp(0.0, 1.0) * 10000.0).round() as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use ciel_checkers::{CheckerOutput, CheckerStatus, Flag, Severity};

    fn make_output(name: &str, severity: Severity) -> CheckerOutput {
        let passed = severity == Severity::None;
        CheckerOutput {
            checker_name: name.to_string(),
            passed,
            severity,
            flags: if passed {
                vec![]
            } else {
                vec![Flag {
                    code: format!("{}_FLAG", name.to_uppercase()),
                    message: format!("{} finding", name),
                    data: serde_json::json!({}),
                }]
            },
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

    // --- compute_safety_score tests ---

    #[test]
    fn test_all_checkers_pass_score_1_0() {
        let results = make_results(vec![
            ("a", CheckerStatus::Completed(make_output("a", Severity::None))),
            ("b", CheckerStatus::Completed(make_output("b", Severity::None))),
            ("c", CheckerStatus::Completed(make_output("c", Severity::None))),
        ]);
        let score = compute_safety_score(&results);
        assert!((score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_single_critical_score_0_0() {
        let results = make_results(vec![
            ("a", CheckerStatus::Completed(make_output("a", Severity::None))),
            ("b", CheckerStatus::Completed(make_output("b", Severity::Critical))),
        ]);
        let score = compute_safety_score(&results);
        assert!((score - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_penalty_accumulation() {
        // Low(0.05) + Medium(0.15) + High(0.40) = 0.60 penalty → score 0.40
        let results = make_results(vec![
            ("a", CheckerStatus::Completed(make_output("a", Severity::Low))),
            ("b", CheckerStatus::Completed(make_output("b", Severity::Medium))),
            ("c", CheckerStatus::Completed(make_output("c", Severity::High))),
        ]);
        let score = compute_safety_score(&results);
        assert!((score - 0.40).abs() < 1e-10, "expected 0.40, got {score}");
    }

    #[test]
    fn test_timed_out_penalty() {
        let results = make_results(vec![
            ("a", CheckerStatus::Completed(make_output("a", Severity::None))),
            ("b", CheckerStatus::TimedOut),
        ]);
        let score = compute_safety_score(&results);
        assert!((score - 0.90).abs() < 1e-10, "expected 0.90, got {score}");
    }

    #[test]
    fn test_score_floor_clamp() {
        // Two Criticals: 1.0 + 1.0 = 2.0 penalty → clamped to 0.0
        let results = make_results(vec![
            ("a", CheckerStatus::Completed(make_output("a", Severity::Critical))),
            ("b", CheckerStatus::Completed(make_output("b", Severity::Critical))),
        ]);
        let score = compute_safety_score(&results);
        assert!((score - 0.0).abs() < f64::EPSILON);
    }

    // --- score_to_verdict tests ---

    #[test]
    fn test_verdict_approve_at_0_70() {
        let results = make_results(vec![]);
        assert_eq!(score_to_verdict(0.70, &results), Verdict::Approve);
    }

    #[test]
    fn test_verdict_warn_below_0_70() {
        let results = make_results(vec![]);
        assert_eq!(score_to_verdict(0.6999, &results), Verdict::Warn);
    }

    #[test]
    fn test_verdict_block_below_0_40() {
        let results = make_results(vec![]);
        assert_eq!(score_to_verdict(0.3999, &results), Verdict::Block);
    }

    #[test]
    fn test_critical_forces_block_even_with_high_score() {
        // Score is 0.0 because Critical penalty is 1.0, but even if we pass
        // an artificial high score, the Critical short-circuit should fire.
        let results = make_results(vec![
            ("a", CheckerStatus::Completed(make_output("a", Severity::Critical))),
        ]);
        assert_eq!(score_to_verdict(0.95, &results), Verdict::Block);
    }

    // --- encode_score_u16 tests ---

    #[test]
    fn test_encode_score_u16() {
        assert_eq!(encode_score_u16(0.75), 7500);
        assert_eq!(encode_score_u16(1.0), 10000);
        assert_eq!(encode_score_u16(0.0), 0);
        assert_eq!(encode_score_u16(0.5), 5000);
        // Clamp above 1.0
        assert_eq!(encode_score_u16(1.5), 10000);
        // Clamp below 0.0
        assert_eq!(encode_score_u16(-0.5), 0);
    }
}
