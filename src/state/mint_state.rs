//! Off-chain mirror of the on-chain amount-preserving quote-mint policy.

use anyhow::{anyhow, Result};
use solana_sdk::pubkey::Pubkey;
use spl_token::solana_program::program_pack::Pack;
use spl_token_2022::extension::{
    transfer_fee::{TransferFee, TransferFeeConfig},
    transfer_hook, BaseStateWithExtensions, ExtensionType, StateWithExtensions,
};
use spl_token_2022::state::Mint;

use crate::accounts::addresses::{SPL_TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID};

fn schedule_is_zero(schedule: &TransferFee) -> bool {
    u16::from(schedule.transfer_fee_basis_points) == 0 && u64::from(schedule.maximum_fee) == 0
}

/// Validate a quote mint used by nominal reserve/tax arithmetic.
///
/// Classic SPL mints are accepted. Token-2022 mints must have zero current
/// and scheduled transfer fees and must not have an armed transfer hook,
/// because the Tax SPL lane does not forward quote-side hook accounts.
pub fn validate_quote_mint(data: &[u8], owner: &Pubkey, expected_program: &Pubkey) -> Result<()> {
    if owner != expected_program {
        return Err(anyhow!(
            "quote mint owner {} does not match PoolState token program {}",
            owner,
            expected_program
        ));
    }
    if *owner == SPL_TOKEN_PROGRAM_ID {
        spl_token::state::Mint::unpack(data)
            .map_err(|e| anyhow!("invalid classic SPL quote mint: {e}"))?;
        return Ok(());
    }
    if *owner != TOKEN_2022_PROGRAM_ID {
        return Err(anyhow!("unsupported quote mint owner {owner}"));
    }

    let mint = StateWithExtensions::<Mint>::unpack(data)
        .map_err(|e| anyhow!("invalid Token-2022 quote mint: {e}"))?;
    let extensions = mint
        .get_extension_types()
        .map_err(|e| anyhow!("invalid Token-2022 quote extensions: {e}"))?;
    if extensions.contains(&ExtensionType::TransferFeeConfig) {
        let fees = mint
            .get_extension::<TransferFeeConfig>()
            .map_err(|e| anyhow!("invalid transfer-fee configuration: {e}"))?;
        if !schedule_is_zero(&fees.older_transfer_fee)
            || !schedule_is_zero(&fees.newer_transfer_fee)
        {
            return Err(anyhow!(
                "quote mint has a nonzero current or scheduled transfer fee"
            ));
        }
    }
    if transfer_hook::get_program_id(&mint).is_some() {
        return Err(anyhow!(
            "quote mint has an armed transfer hook unsupported by the Tax SPL lane"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use spl_token_2022::extension::{
        transfer_hook::TransferHook, BaseStateWithExtensionsMut, StateWithExtensionsMut,
    };

    fn transfer_fee(bps: u16, maximum_fee: u64, epoch: u64) -> TransferFee {
        TransferFee {
            epoch: epoch.into(),
            maximum_fee: maximum_fee.into(),
            transfer_fee_basis_points: bps.into(),
        }
    }

    fn token_2022_mint(
        older_fee: Option<(u16, u64)>,
        newer_fee: Option<(u16, u64)>,
        hook_program: Option<Pubkey>,
        include_hook_extension: bool,
    ) -> Vec<u8> {
        let mut extensions = Vec::new();
        if older_fee.is_some() || newer_fee.is_some() {
            extensions.push(ExtensionType::TransferFeeConfig);
        }
        if include_hook_extension {
            extensions.push(ExtensionType::TransferHook);
        }
        let len = ExtensionType::try_calculate_account_len::<Mint>(&extensions).unwrap();
        let mut data = vec![0u8; len];
        let mut state = StateWithExtensionsMut::<Mint>::unpack_uninitialized(&mut data).unwrap();

        if older_fee.is_some() || newer_fee.is_some() {
            let config = state.init_extension::<TransferFeeConfig>(true).unwrap();
            let (older_bps, older_maximum) = older_fee.unwrap_or((0, 0));
            let (newer_bps, newer_maximum) = newer_fee.unwrap_or((0, 0));
            config.older_transfer_fee = transfer_fee(older_bps, older_maximum, 0);
            config.newer_transfer_fee = transfer_fee(newer_bps, newer_maximum, u64::MAX);
        }
        if include_hook_extension {
            let hook = state.init_extension::<TransferHook>(true).unwrap();
            hook.program_id = hook_program.try_into().unwrap();
        }

        state.base = Mint {
            mint_authority: solana_sdk::program_option::COption::None,
            supply: 1,
            decimals: 6,
            is_initialized: true,
            freeze_authority: solana_sdk::program_option::COption::None,
        };
        state.pack_base();
        state.init_account_type().unwrap();
        data
    }

    #[test]
    fn classic_initialized_mint_is_accepted() {
        let mint = spl_token::state::Mint {
            mint_authority: solana_sdk::program_option::COption::None,
            supply: 1,
            decimals: 6,
            is_initialized: true,
            freeze_authority: solana_sdk::program_option::COption::None,
        };
        let mut data = vec![0u8; spl_token::state::Mint::LEN];
        spl_token::state::Mint::pack(mint, &mut data).unwrap();
        validate_quote_mint(&data, &SPL_TOKEN_PROGRAM_ID, &SPL_TOKEN_PROGRAM_ID).unwrap();
    }

    #[test]
    fn owner_must_match_pool_state() {
        assert!(validate_quote_mint(&[], &TOKEN_2022_PROGRAM_ID, &SPL_TOKEN_PROGRAM_ID).is_err());
    }

    #[test]
    fn token_2022_without_amount_changing_extensions_is_accepted() {
        let data = token_2022_mint(None, None, None, false);
        validate_quote_mint(&data, &TOKEN_2022_PROGRAM_ID, &TOKEN_2022_PROGRAM_ID).unwrap();
    }

    #[test]
    fn zero_current_and_scheduled_transfer_fees_are_accepted() {
        let data = token_2022_mint(Some((0, 0)), Some((0, 0)), None, false);
        validate_quote_mint(&data, &TOKEN_2022_PROGRAM_ID, &TOKEN_2022_PROGRAM_ID).unwrap();
    }

    #[test]
    fn every_nonzero_transfer_fee_component_is_rejected() {
        for (older, newer) in [
            (Some((1, 0)), Some((0, 0))),
            (Some((0, 1)), Some((0, 0))),
            (Some((0, 0)), Some((1, 0))),
            (Some((0, 0)), Some((0, 1))),
        ] {
            let data = token_2022_mint(older, newer, None, false);
            assert!(
                validate_quote_mint(&data, &TOKEN_2022_PROGRAM_ID, &TOKEN_2022_PROGRAM_ID)
                    .unwrap_err()
                    .to_string()
                    .contains("nonzero current or scheduled transfer fee")
            );
        }
    }

    #[test]
    fn armed_quote_hook_is_rejected_but_unarmed_extension_is_accepted() {
        let unarmed = token_2022_mint(None, None, None, true);
        validate_quote_mint(&unarmed, &TOKEN_2022_PROGRAM_ID, &TOKEN_2022_PROGRAM_ID).unwrap();

        let armed = token_2022_mint(None, None, Some(Pubkey::new_unique()), true);
        assert!(
            validate_quote_mint(&armed, &TOKEN_2022_PROGRAM_ID, &TOKEN_2022_PROGRAM_ID)
                .unwrap_err()
                .to_string()
                .contains("armed transfer hook")
        );
    }
}
