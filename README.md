# Dr. Fraudsworth Jupiter Adapter SDK

[![CI](https://github.com/MetalLegBob/drfraudsworth-jupiter-adapter/actions/workflows/ci.yml/badge.svg)](https://github.com/MetalLegBob/drfraudsworth-jupiter-adapter/actions/workflows/ci.yml)

Jupiter AMM adapter for the Dr. Fraudsworth DEX protocol on Solana. Implements the `jupiter-amm-interface::Amm` trait so Jupiter's routing engine can route swaps through Dr. Fraudsworth's on-chain programs.

This repository contains the standalone adapter crate and reviewed IDL copies.
The canonical on-chain programs and math-parity suite (SDK quotes proven equal
to on-chain outputs) live in the protocol repository:
[github.com/MetalLegBob/drfraudsworth](https://github.com/MetalLegBob/drfraudsworth).

**Key properties:**

- Exact quote accuracy — the SDK's math modules are copies of the on-chain math, proven equal by zero-tolerance parity tests in the protocol repository, and validated here against embedded mainnet account data
- Zero network calls in any method (pool state is parsed from Jupiter-provided account snapshots; all protocol-singleton addresses are hardcoded)
- Supports the existing SOL pools, all four vault conversions, and generic SPL-quoted faction pools
- Generic pool construction: `FactionPoolAmm` (the neutral alias for `SolPoolAmm`) derives mints, vaults, token programs, reserves, lifecycle flags, and orientation from AMM-owned `PoolState` data
- Pause-aware routing: quotes are inactive during the manual pause, before `pause_end_slot`, for locked/uninitialized pools, or until fresh account snapshots have been applied

## Pool Types

The optional bootstrap catalog exposes the two original SOL pool keys and four
synthetic vault directions below. AMM-owned `PoolState` discovery covers every
live SOL or SPL quote pool without another static allowlist entry.

| # | Instance | Type | Key Source | Reserves | Fees |
|---|----------|------|------------|----------|------|
| 1 | CRIME/SOL | `SolPoolAmm` | Pool PDA | Dynamic (AMM constant-product) | 1% LP + dynamic tax (1-4% or 11-14%) |
| 2 | FRAUD/SOL | `SolPoolAmm` | Pool PDA | Dynamic (AMM constant-product) | 1% LP + dynamic tax (1-4% or 11-14%) |
| 3 | CRIME->PROFIT | `VaultAmm` | Synthetic PDA | Fixed rate (100:1) | Zero |
| 4 | FRAUD->PROFIT | `VaultAmm` | Synthetic PDA | Fixed rate (100:1) | Zero |
| 5 | PROFIT->CRIME | `VaultAmm` | Synthetic PDA | Fixed rate (1:100) | Zero |
| 6 | PROFIT->FRAUD | `VaultAmm` | Synthetic PDA | Fixed rate (1:100) | Zero |

- The 2 SOL pool instances are **bidirectional** (buy and sell), covering 4 swap directions.
- The 4 vault instances are **unidirectional**, one per conversion direction.
- **CRIME <-> FRAUD is an intentional two-leg route through PROFIT.** The vault
  exposes no single direct instruction; Jupiter composes CRIME -> PROFIT ->
  FRAUD, or the reverse, from two valid unidirectional instances.

## Pool Discovery

Jupiter integrators may use the bootstrap factories for the current deployed
set, but generic discovery should scan the AMM program's `PoolState` accounts:

```rust
use drfraudsworth_jupiter_adapter::{known_instances, known_sol_pool_keys, all_pool_keys};

// SOL pools: returns 2 pool PDAs for SolPoolAmm (created via from_keyed_account)
let sol_keys: Vec<Pubkey> = known_sol_pool_keys();

// Vault instances: returns 4 pre-built (Pubkey, VaultAmm) pairs
let vault_instances: Vec<(Pubkey, VaultAmm)> = known_instances();

// All 6 bootstrap keys in one call
let all_keys: Vec<Pubkey> = all_pool_keys();
```

- `known_sol_pool_keys()` -- Returns 2 SOL pool PDAs. Jupiter fetches account data and calls `SolPoolAmm::from_keyed_account()`.
- `known_instances()` -- Returns 4 pre-constructed `VaultAmm` instances (fixed-pool protocol, no `getProgramAccounts` needed).
- `all_pool_keys()` -- Convenience: all 6 bootstrap keys combined.

### Automatic PoolState discovery

`SolPoolAmm::from_keyed_account` is safe to feed arbitrary accounts and constructs generically:

1. Rejects accounts not owned by the AMM program
2. Rejects data without the `PoolState` Anchor discriminator (`sha256("account:PoolState")[0..8]`)
3. Requires the exact deployed 224-byte layout, canonical mint order, canonical
   pool PDA, initialized state, supported token programs, and exactly one
   CRIME/FRAUD side; a locked pool stays discoverable but inactive
4. Derives mints, vaults, reserves, token programs, and orientation entirely
   from the account bytes
5. Selects the existing Tax SOL lane when the quote is WSOL, otherwise the Tax
   SPL lane

Use `AmmProgramIdToLabel::PROGRAM_ID_TO_LABELS` (AMM program ID) or an equivalent
`getProgramAccounts` scan filtered by the `PoolState` discriminator and 224-byte
data size. Pool accounts are **owned and discovered under the AMM program**;
`program_id()` deliberately returns the **Tax Program**, because that is the
swap entry point that CPI-calls the AMM. This discovery/execution split is
covered by unit tests.

Current live `PoolState` references are listed here for integration testing,
not runtime admission:

| Pair | Pool |
|---|---|
| CRIME/SOL | `ZWUZ3PzGk6bg6g3BS3WdXKbdAecUgZxnruKXQkte7wf` |
| FRAUD/SOL | `AngvViTVGd2zxP8KoFUjGU3TyrQjqeM1idRWiKM8p3mq` |
| CRIME/USDC | `HyJReAfMzABjEgZQNLrkdSR4pD5P78G5ucEXWRoVDNUa` |
| FRAUD/USDC | `ETtBco8RUWNaNE9YozMg2KrpbJN7oqCjdd94QgwsAgzB` |
| CRIME/HYPE | `HummhRt6eZLVDRT3NNCRgTs3Mouje4EypQvQbXKD5Mvy` |
| FRAUD/HYPE | `2EVKU8ZmuZRXc6baRUHZVUjry1AaBJ9mwDgUJDZxrDGz` |

## Fee Structure

### Faction Pools

SOL pool swaps have two fee components:

1. **LP fee:** 1% (100 BPS), fixed, deducted from swap amount
2. **Dynamic tax:** 1-4% (cheap side) or 11-14% (expensive side), VRF-randomized each epoch (roughly 20 minutes). Tax is split across staking rewards (71%), Carnage Fund (24%), and treasury (5%)

**Buy (quote -> faction):** Tax deducted from quote input before the AMM swap.
**Sell (faction -> quote):** Tax deducted from quote output after the AMM swap.

SPL quote mints may use classic SPL Token or Token-2022. Token-2022 quotes are
accepted only when current and scheduled transfer fees are zero and the quote
transfer hook is unarmed, preserving nominal reserve and tax arithmetic.

Tax rates change every epoch (roughly 20 minutes). Jupiter's `update()` method refreshes EpochState to get current rates. Stale rates between quote and execution are handled by on-chain slippage protection (`minimum_output`).

### Vault Conversions

- **Zero fees**, fixed rate conversion
- CRIME/FRAUD -> PROFIT: divide by 100 (100 CRIME = 1 PROFIT)
- PROFIT -> CRIME/FRAUD: multiply by 100 (1 PROFIT = 100 CRIME)

## Epoch Dynamics

Each epoch (roughly 20 minutes), VRF randomness determines:

1. **Which faction is cheap** — 75% chance of flipping each epoch
2. **Exact tax magnitudes** — independently randomized per token from discrete sets

| Side | Buy Tax | Sell Tax |
|------|---------|----------|
| Cheap | 1%, 2%, 3%, or 4% | 11%, 12%, 13%, or 14% |
| Expensive | 11%, 12%, 13%, or 14% | 1%, 2%, 3%, or 4% |

CRIME and FRAUD get **independent magnitude rolls** — e.g., CRIME cheap buy could be 2% while FRAUD expensive buy is 13%. No intermediate values exist (only the 8 discrete rates above).

This creates arbitrage opportunities between the two pools that Jupiter can route through.

The `EpochState` PDA is declared in `get_accounts_to_update()`, so Jupiter automatically refreshes it and passes the latest state to `update()`.

`update()` also reads the clock and the SPL quote mint (when applicable).
`is_active()` becomes true only after a successful refresh, with an initialized
and unlocked pool, initialized EpochState, no manual pause, and
`clock.slot >= pause_end_slot` (the boundary is inclusive). Vault conversions
remain independent of the trading pause.

## Account Metas

Each instruction type requires a specific set of accounts. Pool-specific accounts (pool PDA, mints, vaults, orientation) are derived from the parsed `PoolState`; protocol singletons (authorities, staking, treasury, programs) are hardcoded mainnet addresses. Zero network calls.

### SwapSolBuy (SOL -> CRIME/FRAUD)

20 named accounts + 4 transfer hook accounts = **24 total**

Named accounts: user, epoch_state, swap_authority, tax_authority, pool, pool_vault_a, pool_vault_b, mint_a (WSOL), mint_b (token), user_token_a, user_token_b, stake_pool, staking_escrow, carnage_vault, treasury, amm_program, token_program_a (SPL Token), token_program_b (Token-2022), system_program, staking_program.

### SwapSolSell (CRIME/FRAUD -> SOL)

21 named accounts + 4 transfer hook accounts = **25 total**

Same as buy, plus `wsol_intermediary` PDA (account #16). The sell path routes SOL through an intermediary WSOL account before closing it back to the user.

### SwapSplBuy / SwapSplSell

SPL buys use 17 named accounts plus four faction-hook accounts (**21 total**).
SPL sells use 18 named accounts plus four faction-hook accounts (**22 total**).
The sell path places the Tax program's canonical sweep ATA in the positional
quote-output slot and supplies Jupiter's destination quote account separately.

### Vault Convert (token <-> token)

9 named accounts + 8 transfer hook accounts = **17 total**

Named accounts: user, vault_config, user_input_account, user_output_account, input_mint, output_mint, vault_input, vault_output, token_program (Token-2022).

Hook accounts: 4 for input mint + 4 for output mint (both are Token-2022 mints with transfer hooks).

### Transfer Hook Accounts (per mint)

Each Token-2022 mint has 4 deterministic hook accounts:

1. ExtraAccountMetaList PDA
2. Whitelist entry for source token account
3. Whitelist entry for destination token account
4. Transfer Hook program ID

## Quick Start

```rust
use drfraudsworth_jupiter_adapter::{SolPoolAmm, VaultAmm, known_instances, known_sol_pool_keys};
use jupiter_amm_interface::{Amm, KeyedAccount, QuoteParams, SwapMode};

// -- SOL Pool (production: use from_keyed_account with live account data) --
// Jupiter calls from_keyed_account() automatically during pool registration.
// The SDK's update() method refreshes reserves and tax rates from account snapshots.

// -- Vault Instances (pre-built, no account data needed) --
let vault_instances = known_instances();
for (key, amm) in &vault_instances {
    let quote = amm.quote(&QuoteParams {
        amount: 100_000_000_000, // 100B tokens
        input_mint: amm.get_reserve_mints()[0],
        output_mint: amm.get_reserve_mints()[1],
        swap_mode: SwapMode::ExactIn,
        fee_mode: jupiter_amm_interface::FeeMode::Normal,
    }).unwrap();
    println!("{}: {} -> {}", key, quote.in_amount, quote.out_amount);
}
```

See `examples/quote_example.rs` for a complete working example:

```bash
cargo run --example quote_example
```

## Interface Version

This source tree pins published `jupiter-amm-interface` 0.6.1, including
`FeeMode` and the `user`/`payer` swap fields. The dependency is exact and the
committed lockfile preserves the verified Solana 2.x dependency graph. The
pin does not define Jupiter's final router variant; that remains part of the
Jupiter-side integration.

## Program IDs

Jupiter needs to know which programs are called for each swap type:

| Program | Address | Called For |
|---------|---------|-----------|
| Tax Program | `43fZGRtmEsP7ExnJE1dbTbNjaP1ncvVmMPusSeksWGEj` | SOL and SPL pool swaps (CPI to AMM internally) |
| Conversion Vault | `5uawA6ehYTu69Ggvm3LSK84qFawPKxbWgfngwj15NRJ` | Vault conversions |
| AMM Program | `5JsSAL3kJDUWD4ZveYXYZmgm1eVqueesTZVdAvtZg8cR` | Owns PoolState accounts; called via CPI by Tax Program (not directly by Jupiter) |
| Transfer Hook | `CiQPQrmQh6BPhb9k7dFnsEs5gKPgdrvNKFc5xie5xVGd` | Called by Token-2022 during transfers |
| Epoch Program | `4Heqc8QEjJCspHR8y96wgZBnBfbe3Qb8N6JBZMQt9iw2` | Manages epoch state (not called by Jupiter directly) |
| Staking Program | `12b3t1cNiAUoYLiWFEnFa4w6qYxVAiqCWU7KZuzLPYtH` | Receives staking rewards from tax (not called by Jupiter directly) |

## IDLs

Anchor IDLs for all six programs are included in this repository under [`idl/`](./idl/). Each IDL embeds its mainnet program address; these are the same IDLs the production frontend runs against.

## Token Mints

All three protocol tokens use Token-2022 with transfer hooks:

| Token | Mint Address | Decimals |
|-------|-------------|----------|
| CRIME | `cRiMEhAxoDhcEuh3Yf7Z2QkXUXUMKbakhcVqmDsqPXc` | 6 |
| FRAUD | `FraUdp6YhtVJYPxC2w255yAbpTsPqd8Bfhy9rC56jau5` | 6 |
| PROFIT | `pRoFiTj36haRD5sG2Neqib9KoSrtdYMGrM7SEkZetfR` | 6 |

## Mainnet Addresses

Full address set is in `deployments/mainnet.json` in the protocol repository. Key addresses for Jupiter integration:

| Resource | Address |
|----------|---------|
| CRIME/SOL Pool | `ZWUZ3PzGk6bg6g3BS3WdXKbdAecUgZxnruKXQkte7wf` |
| FRAUD/SOL Pool | `AngvViTVGd2zxP8KoFUjGU3TyrQjqeM1idRWiKM8p3mq` |
| EpochState PDA | `FjJrLcmDjA8FtavGWdhJq3pdirAH889oWXc2bhEAMbDU` |
| VaultConfig PDA | `8vFpSBnCVt8dfX57FKrsGwy39TEo1TjVzrj9QYGxCkcD` |
| Swap Authority | `CoCdbornGtiZ8tLxF5HD2TdGidfgfwbbiDX79BaZGJ2D` |
| Treasury | `GDY4Qu3xGNGZxXdLs1h6eoMXZgJ9aPpv7jtCaqzMoDcN` |

## Jupiter Integration Notes

- **Swap variant:** Published interface 0.6.1 has no protocol-specific or generic pre-integration variant. The current `Swap::TokenSwap` return is a review placeholder only and must not be interpreted as a production SPL Token Swap route. Jupiter's final integration must allocate the real variant/processor and map direction and quote type to `swap_sol_buy`, `swap_sol_sell`, `swap_spl_buy`, or `swap_spl_sell`. `instruction::TaxSwapLane` exports the reviewed lane selection, Anchor discriminators, and 24-byte argument encoding; golden tests derive every discriminator from its Anchor preimage.
- **Vault instance keying:** Synthetic PDAs derived from `[b"jup_vault", input_mint, output_mint]` via `Pubkey::find_program_address`. These are not real on-chain accounts -- they exist solely to give each VaultAmm instance a unique key.
- **`supports_exact_out`:** Returns `false` for all instances. Integer division in vault conversions loses information, and SOL pool exact-out would require iterative solving.
- **No network calls:** All methods (`quote`, `get_swap_and_account_metas`, `get_accounts_to_update`) operate on Jupiter-provided account snapshots and constants. Jupiter handles account fetching externally.
- **WSOL wrapping:** Jupiter handles SOL <-> WSOL wrapping/unwrapping. The SDK returns only the Tax Program swap instruction.
- **`unidirectional()`:** Returns `true` for VaultAmm, `false` (default) for SolPoolAmm. Jupiter uses this to avoid routing backwards through vault instances.
- **Mint-pair validation:** `quote()` and `get_swap_and_account_metas()` reject requests whose mints do not match the instance's pool.
- **Pause-aware:** `update()` reads `pause_end_slot`, `trading_paused`, and initialization state from EpochState plus the current clock. Quotes refuse while the route is inactive.
- **Vault liquidity cap:** `VaultAmm::update()` reads the output-side vault token account balance, and quotes exceeding available liquidity are rejected rather than quoted-then-failed on-chain.
- **`program_dependencies()`:** returns the AMM, Staking, and Transfer Hook programs for SolPool swaps (Transfer Hook for vault conversions) so test harnesses know which dependent programs to load.
- **`underlying_liquidities()`:** the two `*->PROFIT` vault instances report the same underlying PROFIT vault account, exposing their shared liquidity to the routing engine.

## Testing

```bash
# Reproducible standalone verification
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo fmt --check

# Run the quote example
cargo run --example quote_example
```

See [TESTING.md](./TESTING.md) for the full suite breakdown. CI runs the suite plus clippy on every push.

The mainnet-data validation suite parses real (hex-embedded) mainnet account snapshots and includes an equivalence proof that account lists built from parsed on-chain data are byte-identical to the constant-based builders.

The standalone suite currently contains 245 deterministic tests. Cross-crate
proofs live in the protocol repository because they compile against the real
Anchor programs: 37 zero-tolerance quote-math parity tests, direct adapter-to-
Anchor SPL ABI parity across both factions, orientations, and quote-token
programs, plus 64 real-SBF Tax -> AMM SPL CPI tests.

## License

MIT
