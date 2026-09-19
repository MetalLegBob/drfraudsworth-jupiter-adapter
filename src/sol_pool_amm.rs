// Jupiter Amm trait implementation for Dr. Fraudsworth faction pools.
//
// SolPoolAmm (retained compatibility name) handles faction/quote swaps via the Tax Program. Instances are
// constructed generically from PoolState account data: mints, vaults, and
// orientation all come from the parsed account, not per-pool constants.
// Any canonical CRIME/FRAUD pool created by the AMM can be discovered without
// a registry release; WSOL selects the SOL Tax lane and every other admitted
// quote mint selects the generic SPL Tax lane.
//
// Quote flow:
//   Buy (quote -> faction): tax quote input, then swap the post-tax amount.
//   Sell (faction -> quote): swap the faction input, then tax quote output.

use anyhow::{anyhow, Result};
use jupiter_amm_interface::{
    try_get_account_data_and_owner, AccountMap, Amm, AmmContext, AmmProgramIdToLabel, ClockRef,
    KeyedAccount, Quote, QuoteParams, Swap, SwapAndAccountMetas, SwapMode, SwapParams,
};
use rust_decimal::Decimal;
use solana_sdk::pubkey::Pubkey;
use std::sync::atomic::Ordering;

use crate::accounts::addresses::{
    AMM_PROGRAM_ID, CRIME_MINT, EPOCH_PROGRAM_ID, EPOCH_STATE_PDA, FRAUD_MINT, NATIVE_MINT,
    SPL_TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID,
};
use crate::accounts::sol_pool_accounts::{
    build_buy_account_metas_generic, build_sell_account_metas_generic, known_pool_state,
};
use crate::accounts::spl_pool_accounts::{
    build_buy_account_metas_generic as build_spl_buy_account_metas_generic,
    build_sell_account_metas_generic as build_spl_sell_account_metas_generic,
};
use crate::instruction::TaxSwapLane;
use crate::math::amm_math::{calculate_effective_input, calculate_swap_output};
use crate::math::tax_math::calculate_tax;
use crate::state::epoch_state::ParsedEpochState;
use crate::state::mint_state::validate_quote_mint;
use crate::state::pool_state::ParsedPoolState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FactionRole {
    Crime,
    Fraud,
}

impl FactionRole {
    fn from_mint(mint: &Pubkey) -> Option<Self> {
        if *mint == CRIME_MINT {
            Some(Self::Crime)
        } else if *mint == FRAUD_MINT {
            Some(Self::Fraud)
        } else {
            None
        }
    }
}

/// Jupiter Amm implementation for canonical CRIME/FRAUD faction pools.
///
/// Swaps go through the Tax Program which deducts dynamic tax and then
/// CPI-calls the AMM for the actual constant-product swap.
#[derive(Clone)]
pub struct SolPoolAmm {
    /// Pool PDA address
    key: Pubkey,
    /// Full parsed pool state (mints, vaults, reserves, LP fee)
    pool: ParsedPoolState,
    /// The canonical CRIME or FRAUD side.
    faction_mint: Pubkey,
    /// The other side: WSOL today or any admitted SPL/Token-2022 quote mint.
    quote_mint: Pubkey,
    /// Explicit registry role used to select the matching epoch tax lane.
    faction: FactionRole,
    /// Current epoch buy tax in BPS
    buy_tax_bps: u16,
    /// Current epoch sell tax in BPS
    sell_tax_bps: u16,
    pause_end_slot: u64,
    trading_paused: bool,
    epoch_initialized: bool,
    state_refreshed: bool,
    clock_ref: ClockRef,
}

/// Jupiter discovers candidate PoolState accounts by scanning the AMM
/// program. Swap execution is intentionally different and remains the Tax
/// program returned by `Amm::program_id()`.
impl AmmProgramIdToLabel for SolPoolAmm {
    const PROGRAM_ID_TO_LABELS: &'static [(Pubkey, &'static str)] =
        &[(AMM_PROGRAM_ID, "Dr Fraudsworth")];
}

impl Amm for SolPoolAmm {
    fn from_keyed_account(keyed_account: &KeyedAccount, amm_context: &AmmContext) -> Result<Self>
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
        if !pool_state.initialized {
            return Err(anyhow!(
                "FactionPoolAmm: pool {} is uninitialized",
                keyed_account.key
            ));
        }
        if pool_state.mint_a.as_ref() >= pool_state.mint_b.as_ref() {
            return Err(anyhow!(
                "FactionPoolAmm: pool mints are not in canonical byte order"
            ));
        }
        let (expected_key, _) = Pubkey::find_program_address(
            &[
                b"pool",
                pool_state.mint_a.as_ref(),
                pool_state.mint_b.as_ref(),
            ],
            &AMM_PROGRAM_ID,
        );
        if keyed_account.key != expected_key {
            return Err(anyhow!(
                "FactionPoolAmm: account {} is not canonical pool PDA {}",
                keyed_account.key,
                expected_key
            ));
        }
        for (side, program) in [
            ("A", pool_state.token_program_a),
            ("B", pool_state.token_program_b),
        ] {
            if program != SPL_TOKEN_PROGRAM_ID && program != TOKEN_2022_PROGRAM_ID {
                return Err(anyhow!(
                    "FactionPoolAmm: unsupported token program on side {side}: {program}"
                ));
            }
        }

        let a_faction = FactionRole::from_mint(&pool_state.mint_a);
        let b_faction = FactionRole::from_mint(&pool_state.mint_b);
        let (faction_mint, quote_mint, faction, faction_program) = match (a_faction, b_faction) {
            (Some(role), None) => (
                pool_state.mint_a,
                pool_state.mint_b,
                role,
                pool_state.token_program_a,
            ),
            (None, Some(role)) => (
                pool_state.mint_b,
                pool_state.mint_a,
                role,
                pool_state.token_program_b,
            ),
            _ => {
                return Err(anyhow!(
                    "FactionPoolAmm: pool {} must contain exactly one canonical faction mint",
                    keyed_account.key
                ))
            }
        };
        if faction_program != TOKEN_2022_PROGRAM_ID {
            return Err(anyhow!(
                "FactionPoolAmm: faction mint {} must use Token-2022",
                faction_mint
            ));
        }
        if quote_mint == NATIVE_MINT {
            let quote_program = if pool_state.mint_a == quote_mint {
                pool_state.token_program_a
            } else {
                pool_state.token_program_b
            };
            if quote_program != SPL_TOKEN_PROGRAM_ID {
                return Err(anyhow!(
                    "FactionPoolAmm: WSOL quote must use classic SPL Token"
                ));
            }
        }

        Ok(Self {
            key: keyed_account.key,
            pool: pool_state,
            faction_mint,
            quote_mint,
            faction,
            // Tax rates initialized to 0; populated by first update() call.
            // Jupiter always calls update() before quote().
            buy_tax_bps: 0,
            sell_tax_bps: 0,
            pause_end_slot: 0,
            trading_paused: false,
            epoch_initialized: false,
            state_refreshed: false,
            clock_ref: amm_context.clock_ref.clone(),
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
        vec![self.quote_mint, self.faction_mint]
    }

    fn get_accounts_to_update(&self) -> Vec<Pubkey> {
        // Jupiter refreshes these accounts and passes them to update()
        let mut accounts = vec![self.key, EPOCH_STATE_PDA];
        if self.quote_mint != NATIVE_MINT {
            accounts.push(self.quote_mint);
        }
        accounts
    }

    fn update(&mut self, account_map: &AccountMap) -> Result<()> {
        // A failed refresh must never leave a previously active route
        // advertising stale state.
        self.state_refreshed = false;

        // Re-parse pool state for reserves + LP fee
        let (pool_data, pool_owner) = try_get_account_data_and_owner(account_map, &self.key)?;
        if *pool_owner != AMM_PROGRAM_ID {
            return Err(anyhow!(
                "FactionPoolAmm: refreshed pool has wrong owner {pool_owner}"
            ));
        }
        let pool_state = ParsedPoolState::from_bytes(pool_data)?;

        // The pool's mints are immutable on-chain; a change means we were
        // handed data for a different account.
        if pool_state.mint_a != self.pool.mint_a
            || pool_state.mint_b != self.pool.mint_b
            || pool_state.vault_a != self.pool.vault_a
            || pool_state.vault_b != self.pool.vault_b
            || pool_state.token_program_a != self.pool.token_program_a
            || pool_state.token_program_b != self.pool.token_program_b
        {
            return Err(anyhow!(
                "FactionPoolAmm: immutable pool identity changed for {}",
                self.key
            ));
        }
        // Parse epoch state for tax rates
        let (epoch_data, epoch_owner) =
            try_get_account_data_and_owner(account_map, &EPOCH_STATE_PDA)?;
        if *epoch_owner != EPOCH_PROGRAM_ID {
            return Err(anyhow!(
                "FactionPoolAmm: EpochState has wrong owner {epoch_owner}"
            ));
        }
        let epoch_state = ParsedEpochState::from_bytes(epoch_data)?;
        self.buy_tax_bps = epoch_state.get_tax_bps(self.is_crime(), true);
        self.sell_tax_bps = epoch_state.get_tax_bps(self.is_crime(), false);
        self.pause_end_slot = epoch_state.pause_end_slot;
        self.trading_paused = epoch_state.trading_paused;
        self.epoch_initialized = epoch_state.initialized;

        if self.quote_mint != NATIVE_MINT {
            let quote_program = self.quote_token_program();
            let (mint_data, mint_owner) =
                try_get_account_data_and_owner(account_map, &self.quote_mint)?;
            validate_quote_mint(mint_data, mint_owner, &quote_program)?;
        }
        self.pool = pool_state;
        self.state_refreshed = true;

        Ok(())
    }

    fn quote(&self, quote_params: &QuoteParams) -> Result<Quote> {
        if quote_params.swap_mode == SwapMode::ExactOut {
            return Err(anyhow!("ExactOut not supported"));
        }

        let is_buy = quote_params.input_mint == self.quote_mint;
        self.validate_mint_pair(&quote_params.input_mint, &quote_params.output_mint, is_buy)?;

        // Mirror the on-chain manual and slot-based admission gates. ClockRef
        // is shared by Jupiter, so the route reopens at the inclusive slot
        // boundary without waiting for another account refresh.
        if !self.is_active() {
            return Err(anyhow!(
                "FactionPoolAmm: pool {} is inactive (initialized={}, locked={}, epoch_initialized={}, manual_pause={}, current_slot={}, pause_end_slot={}, refreshed={})",
                self.key,
                self.pool.initialized,
                self.pool.locked,
                self.epoch_initialized,
                self.trading_paused,
                self.clock_ref.slot.load(Ordering::Relaxed),
                self.pause_end_slot,
                self.state_refreshed,
            ));
        }

        if is_buy {
            self.quote_buy(quote_params.amount)
        } else {
            self.quote_sell(quote_params.amount)
        }
    }

    fn get_swap_and_account_metas(&self, swap_params: &SwapParams) -> Result<SwapAndAccountMetas> {
        if swap_params.swap_mode == SwapMode::ExactOut {
            return Err(anyhow!("ExactOut not supported"));
        }
        let is_buy = swap_params.source_mint == self.quote_mint;
        self.validate_mint_pair(
            &swap_params.source_mint,
            &swap_params.destination_mint,
            is_buy,
        )?;

        let lane = TaxSwapLane::for_mints(
            &self.quote_mint,
            &self.faction_mint,
            &swap_params.source_mint,
            &swap_params.destination_mint,
        )
        .ok_or_else(|| anyhow!("FactionPoolAmm: unsupported swap mint pair"))?;

        let account_metas = match lane {
            TaxSwapLane::SolBuy => {
                // Buy: source = WSOL, destination = token
                build_buy_account_metas_generic(
                    &swap_params.token_transfer_authority,
                    &swap_params.source_token_account,
                    &swap_params.destination_token_account,
                    &self.key,
                    &self.pool,
                )?
            }
            TaxSwapLane::SolSell => {
                // Sell: source = token, destination = WSOL
                build_sell_account_metas_generic(
                    &swap_params.token_transfer_authority,
                    &swap_params.source_token_account,
                    &swap_params.destination_token_account,
                    &self.key,
                    &self.pool,
                )?
            }
            TaxSwapLane::SplBuy => build_spl_buy_account_metas_generic(
                &swap_params.token_transfer_authority,
                &swap_params.source_token_account,
                &swap_params.destination_token_account,
                &self.key,
                &self.pool,
            )?,
            TaxSwapLane::SplSell => build_spl_sell_account_metas_generic(
                &swap_params.token_transfer_authority,
                &swap_params.source_token_account,
                &swap_params.destination_token_account,
                &self.key,
                &self.pool,
            )?,
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

    fn is_active(&self) -> bool {
        self.pool.initialized
            && !self.pool.locked
            && self.epoch_initialized
            && self.state_refreshed
            && !self.trading_paused
            && self.clock_ref.slot.load(Ordering::Relaxed) >= self.pause_end_slot
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
        let faction_mint = pool.token_mint().expect("known pools are SOL pools");
        let faction = FactionRole::from_mint(&faction_mint)
            .expect("known pools contain a generated faction asset");
        Self {
            key,
            pool,
            faction_mint,
            quote_mint: NATIVE_MINT,
            faction,
            buy_tax_bps,
            sell_tax_bps,
            pause_end_slot: 0,
            trading_paused: false,
            epoch_initialized: true,
            state_refreshed: true,
            clock_ref: ClockRef::default(),
        }
    }

    fn is_crime(&self) -> bool {
        self.faction == FactionRole::Crime
    }

    /// (quote_reserve, faction_reserve) in orientation-independent order.
    fn reserves(&self) -> (u64, u64) {
        if self.pool.mint_a == self.quote_mint {
            (self.pool.reserve_a, self.pool.reserve_b)
        } else {
            (self.pool.reserve_b, self.pool.reserve_a)
        }
    }

    fn quote_token_program(&self) -> Pubkey {
        if self.pool.mint_a == self.quote_mint {
            self.pool.token_program_a
        } else {
            self.pool.token_program_b
        }
    }

    /// Validate that the requested trade pair matches this pool.
    fn validate_mint_pair(&self, input: &Pubkey, output: &Pubkey, is_buy: bool) -> Result<()> {
        let ok = if is_buy {
            *input == self.quote_mint && *output == self.faction_mint
        } else {
            *input == self.faction_mint && *output == self.quote_mint
        };
        if ok {
            Ok(())
        } else {
            Err(anyhow!(
                "FactionPoolAmm: mint pair {} -> {} does not match pool {} ({} <-> {})",
                input,
                output,
                self.key,
                self.quote_mint,
                self.faction_mint
            ))
        }
    }

    /// Quote a buy (SOL -> token).
    ///
    /// Flow: tax deducted from SOL input, LP fee deducted, then constant-product swap.
    fn quote_buy(&self, amount_in: u64) -> Result<Quote> {
        let (reserve_quote, reserve_faction) = self.reserves();

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
                fee_mint: self.quote_mint,
                fee_pct: self.total_buy_fee_pct(),
            });
        }

        // 2. LP fee deducted from post-tax amount
        let effective_input = calculate_effective_input(sol_to_swap, self.pool.lp_fee_bps)
            .ok_or_else(|| anyhow!("Effective input calculation overflow"))?;

        // 3. Constant-product swap
        let out_amount = calculate_swap_output(reserve_quote, reserve_faction, effective_input)
            .ok_or_else(|| anyhow!("Swap output calculation overflow or zero reserves"))?;

        // LP fee in SOL terms
        let lp_fee_sol = sol_to_swap.saturating_sub(effective_input as u64);

        Ok(Quote {
            in_amount: amount_in,
            out_amount,
            fee_amount: tax.checked_add(lp_fee_sol).unwrap_or(tax),
            fee_mint: self.quote_mint,
            fee_pct: self.total_buy_fee_pct(),
        })
    }

    /// Quote a sell (token -> SOL).
    ///
    /// Flow: LP fee deducted from token input, constant-product swap, then tax on SOL output.
    fn quote_sell(&self, amount_in: u64) -> Result<Quote> {
        let (reserve_quote, reserve_faction) = self.reserves();

        // 1. LP fee deducted from token input
        let effective_input = calculate_effective_input(amount_in, self.pool.lp_fee_bps)
            .ok_or_else(|| anyhow!("Effective input calculation overflow"))?;

        // 2. Constant-product swap (token -> SOL)
        let gross_quote = calculate_swap_output(reserve_faction, reserve_quote, effective_input)
            .ok_or_else(|| anyhow!("Swap output calculation overflow or zero reserves"))?;

        // 3. Tax deducted from SOL output
        let tax = calculate_tax(gross_quote, self.sell_tax_bps)
            .ok_or_else(|| anyhow!("Tax calculation overflow"))?;

        let net_quote = gross_quote
            .checked_sub(tax)
            .ok_or_else(|| anyhow!("Tax exceeds gross output"))?;

        // The LP fee is taken from faction input; fee_mint is the quote, so we
        // report only the tax as the primary fee amount. Jupiter uses
        // fee_pct (LP + tax combined) as the authoritative fee indicator.
        Ok(Quote {
            in_amount: amount_in,
            out_amount: net_quote,
            fee_amount: tax,
            fee_mint: self.quote_mint,
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
    use crate::accounts::addresses::{CRIME_MINT, CRIME_SOL_POOL, FRAUD_MINT};
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
        SolPoolAmm::new_for_testing(
            is_crime,
            reserve_sol,
            reserve_token,
            buy_tax_bps,
            sell_tax_bps,
        )
    }

    fn amm_context() -> AmmContext {
        AmmContext {
            clock_ref: ClockRef::default(),
        }
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

    fn canonical_key(data: &[u8]) -> Pubkey {
        let pool = ParsedPoolState::from_bytes(data).unwrap();
        Pubkey::find_program_address(
            &[b"pool", pool.mint_a.as_ref(), pool.mint_b.as_ref()],
            &AMM_PROGRAM_ID,
        )
        .0
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

        let quote = amm
            .quote(&QuoteParams {
                amount: 1_000_000_000, // 1 SOL
                input_mint: NATIVE_MINT,
                output_mint: CRIME_MINT,
                swap_mode: SwapMode::ExactIn,
                fee_mode: jupiter_amm_interface::FeeMode::Normal,
            })
            .unwrap();

        // Tax = 1 SOL * 400/10000 = 0.04 SOL = 40_000_000 lamports
        // sol_to_swap = 960_000_000
        // After 1% LP fee: effective = 960_000_000 * 9900/10000 = 950_400_000
        // Output: 100B * 950_400_000 / (100B + 950_400_000)
        assert!(quote.out_amount > 0, "output should be non-zero");
        assert!(
            quote.out_amount < 1_000_000_000,
            "output should be less than input for equal reserves"
        );
        assert!(
            quote.fee_amount >= 40_000_000,
            "fee should include at least the tax"
        );
        assert_eq!(quote.in_amount, 1_000_000_000);
        assert_eq!(quote.fee_mint, NATIVE_MINT);
    }

    #[test]
    fn sell_quote_applies_tax_after_swap() {
        // Sell 1B tokens, 14% sell tax
        let amm = make_amm(true, 100_000_000_000, 100_000_000_000, 400, 1400);

        let quote = amm
            .quote(&QuoteParams {
                amount: 1_000_000_000, // 1B token units
                input_mint: CRIME_MINT,
                output_mint: NATIVE_MINT,
                swap_mode: SwapMode::ExactIn,
                fee_mode: jupiter_amm_interface::FeeMode::Normal,
            })
            .unwrap();

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

        let quote = amm
            .quote(&QuoteParams {
                amount: 0,
                input_mint: NATIVE_MINT,
                output_mint: CRIME_MINT,
                swap_mode: SwapMode::ExactIn,
                fee_mode: jupiter_amm_interface::FeeMode::Normal,
            })
            .unwrap();

        assert_eq!(quote.out_amount, 0);
    }

    #[test]
    fn sell_zero_sol_reserves_returns_zero_output() {
        // Zero SOL reserves means swap output = 0 (no SOL to extract)
        let amm = make_amm(true, 0, 100_000_000_000, 400, 1400);

        let quote = amm
            .quote(&QuoteParams {
                amount: 1_000_000_000,
                input_mint: CRIME_MINT,
                output_mint: NATIVE_MINT,
                swap_mode: SwapMode::ExactIn,
                fee_mode: jupiter_amm_interface::FeeMode::Normal,
            })
            .unwrap();

        assert_eq!(
            quote.out_amount, 0,
            "should return 0 with zero SOL reserves"
        );
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
            fee_mode: jupiter_amm_interface::FeeMode::Normal,
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
            fee_mode: jupiter_amm_interface::FeeMode::Normal,
        });

        assert!(result.is_err());
    }

    #[test]
    fn quote_refused_while_manual_pause_is_active() {
        let mut amm = make_amm(true, 100_000_000_000, 100_000_000_000, 400, 1400);

        amm.trading_paused = true;
        let err = match amm.quote(&QuoteParams {
            amount: 1_000_000_000,
            input_mint: NATIVE_MINT,
            output_mint: CRIME_MINT,
            swap_mode: SwapMode::ExactIn,
            fee_mode: jupiter_amm_interface::FeeMode::Normal,
        }) {
            Ok(_) => panic!("quote must be refused during a manual pause"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("inactive"), "got: {err}");

        // Window closed -> quotes flow again.
        amm.trading_paused = false;
        assert!(amm
            .quote(&QuoteParams {
                amount: 1_000_000_000,
                input_mint: NATIVE_MINT,
                output_mint: CRIME_MINT,
                swap_mode: SwapMode::ExactIn,
                fee_mode: jupiter_amm_interface::FeeMode::Normal,
            })
            .is_ok());
    }

    #[test]
    fn pause_end_slot_boundary_is_inclusive() {
        let mut amm = make_amm(true, 100_000_000_000, 100_000_000_000, 400, 1400);
        amm.pause_end_slot = 500;
        amm.clock_ref.slot.store(499, Ordering::Relaxed);
        assert!(!amm.is_active());
        assert!(amm
            .quote(&QuoteParams {
                amount: 1_000_000,
                input_mint: NATIVE_MINT,
                output_mint: CRIME_MINT,
                swap_mode: SwapMode::ExactIn,
                fee_mode: jupiter_amm_interface::FeeMode::Normal,
            })
            .is_err());

        amm.clock_ref.slot.store(500, Ordering::Relaxed);
        assert!(amm.is_active(), "slot == pause_end_slot must trade");
        assert!(amm
            .quote(&QuoteParams {
                amount: 1_000_000,
                input_mint: NATIVE_MINT,
                output_mint: CRIME_MINT,
                swap_mode: SwapMode::ExactIn,
                fee_mode: jupiter_amm_interface::FeeMode::Normal,
            })
            .is_ok());
    }

    #[test]
    fn locked_pool_is_inactive() {
        let mut amm = make_amm(true, 1, 1, 400, 1400);
        amm.pool.locked = true;
        assert!(!amm.is_active());
    }

    #[test]
    fn failed_refresh_deactivates_previously_fresh_state() {
        let mut amm = make_amm(true, 1, 1, 400, 1400);
        assert!(amm.is_active());
        assert!(amm.update(&AccountMap::default()).is_err());
        assert!(!amm.is_active());
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
            fee_mode: jupiter_amm_interface::FeeMode::Normal,
        });
        assert!(result.is_err());

        // Buy direction with the wrong output token
        let result = amm.quote(&QuoteParams {
            amount: 1_000_000_000,
            input_mint: NATIVE_MINT,
            output_mint: FRAUD_MINT,
            swap_mode: SwapMode::ExactIn,
            fee_mode: jupiter_amm_interface::FeeMode::Normal,
        });
        assert!(result.is_err());
    }

    #[test]
    fn label_is_dr_fraudsworth() {
        let amm = make_amm(true, 1, 1, 400, 1400);
        assert_eq!(amm.label(), "Dr Fraudsworth");
    }

    #[test]
    fn discovery_uses_amm_program_while_execution_uses_tax_program() {
        let amm = make_amm(true, 1, 1, 400, 1400);
        assert_eq!(
            SolPoolAmm::PROGRAM_ID_TO_LABELS,
            &[(AMM_PROGRAM_ID, "Dr Fraudsworth")]
        );
        assert_eq!(amm.program_id(), crate::accounts::addresses::TAX_PROGRAM_ID);
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
        let buy_quote = amm
            .quote(&QuoteParams {
                amount: 1_000_000_000,
                input_mint: NATIVE_MINT,
                output_mint: CRIME_MINT,
                swap_mode: SwapMode::ExactIn,
                fee_mode: jupiter_amm_interface::FeeMode::Normal,
            })
            .unwrap();
        assert_eq!(
            buy_quote.fee_pct,
            Decimal::from(500u32) / Decimal::from(10_000u32)
        );

        // Sell: LP 100 + tax 1400 = 1500 bps = 15%
        let sell_quote = amm
            .quote(&QuoteParams {
                amount: 1_000_000_000,
                input_mint: CRIME_MINT,
                output_mint: NATIVE_MINT,
                swap_mode: SwapMode::ExactIn,
                fee_mode: jupiter_amm_interface::FeeMode::Normal,
            })
            .unwrap();
        assert_eq!(
            sell_quote.fee_pct,
            Decimal::from(1500u32) / Decimal::from(10_000u32)
        );
    }

    #[test]
    fn buy_quote_high_tax_still_works() {
        // 50% buy tax (extreme case)
        let amm = make_amm(true, 100_000_000_000, 100_000_000_000, 5000, 5000);

        let quote = amm
            .quote(&QuoteParams {
                amount: 1_000_000_000,
                input_mint: NATIVE_MINT,
                output_mint: CRIME_MINT,
                swap_mode: SwapMode::ExactIn,
                fee_mode: jupiter_amm_interface::FeeMode::Normal,
            })
            .unwrap();

        // 50% tax = 500M lamports tax, 500M to swap
        assert!(quote.out_amount > 0);
        assert!(quote.fee_amount >= 500_000_000);
    }

    // =========================================================================
    // from_keyed_account: generic construction + shape validation
    // =========================================================================

    #[test]
    fn from_keyed_account_accepts_unregistered_canonical_pool() {
        // Auto-discovery readiness: construction depends on the canonical
        // PoolState/PDA relationship, not a static registry allowlist.
        let data = crime_shaped_data();
        let new_pool_key = canonical_key(&data);
        let keyed = keyed_account(new_pool_key, data, AMM_PROGRAM_ID);

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
            let key = canonical_key(&data);
            let keyed = keyed_account(key, data, AMM_PROGRAM_ID);
            let amm = SolPoolAmm::from_keyed_account(&keyed, &amm_context())
                .expect("faction token pool must construct");
            assert!(amm.get_reserve_mints().contains(&token));
        }
    }

    #[test]
    fn from_keyed_account_rejects_wrong_owner() {
        let keyed = keyed_account(
            Pubkey::new_unique(),
            crime_shaped_data(),
            Pubkey::new_unique(),
        );

        let err = match SolPoolAmm::from_keyed_account(&keyed, &amm_context()) {
            Ok(_) => panic!("wrong owner must be rejected"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("not the AMM program"),
            "got: {err}"
        );
    }

    #[test]
    fn from_keyed_account_rejects_unsupported_token_mint() {
        let unknown = Pubkey::new_from_array([0xFE; 32]);
        let (mint_a, mint_b) = if NATIVE_MINT.as_ref() < unknown.as_ref() {
            (NATIVE_MINT, unknown)
        } else {
            (unknown, NATIVE_MINT)
        };
        let data = mock_pool_state(
            &mint_a,
            &mint_b, // neither side is CRIME or FRAUD
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            1,
            1,
            100,
        );
        let key = canonical_key(&data);
        let keyed = keyed_account(key, data, AMM_PROGRAM_ID);

        let err = match SolPoolAmm::from_keyed_account(&keyed, &amm_context()) {
            Ok(_) => panic!("unsupported token mint must be rejected"),
            Err(e) => e,
        };
        assert!(
            err.to_string()
                .contains("exactly one canonical faction mint"),
            "got: {err}"
        );
    }

    #[test]
    fn from_keyed_account_rejects_non_sol_pool() {
        let (mint_a, mint_b) = if CRIME_MINT.as_ref() < FRAUD_MINT.as_ref() {
            (CRIME_MINT, FRAUD_MINT)
        } else {
            (FRAUD_MINT, CRIME_MINT)
        };
        let data = mock_pool_state(
            &mint_a,
            &mint_b,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            1,
            1,
            100,
        );
        let key = canonical_key(&data);
        let keyed = keyed_account(key, data, AMM_PROGRAM_ID);

        let err = match SolPoolAmm::from_keyed_account(&keyed, &amm_context()) {
            Ok(_) => panic!("non-SOL pool must be rejected"),
            Err(e) => e,
        };
        assert!(
            err.to_string()
                .contains("exactly one canonical faction mint"),
            "got: {err}"
        );
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
    fn from_keyed_account_spl_quote_with_faction_on_a_constructs() {
        let quote = Pubkey::new_from_array([0xFF; 32]);
        assert!(CRIME_MINT.as_ref() < quote.as_ref());
        let data = mock_pool_state(
            &CRIME_MINT,
            &quote,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            777, // reserve_a = token side here
            555, // reserve_b = quote side here
            100,
        );
        let key = canonical_key(&data);
        let keyed = keyed_account(key, data, AMM_PROGRAM_ID);

        let amm = SolPoolAmm::from_keyed_account(&keyed, &amm_context())
            .expect("generic SPL quote pool must construct");
        assert_eq!(amm.get_reserve_mints(), vec![quote, CRIME_MINT]);
        assert_eq!(amm.reserves(), (555, 777));
    }
}
