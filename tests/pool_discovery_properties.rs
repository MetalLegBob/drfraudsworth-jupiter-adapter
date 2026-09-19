//! Property checks for the untrusted AMM account-discovery boundary.

use jupiter_amm_interface::{Amm, AmmContext, ClockRef, KeyedAccount};
use proptest::prelude::*;
use solana_sdk::{account::Account, pubkey::Pubkey};

use drfraudsworth_jupiter_adapter::{
    accounts::addresses::{
        AMM_PROGRAM_ID, CRIME_MINT, FRAUD_MINT, SPL_TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID,
    },
    constants::POOL_STATE_DISCRIMINATOR,
    sol_pool_amm::SolPoolAmm,
};

fn context() -> AmmContext {
    AmmContext {
        clock_ref: ClockRef::default(),
    }
}

fn keyed(key: Pubkey, data: Vec<u8>) -> KeyedAccount {
    KeyedAccount {
        key,
        account: Account {
            lamports: 1,
            data,
            owner: AMM_PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        },
        params: None,
    }
}

proptest! {
    #[test]
    fn arbitrary_amm_owned_data_never_panics(
        key in any::<[u8; 32]>(),
        data in prop::collection::vec(any::<u8>(), 0..384),
    ) {
        let _ = SolPoolAmm::from_keyed_account(
            &keyed(Pubkey::new_from_array(key), data),
            &context(),
        );
    }

    #[test]
    fn every_canonical_faction_pool_shape_is_discoverable(
        quote_bytes in any::<[u8; 32]>(),
        use_fraud in any::<bool>(),
        quote_is_token_2022 in any::<bool>(),
        reserve_a in any::<u64>(),
        reserve_b in any::<u64>(),
    ) {
        let faction = if use_fraud { FRAUD_MINT } else { CRIME_MINT };
        let quote = Pubkey::new_from_array(quote_bytes);
        prop_assume!(quote != faction && quote != CRIME_MINT && quote != FRAUD_MINT);

        let quote_program = if quote_is_token_2022 {
            TOKEN_2022_PROGRAM_ID
        } else {
            SPL_TOKEN_PROGRAM_ID
        };
        let (mint_a, mint_b, token_program_a, token_program_b) = if quote < faction {
            (quote, faction, quote_program, TOKEN_2022_PROGRAM_ID)
        } else {
            (faction, quote, TOKEN_2022_PROGRAM_ID, quote_program)
        };
        let (pool, _) = Pubkey::find_program_address(
            &[b"pool", mint_a.as_ref(), mint_b.as_ref()],
            &AMM_PROGRAM_ID,
        );
        let mut data = vec![0u8; 224];
        data[..8].copy_from_slice(&POOL_STATE_DISCRIMINATOR);
        data[9..41].copy_from_slice(mint_a.as_ref());
        data[41..73].copy_from_slice(mint_b.as_ref());
        data[73..105].copy_from_slice(Pubkey::new_unique().as_ref());
        data[105..137].copy_from_slice(Pubkey::new_unique().as_ref());
        data[137..145].copy_from_slice(&reserve_a.to_le_bytes());
        data[145..153].copy_from_slice(&reserve_b.to_le_bytes());
        data[153..155].copy_from_slice(&100u16.to_le_bytes());
        data[155] = 1;
        data[160..192].copy_from_slice(token_program_a.as_ref());
        data[192..224].copy_from_slice(token_program_b.as_ref());

        let discovered = SolPoolAmm::from_keyed_account(&keyed(pool, data), &context()).unwrap();
        prop_assert_eq!(discovered.key(), pool);
        prop_assert_eq!(discovered.get_reserve_mints(), vec![quote, faction]);
    }
}
