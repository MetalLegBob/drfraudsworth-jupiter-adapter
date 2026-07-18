// Raw byte parser for SPL Token / Token-2022 token account balances.
//
// Base account layout (identical for both token programs):
//   [0..32]   mint (Pubkey)
//   [32..64]  owner (Pubkey)
//   [64..72]  amount (u64 LE)
// Token-2022 extension data lives after byte 165 and is irrelevant here.

use anyhow::{anyhow, Result};
use solana_sdk::pubkey::Pubkey;

/// Minimum length to read through the amount field.
const MIN_LEN: usize = 72;

/// Parse the balance of a token account, validating the expected mint.
///
/// The mint check guards against being handed the wrong account in the
/// AccountMap (e.g. a stale cache entry for a different vault).
pub fn parse_token_account_amount(data: &[u8], expected_mint: &Pubkey) -> Result<u64> {
    if data.len() < MIN_LEN {
        return Err(anyhow!(
            "token account data too short: {} bytes (need {})",
            data.len(),
            MIN_LEN
        ));
    }

    let mint = Pubkey::try_from(&data[0..32])
        .map_err(|_| anyhow!("failed to parse token account mint from bytes [0..32]"))?;
    if mint != *expected_mint {
        return Err(anyhow!(
            "token account mint mismatch: expected {}, got {}",
            expected_mint,
            mint
        ));
    }

    Ok(u64::from_le_bytes(
        data[64..72]
            .try_into()
            .map_err(|_| anyhow!("failed to parse token account amount from bytes [64..72]"))?,
    ))
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Build a mock token account byte array (base layout, no extensions).
    pub(crate) fn mock_token_account(mint: &Pubkey, owner: &Pubkey, amount: u64) -> Vec<u8> {
        let mut data = vec![0u8; 165];
        data[0..32].copy_from_slice(mint.as_ref());
        data[32..64].copy_from_slice(owner.as_ref());
        data[64..72].copy_from_slice(&amount.to_le_bytes());
        // [108] state = 1 (Initialized) -- not read by the parser, set for realism
        data[108] = 1;
        data
    }

    #[test]
    fn parses_amount() {
        let mint = Pubkey::new_unique();
        let data = mock_token_account(&mint, &Pubkey::new_unique(), 851_612_811_341_341);
        assert_eq!(
            parse_token_account_amount(&data, &mint).unwrap(),
            851_612_811_341_341
        );
    }

    #[test]
    fn rejects_wrong_mint() {
        let data = mock_token_account(&Pubkey::new_unique(), &Pubkey::new_unique(), 1);
        let err = parse_token_account_amount(&data, &Pubkey::new_unique()).unwrap_err();
        assert!(err.to_string().contains("mint mismatch"));
    }

    #[test]
    fn rejects_short_data() {
        assert!(parse_token_account_amount(&[0u8; 40], &Pubkey::new_unique()).is_err());
    }
}
