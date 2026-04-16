// Known-program registry. Populated subset of Solana mainnet protocols the
// Authority Diff checker treats as "known-good" — authority changes targeting
// these warrant a Critical severity bump. See spec Section 4.3.2 and
// docs/11-authority-diff-checker.md.

use std::collections::HashMap;

use solana_sdk::pubkey;
use solana_sdk::pubkey::Pubkey;

// ---------------------------------------------------------------------------
// Well-known program IDs
// ---------------------------------------------------------------------------

pub const SPL_TOKEN_PROGRAM_ID: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
pub const SPL_TOKEN_2022_PROGRAM_ID: Pubkey = pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
pub const BPF_LOADER_UPGRADEABLE_PROGRAM_ID: Pubkey =
    pubkey!("BPFLoaderUpgradeab1e11111111111111111111111");
pub const DRIFT_V2_PROGRAM_ID: Pubkey = pubkey!("dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH");
pub const RAYDIUM_AMM_V4_PROGRAM_ID: Pubkey =
    pubkey!("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8");
pub const JUPITER_V6_PROGRAM_ID: Pubkey = pubkey!("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4");
pub const ORCA_WHIRLPOOL_PROGRAM_ID: Pubkey =
    pubkey!("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc");
pub const SQUADS_V4_PROGRAM_ID: Pubkey = pubkey!("SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf");

// ---------------------------------------------------------------------------
// ProgramRegistry
// ---------------------------------------------------------------------------

/// Registry of known-good Solana programs keyed by program ID.
///
/// Checkers use this to decide whether an operation targeting a pubkey should
/// be treated as a known-protocol action (higher severity on authority changes,
/// lower false-positive rate on routine CPIs). `Default` returns an empty
/// registry — use `with_mainnet_defaults()` for the curated production set.
#[derive(Debug, Clone)]
pub struct ProgramRegistry {
    known_protocols: HashMap<Pubkey, &'static str>,
}

impl ProgramRegistry {
    pub fn new() -> Self {
        Self {
            known_protocols: HashMap::new(),
        }
    }

    pub fn with_mainnet_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(SPL_TOKEN_PROGRAM_ID, "SPL Token");
        registry.register(SPL_TOKEN_2022_PROGRAM_ID, "SPL Token-2022");
        registry.register(BPF_LOADER_UPGRADEABLE_PROGRAM_ID, "BPF Loader Upgradeable");
        registry.register(DRIFT_V2_PROGRAM_ID, "Drift v2");
        registry.register(RAYDIUM_AMM_V4_PROGRAM_ID, "Raydium AMM v4");
        registry.register(JUPITER_V6_PROGRAM_ID, "Jupiter V6");
        registry.register(ORCA_WHIRLPOOL_PROGRAM_ID, "Orca Whirlpool");
        registry.register(SQUADS_V4_PROGRAM_ID, "Squads v4");
        registry
    }

    pub fn is_known_protocol(&self, program_id: &Pubkey) -> Option<&'static str> {
        self.known_protocols.get(program_id).copied()
    }

    pub fn register(&mut self, program_id: Pubkey, name: &'static str) {
        self.known_protocols.insert(program_id, name);
    }

    pub fn len(&self) -> usize {
        self.known_protocols.len()
    }

    pub fn is_empty(&self) -> bool {
        self.known_protocols.is_empty()
    }
}

impl Default for ProgramRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_empty() {
        let registry = ProgramRegistry::default();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_with_mainnet_defaults_populated() {
        let registry = ProgramRegistry::with_mainnet_defaults();
        assert_eq!(registry.len(), 8);
        assert_eq!(registry.is_known_protocol(&SPL_TOKEN_PROGRAM_ID), Some("SPL Token"));
        assert_eq!(
            registry.is_known_protocol(&SPL_TOKEN_2022_PROGRAM_ID),
            Some("SPL Token-2022")
        );
        assert_eq!(registry.is_known_protocol(&DRIFT_V2_PROGRAM_ID), Some("Drift v2"));
        assert_eq!(registry.is_known_protocol(&SQUADS_V4_PROGRAM_ID), Some("Squads v4"));
        assert_eq!(
            registry.is_known_protocol(&RAYDIUM_AMM_V4_PROGRAM_ID),
            Some("Raydium AMM v4")
        );
        assert_eq!(registry.is_known_protocol(&JUPITER_V6_PROGRAM_ID), Some("Jupiter V6"));
        assert_eq!(
            registry.is_known_protocol(&ORCA_WHIRLPOOL_PROGRAM_ID),
            Some("Orca Whirlpool")
        );
        assert_eq!(
            registry.is_known_protocol(&BPF_LOADER_UPGRADEABLE_PROGRAM_ID),
            Some("BPF Loader Upgradeable")
        );
    }

    #[test]
    fn test_unknown_program_returns_none() {
        let registry = ProgramRegistry::with_mainnet_defaults();
        let unknown = Pubkey::new_unique();
        assert_eq!(registry.is_known_protocol(&unknown), None);
    }

    #[test]
    fn test_register_adds_entry() {
        let mut registry = ProgramRegistry::new();
        let custom = Pubkey::new_unique();
        registry.register(custom, "Custom Protocol");
        assert_eq!(registry.is_known_protocol(&custom), Some("Custom Protocol"));
        assert_eq!(registry.len(), 1);
    }
}
