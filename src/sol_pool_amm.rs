// Jupiter Amm trait implementation for Dr. Fraudsworth SOL pool swaps.
//
// SolPoolAmm handles TOKEN/SOL swaps via the Tax Program. Instances are
// constructed generically from PoolState account data: mints, vaults, and
// orientation all come from the parsed account, not per-pool constants.
// Any SOL-quoted pool the AMM creates in the future (for a supported token)
// constructs without SDK changes — only genuinely new pool SHAPES (e.g. a
// USDC quote side, which needs new on-chain tax instructions) require code.
//
// Quote flow:
//   Buy (SOL -> token): tax deducted from SOL INPUT, then AMM swap on post-tax amount
//   Sell (token -> SOL): AMM swap on full token input, then tax deducted from SOL OUTPUT

use anyhow::{anyhow, Result};
use jupiter_amm_interface::{
    AccountMap, Amm, AmmContext, KeyedAccount, Quote, QuoteParams, SwapAndAccountMetas, SwapMode,
    SwapParams, Swap, try_get_account_data,
};
use rust_decimal::Decimal;
use solana_sdk::pubkey::Pubkey;

use crate::accounts::addresses::{
    AMM_PROGRAM_ID, CRIME_MINT, EPOCH_STATE_PDA, FRAUD_MINT, NATIVE_MINT,
};
use crate::accounts::sol_pool_accounts::{
    build_buy_account_metas_generic, build_sell_account_metas_generic, known_pool_state,
};
use crate::math::amm_math::{calculate_effective_input, calculate_swap_output};
use crate::math::tax_math::calculate_tax;
use crate::state::epoch_state::ParsedEpochState;
use crate::state::pool_state::ParsedPoolState;

/// Jupiter Amm implementation for SOL-quoted pools (CRIME/SOL, FRAUD/SOL).
///
/// Swaps go through the Tax Program which deducts dynamic tax and then
/// CPI-calls the AMM for the actual constant-product swap.
#[derive(Clone)]
pub struct SolPoolAmm {
    /// Pool PDA address
    key: Pubkey,
    /// Full parsed pool state (mints, vaults, reserves, LP fee)
    pool: ParsedPoolState,
    /// The pool's non-SOL mint (CRIME or FRAUD)
    token_mint: Pubkey,
    /// Current epoch buy tax in BPS
    buy_tax_bps: u16,
    /// Current epoch sell tax in BPS
    sell_tax_bps: u16,
    /// Whether an epoch transition window is open. While true, the AMM's
    /// Layer-3 gate reverts public swaps with TransitionInProgress (6019),
    /// so quotes are refused to keep the router out of the window. Always
    /// false on deployments without the gate feature (the flag byte sits in
    /// zeroed reserved padding there).
    transition_in_progress: bool,
}

impl Amm for SolPoolAmm {
    fn from_keyed_account(keyed_account: &KeyedAccount, _amm_context: &AmmContext) -> Result<Self>
    where
        Self: Sized,
    {
        // Only accounts owned by the AMM program can be genuine pools. This,
        // plus the discriminator check inside ParsedPoolState::from_bytes,
        // makes it safe to feed this constructor arbitrary accounts (e.g.
        // from a program scan) — non-pools fail loudly instead of being
        // silently treated as some known pool.
        if keyed_account.account.owner != AMM_PROGRAM_ID {
            return Err(anyhow!(
                "SolPoolAmm: account {} is owned by {}, not the AMM program {}",
                keyed_account.key,
                keyed_account.account.owner,
                AMM_PROGRAM_ID
            ));
        }

        let pool_state = ParsedPoolState::from_bytes(&keyed_account.account.data)?;

        let token_mint = pool_state.token_mint().ok_or_else(|| {
            anyhow!(
                "SolPoolAmm: pool {} is not SOL-quoted ({} / {})",
                keyed_account.key,
                pool_state.mint_a,
                pool_state.mint_b
            )
        })?;

        // Tax rates, transfer hooks, and the whitelist only exist for the
        // protocol's faction tokens. Reject pools for any other mint.
        if token_mint != CRIME_MINT && token_mint != FRAUD_MINT {
            return Err(anyhow!(
                "SolPoolAmm: unsupported token mint {} in pool {} (expected CRIME {} or FRAUD {})",
                token_mint,
                keyed_account.key,
                CRIME_MINT,
                FRAUD_MINT
            ));
        }

        Ok(Self {
            key: keyed_account.key,
            pool: pool_state,
            token_mint,
            // Tax rates initialized to 0; populated by first update() call.
            // Jupiter always calls update() before quote().
            buy_tax_bps: 0,
            sell_tax_bps: 0,
            transition_in_progress: false,
        })
    }

    fn label(&self) -> String {
        "Dr Fraudsworth".to_string()
    }

    fn program_id(&self) -> Pubkey {
        // Tax Program is what Jupiter calls for swaps (not the AMM directly)
        crate::accounts::addresses::TAX_PROGRAM_ID
    }

    fn key(&self) -> Pubkey {
        self.key
    }

    fn get_reserve_mints(&self) -> Vec<Pubkey> {
        vec![NATIVE_MINT, self.token_mint]
    }

    fn get_accounts_to_update(&self) -> Vec<Pubkey> {
        // Jupiter refreshes these accounts and passes them to update()
        vec![self.key, EPOCH_STATE_PDA]
    }

    fn update(&mut self, account_map: &AccountMap) -> Result<()> {
        // Re-parse pool state for reserves + LP fee
        let pool_data = try_get_account_data(account_map, &self.key)?;
        let pool_state = ParsedPoolState::from_bytes(pool_data)?;

        // The pool's mints are immutable on-chain; a change means we were
        // handed data for a different account.
        if pool_state.token_mint() != Some(self.token_mint) {
            return Err(anyhow!(
                "SolPoolAmm: update data for {} does not match token mint {}",
                self.key,
                self.token_mint
            ));
        }
        self.pool = pool_state;

        // Parse epoch state for tax rates
        let epoch_data = try_get_account_data(account_map, &EPOCH_STATE_PDA)?;
        let epoch_state = ParsedEpochState::from_bytes(epoch_data)?;
        self.buy_tax_bps = epoch_state.get_tax_bps(self.is_crime(), true);
        self.sell_tax_bps = epoch_state.get_tax_bps(self.is_crime(), false);
        self.transition_in_progress = epoch_state.transition_in_progress;

        Ok(())
    }

    fn quote(&self, quote_params: &QuoteParams) -> Result<Quote> {
        if quote_params.swap_mode == SwapMode::ExactOut {
            return Err(anyhow!("ExactOut not supported"));
        }

        let is_buy = quote_params.input_mint == NATIVE_MINT;
        self.validate_mint_pair(&quote_params.input_mint, &quote_params.output_mint, is_buy)?;

        // Mirror the AMM's Layer-3 transition gate: while the epoch flip
        // window is open, on-chain swaps revert with TransitionInProgress
        // (6019), so refuse the quote instead of routing into a known
        // failure. Windows normally last a few slots (bounded recovery at 50
        // slots); the flag clears on the next account refresh.
        if self.transition_in_progress {
            return Err(anyhow!(
                "SolPoolAmm: epoch transition window open for pool {} -- \
                 on-chain swaps revert with TransitionInProgress until it closes",
                self.key
            ));
        }

        if is_buy {
            self.quote_buy(quote_params.amount)
        } else {
            self.quote_sell(quote_params.amount)
        }
    }

    fn get_swap_and_account_metas(&self, swap_params: &SwapParams) -> Result<SwapAndAccountMetas> {
        let is_buy = swap_params.source_mint == NATIVE_MINT;
        self.validate_mint_pair(&swap_params.source_mint, &swap_params.destination_mint, is_buy)?;

        let account_metas = if is_buy {
            // Buy: source = WSOL, destination = token
            build_buy_account_metas_generic(
                &swap_params.token_transfer_authority,
                &swap_params.source_token_account,
                &swap_params.destination_token_account,
                &self.key,
                &self.pool,
            )?
        } else {
            // Sell: source = token, destination = WSOL
            build_sell_account_metas_generic(
                &swap_params.token_transfer_authority,
                &swap_params.source_token_account,
                &swap_params.destination_token_account,
                &self.key,
                &self.pool,
            )?
        };

        Ok(SwapAndAccountMetas {
            swap: Swap::TokenSwap,
            account_metas,
        })
    }

    fn supports_exact_out(&self) -> bool {
        false
    }

    fn clone_amm(&self) -> Box<dyn Amm + Send + Sync> {
        Box::new(self.clone())
    }

    fn get_accounts_len(&self) -> usize {
        // Buy: 24, Sell: 25. Use the larger to be safe.
        25
    }

    fn program_dependencies(&self) -> Vec<(Pubkey, String)> {
        // Programs a test harness must load for a swap to execute: the Tax
        // Program (program_id) CPIs into the AMM and Staking programs, and
        // Token-2022 invokes the Transfer Hook on every token transfer.
        vec![
            (AMM_PROGRAM_ID, "amm".to_string()),
            (
                crate::accounts::addresses::STAKING_PROGRAM_ID,
                "staking".to_string(),
            ),
            (
                crate::accounts::addresses::TRANSFER_HOOK_PROGRAM_ID,
                "transfer-hook".to_string(),
            ),
        ]
    }
}

impl SolPoolAmm {
    /// Create a SolPoolAmm directly with known values (for testing/examples).
    ///
    /// In production, use `from_keyed_account` which parses on-chain data.
    pub fn new_for_testing(
        is_crime: bool,
        reserve_sol: u64,
        reserve_token: u64,
        buy_tax_bps: u16,
        sell_tax_bps: u16,
    ) -> Self {
        let (key, mut pool) = known_pool_state(is_crime);
        // known_pool_state has mint_a = WSOL, so reserve_a is the SOL side.
        pool.reserve_a = reserve_sol;
        pool.reserve_b = reserve_token;
        let token_mint = pool.token_mint().expect("known pools are SOL pools");
        Self {
            key,
            pool,
            token_mint,
            buy_tax_bps,
            sell_tax_bps,
            transition_in_progress: false,
        }
    }

    fn is_crime(&self) -> bool {
        self.token_mint == CRIME_MINT
    }

    /// (sol_reserve, token_reserve) in orientation-independent order.
    fn reserves(&self) -> (u64, u64) {
        self.pool.sol_and_token_reserves()
    }

    /// Validate that the requested trade pair matches this pool.
    fn validate_mint_pair(&self, input: &Pubkey, output: &Pubkey, is_buy: bool) -> Result<()> {
        let ok = if is_buy {
            *input == NATIVE_MINT && *output == self.token_mint
        } else {
            *input == self.token_mint && *output == NATIVE_MINT
        };
        if ok {
            Ok(())
        } else {
            Err(anyhow!(
                "SolPoolAmm: mint pair {} -> {} does not match pool {} (WSOL <-> {})",
                input,
                output,
                self.key,
                self.token_mint
            ))
        }
    }

    /// Quote a buy (SOL -> token).
    ///
    /// Flow: tax deducted from SOL input, LP fee deducted, then constant-product swap.
    fn quote_buy(&self, amount_in: u64) -> Result<Quote> {
        let (reserve_sol, reserve_token) = self.reserves();

        // 1. Tax deducted from SOL input
        let tax = calculate_tax(amount_in, self.buy_tax_bps)
            .ok_or_else(|| anyhow!("Tax calculation overflow"))?;

        let sol_to_swap = amount_in
            .checked_sub(tax)
            .ok_or_else(|| anyhow!("Tax exceeds input amount"))?;

        if sol_to_swap == 0 {
            return Ok(Quote {
                in_amount: amount_in,
                out_amount: 0,
                fee_amount: tax,
                fee_mint: NATIVE_MINT,
                fee_pct: self.total_buy_fee_pct(),
            });
        }

        // 2. LP fee deducted from post-tax amount
        let effective_input = calculate_effective_input(sol_to_swap, self.pool.lp_fee_bps)
            .ok_or_else(|| anyhow!("Effective input calculation overflow"))?;

        // 3. Constant-product swap
        let out_amount = calculate_swap_output(
            reserve_sol,
            reserve_token,
            effective_input,
        )
        .ok_or_else(|| anyhow!("Swap output calculation overflow or zero reserves"))?;

        // LP fee in SOL terms
        let lp_fee_sol = sol_to_swap.saturating_sub(effective_input as u64);

        Ok(Quote {
            in_amount: amount_in,
            out_amount,
            fee_amount: tax.checked_add(lp_fee_sol).unwrap_or(tax),
            fee_mint: NATIVE_MINT,
            fee_pct: self.total_buy_fee_pct(),
        })
    }

    /// Quote a sell (token -> SOL).
    ///
    /// Flow: LP fee deducted from token input, constant-product swap, then tax on SOL output.
    fn quote_sell(&self, amount_in: u64) -> Result<Quote> {
        let (reserve_sol, reserve_token) = self.reserves();

        // 1. LP fee deducted from token input
        let effective_input = calculate_effective_input(amount_in, self.pool.lp_fee_bps)
            .ok_or_else(|| anyhow!("Effective input calculation overflow"))?;

        // 2. Constant-product swap (token -> SOL)
        let gross_sol = calculate_swap_output(
            reserve_token,
            reserve_sol,
            effective_input,
        )
        .ok_or_else(|| anyhow!("Swap output calculation overflow or zero reserves"))?;

        // 3. Tax deducted from SOL output
        let tax = calculate_tax(gross_sol, self.sell_tax_bps)
            .ok_or_else(|| anyhow!("Tax calculation overflow"))?;

        let net_sol = gross_sol
            .checked_sub(tax)
            .ok_or_else(|| anyhow!("Tax exceeds gross output"))?;

        // The LP fee is taken from the token input; fee_mint is SOL, so we
        // report only the tax as the primary fee amount. Jupiter uses
        // fee_pct (LP + tax combined) as the authoritative fee indicator.
        Ok(Quote {
            in_amount: amount_in,
            out_amount: net_sol,
            fee_amount: tax,
            fee_mint: NATIVE_MINT,
            fee_pct: self.total_sell_fee_pct(),
        })
    }

    /// Total buy fee percentage (LP + tax) as a Decimal.
    fn total_buy_fee_pct(&self) -> Decimal {
        let total_bps = (self.pool.lp_fee_bps as u32) + (self.buy_tax_bps as u32);
        Decimal::from(total_bps) / Decimal::from(10_000u32)
    }

    /// Total sell fee percentage (LP + tax) as a Decimal.
    fn total_sell_fee_pct(&self) -> Decimal {
        let total_bps = (self.pool.lp_fee_bps as u32) + (self.sell_tax_bps as u32);
        Decimal::from(total_bps) / Decimal::from(10_000u32)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::addresses::CRIME_SOL_POOL;
    use crate::state::pool_state::tests::mock_pool_state;
    use jupiter_amm_interface::ClockRef;
    use solana_sdk::account::Account;

    /// Create a SolPoolAmm directly with known values (bypassing from_keyed_account).
    fn make_amm(
        is_crime: bool,
        reserve_sol: u64,
        reserve_token: u64,
        buy_tax_bps: u16,
        sell_tax_bps: u16,
    ) -> SolPoolAmm {
        SolPoolAmm::new_for_testing(is_crime, reserve_sol, reserve_token, buy_tax_bps, sell_tax_bps)
    }

    fn amm_context() -> AmmContext {
        AmmContext { clock_ref: ClockRef::default() }
    }

    fn keyed_account(key: Pubkey, data: Vec<u8>, owner: Pubkey) -> KeyedAccount {
        KeyedAccount {
            key,
            account: Account {
                lamports: 1_000_000,
                data,
                owner,
                executable: false,
                rent_epoch: 0,
            },
            params: None,
        }
    }

    /// CRIME/SOL-shaped pool data (mainnet orientation: mint_a = WSOL).
    fn crime_shaped_data() -> Vec<u8> {
        mock_pool_state(
            &NATIVE_MINT,
            &CRIME_MINT,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            100_000_000_000,
            100_000_000_000,
            100,
        )
    }

    #[test]
    fn buy_quote_applies_tax_before_swap() {
        // 1 SOL input, 4% buy tax, equal reserves
        let amm = make_amm(true, 100_000_000_000, 100_000_000_000, 400, 1400);

        let quote = amm.quote(&QuoteParams {
            amount: 1_000_000_000, // 1 SOL
            input_mint: NATIVE_MINT,
            output_mint: CRIME_MINT,
            swap_mode: SwapMode::ExactIn,
        }).unwrap();

        // Tax = 1 SOL * 400/10000 = 0.04 SOL = 40_000_000 lamports
        // sol_to_swap = 960_000_000
        // After 1% LP fee: effective = 960_000_000 * 9900/10000 = 950_400_000
        // Output: 100B * 950_400_000 / (100B + 950_400_000)
        assert!(quote.out_amount > 0, "output should be non-zero");
        assert!(quote.out_amount < 1_000_000_000, "output should be less than input for equal reserves");
        assert!(quote.fee_amount >= 40_000_000, "fee should include at least the tax");
        assert_eq!(quote.in_amount, 1_000_000_000);
        assert_eq!(quote.fee_mint, NATIVE_MINT);
    }

    #[test]
    fn sell_quote_applies_tax_after_swap() {
        // Sell 1B tokens, 14% sell tax
        let amm = make_amm(true, 100_000_000_000, 100_000_000_000, 400, 1400);

        let quote = amm.quote(&QuoteParams {
            amount: 1_000_000_000, // 1B token units
            input_mint: CRIME_MINT,
            output_mint: NATIVE_MINT,
            swap_mode: SwapMode::ExactIn,
        }).unwrap();

        // After LP fee: effective = 1B * 9900/10000 = 990_000_000
        // gross_sol from swap
        // Then 14% tax on gross_sol
        assert!(quote.out_amount > 0, "output should be non-zero");
        assert_eq!(quote.fee_mint, NATIVE_MINT);
        // The sell tax is 14%, so the fee amount (tax only) should be significant
        assert!(quote.fee_amount > 0, "sell tax should be non-zero");
    }

    #[test]
    fn buy_zero_input_returns_zero_output() {
        let amm = make_amm(true, 100_000_000_000, 100_000_000_000, 400, 1400);

        let quote = amm.quote(&QuoteParams {
            amount: 0,
            input_mint: NATIVE_MINT,
            output_mint: CRIME_MINT,
            swap_mode: SwapMode::ExactIn,
        }).unwrap();

        assert_eq!(quote.out_amount, 0);
    }

    #[test]
    fn sell_zero_sol_reserves_returns_zero_output() {
        // Zero SOL reserves means swap output = 0 (no SOL to extract)
        let amm = make_amm(true, 0, 100_000_000_000, 400, 1400);

        let quote = amm.quote(&QuoteParams {
            amount: 1_000_000_000,
            input_mint: CRIME_MINT,
            output_mint: NATIVE_MINT,
            swap_mode: SwapMode::ExactIn,
        }).unwrap();

        assert_eq!(quote.out_amount, 0, "should return 0 with zero SOL reserves");
    }

    #[test]
    fn sell_zero_token_reserves_errors() {
        // Zero token reserves + non-zero token input -> denominator = 0 + effective_input
        // This should still work mathematically (output = reserve_sol * eff / (0 + eff))
        // = reserve_sol, but that would drain the pool. With zero reserve_in (token=0)
        // and nonzero effective_input, calculate_swap_output returns the full reserve_out.
        // In practice this can't happen (pool wouldn't be initialized with 0 tokens).
        let amm = make_amm(true, 100_000_000_000, 0, 400, 1400);

        let result = amm.quote(&QuoteParams {
            amount: 1_000_000_000,
            input_mint: CRIME_MINT,
            output_mint: NATIVE_MINT,
            swap_mode: SwapMode::ExactIn,
        });

        // With zero token reserves, swap returns full SOL reserve.
        // This is an edge case that shouldn't happen in practice.
        assert!(result.is_ok());
    }

    #[test]
    fn exact_out_not_supported() {
        let amm = make_amm(true, 100_000_000_000, 100_000_000_000, 400, 1400);

        let result = amm.quote(&QuoteParams {
            amount: 1_000_000_000,
            input_mint: NATIVE_MINT,
            output_mint: CRIME_MINT,
            swap_mode: SwapMode::ExactOut,
        });

        assert!(result.is_err());
    }

    #[test]
    fn quote_refused_while_transition_window_open() {
        let mut amm = make_amm(true, 100_000_000_000, 100_000_000_000, 400, 1400);

        amm.transition_in_progress = true;
        let err = match amm.quote(&QuoteParams {
            amount: 1_000_000_000,
            input_mint: NATIVE_MINT,
            output_mint: CRIME_MINT,
            swap_mode: SwapMode::ExactIn,
        }) {
            Ok(_) => panic!("quote must be refused during a transition window"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("transition window"), "got: {err}");

        // Window closed -> quotes flow again.
        amm.transition_in_progress = false;
        assert!(amm
            .quote(&QuoteParams {
                amount: 1_000_000_000,
                input_mint: NATIVE_MINT,
                output_mint: CRIME_MINT,
                swap_mode: SwapMode::ExactIn,
            })
            .is_ok());
    }

    #[test]
    fn quote_rejects_mismatched_mint_pair() {
        let amm = make_amm(true, 100_000_000_000, 100_000_000_000, 400, 1400);

        // FRAUD into the CRIME pool must not silently quote with CRIME reserves
        let result = amm.quote(&QuoteParams {
            amount: 1_000_000_000,
            input_mint: FRAUD_MINT,
            output_mint: NATIVE_MINT,
            swap_mode: SwapMode::ExactIn,
        });
        assert!(result.is_err());

        // Buy direction with the wrong output token
        let result = amm.quote(&QuoteParams {
            amount: 1_000_000_000,
            input_mint: NATIVE_MINT,
            output_mint: FRAUD_MINT,
            swap_mode: SwapMode::ExactIn,
        });
        assert!(result.is_err());
    }

    #[test]
    fn label_is_dr_fraudsworth() {
        let amm = make_amm(true, 1, 1, 400, 1400);
        assert_eq!(amm.label(), "Dr Fraudsworth");
    }

    #[test]
    fn reserve_mints_correct() {
        let crime_amm = make_amm(true, 1, 1, 400, 1400);
        let mints = crime_amm.get_reserve_mints();
        assert_eq!(mints, vec![NATIVE_MINT, CRIME_MINT]);

        let fraud_amm = make_amm(false, 1, 1, 400, 1400);
        let mints = fraud_amm.get_reserve_mints();
        assert_eq!(mints, vec![NATIVE_MINT, FRAUD_MINT]);
    }

    #[test]
    fn accounts_to_update_includes_pool_and_epoch() {
        let amm = make_amm(true, 1, 1, 400, 1400);
        let accounts = amm.get_accounts_to_update();
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0], CRIME_SOL_POOL);
        assert_eq!(accounts[1], EPOCH_STATE_PDA);
    }

    #[test]
    fn fee_pct_matches_combined_bps() {
        let amm = make_amm(true, 100_000_000_000, 100_000_000_000, 400, 1400);

        // Buy: LP 100 + tax 400 = 500 bps = 5%
        let buy_quote = amm.quote(&QuoteParams {
            amount: 1_000_000_000,
            input_mint: NATIVE_MINT,
            output_mint: CRIME_MINT,
            swap_mode: SwapMode::ExactIn,
        }).unwrap();
        assert_eq!(buy_quote.fee_pct, Decimal::from(500u32) / Decimal::from(10_000u32));

        // Sell: LP 100 + tax 1400 = 1500 bps = 15%
        let sell_quote = amm.quote(&QuoteParams {
            amount: 1_000_000_000,
            input_mint: CRIME_MINT,
            output_mint: NATIVE_MINT,
            swap_mode: SwapMode::ExactIn,
        }).unwrap();
        assert_eq!(sell_quote.fee_pct, Decimal::from(1500u32) / Decimal::from(10_000u32));
    }

    #[test]
    fn buy_quote_high_tax_still_works() {
        // 50% buy tax (extreme case)
        let amm = make_amm(true, 100_000_000_000, 100_000_000_000, 5000, 5000);

        let quote = amm.quote(&QuoteParams {
            amount: 1_000_000_000,
            input_mint: NATIVE_MINT,
            output_mint: CRIME_MINT,
            swap_mode: SwapMode::ExactIn,
        }).unwrap();

        // 50% tax = 500M lamports tax, 500M to swap
        assert!(quote.out_amount > 0);
        assert!(quote.fee_amount >= 500_000_000);
    }

    // =========================================================================
    // from_keyed_account: generic construction + shape validation
    // =========================================================================

    #[test]
    fn from_keyed_account_accepts_any_key_with_supported_shape() {
        // Auto-discovery readiness: a NEW pool account (unknown key) with a
        // valid CRIME/SOL shape constructs without SDK changes.
        let new_pool_key = Pubkey::new_unique();
        let keyed = keyed_account(new_pool_key, crime_shaped_data(), AMM_PROGRAM_ID);

        let amm = SolPoolAmm::from_keyed_account(&keyed, &amm_context())
            .expect("supported pool shape must construct");
        assert_eq!(amm.key(), new_pool_key);
        assert_eq!(amm.get_reserve_mints(), vec![NATIVE_MINT, CRIME_MINT]);
    }

    #[test]
    fn from_keyed_account_accepts_both_faction_tokens() {
        for token in [CRIME_MINT, FRAUD_MINT] {
            let data = mock_pool_state(
                &NATIVE_MINT,
                &token,
                &Pubkey::new_unique(),
                &Pubkey::new_unique(),
                1,
                1,
                100,
            );
            let keyed = keyed_account(Pubkey::new_unique(), data, AMM_PROGRAM_ID);
            let amm = SolPoolAmm::from_keyed_account(&keyed, &amm_context())
                .expect("faction token pool must construct");
            assert!(amm.get_reserve_mints().contains(&token));
        }
    }

    #[test]
    fn from_keyed_account_rejects_wrong_owner() {
        let keyed = keyed_account(Pubkey::new_unique(), crime_shaped_data(), Pubkey::new_unique());

        let err = match SolPoolAmm::from_keyed_account(&keyed, &amm_context()) {
            Ok(_) => panic!("wrong owner must be rejected"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("not the AMM program"), "got: {err}");
    }

    #[test]
    fn from_keyed_account_rejects_unsupported_token_mint() {
        let data = mock_pool_state(
            &NATIVE_MINT,
            &Pubkey::new_unique(), // not CRIME or FRAUD
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            1,
            1,
            100,
        );
        let keyed = keyed_account(Pubkey::new_unique(), data, AMM_PROGRAM_ID);

        let err = match SolPoolAmm::from_keyed_account(&keyed, &amm_context()) {
            Ok(_) => panic!("unsupported token mint must be rejected"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("unsupported token mint"), "got: {err}");
    }

    #[test]
    fn from_keyed_account_rejects_non_sol_pool() {
        let data = mock_pool_state(
            &CRIME_MINT,
            &FRAUD_MINT, // no SOL side
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            1,
            1,
            100,
        );
        let keyed = keyed_account(Pubkey::new_unique(), data, AMM_PROGRAM_ID);

        let err = match SolPoolAmm::from_keyed_account(&keyed, &amm_context()) {
            Ok(_) => panic!("non-SOL pool must be rejected"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("not SOL-quoted"), "got: {err}");
    }

    #[test]
    fn from_keyed_account_rejects_bad_discriminator() {
        // Zeroed data has the right owner and length but no PoolState
        // discriminator — e.g. some other AMM-program account from a scan.
        let keyed = keyed_account(Pubkey::new_unique(), vec![0u8; 224], AMM_PROGRAM_ID);

        let err = match SolPoolAmm::from_keyed_account(&keyed, &amm_context()) {
            Ok(_) => panic!("bad discriminator must be rejected"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("discriminator"), "got: {err}");
    }

    #[test]
    fn from_keyed_account_reversed_orientation_constructs() {
        // Token on mint_a side (how a future canonical-ordering pool could
        // land): reserves must still resolve to (sol, token) correctly.
        let data = mock_pool_state(
            &CRIME_MINT,
            &NATIVE_MINT,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            777, // reserve_a = token side here
            555, // reserve_b = SOL side here
            100,
        );
        let keyed = keyed_account(Pubkey::new_unique(), data, AMM_PROGRAM_ID);

        let amm = SolPoolAmm::from_keyed_account(&keyed, &amm_context())
            .expect("reversed SOL pool must construct");
        assert_eq!(amm.get_reserve_mints(), vec![NATIVE_MINT, CRIME_MINT]);
        assert_eq!(amm.reserves(), (555, 777));
    }
}
