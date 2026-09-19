//! Account-meta builders for the Tax Program's generic SPL quote lanes.
//!
//! The sell lane deliberately gives the AMM the canonical sweep ATA in the
//! pool-positional quote slot, then supplies Jupiter's destination token
//! account separately as `user_quote`.

use anyhow::{anyhow, Result};
use solana_sdk::{instruction::AccountMeta, pubkey::Pubkey};
use spl_associated_token_account::get_associated_token_address_with_program_id;

use crate::state::pool_state::ParsedPoolState;

use super::addresses::{
    AMM_PROGRAM_ID, CRIME_MINT, EPOCH_STATE_PDA, FRAUD_MINT, SPL_TOKEN_PROGRAM_ID,
    SWAP_AUTHORITY_PDA, TAX_PROGRAM_ID, TOKEN_2022_PROGRAM_ID,
};
use super::hook_accounts::hook_metas_for_mint;

pub const SWEEP_AUTHORITY_SEED: &[u8] = b"sweep_authority";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FactionPoolSides {
    pub faction_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub faction_vault: Pubkey,
    pub faction_is_a: bool,
    pub quote_token_program: Pubkey,
}

pub fn resolve_faction_sides(pool: &ParsedPoolState) -> Result<FactionPoolSides> {
    let a_is_faction = pool.mint_a == CRIME_MINT || pool.mint_a == FRAUD_MINT;
    let b_is_faction = pool.mint_b == CRIME_MINT || pool.mint_b == FRAUD_MINT;
    if a_is_faction == b_is_faction {
        return Err(anyhow!(
            "pool must contain exactly one canonical faction mint: {} / {}",
            pool.mint_a,
            pool.mint_b
        ));
    }

    let (faction_mint, quote_mint, faction_vault, faction_is_a, quote_token_program) =
        if a_is_faction {
            (
                pool.mint_a,
                pool.mint_b,
                pool.vault_a,
                true,
                pool.token_program_b,
            )
        } else {
            (
                pool.mint_b,
                pool.mint_a,
                pool.vault_b,
                false,
                pool.token_program_a,
            )
        };

    if quote_token_program != SPL_TOKEN_PROGRAM_ID && quote_token_program != TOKEN_2022_PROGRAM_ID {
        return Err(anyhow!(
            "unsupported quote token program {} for mint {}",
            quote_token_program,
            quote_mint
        ));
    }

    Ok(FactionPoolSides {
        faction_mint,
        quote_mint,
        faction_vault,
        faction_is_a,
        quote_token_program,
    })
}

pub fn sweep_accounts(pool: &ParsedPoolState) -> Result<(Pubkey, Pubkey)> {
    let sides = resolve_faction_sides(pool)?;
    let (authority, _) = Pubkey::find_program_address(&[SWEEP_AUTHORITY_SEED], &TAX_PROGRAM_ID);
    let ata = get_associated_token_address_with_program_id(
        &authority,
        &sides.quote_mint,
        &sides.quote_token_program,
    );
    Ok((authority, ata))
}

pub fn build_buy_account_metas_generic(
    user: &Pubkey,
    user_quote: &Pubkey,
    user_faction: &Pubkey,
    pool_key: &Pubkey,
    pool: &ParsedPoolState,
) -> Result<Vec<AccountMeta>> {
    let sides = resolve_faction_sides(pool)?;
    let (sweep_authority, sweep_ata) = sweep_accounts(pool)?;
    let (user_a, user_b) = if sides.faction_is_a {
        (user_faction, user_quote)
    } else {
        (user_quote, user_faction)
    };

    let mut metas = vec![
        AccountMeta::new(*user, true),
        AccountMeta::new_readonly(EPOCH_STATE_PDA, false),
        AccountMeta::new(*pool_key, false),
        AccountMeta::new(pool.vault_a, false),
        AccountMeta::new(pool.vault_b, false),
        AccountMeta::new_readonly(pool.mint_a, false),
        AccountMeta::new_readonly(pool.mint_b, false),
        AccountMeta::new(*user_a, false),
        AccountMeta::new(*user_b, false),
        AccountMeta::new_readonly(SWAP_AUTHORITY_PDA, false),
        AccountMeta::new_readonly(sweep_authority, false),
        AccountMeta::new_readonly(sides.quote_mint, false),
        AccountMeta::new(sweep_ata, false),
        AccountMeta::new_readonly(sides.quote_token_program, false),
        AccountMeta::new_readonly(pool.token_program_a, false),
        AccountMeta::new_readonly(pool.token_program_b, false),
        AccountMeta::new_readonly(AMM_PROGRAM_ID, false),
    ];
    metas.extend(hook_metas_for_mint(
        &sides.faction_mint,
        &sides.faction_vault,
        user_faction,
    ));
    Ok(metas)
}

pub fn build_sell_account_metas_generic(
    user: &Pubkey,
    user_faction: &Pubkey,
    user_quote: &Pubkey,
    pool_key: &Pubkey,
    pool: &ParsedPoolState,
) -> Result<Vec<AccountMeta>> {
    let sides = resolve_faction_sides(pool)?;
    let (sweep_authority, sweep_ata) = sweep_accounts(pool)?;
    let (user_a, user_b) = if sides.faction_is_a {
        (user_faction, &sweep_ata)
    } else {
        (&sweep_ata, user_faction)
    };

    let mut metas = vec![
        AccountMeta::new(*user, true),
        AccountMeta::new_readonly(EPOCH_STATE_PDA, false),
        AccountMeta::new(*pool_key, false),
        AccountMeta::new(pool.vault_a, false),
        AccountMeta::new(pool.vault_b, false),
        AccountMeta::new_readonly(pool.mint_a, false),
        AccountMeta::new_readonly(pool.mint_b, false),
        AccountMeta::new(*user_a, false),
        AccountMeta::new(*user_b, false),
        AccountMeta::new_readonly(SWAP_AUTHORITY_PDA, false),
        AccountMeta::new_readonly(sweep_authority, false),
        AccountMeta::new_readonly(sides.quote_mint, false),
        AccountMeta::new(sweep_ata, false),
        AccountMeta::new(*user_quote, false),
        AccountMeta::new_readonly(sides.quote_token_program, false),
        AccountMeta::new_readonly(pool.token_program_a, false),
        AccountMeta::new_readonly(pool.token_program_b, false),
        AccountMeta::new_readonly(AMM_PROGRAM_ID, false),
    ];
    metas.extend(hook_metas_for_mint(
        &sides.faction_mint,
        user_faction,
        &sides.faction_vault,
    ));
    Ok(metas)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::addresses::CRIME_HOOK_META;

    fn pool(faction_is_a: bool) -> ParsedPoolState {
        let quote = Pubkey::new_unique();
        let (mint_a, mint_b) = if faction_is_a {
            (CRIME_MINT, quote)
        } else {
            (quote, CRIME_MINT)
        };
        ParsedPoolState {
            mint_a,
            mint_b,
            vault_a: Pubkey::new_unique(),
            vault_b: Pubkey::new_unique(),
            reserve_a: 10,
            reserve_b: 20,
            lp_fee_bps: 100,
            initialized: true,
            locked: false,
            bump: 1,
            vault_a_bump: 2,
            vault_b_bump: 3,
            token_program_a: if faction_is_a {
                TOKEN_2022_PROGRAM_ID
            } else {
                SPL_TOKEN_PROGRAM_ID
            },
            token_program_b: if faction_is_a {
                SPL_TOKEN_PROGRAM_ID
            } else {
                TOKEN_2022_PROGRAM_ID
            },
        }
    }

    #[test]
    fn buy_layout_matches_anchor_order_in_both_orientations() {
        for reversed in [false, true] {
            let pool = pool(reversed);
            let user = Pubkey::new_unique();
            let quote = Pubkey::new_unique();
            let faction = Pubkey::new_unique();
            let metas = build_buy_account_metas_generic(
                &user,
                &quote,
                &faction,
                &Pubkey::new_unique(),
                &pool,
            )
            .unwrap();
            assert_eq!(metas.len(), 21);
            assert_eq!(metas[0].pubkey, user);
            assert_eq!(
                metas[11].pubkey,
                resolve_faction_sides(&pool).unwrap().quote_mint
            );
            assert_eq!(metas[17].pubkey, CRIME_HOOK_META);
            if reversed {
                assert_eq!(metas[7].pubkey, faction);
                assert_eq!(metas[8].pubkey, quote);
            } else {
                assert_eq!(metas[7].pubkey, quote);
                assert_eq!(metas[8].pubkey, faction);
            }
        }
    }

    #[test]
    fn sell_uses_sweep_in_quote_slot_and_separate_destination() {
        for faction_is_a in [false, true] {
            let pool = pool(faction_is_a);
            let faction = Pubkey::new_unique();
            let destination = Pubkey::new_unique();
            let metas = build_sell_account_metas_generic(
                &Pubkey::new_unique(),
                &faction,
                &destination,
                &Pubkey::new_unique(),
                &pool,
            )
            .unwrap();
            let (_, sweep) = sweep_accounts(&pool).unwrap();
            assert_eq!(metas.len(), 22);
            assert_eq!(metas[13].pubkey, destination);
            if faction_is_a {
                assert_eq!(metas[7].pubkey, faction);
                assert_eq!(metas[8].pubkey, sweep);
            } else {
                assert_eq!(metas[7].pubkey, sweep);
                assert_eq!(metas[8].pubkey, faction);
            }
        }
    }
}
