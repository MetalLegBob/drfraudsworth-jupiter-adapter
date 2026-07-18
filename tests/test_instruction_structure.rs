// Instruction structure verification — account counts, ordering, signer flags.
//
// Adapted from Titan adapter's instruction tests (Phase 8.7 Part 5).
// Jupiter's Amm trait returns SwapAndAccountMetas (not full instructions),
// so we verify account metas structure instead of instruction data bytes.

use jupiter_amm_interface::{Amm, SwapMode, SwapParams};
use solana_sdk::pubkey::Pubkey;

use drfraudsworth_jupiter_adapter::accounts::addresses::*;
use drfraudsworth_jupiter_adapter::sol_pool_amm::SolPoolAmm;
use drfraudsworth_jupiter_adapter::vault_amm::VaultAmm;

fn crime_amm() -> SolPoolAmm {
    SolPoolAmm::new_for_testing(true, 100_000_000_000, 1_000_000_000_000, 400, 1400)
}

fn fraud_amm() -> SolPoolAmm {
    SolPoolAmm::new_for_testing(false, 100_000_000_000, 1_000_000_000_000, 1400, 400)
}

fn make_swap_params() -> (Pubkey, Pubkey, Pubkey, Pubkey) {
    let user = Pubkey::new_unique();
    let source_ata = Pubkey::new_unique();
    let dest_ata = Pubkey::new_unique();
    let jup_program = Pubkey::new_unique();
    (user, source_ata, dest_ata, jup_program)
}

// =============================================================================
// Buy instruction structure
// =============================================================================

#[test]
fn buy_crime_has_24_accounts() {
    let amm = crime_amm();
    let (user, wsol_ata, token_ata, jup_id) = make_swap_params();

    let result = amm.get_swap_and_account_metas(&SwapParams {
        swap_mode: SwapMode::ExactIn,
        in_amount: 1_000_000_000,
        out_amount: 0,
        source_mint: NATIVE_MINT,
        destination_mint: CRIME_MINT,
        source_token_account: wsol_ata,
        destination_token_account: token_ata,
        token_transfer_authority: user,
        quote_mint_to_referrer: None,
        jupiter_program_id: &jup_id,
        missing_dynamic_accounts_as_default: false,
    }).unwrap();

    assert_eq!(result.account_metas.len(), 24, "Buy: 20 named + 4 hook");
}

#[test]
fn buy_fraud_has_24_accounts() {
    let amm = fraud_amm();
    let (user, wsol_ata, token_ata, jup_id) = make_swap_params();

    let result = amm.get_swap_and_account_metas(&SwapParams {
        swap_mode: SwapMode::ExactIn,
        in_amount: 1_000_000_000,
        out_amount: 0,
        source_mint: NATIVE_MINT,
        destination_mint: FRAUD_MINT,
        source_token_account: wsol_ata,
        destination_token_account: token_ata,
        token_transfer_authority: user,
        quote_mint_to_referrer: None,
        jupiter_program_id: &jup_id,
        missing_dynamic_accounts_as_default: false,
    }).unwrap();

    assert_eq!(result.account_metas.len(), 24);
}

// =============================================================================
// Sell instruction structure
// =============================================================================

#[test]
fn sell_crime_has_25_accounts() {
    let amm = crime_amm();
    let (user, token_ata, wsol_ata, jup_id) = make_swap_params();

    let result = amm.get_swap_and_account_metas(&SwapParams {
        swap_mode: SwapMode::ExactIn,
        in_amount: 1_000_000_000,
        out_amount: 0,
        source_mint: CRIME_MINT,
        destination_mint: NATIVE_MINT,
        source_token_account: token_ata,
        destination_token_account: wsol_ata,
        token_transfer_authority: user,
        quote_mint_to_referrer: None,
        jupiter_program_id: &jup_id,
        missing_dynamic_accounts_as_default: false,
    }).unwrap();

    assert_eq!(result.account_metas.len(), 25, "Sell: 21 named + 4 hook");
}

#[test]
fn sell_fraud_has_25_accounts() {
    let amm = fraud_amm();
    let (user, token_ata, wsol_ata, jup_id) = make_swap_params();

    let result = amm.get_swap_and_account_metas(&SwapParams {
        swap_mode: SwapMode::ExactIn,
        in_amount: 1_000_000_000,
        out_amount: 0,
        source_mint: FRAUD_MINT,
        destination_mint: NATIVE_MINT,
        source_token_account: token_ata,
        destination_token_account: wsol_ata,
        token_transfer_authority: user,
        quote_mint_to_referrer: None,
        jupiter_program_id: &jup_id,
        missing_dynamic_accounts_as_default: false,
    }).unwrap();

    assert_eq!(result.account_metas.len(), 25);
}

// =============================================================================
// Vault instruction structure
// =============================================================================

#[test]
fn vault_crime_to_profit_has_17_accounts() {
    let amm = VaultAmm::new_for_testing(CRIME_MINT, PROFIT_MINT);
    let (user, input_ata, output_ata, jup_id) = make_swap_params();

    let result = amm.get_swap_and_account_metas(&SwapParams {
        swap_mode: SwapMode::ExactIn,
        in_amount: 10_000,
        out_amount: 0,
        source_mint: CRIME_MINT,
        destination_mint: PROFIT_MINT,
        source_token_account: input_ata,
        destination_token_account: output_ata,
        token_transfer_authority: user,
        quote_mint_to_referrer: None,
        jupiter_program_id: &jup_id,
        missing_dynamic_accounts_as_default: false,
    }).unwrap();

    assert_eq!(result.account_metas.len(), 17, "Vault: 9 named + 8 hook");
}

#[test]
fn vault_profit_to_fraud_has_17_accounts() {
    let amm = VaultAmm::new_for_testing(PROFIT_MINT, FRAUD_MINT);
    let (user, input_ata, output_ata, jup_id) = make_swap_params();

    let result = amm.get_swap_and_account_metas(&SwapParams {
        swap_mode: SwapMode::ExactIn,
        in_amount: 100,
        out_amount: 0,
        source_mint: PROFIT_MINT,
        destination_mint: FRAUD_MINT,
        source_token_account: input_ata,
        destination_token_account: output_ata,
        token_transfer_authority: user,
        quote_mint_to_referrer: None,
        jupiter_program_id: &jup_id,
        missing_dynamic_accounts_as_default: false,
    }).unwrap();

    assert_eq!(result.account_metas.len(), 17);
}

// =============================================================================
// Account ordering and flags
// =============================================================================

#[test]
fn buy_user_is_signer_and_writable() {
    let amm = crime_amm();
    let user = Pubkey::new_unique();
    let jup_id = Pubkey::new_unique();

    let result = amm.get_swap_and_account_metas(&SwapParams {
        swap_mode: SwapMode::ExactIn,
        in_amount: 1_000_000_000,
        out_amount: 0,
        source_mint: NATIVE_MINT,
        destination_mint: CRIME_MINT,
        source_token_account: Pubkey::new_unique(),
        destination_token_account: Pubkey::new_unique(),
        token_transfer_authority: user,
        quote_mint_to_referrer: None,
        jupiter_program_id: &jup_id,
        missing_dynamic_accounts_as_default: false,
    }).unwrap();

    assert_eq!(result.account_metas[0].pubkey, user);
    assert!(result.account_metas[0].is_signer);
    assert!(result.account_metas[0].is_writable);
}

#[test]
fn buy_epoch_state_is_readonly() {
    let amm = crime_amm();
    let jup_id = Pubkey::new_unique();

    let result = amm.get_swap_and_account_metas(&SwapParams {
        swap_mode: SwapMode::ExactIn,
        in_amount: 1_000_000_000,
        out_amount: 0,
        source_mint: NATIVE_MINT,
        destination_mint: CRIME_MINT,
        source_token_account: Pubkey::new_unique(),
        destination_token_account: Pubkey::new_unique(),
        token_transfer_authority: Pubkey::new_unique(),
        quote_mint_to_referrer: None,
        jupiter_program_id: &jup_id,
        missing_dynamic_accounts_as_default: false,
    }).unwrap();

    assert_eq!(result.account_metas[1].pubkey, EPOCH_STATE_PDA);
    assert!(!result.account_metas[1].is_writable);
    assert!(!result.account_metas[1].is_signer);
}

#[test]
fn buy_swap_authority_is_readonly() {
    let amm = crime_amm();
    let jup_id = Pubkey::new_unique();

    let result = amm.get_swap_and_account_metas(&SwapParams {
        swap_mode: SwapMode::ExactIn,
        in_amount: 1_000_000_000,
        out_amount: 0,
        source_mint: NATIVE_MINT,
        destination_mint: CRIME_MINT,
        source_token_account: Pubkey::new_unique(),
        destination_token_account: Pubkey::new_unique(),
        token_transfer_authority: Pubkey::new_unique(),
        quote_mint_to_referrer: None,
        jupiter_program_id: &jup_id,
        missing_dynamic_accounts_as_default: false,
    }).unwrap();

    assert_eq!(result.account_metas[2].pubkey, SWAP_AUTHORITY_PDA);
    assert!(!result.account_metas[2].is_writable, "Buy: swap_authority should be readonly");
}

#[test]
fn sell_swap_authority_is_writable() {
    let amm = crime_amm();
    let jup_id = Pubkey::new_unique();

    let result = amm.get_swap_and_account_metas(&SwapParams {
        swap_mode: SwapMode::ExactIn,
        in_amount: 1_000_000_000,
        out_amount: 0,
        source_mint: CRIME_MINT,
        destination_mint: NATIVE_MINT,
        source_token_account: Pubkey::new_unique(),
        destination_token_account: Pubkey::new_unique(),
        token_transfer_authority: Pubkey::new_unique(),
        quote_mint_to_referrer: None,
        jupiter_program_id: &jup_id,
        missing_dynamic_accounts_as_default: false,
    }).unwrap();

    assert_eq!(result.account_metas[2].pubkey, SWAP_AUTHORITY_PDA);
    assert!(result.account_metas[2].is_writable, "Sell: swap_authority should be writable");
}

#[test]
fn sell_has_wsol_intermediary_at_index_15() {
    let amm = crime_amm();
    let jup_id = Pubkey::new_unique();

    let result = amm.get_swap_and_account_metas(&SwapParams {
        swap_mode: SwapMode::ExactIn,
        in_amount: 1_000_000_000,
        out_amount: 0,
        source_mint: CRIME_MINT,
        destination_mint: NATIVE_MINT,
        source_token_account: Pubkey::new_unique(),
        destination_token_account: Pubkey::new_unique(),
        token_transfer_authority: Pubkey::new_unique(),
        quote_mint_to_referrer: None,
        jupiter_program_id: &jup_id,
        missing_dynamic_accounts_as_default: false,
    }).unwrap();

    // Sell has WSOL intermediary at index 15 (extra account vs buy)
    assert_eq!(result.account_metas[15].pubkey, WSOL_INTERMEDIARY_PDA);
    assert!(result.account_metas[15].is_writable);
}

#[test]
fn buy_pool_address_matches_crime() {
    let amm = crime_amm();
    let jup_id = Pubkey::new_unique();

    let result = amm.get_swap_and_account_metas(&SwapParams {
        swap_mode: SwapMode::ExactIn,
        in_amount: 1_000_000_000,
        out_amount: 0,
        source_mint: NATIVE_MINT,
        destination_mint: CRIME_MINT,
        source_token_account: Pubkey::new_unique(),
        destination_token_account: Pubkey::new_unique(),
        token_transfer_authority: Pubkey::new_unique(),
        quote_mint_to_referrer: None,
        jupiter_program_id: &jup_id,
        missing_dynamic_accounts_as_default: false,
    }).unwrap();

    assert_eq!(result.account_metas[4].pubkey, CRIME_SOL_POOL);
    assert_eq!(result.account_metas[8].pubkey, CRIME_MINT);
}

#[test]
fn buy_pool_address_matches_fraud() {
    let amm = fraud_amm();
    let jup_id = Pubkey::new_unique();

    let result = amm.get_swap_and_account_metas(&SwapParams {
        swap_mode: SwapMode::ExactIn,
        in_amount: 1_000_000_000,
        out_amount: 0,
        source_mint: NATIVE_MINT,
        destination_mint: FRAUD_MINT,
        source_token_account: Pubkey::new_unique(),
        destination_token_account: Pubkey::new_unique(),
        token_transfer_authority: Pubkey::new_unique(),
        quote_mint_to_referrer: None,
        jupiter_program_id: &jup_id,
        missing_dynamic_accounts_as_default: false,
    }).unwrap();

    assert_eq!(result.account_metas[4].pubkey, FRAUD_SOL_POOL);
    assert_eq!(result.account_metas[8].pubkey, FRAUD_MINT);
}

// =============================================================================
// Hook accounts placement
// =============================================================================

#[test]
fn buy_hook_accounts_are_last_4() {
    let amm = crime_amm();
    let jup_id = Pubkey::new_unique();

    let result = amm.get_swap_and_account_metas(&SwapParams {
        swap_mode: SwapMode::ExactIn,
        in_amount: 1_000_000_000,
        out_amount: 0,
        source_mint: NATIVE_MINT,
        destination_mint: CRIME_MINT,
        source_token_account: Pubkey::new_unique(),
        destination_token_account: Pubkey::new_unique(),
        token_transfer_authority: Pubkey::new_unique(),
        quote_mint_to_referrer: None,
        jupiter_program_id: &jup_id,
        missing_dynamic_accounts_as_default: false,
    }).unwrap();

    // Last 4 accounts are hook accounts
    // [20] = CRIME_HOOK_META
    assert_eq!(result.account_metas[20].pubkey, CRIME_HOOK_META);
    // [23] = TRANSFER_HOOK_PROGRAM_ID
    assert_eq!(result.account_metas[23].pubkey, TRANSFER_HOOK_PROGRAM_ID);

    // All hook accounts should be readonly
    for i in 20..24 {
        assert!(!result.account_metas[i].is_writable);
        assert!(!result.account_metas[i].is_signer);
    }
}

#[test]
fn vault_has_8_hook_accounts_at_end() {
    let amm = VaultAmm::new_for_testing(CRIME_MINT, PROFIT_MINT);
    let jup_id = Pubkey::new_unique();

    let result = amm.get_swap_and_account_metas(&SwapParams {
        swap_mode: SwapMode::ExactIn,
        in_amount: 10_000,
        out_amount: 0,
        source_mint: CRIME_MINT,
        destination_mint: PROFIT_MINT,
        source_token_account: Pubkey::new_unique(),
        destination_token_account: Pubkey::new_unique(),
        token_transfer_authority: Pubkey::new_unique(),
        quote_mint_to_referrer: None,
        jupiter_program_id: &jup_id,
        missing_dynamic_accounts_as_default: false,
    }).unwrap();

    // 9 named + 4 input hooks + 4 output hooks = 17
    assert_eq!(result.account_metas.len(), 17);

    // Vault config at index 1
    assert_eq!(result.account_metas[1].pubkey, VAULT_CONFIG_PDA);

    // Token-2022 at index 8
    assert_eq!(result.account_metas[8].pubkey, TOKEN_2022_PROGRAM_ID);

    // Input hooks [9..13], output hooks [13..17]
    // Input side hook meta = CRIME_HOOK_META
    assert_eq!(result.account_metas[9].pubkey, CRIME_HOOK_META);
    // Input side hook program = TRANSFER_HOOK_PROGRAM_ID
    assert_eq!(result.account_metas[12].pubkey, TRANSFER_HOOK_PROGRAM_ID);
    // Output side hook meta = PROFIT_HOOK_META
    assert_eq!(result.account_metas[13].pubkey, PROFIT_HOOK_META);
    // Output side hook program = TRANSFER_HOOK_PROGRAM_ID
    assert_eq!(result.account_metas[16].pubkey, TRANSFER_HOOK_PROGRAM_ID);
}
