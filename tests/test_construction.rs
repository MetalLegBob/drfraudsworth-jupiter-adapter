// Construction lifecycle tests — full from_keyed_account → update → quote cycle.
//
// Adapted from Titan adapter's test_construction.rs.
// Tests the Jupiter Amm lifecycle with mock AccountMap data.

use jupiter_amm_interface::{AccountMap, Amm, AmmContext, ClockRef, KeyedAccount, QuoteParams, SwapMode};
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;

use drfraudsworth_jupiter_adapter::accounts::addresses::*;
use drfraudsworth_jupiter_adapter::constants::*;
use drfraudsworth_jupiter_adapter::sol_pool_amm::SolPoolAmm;
use drfraudsworth_jupiter_adapter::vault_amm::{VaultAmm, known_instances};

// =============================================================================
// Mock data builders
// =============================================================================

fn amm_context() -> AmmContext {
    AmmContext { clock_ref: ClockRef::default() }
}

fn mock_pool_state_bytes(
    mint_a: &Pubkey,
    reserve_a: u64,
    reserve_b: u64,
    lp_fee_bps: u16,
) -> Vec<u8> {
    let mut data = vec![0u8; 224];
    data[8] = 0; // pool_type
    data[9..41].copy_from_slice(mint_a.as_ref());
    data[137..145].copy_from_slice(&reserve_a.to_le_bytes());
    data[145..153].copy_from_slice(&reserve_b.to_le_bytes());
    data[153..155].copy_from_slice(&lp_fee_bps.to_le_bytes());
    data
}

fn mock_epoch_state_bytes(
    crime_buy: u16,
    crime_sell: u16,
    fraud_buy: u16,
    fraud_sell: u16,
) -> Vec<u8> {
    let mut data = vec![0u8; 172];
    data[0..8].copy_from_slice(&EPOCH_STATE_DISCRIMINATOR);
    data[33..35].copy_from_slice(&crime_buy.to_le_bytes());
    data[35..37].copy_from_slice(&crime_sell.to_le_bytes());
    data[37..39].copy_from_slice(&fraud_buy.to_le_bytes());
    data[39..41].copy_from_slice(&fraud_sell.to_le_bytes());
    data
}

fn build_account_map(entries: Vec<(Pubkey, Vec<u8>, Pubkey)>) -> AccountMap {
    let mut map = AccountMap::default();
    for (key, data, owner) in entries {
        map.insert(key, Account {
            lamports: 1_000_000, data, owner, executable: false, rent_epoch: 0,
        });
    }
    map
}

// =============================================================================
// from_keyed_account tests
// =============================================================================

#[test]
fn sol_pool_from_keyed_account_crime() {
    let pool_data = mock_pool_state_bytes(&NATIVE_MINT, 50_000_000_000, 1_000_000_000_000, 100);
    let keyed = KeyedAccount {
        key: CRIME_SOL_POOL,
        account: Account {
            lamports: 1_000_000, data: pool_data, owner: AMM_PROGRAM_ID,
            executable: false, rent_epoch: 0,
        },
        params: None,
    };

    let amm = SolPoolAmm::from_keyed_account(&keyed, &amm_context()).unwrap();
    assert_eq!(amm.key(), CRIME_SOL_POOL);
    assert_eq!(amm.label(), "Dr Fraudsworth");
}

#[test]
fn sol_pool_from_keyed_account_fraud() {
    let pool_data = mock_pool_state_bytes(&NATIVE_MINT, 50_000_000_000, 1_000_000_000_000, 100);
    let keyed = KeyedAccount {
        key: FRAUD_SOL_POOL,
        account: Account {
            lamports: 1_000_000, data: pool_data, owner: AMM_PROGRAM_ID,
            executable: false, rent_epoch: 0,
        },
        params: None,
    };

    let amm = SolPoolAmm::from_keyed_account(&keyed, &amm_context()).unwrap();
    assert_eq!(amm.key(), FRAUD_SOL_POOL);
}

#[test]
fn sol_pool_from_keyed_account_rejects_short_data() {
    let keyed = KeyedAccount {
        key: CRIME_SOL_POOL,
        account: Account {
            lamports: 1_000_000, data: vec![0u8; 50], owner: AMM_PROGRAM_ID,
            executable: false, rent_epoch: 0,
        },
        params: None,
    };

    assert!(SolPoolAmm::from_keyed_account(&keyed, &amm_context()).is_err());
}

// =============================================================================
// Full lifecycle: from_keyed_account → update → quote
// =============================================================================

#[test]
fn full_lifecycle_crime_pool_buy() {
    // 1. Parse from account
    let pool_data = mock_pool_state_bytes(&NATIVE_MINT, 100_000_000_000, 500_000_000_000, 100);
    let keyed = KeyedAccount {
        key: CRIME_SOL_POOL,
        account: Account {
            lamports: 1_000_000, data: pool_data.clone(), owner: AMM_PROGRAM_ID,
            executable: false, rent_epoch: 0,
        },
        params: None,
    };
    let mut amm = SolPoolAmm::from_keyed_account(&keyed, &amm_context()).unwrap();

    // 2. Update with mock account map
    let epoch_data = mock_epoch_state_bytes(400, 1400, 1400, 400);
    let account_map = build_account_map(vec![
        (CRIME_SOL_POOL, pool_data, AMM_PROGRAM_ID),
        (EPOCH_STATE_PDA, epoch_data, EPOCH_PROGRAM_ID),
    ]);
    amm.update(&account_map).unwrap();

    // 3. Quote
    let q = amm.quote(&QuoteParams {
        amount: 1_000_000_000,
        input_mint: NATIVE_MINT, output_mint: CRIME_MINT,
        swap_mode: SwapMode::ExactIn,
    }).unwrap();

    assert!(q.out_amount > 0, "Should produce output after full lifecycle");
    assert!(q.fee_amount > 0, "Should have fees");
}

#[test]
fn full_lifecycle_fraud_pool_sell() {
    let pool_data = mock_pool_state_bytes(&NATIVE_MINT, 100_000_000_000, 500_000_000_000, 100);
    let keyed = KeyedAccount {
        key: FRAUD_SOL_POOL,
        account: Account {
            lamports: 1_000_000, data: pool_data.clone(), owner: AMM_PROGRAM_ID,
            executable: false, rent_epoch: 0,
        },
        params: None,
    };
    let mut amm = SolPoolAmm::from_keyed_account(&keyed, &amm_context()).unwrap();

    let epoch_data = mock_epoch_state_bytes(400, 1400, 1400, 400);
    let account_map = build_account_map(vec![
        (FRAUD_SOL_POOL, pool_data, AMM_PROGRAM_ID),
        (EPOCH_STATE_PDA, epoch_data, EPOCH_PROGRAM_ID),
    ]);
    amm.update(&account_map).unwrap();

    let q = amm.quote(&QuoteParams {
        amount: 10_000_000_000, // 10K tokens
        input_mint: FRAUD_MINT, output_mint: NATIVE_MINT,
        swap_mode: SwapMode::ExactIn,
    }).unwrap();

    assert!(q.out_amount > 0);
}

// =============================================================================
// update error cases
// =============================================================================

#[test]
fn update_missing_pool_errors() {
    let pool_data = mock_pool_state_bytes(&NATIVE_MINT, 100_000_000_000, 500_000_000_000, 100);
    let keyed = KeyedAccount {
        key: CRIME_SOL_POOL,
        account: Account {
            lamports: 1_000_000, data: pool_data, owner: AMM_PROGRAM_ID,
            executable: false, rent_epoch: 0,
        },
        params: None,
    };
    let mut amm = SolPoolAmm::from_keyed_account(&keyed, &amm_context()).unwrap();

    // Empty account map — pool not found
    let account_map = AccountMap::default();
    assert!(amm.update(&account_map).is_err());
}

#[test]
fn update_missing_epoch_errors() {
    let pool_data = mock_pool_state_bytes(&NATIVE_MINT, 100_000_000_000, 500_000_000_000, 100);
    let keyed = KeyedAccount {
        key: CRIME_SOL_POOL,
        account: Account {
            lamports: 1_000_000, data: pool_data.clone(), owner: AMM_PROGRAM_ID,
            executable: false, rent_epoch: 0,
        },
        params: None,
    };
    let mut amm = SolPoolAmm::from_keyed_account(&keyed, &amm_context()).unwrap();

    // Only pool, no epoch state
    let account_map = build_account_map(vec![
        (CRIME_SOL_POOL, pool_data, AMM_PROGRAM_ID),
    ]);
    assert!(amm.update(&account_map).is_err());
}

#[test]
fn vault_update_missing_config_errors() {
    let (_, mut amm) = known_instances().into_iter().next().unwrap();
    let account_map = AccountMap::default();
    assert!(amm.update(&account_map).is_err());
}

#[test]
fn vault_update_empty_config_errors() {
    let (_, mut amm) = known_instances().into_iter().next().unwrap();
    let account_map = build_account_map(vec![
        (VAULT_CONFIG_PDA, vec![], CONVERSION_VAULT_PROGRAM_ID), // empty data
    ]);
    assert!(amm.update(&account_map).is_err());
}

// =============================================================================
// Reserve mints
// =============================================================================

#[test]
fn crime_pool_reserve_mints() {
    let amm = SolPoolAmm::new_for_testing(true, 1, 1, 400, 1400);
    let mints = amm.get_reserve_mints();
    assert_eq!(mints, vec![NATIVE_MINT, CRIME_MINT]);
}

#[test]
fn fraud_pool_reserve_mints() {
    let amm = SolPoolAmm::new_for_testing(false, 1, 1, 400, 1400);
    let mints = amm.get_reserve_mints();
    assert_eq!(mints, vec![NATIVE_MINT, FRAUD_MINT]);
}

#[test]
fn vault_reserve_mints_all_directions() {
    let instances = known_instances();
    let expected = [
        (CRIME_MINT, PROFIT_MINT),
        (FRAUD_MINT, PROFIT_MINT),
        (PROFIT_MINT, CRIME_MINT),
        (PROFIT_MINT, FRAUD_MINT),
    ];

    for ((_, amm), (exp_in, exp_out)) in instances.iter().zip(expected.iter()) {
        let mints = amm.get_reserve_mints();
        assert_eq!(mints, vec![*exp_in, *exp_out]);
    }
}

// =============================================================================
// accounts_to_update
// =============================================================================

#[test]
fn sol_pool_accounts_to_update() {
    let amm = SolPoolAmm::new_for_testing(true, 1, 1, 400, 1400);
    let accounts = amm.get_accounts_to_update();
    assert_eq!(accounts.len(), 2);
    assert_eq!(accounts[0], CRIME_SOL_POOL);
    assert_eq!(accounts[1], EPOCH_STATE_PDA);
}

#[test]
fn vault_accounts_to_update() {
    for (_, amm) in known_instances() {
        let accounts = amm.get_accounts_to_update();
        assert_eq!(accounts, vec![VAULT_CONFIG_PDA]);
    }
}

// =============================================================================
// clone_amm
// =============================================================================

#[test]
fn clone_amm_preserves_state() {
    let amm = SolPoolAmm::new_for_testing(true, 100_000_000_000, 500_000_000_000, 400, 1400);
    let cloned = amm.clone_amm();

    let q_original = amm.quote(&QuoteParams {
        amount: 1_000_000_000, input_mint: NATIVE_MINT, output_mint: CRIME_MINT,
        swap_mode: SwapMode::ExactIn,
    }).unwrap();

    let q_cloned = cloned.quote(&QuoteParams {
        amount: 1_000_000_000, input_mint: NATIVE_MINT, output_mint: CRIME_MINT,
        swap_mode: SwapMode::ExactIn,
    }).unwrap();

    assert_eq!(q_original.out_amount, q_cloned.out_amount, "Clone should produce same output");
}

// =============================================================================
// No-alloc structural test
// =============================================================================

#[test]
fn quote_many_inputs_no_accumulation() {
    let amm = SolPoolAmm::new_for_testing(true, 100_000_000_000, 1_000_000_000_000, 400, 1400);

    for amount in [1u64, 100, 1_000, 1_000_000, 1_000_000_000, 100_000_000_000] {
        let _ = amm.quote(&QuoteParams {
            amount, input_mint: NATIVE_MINT, output_mint: CRIME_MINT,
            swap_mode: SwapMode::ExactIn,
        });
    }

    let vault = VaultAmm::new_for_testing(CRIME_MINT, PROFIT_MINT);
    for amount in [100u64, 1_000, 10_000, 1_000_000, 1_000_000_000] {
        let _ = vault.quote(&QuoteParams {
            amount, input_mint: CRIME_MINT, output_mint: PROFIT_MINT,
            swap_mode: SwapMode::ExactIn,
        });
    }
}

// =============================================================================
// Max tax edge case
// =============================================================================

#[test]
fn max_tax_50pct_still_produces_output() {
    let amm = SolPoolAmm::new_for_testing(true, 100_000_000_000, 100_000_000_000, 5000, 5000);

    let q = amm.quote(&QuoteParams {
        amount: 1_000_000_000, input_mint: NATIVE_MINT, output_mint: CRIME_MINT,
        swap_mode: SwapMode::ExactIn,
    }).unwrap();

    assert!(q.out_amount > 0);
    assert!(q.fee_amount >= 500_000_000, "50% tax should take at least 500M lamports");
}

#[test]
fn zero_tax_higher_output() {
    let amm_taxed = SolPoolAmm::new_for_testing(true, 100_000_000_000, 100_000_000_000, 400, 1400);
    let amm_no_tax = SolPoolAmm::new_for_testing(true, 100_000_000_000, 100_000_000_000, 0, 0);

    let q_taxed = amm_taxed.quote(&QuoteParams {
        amount: 1_000_000_000, input_mint: NATIVE_MINT, output_mint: CRIME_MINT,
        swap_mode: SwapMode::ExactIn,
    }).unwrap();

    let q_free = amm_no_tax.quote(&QuoteParams {
        amount: 1_000_000_000, input_mint: NATIVE_MINT, output_mint: CRIME_MINT,
        swap_mode: SwapMode::ExactIn,
    }).unwrap();

    assert!(q_free.out_amount > q_taxed.out_amount, "Zero-tax should produce more output");
}
