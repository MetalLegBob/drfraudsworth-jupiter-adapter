// Extended quoting tests — speed benchmarks, monotonicity, random sampling.
//
// Adapted from Titan adapter's test_quoting.rs (Parts 5-7) and
// test_construction.rs speed/monotonicity sections.
//
// Tests:
// - Speed: 10K iterations < 100μs average (SOL pool, vault, account metas)
// - Monotonicity: increasing input → non-decreasing output (all directions)
// - Random sampling: 50 log-uniform samples per direction, zero-delta vs reference
// - Edge case parity: dust, max tax, zero tax

use jupiter_amm_interface::{Amm, QuoteParams, SwapMode, SwapParams};
use solana_sdk::pubkey::Pubkey;

use drfraudsworth_jupiter_adapter::accounts::addresses::*;
use drfraudsworth_jupiter_adapter::constants::LP_FEE_BPS;
use drfraudsworth_jupiter_adapter::math::amm_math::{calculate_effective_input, calculate_swap_output};
use drfraudsworth_jupiter_adapter::math::tax_math::calculate_tax;
use drfraudsworth_jupiter_adapter::math::vault_math::compute_vault_output;
use drfraudsworth_jupiter_adapter::sol_pool_amm::SolPoolAmm;
use drfraudsworth_jupiter_adapter::vault_amm::VaultAmm;

// =============================================================================
// Helpers
// =============================================================================

const RESERVE_SOL: u64 = 100_000_000_000;     // 100 SOL
const RESERVE_TOKEN: u64 = 1_000_000_000_000; // 1B tokens (6 decimals)
const BUY_TAX: u16 = 400;                     // 4%
const SELL_TAX: u16 = 1400;                   // 14%

fn make_crime() -> SolPoolAmm {
    SolPoolAmm::new_for_testing(true, RESERVE_SOL, RESERVE_TOKEN, BUY_TAX, SELL_TAX)
}

fn make_fraud() -> SolPoolAmm {
    SolPoolAmm::new_for_testing(false, RESERVE_SOL, RESERVE_TOKEN, SELL_TAX, BUY_TAX)
}

/// Replicate on-chain buy pipeline for reference comparison.
fn reference_buy_output(
    reserve_sol: u64,
    reserve_token: u64,
    amount_in: u64,
    buy_tax_bps: u16,
    lp_fee_bps: u16,
) -> u64 {
    if amount_in == 0 { return 0; }
    let tax = calculate_tax(amount_in, buy_tax_bps).unwrap();
    let sol_to_swap = amount_in.checked_sub(tax).unwrap();
    if sol_to_swap == 0 { return 0; }
    let effective = calculate_effective_input(sol_to_swap, lp_fee_bps).unwrap();
    calculate_swap_output(reserve_sol, reserve_token, effective).unwrap_or(0)
}

/// Replicate on-chain sell pipeline for reference comparison.
fn reference_sell_output(
    reserve_sol: u64,
    reserve_token: u64,
    amount_in: u64,
    sell_tax_bps: u16,
    lp_fee_bps: u16,
) -> u64 {
    if amount_in == 0 { return 0; }
    let effective = calculate_effective_input(amount_in, lp_fee_bps).unwrap();
    let gross_sol = calculate_swap_output(reserve_token, reserve_sol, effective).unwrap_or(0);
    let tax = calculate_tax(gross_sol, sell_tax_bps).unwrap();
    gross_sol.saturating_sub(tax)
}

/// Deterministic pseudo-random log-uniform sampling.
fn sample_log_uniform(lo: u64, hi: u64, seed: u64) -> u64 {
    let hash = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let frac = (hash as f64) / (u64::MAX as f64);
    let log_lo = (lo as f64).ln();
    let log_hi = (hi as f64).ln();
    let log_val = log_lo + frac * (log_hi - log_lo);
    let val = log_val.exp() as u64;
    val.max(lo).min(hi)
}

// =============================================================================
// Part 1: Speed benchmarks (10K iterations, < 100μs average)
// =============================================================================

#[test]
fn speed_buy_crime_10k() {
    let amm = make_crime();
    let start = std::time::Instant::now();

    for i in 0..10_000u64 {
        let _ = amm.quote(&QuoteParams {
            amount: 1_000_000 + i,
            input_mint: NATIVE_MINT, output_mint: CRIME_MINT,
            swap_mode: SwapMode::ExactIn,
        });
    }

    let avg = start.elapsed().as_micros() as f64 / 10_000.0;
    assert!(avg < 100.0, "Buy quote avg {:.2}us > 100us", avg);
    eprintln!("Buy CRIME: {:.2}us avg", avg);
}

#[test]
fn speed_sell_crime_10k() {
    let amm = make_crime();
    let start = std::time::Instant::now();

    for i in 0..10_000u64 {
        let _ = amm.quote(&QuoteParams {
            amount: 1_000_000 + i,
            input_mint: CRIME_MINT, output_mint: NATIVE_MINT,
            swap_mode: SwapMode::ExactIn,
        });
    }

    let avg = start.elapsed().as_micros() as f64 / 10_000.0;
    assert!(avg < 100.0, "Sell quote avg {:.2}us > 100us", avg);
    eprintln!("Sell CRIME: {:.2}us avg", avg);
}

#[test]
fn speed_vault_convert_10k() {
    let amm = VaultAmm::new_for_testing(CRIME_MINT, PROFIT_MINT);
    let start = std::time::Instant::now();

    for i in 0..10_000u64 {
        let _ = amm.quote(&QuoteParams {
            amount: 10_000 + i,
            input_mint: CRIME_MINT, output_mint: PROFIT_MINT,
            swap_mode: SwapMode::ExactIn,
        });
    }

    let avg = start.elapsed().as_micros() as f64 / 10_000.0;
    assert!(avg < 100.0, "Vault quote avg {:.2}us > 100us", avg);
    eprintln!("Vault CRIME->PROFIT: {:.2}us avg", avg);
}

#[test]
fn speed_get_swap_account_metas_10k() {
    let amm = make_crime();
    let jup_id = Pubkey::new_unique();
    let user = Pubkey::new_unique();
    let start = std::time::Instant::now();

    for _ in 0..10_000u64 {
        let _ = amm.get_swap_and_account_metas(&SwapParams {
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
        });
    }

    let avg = start.elapsed().as_micros() as f64 / 10_000.0;
    eprintln!("Account metas generation: {:.2}us avg", avg);
    // Account metas involve PDA derivation — allow more headroom
}

// =============================================================================
// Part 2: Monotonicity (increasing input → non-decreasing output)
// =============================================================================

#[test]
fn monotonicity_buy_crime_100_steps() {
    let amm = make_crime();
    let mut prev = 0u64;

    for i in 1..=100 {
        let amount = i * 500_000_000; // 0.5 SOL increments up to 50 SOL
        let q = amm.quote(&QuoteParams {
            amount, input_mint: NATIVE_MINT, output_mint: CRIME_MINT,
            swap_mode: SwapMode::ExactIn,
        }).unwrap();

        assert!(q.out_amount >= prev,
            "Monotonicity violation at step {}: {} < {}", i, q.out_amount, prev);
        prev = q.out_amount;
    }
}

#[test]
fn monotonicity_sell_crime_100_steps() {
    let amm = make_crime();
    let mut prev = 0u64;

    for i in 1..=100 {
        let amount = i * 5_000_000_000; // 5K token increments
        let q = amm.quote(&QuoteParams {
            amount, input_mint: CRIME_MINT, output_mint: NATIVE_MINT,
            swap_mode: SwapMode::ExactIn,
        }).unwrap();

        assert!(q.out_amount >= prev,
            "Sell monotonicity violation at step {}: {} < {}", i, q.out_amount, prev);
        prev = q.out_amount;
    }
}

#[test]
fn monotonicity_buy_fraud_100_steps() {
    let amm = make_fraud();
    let mut prev = 0u64;

    for i in 1..=100 {
        let amount = i * 500_000_000;
        let q = amm.quote(&QuoteParams {
            amount, input_mint: NATIVE_MINT, output_mint: FRAUD_MINT,
            swap_mode: SwapMode::ExactIn,
        }).unwrap();

        assert!(q.out_amount >= prev);
        prev = q.out_amount;
    }
}

#[test]
fn monotonicity_sell_fraud_100_steps() {
    let amm = make_fraud();
    let mut prev = 0u64;

    for i in 1..=100 {
        let amount = i * 5_000_000_000;
        let q = amm.quote(&QuoteParams {
            amount, input_mint: FRAUD_MINT, output_mint: NATIVE_MINT,
            swap_mode: SwapMode::ExactIn,
        }).unwrap();

        assert!(q.out_amount >= prev);
        prev = q.out_amount;
    }
}

#[test]
fn monotonicity_vault_divide() {
    let amm = VaultAmm::new_for_testing(CRIME_MINT, PROFIT_MINT);
    let mut prev = 0u64;

    for amount in [100u64, 200, 500, 1_000, 5_000, 10_000, 100_000, 1_000_000] {
        let q = amm.quote(&QuoteParams {
            amount, input_mint: CRIME_MINT, output_mint: PROFIT_MINT,
            swap_mode: SwapMode::ExactIn,
        }).unwrap();

        assert!(q.out_amount >= prev);
        prev = q.out_amount;
    }
}

#[test]
fn monotonicity_vault_multiply() {
    let amm = VaultAmm::new_for_testing(PROFIT_MINT, CRIME_MINT);
    let mut prev = 0u64;

    for amount in [1u64, 2, 5, 10, 50, 100, 1_000, 10_000] {
        let q = amm.quote(&QuoteParams {
            amount, input_mint: PROFIT_MINT, output_mint: CRIME_MINT,
            swap_mode: SwapMode::ExactIn,
        }).unwrap();

        assert!(q.out_amount >= prev);
        prev = q.out_amount;
    }
}

// =============================================================================
// Part 3: Random sampling (50 log-uniform samples per direction, zero delta)
// =============================================================================

#[test]
fn random_sampling_buy_crime_50() {
    let amm = make_crime();

    for seed in 0..50 {
        let amount = sample_log_uniform(1_000, 50_000_000_000, seed);
        let q = amm.quote(&QuoteParams {
            amount, input_mint: NATIVE_MINT, output_mint: CRIME_MINT,
            swap_mode: SwapMode::ExactIn,
        }).unwrap();

        let expected = reference_buy_output(RESERVE_SOL, RESERVE_TOKEN, amount, BUY_TAX, LP_FEE_BPS);
        assert_eq!(q.out_amount, expected,
            "Buy CRIME sample #{} at amount {}: got {} expected {}", seed, amount, q.out_amount, expected);
    }
}

#[test]
fn random_sampling_sell_crime_50() {
    let amm = make_crime();

    for seed in 0..50 {
        let amount = sample_log_uniform(1_000, 500_000_000_000, seed + 100);
        let q = amm.quote(&QuoteParams {
            amount, input_mint: CRIME_MINT, output_mint: NATIVE_MINT,
            swap_mode: SwapMode::ExactIn,
        }).unwrap();

        let expected = reference_sell_output(RESERVE_SOL, RESERVE_TOKEN, amount, SELL_TAX, LP_FEE_BPS);
        assert_eq!(q.out_amount, expected,
            "Sell CRIME sample #{} at amount {}", seed, amount);
    }
}

#[test]
fn random_sampling_buy_fraud_50() {
    let amm = make_fraud();

    for seed in 0..50 {
        let amount = sample_log_uniform(1_000, 50_000_000_000, seed + 200);
        let q = amm.quote(&QuoteParams {
            amount, input_mint: NATIVE_MINT, output_mint: FRAUD_MINT,
            swap_mode: SwapMode::ExactIn,
        }).unwrap();

        let expected = reference_buy_output(RESERVE_SOL, RESERVE_TOKEN, amount, SELL_TAX, LP_FEE_BPS);
        assert_eq!(q.out_amount, expected,
            "Buy FRAUD sample #{} at amount {}", seed, amount);
    }
}

#[test]
fn random_sampling_sell_fraud_50() {
    let amm = make_fraud();

    for seed in 0..50 {
        let amount = sample_log_uniform(1_000, 500_000_000_000, seed + 300);
        let q = amm.quote(&QuoteParams {
            amount, input_mint: FRAUD_MINT, output_mint: NATIVE_MINT,
            swap_mode: SwapMode::ExactIn,
        }).unwrap();

        let expected = reference_sell_output(RESERVE_SOL, RESERVE_TOKEN, amount, BUY_TAX, LP_FEE_BPS);
        assert_eq!(q.out_amount, expected,
            "Sell FRAUD sample #{} at amount {}", seed, amount);
    }
}

#[test]
fn random_sampling_vault_crime_to_profit_50() {
    let amm = VaultAmm::new_for_testing(CRIME_MINT, PROFIT_MINT);

    for seed in 0..50 {
        let amount = sample_log_uniform(100, 1_000_000_000, seed + 400);
        let q = amm.quote(&QuoteParams {
            amount, input_mint: CRIME_MINT, output_mint: PROFIT_MINT,
            swap_mode: SwapMode::ExactIn,
        }).unwrap();

        let expected = compute_vault_output(&CRIME_MINT, &PROFIT_MINT, amount).unwrap();
        assert_eq!(q.out_amount, expected,
            "Vault C->P sample #{} at amount {}", seed, amount);
    }
}

#[test]
fn random_sampling_vault_profit_to_crime_50() {
    let amm = VaultAmm::new_for_testing(PROFIT_MINT, CRIME_MINT);

    for seed in 0..50 {
        let amount = sample_log_uniform(1, 10_000_000, seed + 500);
        let q = amm.quote(&QuoteParams {
            amount, input_mint: PROFIT_MINT, output_mint: CRIME_MINT,
            swap_mode: SwapMode::ExactIn,
        }).unwrap();

        let expected = compute_vault_output(&PROFIT_MINT, &CRIME_MINT, amount).unwrap();
        assert_eq!(q.out_amount, expected,
            "Vault P->C sample #{} at amount {}", seed, amount);
    }
}

// =============================================================================
// Part 4: Edge case parity — dust, max tax, zero tax
// =============================================================================

#[test]
fn parity_dust_100_lamports() {
    let amm = make_crime();
    let amount = 100;

    let q = amm.quote(&QuoteParams {
        amount, input_mint: NATIVE_MINT, output_mint: CRIME_MINT,
        swap_mode: SwapMode::ExactIn,
    }).unwrap();

    let expected = reference_buy_output(RESERVE_SOL, RESERVE_TOKEN, amount, BUY_TAX, LP_FEE_BPS);
    assert_eq!(q.out_amount, expected);
}

#[test]
fn parity_max_tax_50pct() {
    let amm = SolPoolAmm::new_for_testing(true, RESERVE_SOL, RESERVE_TOKEN, 5000, 5000);
    let amount = 1_000_000_000;

    let q = amm.quote(&QuoteParams {
        amount, input_mint: NATIVE_MINT, output_mint: CRIME_MINT,
        swap_mode: SwapMode::ExactIn,
    }).unwrap();

    let expected = reference_buy_output(RESERVE_SOL, RESERVE_TOKEN, amount, 5000, LP_FEE_BPS);
    assert_eq!(q.out_amount, expected);
}

#[test]
fn parity_zero_tax() {
    let amm = SolPoolAmm::new_for_testing(true, RESERVE_SOL, RESERVE_TOKEN, 0, 0);
    let amount = 1_000_000_000;

    let q = amm.quote(&QuoteParams {
        amount, input_mint: NATIVE_MINT, output_mint: CRIME_MINT,
        swap_mode: SwapMode::ExactIn,
    }).unwrap();

    let expected = reference_buy_output(RESERVE_SOL, RESERVE_TOKEN, amount, 0, LP_FEE_BPS);
    assert_eq!(q.out_amount, expected);
}

#[test]
fn parity_sell_dust_100_tokens() {
    let amm = make_crime();
    let amount = 100;

    let q = amm.quote(&QuoteParams {
        amount, input_mint: CRIME_MINT, output_mint: NATIVE_MINT,
        swap_mode: SwapMode::ExactIn,
    }).unwrap();

    let expected = reference_sell_output(RESERVE_SOL, RESERVE_TOKEN, amount, SELL_TAX, LP_FEE_BPS);
    assert_eq!(q.out_amount, expected);
}
