// Stub checker implementations. Each returns passed: true with no findings.
// These are placeholders until real checkers are implemented in Units 10-14, 24-25.
// See spec Section 2.2 for the list of 7 checkers.

use async_trait::async_trait;

use crate::traits::{Checker, CheckerContext, CheckerOutput, Severity};

/// Generate a stub checker that always passes.
macro_rules! stub_checker {
    ($struct_name:ident, $checker_name:expr) => {
        pub struct $struct_name;

        #[async_trait]
        impl Checker for $struct_name {
            fn name(&self) -> &'static str {
                $checker_name
            }

            async fn check(&self, _ctx: &CheckerContext) -> CheckerOutput {
                CheckerOutput {
                    checker_name: $checker_name.to_string(),
                    passed: true,
                    severity: Severity::None,
                    flags: vec![],
                    details: "stub".to_string(),
                }
            }
        }
    };
}

stub_checker!(OracleSanityStub, "oracle_sanity");
stub_checker!(AuthorityDiffStub, "authority_diff");
stub_checker!(IntentDiffStub, "intent_diff");
stub_checker!(ContagionMapStub, "contagion_map");
stub_checker!(MevSandwichStub, "mev_sandwich");
stub_checker!(ApprovalAbuseStub, "approval_abuse");
stub_checker!(SimSpoofStub, "sim_spoof");

/// Returns all 7 stub checkers. Used for testing and as the initial checker set
/// before real implementations are wired in.
pub fn all_stub_checkers() -> Vec<Box<dyn Checker>> {
    vec![
        Box::new(OracleSanityStub),
        Box::new(AuthorityDiffStub),
        Box::new(IntentDiffStub),
        Box::new(ContagionMapStub),
        Box::new(MevSandwichStub),
        Box::new(ApprovalAbuseStub),
        Box::new(SimSpoofStub),
    ]
}
