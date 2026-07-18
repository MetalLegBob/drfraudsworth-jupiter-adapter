// AccountMeta builders for Tax Program SOL pool swap instructions.
//
// These match the EXACT account ordering of:
// - SwapSolBuy struct (20 named accounts) in programs/tax-program/src/instructions/swap_sol_buy.rs
// - SwapSolSell struct (21 named accounts) in programs/tax-program/src/instructions/swap_sol_sell.rs
//
// After the named accounts, transfer hook remaining_accounts are appended
// (4 per T22 mint) for the token side.
//
// The generic builders derive every pool-specific account (pool, mints,
// vaults, orientation) from a ParsedPoolState, so any SOL-quoted pool the
// AMM creates in the future works without new constants. The is_crime
// wrappers preserve the original constant-based API and are proven
// equivalent to the generic path in tests/test_mainnet_validation.rs.

use anyhow::{anyhow, Result};
use solana_sdk::{instruction::AccountMeta, pubkey::Pubkey};

use crate::constants::LP_FEE_BPS;
use crate::state::pool_state::ParsedPoolState;

use super::addresses::{
    AMM_PROGRAM_ID, CARNAGE_SOL_VAULT_PDA, CRIME_MINT, CRIME_SOL_POOL, CRIME_SOL_VAULT_A,
    CRIME_SOL_VAULT_B, EPOCH_STATE_PDA, ESCROW_VAULT_PDA, FRAUD_MINT, FRAUD_SOL_POOL,
    FRAUD_SOL_VAULT_A, FRAUD_SOL_VAULT_B, NATIVE_MINT, SPL_TOKEN_PROGRAM_ID, STAKING_PROGRAM_ID,
    STAKE_POOL_PDA, SWAP_AUTHORITY_PDA, SYSTEM_PROGRAM_ID, TAX_AUTHORITY_PDA, TOKEN_2022_PROGRAM_ID,
    TREASURY, WSOL_INTERMEDIARY_PDA,
};
use super::hook_accounts::hook_metas_for_mint;

/// Orientation-resolved view of a SOL pool used for meta building.
struct PoolSides {
    token_mint: Pubkey,
    token_vault: Pubkey,
    /// True if the token is on the mint_a side (reversed pool).
    token_on_a: bool,
}

fn resolve_sides(pool: &ParsedPoolState) -> Result<PoolSides> {
    let token_mint = pool
        .token_mint()
        .ok_or_else(|| anyhow!("not a SOL-quoted pool: {} / {}", pool.mint_a, pool.mint_b))?;
    let (_, token_vault) = pool.sol_and_token_vaults();
    Ok(PoolSides {
        token_mint,
        token_vault,
        token_on_a: pool.mint_a != NATIVE_MINT,
    })
}

/// Build the 20-account list for SwapSolBuy (SOL -> token) from parsed pool
/// state, plus 4 transfer hook accounts for the T22 token mint.
/// Total: 24 AccountMetas.
///
/// # Arguments
/// * `user` - User wallet (signer, mutable)
/// * `user_sol_ata` - User's WSOL token account
/// * `user_token_ata` - User's token account for the pool's token side
/// * `pool_key` - Pool PDA address
/// * `pool` - Parsed pool state (mints, vaults, orientation)
pub fn build_buy_account_metas_generic(
    user: &Pubkey,
    user_sol_ata: &Pubkey,
    user_token_ata: &Pubkey,
    pool_key: &Pubkey,
    pool: &ParsedPoolState,
) -> Result<Vec<AccountMeta>> {
    let sides = resolve_sides(pool)?;

    // Side-a / side-b assignments follow the pool's stored mint order, so a
    // reversed pool (token = mint_a) slots user accounts and token programs
    // on the opposite sides.
    let (user_ata_a, user_ata_b) = if sides.token_on_a {
        (user_token_ata, user_sol_ata)
    } else {
        (user_sol_ata, user_token_ata)
    };
    let (token_program_a, token_program_b) = if sides.token_on_a {
        (TOKEN_2022_PROGRAM_ID, SPL_TOKEN_PROGRAM_ID)
    } else {
        (SPL_TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID)
    };

    let mut metas = vec![
        // 1. user (signer, mut)
        AccountMeta::new(*user, true),
        // 2. epoch_state (readonly)
        AccountMeta::new_readonly(EPOCH_STATE_PDA, false),
        // 3. swap_authority (readonly)
        AccountMeta::new_readonly(SWAP_AUTHORITY_PDA, false),
        // 4. tax_authority (readonly)
        AccountMeta::new_readonly(TAX_AUTHORITY_PDA, false),
        // 5. pool (mut)
        AccountMeta::new(*pool_key, false),
        // 6. pool_vault_a (mut)
        AccountMeta::new(pool.vault_a, false),
        // 7. pool_vault_b (mut)
        AccountMeta::new(pool.vault_b, false),
        // 8. mint_a (readonly)
        AccountMeta::new_readonly(pool.mint_a, false),
        // 9. mint_b (readonly)
        AccountMeta::new_readonly(pool.mint_b, false),
        // 10. user_token_a (mut)
        AccountMeta::new(*user_ata_a, false),
        // 11. user_token_b (mut)
        AccountMeta::new(*user_ata_b, false),
        // 12. stake_pool (mut)
        AccountMeta::new(STAKE_POOL_PDA, false),
        // 13. staking_escrow (mut)
        AccountMeta::new(ESCROW_VAULT_PDA, false),
        // 14. carnage_vault (mut)
        AccountMeta::new(CARNAGE_SOL_VAULT_PDA, false),
        // 15. treasury (mut)
        AccountMeta::new(TREASURY, false),
        // 16. amm_program (readonly)
        AccountMeta::new_readonly(AMM_PROGRAM_ID, false),
        // 17. token_program_a (readonly)
        AccountMeta::new_readonly(token_program_a, false),
        // 18. token_program_b (readonly)
        AccountMeta::new_readonly(token_program_b, false),
        // 19. system_program (readonly)
        AccountMeta::new_readonly(SYSTEM_PROGRAM_ID, false),
        // 20. staking_program (readonly)
        AccountMeta::new_readonly(STAKING_PROGRAM_ID, false),
    ];

    // Transfer hook accounts for the T22 token side. For buy (SOL -> token),
    // the hooked transfer is token_vault -> user_token_ata.
    let hook_metas = hook_metas_for_mint(&sides.token_mint, &sides.token_vault, user_token_ata);
    metas.extend(hook_metas);

    Ok(metas)
}

/// Build the 21-account list for SwapSolSell (token -> SOL) from parsed pool
/// state, plus 4 transfer hook accounts for the T22 token mint.
/// Total: 25 AccountMetas.
///
/// # Arguments
/// * `user` - User wallet (signer, mutable)
/// * `user_token_ata` - User's token account for the pool's token side (input)
/// * `user_sol_ata` - User's WSOL token account (output)
/// * `pool_key` - Pool PDA address
/// * `pool` - Parsed pool state (mints, vaults, orientation)
pub fn build_sell_account_metas_generic(
    user: &Pubkey,
    user_token_ata: &Pubkey,
    user_sol_ata: &Pubkey,
    pool_key: &Pubkey,
    pool: &ParsedPoolState,
) -> Result<Vec<AccountMeta>> {
    let sides = resolve_sides(pool)?;

    let (user_ata_a, user_ata_b) = if sides.token_on_a {
        (user_token_ata, user_sol_ata)
    } else {
        (user_sol_ata, user_token_ata)
    };
    let (token_program_a, token_program_b) = if sides.token_on_a {
        (TOKEN_2022_PROGRAM_ID, SPL_TOKEN_PROGRAM_ID)
    } else {
        (SPL_TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID)
    };

    let mut metas = vec![
        // 1. user (signer, mut)
        AccountMeta::new(*user, true),
        // 2. epoch_state (readonly)
        AccountMeta::new_readonly(EPOCH_STATE_PDA, false),
        // 3. swap_authority (mut for sell -- receives lamports from close_account)
        AccountMeta::new(SWAP_AUTHORITY_PDA, false),
        // 4. tax_authority (readonly)
        AccountMeta::new_readonly(TAX_AUTHORITY_PDA, false),
        // 5. pool (mut)
        AccountMeta::new(*pool_key, false),
        // 6. pool_vault_a (mut)
        AccountMeta::new(pool.vault_a, false),
        // 7. pool_vault_b (mut)
        AccountMeta::new(pool.vault_b, false),
        // 8. mint_a (readonly)
        AccountMeta::new_readonly(pool.mint_a, false),
        // 9. mint_b (readonly)
        AccountMeta::new_readonly(pool.mint_b, false),
        // 10. user_token_a (mut)
        AccountMeta::new(*user_ata_a, false),
        // 11. user_token_b (mut)
        AccountMeta::new(*user_ata_b, false),
        // 12. stake_pool (mut)
        AccountMeta::new(STAKE_POOL_PDA, false),
        // 13. staking_escrow (mut)
        AccountMeta::new(ESCROW_VAULT_PDA, false),
        // 14. carnage_vault (mut)
        AccountMeta::new(CARNAGE_SOL_VAULT_PDA, false),
        // 15. treasury (mut)
        AccountMeta::new(TREASURY, false),
        // 16. wsol_intermediary (mut) -- EXTRA account in sell struct
        AccountMeta::new(WSOL_INTERMEDIARY_PDA, false),
        // 17. amm_program (readonly)
        AccountMeta::new_readonly(AMM_PROGRAM_ID, false),
        // 18. token_program_a (readonly)
        AccountMeta::new_readonly(token_program_a, false),
        // 19. token_program_b (readonly)
        AccountMeta::new_readonly(token_program_b, false),
        // 20. system_program (readonly)
        AccountMeta::new_readonly(SYSTEM_PROGRAM_ID, false),
        // 21. staking_program (readonly)
        AccountMeta::new_readonly(STAKING_PROGRAM_ID, false),
    ];

    // Transfer hook accounts for the T22 token side. For sell (token -> SOL),
    // the hooked transfer is user_token_ata -> token_vault.
    let hook_metas = hook_metas_for_mint(&sides.token_mint, user_token_ata, &sides.token_vault);
    metas.extend(hook_metas);

    Ok(metas)
}

/// Synthesize the ParsedPoolState for a known mainnet pool from constants.
///
/// Reserves are zero (irrelevant for meta building); orientation matches
/// mainnet (mint_a = WSOL on both pools).
pub(crate) fn known_pool_state(is_crime: bool) -> (Pubkey, ParsedPoolState) {
    let (pool, vault_a, vault_b, token_mint) = pool_addresses(is_crime);
    (
        pool,
        ParsedPoolState {
            mint_a: NATIVE_MINT,
            mint_b: token_mint,
            vault_a,
            vault_b,
            reserve_a: 0,
            reserve_b: 0,
            lp_fee_bps: LP_FEE_BPS,
        },
    )
}

/// Build the buy account list for a known mainnet pool (constant-based API).
///
/// # Arguments
/// * `user` - User wallet (signer, mutable)
/// * `user_wsol_ata` - User's WSOL token account
/// * `user_token_ata` - User's CRIME/FRAUD token account
/// * `is_crime` - true = CRIME pool, false = FRAUD pool
pub fn build_buy_account_metas(
    user: &Pubkey,
    user_wsol_ata: &Pubkey,
    user_token_ata: &Pubkey,
    is_crime: bool,
) -> Vec<AccountMeta> {
    let (pool_key, pool) = known_pool_state(is_crime);
    build_buy_account_metas_generic(user, user_wsol_ata, user_token_ata, &pool_key, &pool)
        .expect("known pools are valid SOL pools")
}

/// Build the sell account list for a known mainnet pool (constant-based API).
///
/// # Arguments
/// * `user` - User wallet (signer, mutable)
/// * `user_token_ata` - User's CRIME/FRAUD token account (input)
/// * `user_wsol_ata` - User's WSOL token account (output)
/// * `is_crime` - true = CRIME pool, false = FRAUD pool
pub fn build_sell_account_metas(
    user: &Pubkey,
    user_token_ata: &Pubkey,
    user_wsol_ata: &Pubkey,
    is_crime: bool,
) -> Vec<AccountMeta> {
    let (pool_key, pool) = known_pool_state(is_crime);
    build_sell_account_metas_generic(user, user_token_ata, user_wsol_ata, &pool_key, &pool)
        .expect("known pools are valid SOL pools")
}

/// Resolve pool-specific addresses based on is_crime flag.
///
/// Returns (pool, vault_a, vault_b, token_mint).
fn pool_addresses(is_crime: bool) -> (Pubkey, Pubkey, Pubkey, Pubkey) {
    if is_crime {
        (CRIME_SOL_POOL, CRIME_SOL_VAULT_A, CRIME_SOL_VAULT_B, CRIME_MINT)
    } else {
        (FRAUD_SOL_POOL, FRAUD_SOL_VAULT_A, FRAUD_SOL_VAULT_B, FRAUD_MINT)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buy_metas_has_20_named_accounts() {
        let user = Pubkey::new_unique();
        let wsol = Pubkey::new_unique();
        let token = Pubkey::new_unique();

        let metas = build_buy_account_metas(&user, &wsol, &token, true);

        // 20 named + 4 hook = 24 total
        assert_eq!(metas.len(), 24, "buy should have 20 named + 4 hook accounts");

        // Verify first 20 are the named accounts (no hooks yet)
        // Account 1: user is signer + mutable
        assert_eq!(metas[0].pubkey, user);
        assert!(metas[0].is_signer);
        assert!(metas[0].is_writable);

        // Account 2: epoch_state readonly
        assert_eq!(metas[1].pubkey, EPOCH_STATE_PDA);
        assert!(!metas[1].is_writable);

        // Account 16: amm_program
        assert_eq!(metas[15].pubkey, AMM_PROGRAM_ID);

        // Account 20: staking_program
        assert_eq!(metas[19].pubkey, STAKING_PROGRAM_ID);
    }

    #[test]
    fn sell_metas_has_21_named_accounts() {
        let user = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let wsol = Pubkey::new_unique();

        let metas = build_sell_account_metas(&user, &token, &wsol, true);

        // 21 named + 4 hook = 25 total
        assert_eq!(metas.len(), 25, "sell should have 21 named + 4 hook accounts");

        // Account 16: wsol_intermediary (sell-only)
        assert_eq!(metas[15].pubkey, WSOL_INTERMEDIARY_PDA);
        assert!(metas[15].is_writable);

        // Account 17: amm_program
        assert_eq!(metas[16].pubkey, AMM_PROGRAM_ID);

        // Account 21: staking_program
        assert_eq!(metas[20].pubkey, STAKING_PROGRAM_ID);
    }

    #[test]
    fn crime_pool_uses_crime_addresses() {
        let user = Pubkey::new_unique();
        let wsol = Pubkey::new_unique();
        let token = Pubkey::new_unique();

        let metas = build_buy_account_metas(&user, &wsol, &token, true);

        // pool = CRIME_SOL_POOL
        assert_eq!(metas[4].pubkey, CRIME_SOL_POOL);
        // mint_b = CRIME_MINT
        assert_eq!(metas[8].pubkey, CRIME_MINT);
    }

    #[test]
    fn fraud_pool_uses_fraud_addresses() {
        let user = Pubkey::new_unique();
        let wsol = Pubkey::new_unique();
        let token = Pubkey::new_unique();

        let metas = build_buy_account_metas(&user, &wsol, &token, false);

        // pool = FRAUD_SOL_POOL
        assert_eq!(metas[4].pubkey, FRAUD_SOL_POOL);
        // mint_b = FRAUD_MINT
        assert_eq!(metas[8].pubkey, FRAUD_MINT);
    }

    #[test]
    fn buy_hook_accounts_at_end() {
        let user = Pubkey::new_unique();
        let wsol = Pubkey::new_unique();
        let token = Pubkey::new_unique();

        let metas = build_buy_account_metas(&user, &wsol, &token, true);

        // Last 4 are hook accounts for CRIME mint
        // [20] = CRIME_HOOK_META
        assert_eq!(metas[20].pubkey, super::super::addresses::CRIME_HOOK_META);
        // [23] = TRANSFER_HOOK_PROGRAM_ID
        assert_eq!(metas[23].pubkey, super::super::addresses::TRANSFER_HOOK_PROGRAM_ID);
    }

    #[test]
    fn sell_swap_authority_is_mutable() {
        let user = Pubkey::new_unique();
        let token = Pubkey::new_unique();
        let wsol = Pubkey::new_unique();

        let metas = build_sell_account_metas(&user, &token, &wsol, true);

        // swap_authority is account 3 (index 2), mutable for sell
        assert_eq!(metas[2].pubkey, SWAP_AUTHORITY_PDA);
        assert!(metas[2].is_writable);
    }

    #[test]
    fn buy_swap_authority_is_readonly() {
        let user = Pubkey::new_unique();
        let wsol = Pubkey::new_unique();
        let token = Pubkey::new_unique();

        let metas = build_buy_account_metas(&user, &wsol, &token, true);

        // swap_authority is account 3 (index 2), readonly for buy
        assert_eq!(metas[2].pubkey, SWAP_AUTHORITY_PDA);
        assert!(!metas[2].is_writable);
    }

    #[test]
    fn generic_builder_handles_reversed_pool_orientation() {
        // Reversed pool: token = mint_a, SOL = mint_b. User ATAs and token
        // programs must land on the opposite struct sides.
        let user = Pubkey::new_unique();
        let wsol = Pubkey::new_unique();
        let token_ata = Pubkey::new_unique();
        let pool_key = Pubkey::new_unique();

        let pool = ParsedPoolState {
            mint_a: CRIME_MINT,
            mint_b: NATIVE_MINT,
            vault_a: Pubkey::new_unique(), // token vault (side a)
            vault_b: Pubkey::new_unique(), // SOL vault (side b)
            reserve_a: 0,
            reserve_b: 0,
            lp_fee_bps: LP_FEE_BPS,
        };

        let metas =
            build_buy_account_metas_generic(&user, &wsol, &token_ata, &pool_key, &pool).unwrap();

        assert_eq!(metas.len(), 24);
        // Positional mints follow the pool's stored order
        assert_eq!(metas[7].pubkey, CRIME_MINT);
        assert_eq!(metas[8].pubkey, NATIVE_MINT);
        // User ATA on side a is the TOKEN ata; side b is WSOL
        assert_eq!(metas[9].pubkey, token_ata);
        assert_eq!(metas[10].pubkey, wsol);
        // Token programs swap sides: T22 on a, SPL on b
        assert_eq!(metas[16].pubkey, TOKEN_2022_PROGRAM_ID);
        assert_eq!(metas[17].pubkey, SPL_TOKEN_PROGRAM_ID);
        // Hooks are for the token side vault (vault_a) -> user token ata
        assert_eq!(metas[20].pubkey, super::super::addresses::CRIME_HOOK_META);
    }

    #[test]
    fn generic_builder_rejects_non_sol_pool() {
        let pool = ParsedPoolState {
            mint_a: CRIME_MINT,
            mint_b: Pubkey::new_unique(),
            vault_a: Pubkey::new_unique(),
            vault_b: Pubkey::new_unique(),
            reserve_a: 0,
            reserve_b: 0,
            lp_fee_bps: LP_FEE_BPS,
        };

        let result = build_buy_account_metas_generic(
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &pool,
        );
        assert!(result.is_err());
    }
}
