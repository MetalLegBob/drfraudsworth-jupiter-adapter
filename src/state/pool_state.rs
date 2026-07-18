// Raw byte parser for AMM PoolState account data.
//
// Extracts mints, vaults, reserves, and lp_fee_bps from on-chain PoolState
// without anchor-lang dependency. Byte offsets verified against
// programs/tax-program/src/helpers/pool_reader.rs and against embedded
// mainnet account data (tests/test_mainnet_validation.rs).
//
// PoolState byte layout:
//   [0..8]     Anchor discriminator (sha256("account:PoolState")[0..8])
//   [8]        pool_type (1 byte, enum)
//   [9..41]    mint_a (Pubkey, 32 bytes)
//   [41..73]   mint_b (Pubkey, 32 bytes)
//   [73..105]  vault_a (Pubkey, 32 bytes)
//   [105..137] vault_b (Pubkey, 32 bytes)
//   [137..145] reserve_a (u64, 8 bytes)
//   [145..153] reserve_b (u64, 8 bytes)
//   [153..155] lp_fee_bps (u16, 2 bytes)
//
// Mint ordering: the on-chain AMM stores mints in canonical byte order
// (mint_a < mint_b), so SOL may be on either side. All orientation-sensitive
// consumers must go through the side-aware helpers below rather than
// assuming mint_a == SOL.

use anyhow::{anyhow, Result};
use solana_sdk::pubkey::Pubkey;

use crate::accounts::addresses::NATIVE_MINT;
use crate::constants::POOL_STATE_DISCRIMINATOR;

/// Minimum account data length for PoolState (need through lp_fee_bps).
const MIN_LEN: usize = 155;

/// Parsed PoolState -- full field set, enabling generic pool construction
/// from account data alone (no hardcoded per-pool constants).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedPoolState {
    pub mint_a: Pubkey,
    pub mint_b: Pubkey,
    pub vault_a: Pubkey,
    pub vault_b: Pubkey,
    pub reserve_a: u64,
    pub reserve_b: u64,
    pub lp_fee_bps: u16,
}

impl ParsedPoolState {
    /// Parse PoolState from raw account data bytes.
    ///
    /// Validates:
    /// - Minimum length (155 bytes)
    /// - Anchor discriminator (sha256("account:PoolState")[0..8])
    ///
    /// The discriminator check makes it safe to feed this parser arbitrary
    /// accounts owned by the AMM program (e.g. from a getProgramAccounts
    /// scan): non-PoolState accounts (AdminConfig, etc.) are rejected
    /// instead of mis-parsed.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < MIN_LEN {
            return Err(anyhow!(
                "PoolState data too short: {} bytes (need {})",
                data.len(),
                MIN_LEN
            ));
        }

        if data[0..8] != POOL_STATE_DISCRIMINATOR {
            return Err(anyhow!(
                "PoolState discriminator mismatch: expected {:?}, got {:?}",
                POOL_STATE_DISCRIMINATOR,
                &data[0..8]
            ));
        }

        let pubkey_at = |range: core::ops::Range<usize>| -> Result<Pubkey> {
            Pubkey::try_from(&data[range.clone()])
                .map_err(|_| anyhow!("Failed to parse pubkey from bytes [{:?}]", range))
        };

        Ok(Self {
            mint_a: pubkey_at(9..41)?,
            mint_b: pubkey_at(41..73)?,
            vault_a: pubkey_at(73..105)?,
            vault_b: pubkey_at(105..137)?,
            reserve_a: u64::from_le_bytes(
                data[137..145].try_into()
                    .map_err(|_| anyhow!("Failed to parse reserve_a from bytes [137..145]"))?
            ),
            reserve_b: u64::from_le_bytes(
                data[145..153].try_into()
                    .map_err(|_| anyhow!("Failed to parse reserve_b from bytes [145..153]"))?
            ),
            lp_fee_bps: u16::from_le_bytes([data[153], data[154]]),
        })
    }

    /// True if this pool has exactly one SOL (WSOL) side.
    pub fn is_sol_pool(&self) -> bool {
        (self.mint_a == NATIVE_MINT) != (self.mint_b == NATIVE_MINT)
    }

    /// The non-SOL mint of a SOL-quoted pool.
    ///
    /// Returns None if neither or both sides are the native mint.
    pub fn token_mint(&self) -> Option<Pubkey> {
        if self.mint_a == NATIVE_MINT && self.mint_b != NATIVE_MINT {
            Some(self.mint_b)
        } else if self.mint_b == NATIVE_MINT && self.mint_a != NATIVE_MINT {
            Some(self.mint_a)
        } else {
            None
        }
    }

    /// Returns (sol_reserve, token_reserve) with is_reversed detection.
    ///
    /// If mint_a == NATIVE_MINT, reserves are in canonical order (SOL, token).
    /// Otherwise, the pool is reversed (token, SOL) and we swap them.
    pub fn sol_and_token_reserves(&self) -> (u64, u64) {
        if self.mint_a == NATIVE_MINT {
            (self.reserve_a, self.reserve_b)
        } else {
            (self.reserve_b, self.reserve_a)
        }
    }

    /// Returns (sol_vault, token_vault) with the same is_reversed detection
    /// as sol_and_token_reserves.
    pub fn sol_and_token_vaults(&self) -> (Pubkey, Pubkey) {
        if self.mint_a == NATIVE_MINT {
            (self.vault_a, self.vault_b)
        } else {
            (self.vault_b, self.vault_a)
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Build a mock PoolState byte array with full field coverage.
    pub(crate) fn mock_pool_state(
        mint_a: &Pubkey,
        mint_b: &Pubkey,
        vault_a: &Pubkey,
        vault_b: &Pubkey,
        reserve_a: u64,
        reserve_b: u64,
        lp_fee_bps: u16,
    ) -> Vec<u8> {
        let mut data = vec![0u8; 224]; // Full PoolState size

        data[0..8].copy_from_slice(&POOL_STATE_DISCRIMINATOR);
        // [8] pool_type = 0 (MixedPool)
        data[8] = 0;
        data[9..41].copy_from_slice(mint_a.as_ref());
        data[41..73].copy_from_slice(mint_b.as_ref());
        data[73..105].copy_from_slice(vault_a.as_ref());
        data[105..137].copy_from_slice(vault_b.as_ref());
        data[137..145].copy_from_slice(&reserve_a.to_le_bytes());
        data[145..153].copy_from_slice(&reserve_b.to_le_bytes());
        data[153..155].copy_from_slice(&lp_fee_bps.to_le_bytes());

        data
    }

    #[test]
    fn pool_state_discriminator_matches_sha256() {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"account:PoolState");
        let hash = hasher.finalize();
        assert_eq!(
            &hash[0..8],
            &POOL_STATE_DISCRIMINATOR,
            "POOL_STATE_DISCRIMINATOR must equal sha256(\"account:PoolState\")[0..8]"
        );
    }

    #[test]
    fn parse_normal_order_sol_pool() {
        // mint_a = NATIVE_MINT (SOL), so reserve_a = SOL, reserve_b = token
        let token = Pubkey::new_unique();
        let (va, vb) = (Pubkey::new_unique(), Pubkey::new_unique());
        let data = mock_pool_state(&NATIVE_MINT, &token, &va, &vb, 100_000_000, 500_000_000, 100);
        let parsed = ParsedPoolState::from_bytes(&data).unwrap();

        assert_eq!(parsed.mint_a, NATIVE_MINT);
        assert_eq!(parsed.mint_b, token);
        assert_eq!(parsed.vault_a, va);
        assert_eq!(parsed.vault_b, vb);
        assert_eq!(parsed.reserve_a, 100_000_000);
        assert_eq!(parsed.reserve_b, 500_000_000);
        assert_eq!(parsed.lp_fee_bps, 100);

        assert!(parsed.is_sol_pool());
        assert_eq!(parsed.token_mint(), Some(token));
        assert_eq!(parsed.sol_and_token_reserves(), (100_000_000, 500_000_000));
        assert_eq!(parsed.sol_and_token_vaults(), (va, vb));
    }

    #[test]
    fn parse_reversed_order_pool() {
        // mint_a != NATIVE_MINT, so reserves/vaults are reversed
        let token = Pubkey::new_unique();
        let (va, vb) = (Pubkey::new_unique(), Pubkey::new_unique());
        let data = mock_pool_state(&token, &NATIVE_MINT, &va, &vb, 500_000_000, 100_000_000, 100);
        let parsed = ParsedPoolState::from_bytes(&data).unwrap();

        assert!(parsed.is_sol_pool());
        assert_eq!(parsed.token_mint(), Some(token));
        // sol_and_token_* swap because mint_a != NATIVE_MINT
        assert_eq!(parsed.sol_and_token_reserves(), (100_000_000, 500_000_000));
        assert_eq!(parsed.sol_and_token_vaults(), (vb, va));
    }

    #[test]
    fn non_sol_pool_has_no_token_mint() {
        let (m1, m2) = (Pubkey::new_unique(), Pubkey::new_unique());
        let data = mock_pool_state(&m1, &m2, &Pubkey::new_unique(), &Pubkey::new_unique(), 1, 1, 100);
        let parsed = ParsedPoolState::from_bytes(&data).unwrap();

        assert!(!parsed.is_sol_pool());
        assert_eq!(parsed.token_mint(), None);
    }

    #[test]
    fn reject_too_short() {
        let data = vec![0u8; 100];
        assert!(ParsedPoolState::from_bytes(&data).is_err());
    }

    #[test]
    fn reject_wrong_discriminator() {
        let token = Pubkey::new_unique();
        let mut data = mock_pool_state(
            &NATIVE_MINT, &token, &Pubkey::new_unique(), &Pubkey::new_unique(), 1, 1, 100,
        );
        data[0] ^= 0xFF;
        let err = ParsedPoolState::from_bytes(&data).unwrap_err();
        assert!(err.to_string().contains("discriminator"));
    }

    #[test]
    fn lp_fee_parsed_correctly() {
        let token = Pubkey::new_unique();
        let data = mock_pool_state(
            &NATIVE_MINT, &token, &Pubkey::new_unique(), &Pubkey::new_unique(), 1, 1, 50,
        );
        let parsed = ParsedPoolState::from_bytes(&data).unwrap();
        assert_eq!(parsed.lp_fee_bps, 50);
    }
}
