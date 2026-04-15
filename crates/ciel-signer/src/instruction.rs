// Ciel Ed25519SigVerify Instruction Builder
// See spec Section 7.3 and 8.1 for instruction layout.

use solana_sdk::instruction::Instruction;

/// Build a Solana Ed25519SigVerify precompile instruction that verifies
/// a single Ed25519 signature over a message.
///
/// This instruction should be placed at index 0 in the transaction.
/// The CielAssert program (at index 1) reads it back via
/// `sysvar::instructions::load_instruction_at_checked(0, ...)`.
///
/// See spec Section 8.1 for transaction layout.
pub fn build_ed25519_verify_instruction(
    pubkey: &[u8; 32],
    message: &[u8],
    signature: &[u8; 64],
) -> Instruction {
    solana_sdk::ed25519_instruction::new_ed25519_instruction_with_signature(
        message, signature, pubkey,
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PUBKEY: [u8; 32] = [0xCC; 32];
    const TEST_SIGNATURE: [u8; 64] = [0xDD; 64];
    const TEST_MESSAGE: &[u8] = &[0xEE; 132]; // Simulating a 132-byte attestation

    // Offsets from the official solana-ed25519-program layout.
    // See solana-ed25519-program-2.2.3/src/lib.rs.
    const DATA_START: usize = 16; // 2 (header) + 14 (Ed25519SignatureOffsets)
    const PUBKEY_OFFSET: usize = DATA_START; // 16
    const SIGNATURE_OFFSET: usize = PUBKEY_OFFSET + 32; // 48
    const MESSAGE_OFFSET: usize = SIGNATURE_OFFSET + 64; // 112

    #[test]
    fn test_ed25519_instruction_layout() {
        let ix = build_ed25519_verify_instruction(&TEST_PUBKEY, TEST_MESSAGE, &TEST_SIGNATURE);

        // Program ID is the Ed25519SigVerify native program
        assert_eq!(ix.program_id, solana_sdk::ed25519_program::id());

        // No accounts (stateless precompile)
        assert!(ix.accounts.is_empty());

        // Data layout: 16 header + 32 pubkey + 64 sig + 132 message = 244
        assert_eq!(
            ix.data.len(),
            DATA_START + 32 + 64 + TEST_MESSAGE.len(),
            "total instruction data length"
        );

        // Byte 0: num_signatures = 1
        assert_eq!(ix.data[0], 1, "num_signatures must be 1");

        // Byte 1: padding = 0
        assert_eq!(ix.data[1], 0, "padding byte must be 0");

        // Parse the Ed25519SignatureOffsets from bytes 2..16 (7 x u16 LE)
        let offsets = &ix.data[2..16];

        let sig_offset = u16::from_le_bytes([offsets[0], offsets[1]]);
        let sig_ix_index = u16::from_le_bytes([offsets[2], offsets[3]]);
        let pk_offset = u16::from_le_bytes([offsets[4], offsets[5]]);
        let pk_ix_index = u16::from_le_bytes([offsets[6], offsets[7]]);
        let msg_offset = u16::from_le_bytes([offsets[8], offsets[9]]);
        let msg_size = u16::from_le_bytes([offsets[10], offsets[11]]);
        let msg_ix_index = u16::from_le_bytes([offsets[12], offsets[13]]);

        assert_eq!(sig_offset, SIGNATURE_OFFSET as u16, "signature_offset");
        assert_eq!(sig_ix_index, u16::MAX, "signature_instruction_index");
        assert_eq!(pk_offset, PUBKEY_OFFSET as u16, "public_key_offset");
        assert_eq!(pk_ix_index, u16::MAX, "public_key_instruction_index");
        assert_eq!(msg_offset, MESSAGE_OFFSET as u16, "message_data_offset");
        assert_eq!(
            msg_size,
            TEST_MESSAGE.len() as u16,
            "message_data_size"
        );
        assert_eq!(msg_ix_index, u16::MAX, "message_instruction_index");
    }

    #[test]
    fn test_ed25519_instruction_contains_correct_data() {
        let ix = build_ed25519_verify_instruction(&TEST_PUBKEY, TEST_MESSAGE, &TEST_SIGNATURE);

        // Public key at offset 16
        assert_eq!(
            &ix.data[PUBKEY_OFFSET..PUBKEY_OFFSET + 32],
            &TEST_PUBKEY,
            "pubkey at correct offset"
        );

        // Signature at offset 48
        assert_eq!(
            &ix.data[SIGNATURE_OFFSET..SIGNATURE_OFFSET + 64],
            &TEST_SIGNATURE,
            "signature at correct offset"
        );

        // Message at offset 112
        assert_eq!(
            &ix.data[MESSAGE_OFFSET..MESSAGE_OFFSET + TEST_MESSAGE.len()],
            TEST_MESSAGE,
            "message at correct offset"
        );
    }
}
