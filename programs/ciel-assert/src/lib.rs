// CielAssert On-Chain Program
// See spec Section 8.1 for the verification pseudocode.
// See docs/20-ciel-assert-program.md for implementation guide.
//
// This program:
// 1. Reads sysvar::instructions to find the Ed25519SigVerify precompile instruction
// 2. Extracts the CielAttestation from the message data
// 3. Verifies the signer matches the expected Ciel public key
// 4. Checks verdict is APPROVE or WARN (rejects BLOCK and TIMEOUT)
// 5. Checks slot freshness (current_slot <= expiry_slot)

use anchor_lang::prelude::*;

declare_id!("CieL111111111111111111111111111111111111111");

#[program]
pub mod ciel_assert {
    use super::*;

    pub fn assert_attestation(
        _ctx: Context<AssertAttestation>,
        _expected_signer: Pubkey,
        _ed25519_ix_index: u8,
    ) -> Result<()> {
        // TODO: implement — see spec Section 8.1 and docs/20-ciel-assert-program.md
        msg!("CielAssert: not yet implemented");
        Ok(())
    }
}

#[derive(Accounts)]
pub struct AssertAttestation<'info> {
    /// CHECK: instructions sysvar
    #[account(address = solana_program::sysvar::instructions::ID)]
    pub instructions_sysvar: AccountInfo<'info>,
}
