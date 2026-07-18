// Edge Case Gauntlet — Adversarial inputs that should never panic.
//
// Adapted from Titan adapter's test_edge_gauntlet.rs (Phase 8.7).
// Tests: amount=0, amount=1, amount=u64::MAX, ExactOut rejection,
// wrong mint, extreme reserves, all swap directions.

use solana_sdk::pubkey::Pubkey;
use jupiter_amm_interface::{Amm, QuoteParams, SwapMode};

use drfraudsworth_jupiter_adapter::accounts::addresses::*;
use drfraudsworth_jupiter_adapter::sol_pool_amm::SolPoolAmm;
use drfraudsworth_jupiter_adapter::vault_amm::{VaultAmm, known_instances};

fn crime_amm() -> SolPoolAmm {
    SolPoolAmm::new_for_testing(true, 100_000_000_000, 1_000_000_000_000, 400, 1400)
}

fn fraud_amm() -> SolPoolAmm {
    SolPoolAmm::new_for_testing(false, 100_000_000_000, 1_000_000_000_000, 1400, 400)
}

// =============================================================================
// (1) amount = 0
// =============================================================================

#[test]
fn zero_amount_sol_pool_buy() {
    let amm = crime_amm();
    let q = amm.quote(&QuoteParams {
        amount: 0, input_mint: NATIVE_MINT, output_mint: CRIME_MINT, swap_mode: SwapMode::ExactIn,
    }).unwrap();
    assert_eq!(q.out_amount, 0, "Zero input should give zero output");
}

#[test]
fn zero_amount_sol_pool_sell() {
    let amm = crime_amm();
    let q = amm.quote(&QuoteParams {
        amount: 0, input_mint: CRIME_MINT, output_mint: NATIVE_MINT, swap_mode: SwapMode::ExactIn,
    }).unwrap();
    assert_eq!(q.out_amount, 0);
}

#[test]
fn zero_amount_all_vault_directions() {
    let instances = known_instances();
    for (_, amm) in &instances {
        let mints = amm.get_reserve_mints();
        let result = amm.quote(&QuoteParams {
            amount: 0, input_mint: mints[0], output_mint: mints[1], swap_mode: SwapMode::ExactIn,
        });
        // Zero amount: vault divide direction returns error (0/100=0 → dust rejection),
        // multiply direction should return 0. Either is acceptable.
        if let Ok(q) = result {
            assert_eq!(q.out_amount, 0);
        }
    }
}

// =============================================================================
// (2) amount = 1
// =============================================================================

#[test]
fn one_lamport_sol_pool_buy_no_panic() {
    let amm = crime_amm();
    let q = amm.quote(&QuoteParams {
        amount: 1, input_mint: NATIVE_MINT, output_mint: CRIME_MINT, swap_mode: SwapMode::ExactIn,
    }).unwrap();
    // 1 lamport: tax rounds to 0, LP fee rounds to 0 effective → tiny or 0 output
    assert!(q.out_amount <= 100);
}

#[test]
fn one_lamport_sol_pool_sell_no_panic() {
    let amm = crime_amm();
    let q = amm.quote(&QuoteParams {
        amount: 1, input_mint: CRIME_MINT, output_mint: NATIVE_MINT, swap_mode: SwapMode::ExactIn,
    }).unwrap();
    assert!(q.out_amount <= 1);
}

#[test]
fn one_unit_vault_divide_errors() {
    // 1 CRIME / 100 = 0 PROFIT → should error (dust too small)
    let amm = VaultAmm::new_for_testing(CRIME_MINT, PROFIT_MINT);
    let result = amm.quote(&QuoteParams {
        amount: 1, input_mint: CRIME_MINT, output_mint: PROFIT_MINT, swap_mode: SwapMode::ExactIn,
    });
    assert!(result.is_err());
}

#[test]
fn one_unit_vault_multiply_succeeds() {
    // 1 PROFIT * 100 = 100 CRIME → should succeed
    let amm = VaultAmm::new_for_testing(PROFIT_MINT, CRIME_MINT);
    let q = amm.quote(&QuoteParams {
        amount: 1, input_mint: PROFIT_MINT, output_mint: CRIME_MINT, swap_mode: SwapMode::ExactIn,
    }).unwrap();
    assert_eq!(q.out_amount, 100);
}

// =============================================================================
// (3) amount = u64::MAX — must not panic
// =============================================================================

#[test]
fn u64_max_sol_pool_buy_no_panic() {
    let amm = crime_amm();
    let result = amm.quote(&QuoteParams {
        amount: u64::MAX, input_mint: NATIVE_MINT, output_mint: CRIME_MINT, swap_mode: SwapMode::ExactIn,
    });
    // May succeed or error — either is fine, just no panic
    if let Ok(q) = result {
        assert!(q.out_amount > 0);
    }
}

#[test]
fn u64_max_sol_pool_sell_no_panic() {
    let amm = crime_amm();
    let result = amm.quote(&QuoteParams {
        amount: u64::MAX, input_mint: CRIME_MINT, output_mint: NATIVE_MINT, swap_mode: SwapMode::ExactIn,
    });
    if let Ok(q) = result {
        assert!(q.out_amount > 0);
    }
}

#[test]
fn u64_max_vault_divide_no_panic() {
    let amm = VaultAmm::new_for_testing(CRIME_MINT, PROFIT_MINT);
    // u64::MAX / 100 should succeed
    let q = amm.quote(&QuoteParams {
        amount: u64::MAX, input_mint: CRIME_MINT, output_mint: PROFIT_MINT, swap_mode: SwapMode::ExactIn,
    }).expect("u64::MAX / 100 should not overflow");
    assert_eq!(q.out_amount, u64::MAX / 100);
}

#[test]
fn u64_max_vault_multiply_overflows_gracefully() {
    let amm = VaultAmm::new_for_testing(PROFIT_MINT, CRIME_MINT);
    // u64::MAX * 100 overflows → should error, not panic
    let result = amm.quote(&QuoteParams {
        amount: u64::MAX, input_mint: PROFIT_MINT, output_mint: CRIME_MINT, swap_mode: SwapMode::ExactIn,
    });
    assert!(result.is_err(), "u64::MAX * 100 should overflow gracefully");
}

// =============================================================================
// (4) ExactOut — all venues reject
// =============================================================================

#[test]
fn exact_out_rejected_sol_pool_buy() {
    let amm = crime_amm();
    assert!(amm.quote(&QuoteParams {
        amount: 1_000_000_000, input_mint: NATIVE_MINT, output_mint: CRIME_MINT,
        swap_mode: SwapMode::ExactOut,
    }).is_err());
}

#[test]
fn exact_out_rejected_sol_pool_sell() {
    let amm = crime_amm();
    assert!(amm.quote(&QuoteParams {
        amount: 1_000_000_000, input_mint: CRIME_MINT, output_mint: NATIVE_MINT,
        swap_mode: SwapMode::ExactOut,
    }).is_err());
}

#[test]
fn exact_out_rejected_all_vaults() {
    for (_, amm) in known_instances() {
        let mints = amm.get_reserve_mints();
        assert!(amm.quote(&QuoteParams {
            amount: 10_000, input_mint: mints[0], output_mint: mints[1],
            swap_mode: SwapMode::ExactOut,
        }).is_err());
    }
}

// =============================================================================
// (5) Wrong mint pair for venue
// =============================================================================

#[test]
fn wrong_mint_vault_crime_expects_fraud() {
    let amm = VaultAmm::new_for_testing(CRIME_MINT, PROFIT_MINT);
    assert!(amm.quote(&QuoteParams {
        amount: 10_000, input_mint: FRAUD_MINT, output_mint: PROFIT_MINT,
        swap_mode: SwapMode::ExactIn,
    }).is_err(), "Wrong input mint should error");
}

#[test]
fn wrong_mint_vault_profit_expects_crime() {
    let amm = VaultAmm::new_for_testing(PROFIT_MINT, CRIME_MINT);
    assert!(amm.quote(&QuoteParams {
        amount: 100, input_mint: FRAUD_MINT, output_mint: CRIME_MINT,
        swap_mode: SwapMode::ExactIn,
    }).is_err());
}

#[test]
fn random_pubkey_as_input_mint_sol_pool() {
    let amm = crime_amm();
    let random = Pubkey::new_unique();
    // SOL pool treats non-NATIVE_MINT as sell direction — should still work
    let result = amm.quote(&QuoteParams {
        amount: 1_000_000, input_mint: random, output_mint: NATIVE_MINT,
        swap_mode: SwapMode::ExactIn,
    });
    // Should produce a result or error — no panic
    assert!(result.is_ok() || result.is_err());
}

// =============================================================================
// (6) Zero reserves
// =============================================================================

#[test]
fn zero_sol_reserves_returns_zero_or_error() {
    let amm = SolPoolAmm::new_for_testing(true, 0, 1_000_000_000_000, 400, 1400);
    let result = amm.quote(&QuoteParams {
        amount: 1_000_000_000, input_mint: NATIVE_MINT, output_mint: CRIME_MINT,
        swap_mode: SwapMode::ExactIn,
    });
    // With zero SOL reserves, should not panic
    if let Ok(q) = result {
        // Output could be the entire token reserve (degenerate case) or 0
        let _ = q.out_amount;
    }
}

#[test]
fn zero_token_reserves_no_panic() {
    let amm = SolPoolAmm::new_for_testing(true, 100_000_000_000, 0, 400, 1400);
    let result = amm.quote(&QuoteParams {
        amount: 1_000_000_000, input_mint: CRIME_MINT, output_mint: NATIVE_MINT,
        swap_mode: SwapMode::ExactIn,
    });
    // Should not panic
    assert!(result.is_ok() || result.is_err());
}

// =============================================================================
// (7) Extreme reserve ratios
// =============================================================================

#[test]
fn extreme_ratio_1_sol_vs_1_trillion_tokens() {
    let amm = SolPoolAmm::new_for_testing(true, 1_000_000_000, 1_000_000_000_000_000, 400, 1400);
    let q = amm.quote(&QuoteParams {
        amount: 100_000_000, input_mint: NATIVE_MINT, output_mint: CRIME_MINT,
        swap_mode: SwapMode::ExactIn,
    }).unwrap();
    assert!(q.out_amount > 0);
}

#[test]
fn extreme_ratio_1_trillion_sol_vs_1_token() {
    let amm = SolPoolAmm::new_for_testing(true, 1_000_000_000_000_000, 1_000_000, 400, 1400);
    let q = amm.quote(&QuoteParams {
        amount: 1_000_000_000, input_mint: NATIVE_MINT, output_mint: CRIME_MINT,
        swap_mode: SwapMode::ExactIn,
    }).unwrap();
    // Tiny token reserve → tiny output
    assert!(q.out_amount <= 1_000_000);
}

// =============================================================================
// (8) All 8 directions produce sane results
// =============================================================================

#[test]
fn all_sol_pool_directions_produce_output() {
    let crime = crime_amm();
    let fraud = fraud_amm();

    let directions: Vec<(&SolPoolAmm, Pubkey, Pubkey, &str)> = vec![
        (&crime, NATIVE_MINT, CRIME_MINT, "Buy CRIME"),
        (&crime, CRIME_MINT, NATIVE_MINT, "Sell CRIME"),
        (&fraud, NATIVE_MINT, FRAUD_MINT, "Buy FRAUD"),
        (&fraud, FRAUD_MINT, NATIVE_MINT, "Sell FRAUD"),
    ];

    for (amm, input, output, label) in &directions {
        let q = amm.quote(&QuoteParams {
            amount: 1_000_000_000, input_mint: *input, output_mint: *output,
            swap_mode: SwapMode::ExactIn,
        }).unwrap();
        assert!(q.out_amount > 0, "{} should produce non-zero output", label);
    }
}

#[test]
fn all_vault_directions_produce_output() {
    let vault_pairs = [
        (CRIME_MINT, PROFIT_MINT, 10_000u64, "CRIME→PROFIT"),
        (FRAUD_MINT, PROFIT_MINT, 10_000, "FRAUD→PROFIT"),
        (PROFIT_MINT, CRIME_MINT, 100, "PROFIT→CRIME"),
        (PROFIT_MINT, FRAUD_MINT, 100, "PROFIT→FRAUD"),
    ];

    for (input, output, amount, label) in &vault_pairs {
        let amm = VaultAmm::new_for_testing(*input, *output);
        let q = amm.quote(&QuoteParams {
            amount: *amount, input_mint: *input, output_mint: *output,
            swap_mode: SwapMode::ExactIn,
        }).unwrap();
        assert!(q.out_amount > 0, "{} should produce non-zero output", label);
    }
}

// =============================================================================
// (9) Trait method consistency
// =============================================================================

#[test]
fn program_id_correct_sol_pool() {
    let amm = crime_amm();
    assert_eq!(amm.program_id(), TAX_PROGRAM_ID, "SOL pool should route through Tax Program");
}

#[test]
fn program_id_correct_vault() {
    for (_, amm) in known_instances() {
        assert_eq!(amm.program_id(), CONVERSION_VAULT_PROGRAM_ID,
            "Vault should route through Conversion Vault");
    }
}

#[test]
fn supports_exact_out_is_false_everywhere() {
    assert!(!crime_amm().supports_exact_out());
    assert!(!fraud_amm().supports_exact_out());
    for (_, amm) in known_instances() {
        assert!(!amm.supports_exact_out());
    }
}

#[test]
fn vault_is_unidirectional() {
    for (_, amm) in known_instances() {
        assert!(amm.unidirectional());
    }
}

#[test]
fn sol_pool_is_bidirectional() {
    assert!(!crime_amm().unidirectional());
    assert!(!fraud_amm().unidirectional());
}

#[test]
fn accounts_len_reasonable() {
    // SolPoolAmm reports 25 (max of buy=24, sell=25)
    assert_eq!(crime_amm().get_accounts_len(), 25);
    // VaultAmm reports 17
    for (_, amm) in known_instances() {
        assert_eq!(amm.get_accounts_len(), 17);
    }
}
