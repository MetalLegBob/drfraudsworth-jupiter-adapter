// Mainnet RPC Validation — real mainnet account data embedded as hex.
//
// Adapted from Titan adapter's test_mainnet_validation.rs (Phase 8.4).
// Uses the SAME hex-encoded mainnet data (fetched 2026-03-30).
//
// Tests: parse real data, construct Amm, update from AccountMap, quote with
// live reserves, verify discriminators.

use jupiter_amm_interface::{AccountMap, Amm, AmmContext, ClockRef, KeyedAccount, QuoteParams, SwapMode};
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;

use drfraudsworth_jupiter_adapter::accounts::addresses::*;
use drfraudsworth_jupiter_adapter::constants::EPOCH_STATE_DISCRIMINATOR;
use drfraudsworth_jupiter_adapter::sol_pool_amm::SolPoolAmm;
use drfraudsworth_jupiter_adapter::vault_amm::{VaultAmm, known_instances};
use drfraudsworth_jupiter_adapter::state::epoch_state::compute_epoch_state_discriminator;

// =============================================================================
// Real mainnet account data (fetched 2026-03-30)
// =============================================================================

// CRIME/SOL pool: 224 bytes, owner=AMM_PROGRAM_ID
const CRIME_POOL_HEX: &str = "f7ede3f5d7c3de4600069b8857feab8184fb687f634618c035dac439dc1aeb3b5598a0f0000000000109134573ad65aad688e3a59dbac1022ea9152f3e64e6753c6c8b8f4c88d85d2700fc6d2c6f43b2c0024d48a5403929af7eedecf8aab6a1aaae0c6972d120936a571fd3b6c0f3e3ff9e8a33dd5196c7d9d0d5e04608be2397293746594a086542ba7385e17c000000db49fe189df9000064000100fffeff06ddf6e1d765a193d9cbe146ceeb79ac1cb485ed5f5b37913a8cf5857eff00a906ddf6e1ee758fde18425dbce46ccddab61afc4d83b90d27febdf928d8a18bfc";

// FRAUD/SOL pool: 224 bytes, owner=AMM_PROGRAM_ID
const FRAUD_POOL_HEX: &str = "f7ede3f5d7c3de4600069b8857feab8184fb687f634618c035dac439dc1aeb3b5598a0f00000000001dcb6edb69e6c5568401b5cbe4ef39fb349f28a21600af3ff6570c64ec4158f102aa53245d63de0e5354ee107fbe3c25b196b14d292a9e924780cd442f5888b701aa43833c243b98db7ea60a5cec425bfee39436c07cede06f7ad893b37974fb29d96ceee7b00000003db490ef6fb000064000100fffeff06ddf6e1d765a193d9cbe146ceeb79ac1cb485ed5f5b37913a8cf5857eff00a906ddf6e1ee758fde18425dbce46ccddab61afc4d83b90d27febdf928d8a18bfc";

// EpochState: 172 bytes, owner=EPOCH_PROGRAM_ID
const EPOCH_STATE_HEX: &str = "bf3f8bed900cdfd20cf03c1800000000d40200009ca66e1800000000002c01b0042c014c04b0049001f6a66e1800000000000193caf4458e03ae8b293942525851761a4a586c84d6f968d8bef5ecd9a7ae136e000000c0ac6a1800000000c6ab6a18000000009a0200000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001fe";

// VaultConfig: 9 bytes, owner=CONVERSION_VAULT_PROGRAM_ID
const VAULT_CONFIG_HEX: &str = "63562bd8b866774dff";

fn decode_hex(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

fn amm_context() -> AmmContext {
    AmmContext { clock_ref: ClockRef::default() }
}

fn build_account_map(entries: Vec<(Pubkey, Vec<u8>, Pubkey)>) -> AccountMap {
    let mut map = AccountMap::default();
    for (key, data, owner) in entries {
        map.insert(key, Account {
            lamports: 2_000_000,
            data,
            owner,
            executable: false,
            rent_epoch: 0,
        });
    }
    map
}

// =============================================================================
// from_keyed_account with real data
// =============================================================================

#[test]
fn crime_pool_from_keyed_account_real_data() {
    let data = decode_hex(CRIME_POOL_HEX);
    let keyed = KeyedAccount {
        key: CRIME_SOL_POOL,
        account: Account {
            lamports: 2_449_920, data, owner: AMM_PROGRAM_ID,
            executable: false, rent_epoch: 0,
        },
        params: None,
    };

    let amm = SolPoolAmm::from_keyed_account(&keyed, &amm_context()).unwrap();
    assert_eq!(amm.key(), CRIME_SOL_POOL);
    assert_eq!(amm.label(), "Dr Fraudsworth");
}

#[test]
fn fraud_pool_from_keyed_account_real_data() {
    let data = decode_hex(FRAUD_POOL_HEX);
    let keyed = KeyedAccount {
        key: FRAUD_SOL_POOL,
        account: Account {
            lamports: 2_449_920, data, owner: AMM_PROGRAM_ID,
            executable: false, rent_epoch: 0,
        },
        params: None,
    };

    let amm = SolPoolAmm::from_keyed_account(&keyed, &amm_context()).unwrap();
    assert_eq!(amm.key(), FRAUD_SOL_POOL);
}

// =============================================================================
// update with real data
// =============================================================================

#[test]
fn crime_pool_update_with_real_data() {
    let keyed = KeyedAccount {
        key: CRIME_SOL_POOL,
        account: Account {
            lamports: 2_449_920, data: decode_hex(CRIME_POOL_HEX), owner: AMM_PROGRAM_ID,
            executable: false, rent_epoch: 0,
        },
        params: None,
    };

    let mut amm = SolPoolAmm::from_keyed_account(&keyed, &amm_context()).unwrap();

    let account_map = build_account_map(vec![
        (CRIME_SOL_POOL, decode_hex(CRIME_POOL_HEX), AMM_PROGRAM_ID),
        (EPOCH_STATE_PDA, decode_hex(EPOCH_STATE_HEX), EPOCH_PROGRAM_ID),
    ]);

    amm.update(&account_map).unwrap();
}

#[test]
fn fraud_pool_update_with_real_data() {
    let keyed = KeyedAccount {
        key: FRAUD_SOL_POOL,
        account: Account {
            lamports: 2_449_920, data: decode_hex(FRAUD_POOL_HEX), owner: AMM_PROGRAM_ID,
            executable: false, rent_epoch: 0,
        },
        params: None,
    };

    let mut amm = SolPoolAmm::from_keyed_account(&keyed, &amm_context()).unwrap();

    let account_map = build_account_map(vec![
        (FRAUD_SOL_POOL, decode_hex(FRAUD_POOL_HEX), AMM_PROGRAM_ID),
        (EPOCH_STATE_PDA, decode_hex(EPOCH_STATE_HEX), EPOCH_PROGRAM_ID),
    ]);

    amm.update(&account_map).unwrap();
}

// =============================================================================
// Quotes with real data
// =============================================================================

#[test]
fn crime_pool_buy_quote_real_data() {
    let keyed = KeyedAccount {
        key: CRIME_SOL_POOL,
        account: Account {
            lamports: 2_449_920, data: decode_hex(CRIME_POOL_HEX), owner: AMM_PROGRAM_ID,
            executable: false, rent_epoch: 0,
        },
        params: None,
    };
    let mut amm = SolPoolAmm::from_keyed_account(&keyed, &amm_context()).unwrap();

    let account_map = build_account_map(vec![
        (CRIME_SOL_POOL, decode_hex(CRIME_POOL_HEX), AMM_PROGRAM_ID),
        (EPOCH_STATE_PDA, decode_hex(EPOCH_STATE_HEX), EPOCH_PROGRAM_ID),
    ]);
    amm.update(&account_map).unwrap();

    let q = amm.quote(&QuoteParams {
        amount: 1_000_000_000, // 1 SOL
        input_mint: NATIVE_MINT, output_mint: CRIME_MINT,
        swap_mode: SwapMode::ExactIn,
    }).unwrap();

    eprintln!("CRIME buy 1 SOL: {} tokens out", q.out_amount);
    assert!(q.out_amount > 0, "Should produce tokens");
    assert!(q.out_amount < 1_000_000_000_000, "Shouldn't exceed reasonable bounds");
}

#[test]
fn crime_pool_sell_quote_real_data() {
    let keyed = KeyedAccount {
        key: CRIME_SOL_POOL,
        account: Account {
            lamports: 2_449_920, data: decode_hex(CRIME_POOL_HEX), owner: AMM_PROGRAM_ID,
            executable: false, rent_epoch: 0,
        },
        params: None,
    };
    let mut amm = SolPoolAmm::from_keyed_account(&keyed, &amm_context()).unwrap();

    let account_map = build_account_map(vec![
        (CRIME_SOL_POOL, decode_hex(CRIME_POOL_HEX), AMM_PROGRAM_ID),
        (EPOCH_STATE_PDA, decode_hex(EPOCH_STATE_HEX), EPOCH_PROGRAM_ID),
    ]);
    amm.update(&account_map).unwrap();

    let q = amm.quote(&QuoteParams {
        amount: 1_000_000, // 1 CRIME token (6 decimals)
        input_mint: CRIME_MINT, output_mint: NATIVE_MINT,
        swap_mode: SwapMode::ExactIn,
    }).unwrap();

    eprintln!("CRIME sell 1 token: {} lamports out", q.out_amount);
    assert!(q.out_amount > 0, "Should produce some SOL");
}

#[test]
fn fraud_pool_buy_quote_real_data() {
    let keyed = KeyedAccount {
        key: FRAUD_SOL_POOL,
        account: Account {
            lamports: 2_449_920, data: decode_hex(FRAUD_POOL_HEX), owner: AMM_PROGRAM_ID,
            executable: false, rent_epoch: 0,
        },
        params: None,
    };
    let mut amm = SolPoolAmm::from_keyed_account(&keyed, &amm_context()).unwrap();

    let account_map = build_account_map(vec![
        (FRAUD_SOL_POOL, decode_hex(FRAUD_POOL_HEX), AMM_PROGRAM_ID),
        (EPOCH_STATE_PDA, decode_hex(EPOCH_STATE_HEX), EPOCH_PROGRAM_ID),
    ]);
    amm.update(&account_map).unwrap();

    let q = amm.quote(&QuoteParams {
        amount: 1_000_000_000,
        input_mint: NATIVE_MINT, output_mint: FRAUD_MINT,
        swap_mode: SwapMode::ExactIn,
    }).unwrap();

    eprintln!("FRAUD buy 1 SOL: {} tokens out", q.out_amount);
    assert!(q.out_amount > 0);
}

#[test]
fn fraud_pool_sell_quote_real_data() {
    let keyed = KeyedAccount {
        key: FRAUD_SOL_POOL,
        account: Account {
            lamports: 2_449_920, data: decode_hex(FRAUD_POOL_HEX), owner: AMM_PROGRAM_ID,
            executable: false, rent_epoch: 0,
        },
        params: None,
    };
    let mut amm = SolPoolAmm::from_keyed_account(&keyed, &amm_context()).unwrap();

    let account_map = build_account_map(vec![
        (FRAUD_SOL_POOL, decode_hex(FRAUD_POOL_HEX), AMM_PROGRAM_ID),
        (EPOCH_STATE_PDA, decode_hex(EPOCH_STATE_HEX), EPOCH_PROGRAM_ID),
    ]);
    amm.update(&account_map).unwrap();

    let q = amm.quote(&QuoteParams {
        amount: 1_000_000_000, // 1000 FRAUD tokens
        input_mint: FRAUD_MINT, output_mint: NATIVE_MINT,
        swap_mode: SwapMode::ExactIn,
    }).unwrap();

    eprintln!("FRAUD sell 1000 tokens: {} lamports out", q.out_amount);
    assert!(q.out_amount > 0);
}

// =============================================================================
// Vault with real VaultConfig
// =============================================================================

#[test]
fn vault_update_with_real_config() {
    let account_map = build_account_map(vec![
        (VAULT_CONFIG_PDA, decode_hex(VAULT_CONFIG_HEX), CONVERSION_VAULT_PROGRAM_ID),
    ]);

    for (_, mut amm) in known_instances() {
        amm.update(&account_map).unwrap();
    }
}

#[test]
fn vault_quotes_after_real_update() {
    let account_map = build_account_map(vec![
        (VAULT_CONFIG_PDA, decode_hex(VAULT_CONFIG_HEX), CONVERSION_VAULT_PROGRAM_ID),
    ]);

    let venues = [
        (CRIME_MINT, PROFIT_MINT, 10_000_000u64, "CRIME→PROFIT"),
        (FRAUD_MINT, PROFIT_MINT, 10_000_000, "FRAUD→PROFIT"),
        (PROFIT_MINT, CRIME_MINT, 100_000, "PROFIT→CRIME"),
        (PROFIT_MINT, FRAUD_MINT, 100_000, "PROFIT→FRAUD"),
    ];

    for (input, output, amount, label) in &venues {
        let mut amm = VaultAmm::new_for_testing(*input, *output);
        amm.update(&account_map).unwrap();

        let q = amm.quote(&QuoteParams {
            amount: *amount, input_mint: *input, output_mint: *output,
            swap_mode: SwapMode::ExactIn,
        }).unwrap();

        eprintln!("{}: {} in → {} out", label, amount, q.out_amount);
        assert!(q.out_amount > 0);
    }
}

// =============================================================================
// Discriminator match
// =============================================================================

#[test]
fn epoch_state_discriminator_matches_live() {
    let data = decode_hex(EPOCH_STATE_HEX);
    let live_disc = &data[0..8];
    assert_eq!(live_disc, EPOCH_STATE_DISCRIMINATOR,
        "Hardcoded EpochState discriminator does not match live mainnet data!");
}

#[test]
fn epoch_state_discriminator_matches_computed() {
    let computed = compute_epoch_state_discriminator();
    assert_eq!(computed, EPOCH_STATE_DISCRIMINATOR,
        "Computed discriminator doesn't match hardcoded constant!");
}

// =============================================================================
// Account meta structure with real data
// =============================================================================

#[test]
fn get_swap_and_account_metas_buy_real_data() {
    let keyed = KeyedAccount {
        key: CRIME_SOL_POOL,
        account: Account {
            lamports: 2_449_920, data: decode_hex(CRIME_POOL_HEX), owner: AMM_PROGRAM_ID,
            executable: false, rent_epoch: 0,
        },
        params: None,
    };
    let mut amm = SolPoolAmm::from_keyed_account(&keyed, &amm_context()).unwrap();

    let account_map = build_account_map(vec![
        (CRIME_SOL_POOL, decode_hex(CRIME_POOL_HEX), AMM_PROGRAM_ID),
        (EPOCH_STATE_PDA, decode_hex(EPOCH_STATE_HEX), EPOCH_PROGRAM_ID),
    ]);
    amm.update(&account_map).unwrap();

    let user = Pubkey::new_unique();
    let user_wsol = Pubkey::new_unique();
    let user_token = Pubkey::new_unique();

    let result = amm.get_swap_and_account_metas(&jupiter_amm_interface::SwapParams {
        swap_mode: SwapMode::ExactIn,
        in_amount: 1_000_000_000,
        out_amount: 0,
        source_mint: NATIVE_MINT,
        destination_mint: CRIME_MINT,
        source_token_account: user_wsol,
        destination_token_account: user_token,
        token_transfer_authority: user,
        quote_mint_to_referrer: None,
        jupiter_program_id: &Pubkey::new_unique(),
        missing_dynamic_accounts_as_default: false,
    }).unwrap();

    assert_eq!(result.account_metas.len(), 24, "Buy: 20 named + 4 hook");
    assert_eq!(result.account_metas[0].pubkey, user);
    assert!(result.account_metas[0].is_signer);
    eprintln!("Generated mainnet buy instruction: {} accounts", result.account_metas.len());
}
