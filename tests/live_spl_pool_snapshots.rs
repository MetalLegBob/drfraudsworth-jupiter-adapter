//! Deterministic mainnet snapshots for the four Phase 180 SPL pilot pools.
//!
//! The addresses below are test evidence, not a production allowlist. The
//! adapter discovers every valid faction pool from its AMM-owned PoolState.
//! Account bytes were fetched from mainnet on 2026-09-23 during epoch 9527.

use std::{str::FromStr, sync::atomic::Ordering};

use drfraudsworth_jupiter_adapter::{
    accounts::addresses::{
        AMM_PROGRAM_ID, CRIME_MINT, EPOCH_PROGRAM_ID, EPOCH_STATE_PDA, FRAUD_MINT,
        SPL_TOKEN_PROGRAM_ID, TAX_PROGRAM_ID,
    },
    instruction::TaxSwapLane,
    FactionPoolAmm,
};
use jupiter_amm_interface::{
    AccountMap, Amm, AmmContext, FeeMode, KeyedAccount, QuoteParams, Swap, SwapMode, SwapParams,
};
use solana_sdk::{account::Account, pubkey::Pubkey};

const EPOCH_HEX: &str = "bf3f8bed900cdfd20cf03c180000000037250000ed34cf1a000000000064007805900178051405640000000000000000000001000000000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000352500000635cf1a000000000001ed34cf1a000000000000000000000000010000000043240000020000000000000000000000000000000000000000000000000000000001fe";
const USDC_HEX: &str = "0100000098fe86e88d9be2ea8bc1cca4878b2988c240f52b8424bfb40ed1a2ddcb5e199b9dfc37a660391d000601010000006270aa8a59c59405b45286c86772e6cd126e9b8a5d3a38536d37f7b414e8b667";
const HYPE_HEX: &str = "01000000256a4dc30ce6c6e725ec7a56d0328cf32fdecf7e6ba1ad08ef55a0a50c09d669cc167c04ad8a02000901000000000000000000000000000000000000000000000000000000000000000000000000";

struct Case {
    pool: &'static str,
    pool_data: &'static str,
    quote: &'static str,
    quote_data: &'static str,
    faction: Pubkey,
    faction_is_a: bool,
    buy_out: u64,
    buy_fee: u64,
    sell_out: u64,
    sell_fee: u64,
}

fn decode_hex(value: &str) -> Vec<u8> {
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).unwrap())
        .collect()
}

fn account(owner: Pubkey, data: &str) -> Account {
    Account {
        lamports: 10_000_000,
        data: decode_hex(data),
        owner,
        executable: false,
        rent_epoch: u64::MAX,
    }
}

#[test]
fn live_spl_poolstates_discover_quote_and_build_both_directions() {
    let cases = [
        Case {
            pool: "HyJReAfMzABjEgZQNLrkdSR4pD5P78G5ucEXWRoVDNUa",
            pool_data: "f7ede3f5d7c3de460009134573ad65aad688e3a59dbac1022ea9152f3e64e6753c6c8b8f4c88d85d27c6fa7af3bedbad3a3d65f36aabc97431b1bbe4c2d2f6e0e47ca60203452f5d61589300399193193e1bef232f01638b14eb11cea84bafc1c03b8b63fa224c99d5fbb2bfc7144d2b90ec839e1d14a8969c2e84a3a0f470528c21eb8908dd676bf7bcea9255470c0000f4a69a890300000064000100fefff706ddf6e1ee758fde18425dbce46ccddab61afc4d83b90d27febdf928d8a18bfc06ddf6e1d765a193d9cbe146ceeb79ac1cb485ed5f5b37913a8cf5857eff00a9",
            quote: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            quote_data: USDC_HEX,
            faction: CRIME_MINT,
            faction_is_a: true,
            buy_out: 844_445_081,
            buy_fee: 49_600,
            sell_out: 959,
            sell_fee: 155,
        },
        Case {
            pool: "ETtBco8RUWNaNE9YozMg2KrpbJN7oqCjdd94QgwsAgzB",
            pool_data: "f7ede3f5d7c3de4600c6fa7af3bedbad3a3d65f36aabc97431b1bbe4c2d2f6e0e47ca60203452f5d61dcb6edb69e6c5568401b5cbe4ef39fb349f28a21600af3ff6570c64ec4158f103a6cb821b46eb8f9e1f5226d6a24934ecc0bcee94060bb7e53c6352bf6bcb301c77c6e616ba6aa5655af11e1a7f20b7cf20829cdc83935d5bd8124faa7935e5554481160030000002d4c1ae9850c000064000100fffdff06ddf6e1d765a193d9cbe146ceeb79ac1cb485ed5f5b37913a8cf5857eff00a906ddf6e1ee758fde18425dbce46ccddab61afc4d83b90d27febdf928d8a18bfc",
            quote: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            quote_data: USDC_HEX,
            faction: FRAUD_MINT,
            faction_is_a: false,
            buy_out: 818_035_849,
            buy_fee: 138_700,
            sell_out: 1_032,
            sell_fee: 10,
        },
        Case {
            pool: "HummhRt6eZLVDRT3NNCRgTs3Mouje4EypQvQbXKD5Mvy",
            pool_data: "f7ede3f5d7c3de460009134573ad65aad688e3a59dbac1022ea9152f3e64e6753c6c8b8f4c88d85d2778e17ff9cf9ef28b15e3d5832741ece7804e07d5c97f42ef33ed5f059c74925fb760bb76b540bf3571131082e3233104333def22993013363901fe220c7c53199942d9cebbbebf591d22ff8dc1dd4a8a887342018ba171da0d2e3da06155ecfbb1d40e86121800006e8748114900000064000100fefffe06ddf6e1ee758fde18425dbce46ccddab61afc4d83b90d27febdf928d8a18bfc06ddf6e1d765a193d9cbe146ceeb79ac1cb485ed5f5b37913a8cf5857eff00a9",
            quote: "98sMhvDwXj1RQi5c5Mndm3vPe9cBqPrbLaufMXFNMh5g",
            quote_data: HYPE_HEX,
            faction: CRIME_MINT,
            faction_is_a: true,
            buy_out: 80_156_618,
            buy_fee: 49_600,
            sell_out: 10_095,
            sell_fee: 1_643,
        },
        Case {
            pool: "2EVKU8ZmuZRXc6baRUHZVUjry1AaBJ9mwDgUJDZxrDGz",
            pool_data: "f7ede3f5d7c3de460078e17ff9cf9ef28b15e3d5832741ece7804e07d5c97f42ef33ed5f059c74925fdcb6edb69e6c5568401b5cbe4ef39fb349f28a21600af3ff6570c64ec4158f10d5a7f4905a7294a3ba9b166ae5bd7ccbdcf0af272df18498430f809ec89053baa8780ca4dc0a988b4d67e441a0f4fa41c33a1a46ea52763266c0787d0a4df362c01033ac47000000dc012be98e18000064000100feffff06ddf6e1d765a193d9cbe146ceeb79ac1cb485ed5f5b37913a8cf5857eff00a906ddf6e1ee758fde18425dbce46ccddab61afc4d83b90d27febdf928d8a18bfc",
            quote: "98sMhvDwXj1RQi5c5Mndm3vPe9cBqPrbLaufMXFNMh5g",
            quote_data: HYPE_HEX,
            faction: FRAUD_MINT,
            faction_is_a: false,
            buy_out: 75_550_447,
            buy_fee: 138_700,
            sell_out: 11_174,
            sell_fee: 112,
        },
    ];

    for case in cases {
        let pool = Pubkey::from_str(case.pool).unwrap();
        let quote_mint = Pubkey::from_str(case.quote).unwrap();
        let pool_account = account(AMM_PROGRAM_ID, case.pool_data);
        let context = AmmContext::default();
        context.clock_ref.slot.store(u64::MAX, Ordering::Relaxed);
        let mut amm = FactionPoolAmm::from_keyed_account(
            &KeyedAccount {
                key: pool,
                account: pool_account.clone(),
                params: None,
            },
            &context,
        )
        .unwrap();

        assert_eq!(amm.program_id(), TAX_PROGRAM_ID);
        assert_eq!(amm.get_reserve_mints(), vec![quote_mint, case.faction]);
        assert_eq!(
            amm.get_accounts_to_update(),
            vec![pool, EPOCH_STATE_PDA, quote_mint]
        );

        let mut accounts = AccountMap::default();
        accounts.insert(pool, pool_account);
        accounts.insert(EPOCH_STATE_PDA, account(EPOCH_PROGRAM_ID, EPOCH_HEX));
        accounts.insert(quote_mint, account(SPL_TOKEN_PROGRAM_ID, case.quote_data));
        amm.update(&accounts).unwrap();
        assert!(amm.is_active());

        for (input, output, expected_out, expected_fee, expected_accounts, expected_lane) in [
            (
                quote_mint,
                case.faction,
                case.buy_out,
                case.buy_fee,
                21usize,
                TaxSwapLane::SplBuy,
            ),
            (
                case.faction,
                quote_mint,
                case.sell_out,
                case.sell_fee,
                22usize,
                TaxSwapLane::SplSell,
            ),
        ] {
            let quoted = amm
                .quote(&QuoteParams {
                    amount: 1_000_000,
                    input_mint: input,
                    output_mint: output,
                    swap_mode: SwapMode::ExactIn,
                    fee_mode: FeeMode::Normal,
                })
                .unwrap();
            assert_eq!(quoted.out_amount, expected_out, "pool {pool}");
            assert_eq!(quoted.fee_amount, expected_fee, "pool {pool}");
            assert_eq!(
                TaxSwapLane::for_mints(&quote_mint, &case.faction, &input, &output),
                Some(expected_lane)
            );

            let user = Pubkey::new_unique();
            let source = Pubkey::new_unique();
            let destination = Pubkey::new_unique();
            let jupiter = Pubkey::new_unique();
            let built = amm
                .get_swap_and_account_metas(&SwapParams {
                    swap_mode: SwapMode::ExactIn,
                    in_amount: quoted.in_amount,
                    out_amount: quoted.out_amount,
                    source_mint: input,
                    destination_mint: output,
                    source_token_account: source,
                    destination_token_account: destination,
                    token_transfer_authority: user,
                    user,
                    payer: user,
                    quote_mint_to_referrer: None,
                    jupiter_program_id: &jupiter,
                    missing_dynamic_accounts_as_default: false,
                })
                .unwrap();

            assert_eq!(built.account_metas.len(), expected_accounts);
            assert_eq!(built.account_metas[0].pubkey, user);
            assert_eq!(built.account_metas[2].pubkey, pool);
            assert_eq!(
                built.account_metas[5].pubkey,
                if case.faction_is_a {
                    case.faction
                } else {
                    quote_mint
                }
            );
            assert_eq!(
                built.account_metas[6].pubkey,
                if case.faction_is_a {
                    quote_mint
                } else {
                    case.faction
                }
            );
            // Published interface 0.6.1 has no neutral placeholder variant.
            // This marker must be replaced by Jupiter's approved mapping.
            assert!(matches!(built.swap, Swap::TokenSwap));
        }
    }
}
