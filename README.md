# Dr. Fraudsworth Jupiter Adapter SDK

[![CI](https://github.com/MetalLegBob/drfraudsworth-jupiter-adapter/actions/workflows/ci.yml/badge.svg)](https://github.com/MetalLegBob/drfraudsworth-jupiter-adapter/actions/workflows/ci.yml)

Jupiter AMM adapter for the Dr. Fraudsworth DEX protocol on Solana. Implements the `jupiter-amm-interface::Amm` trait so Jupiter's routing engine can route swaps through Dr. Fraudsworth's on-chain programs.

This repository contains the standalone adapter crate. The on-chain programs, Anchor IDLs, and the math-parity test suite (SDK quotes proven equal to on-chain outputs) live in the protocol repository: [github.com/MetalLegBob/drfraudsworth](https://github.com/MetalLegBob/drfraudsworth).

**Key properties:**

- Exact quote accuracy — the SDK's math modules are copies of the on-chain math, proven equal by zero-tolerance parity tests in the protocol repository, and validated here against embedded mainnet account data
- Zero network calls in any method (pool state is parsed from Jupiter-provided account snapshots; all protocol-singleton addresses are hardcoded)
- Supports all 8 swap directions across 6 Amm instances
- Generic pool construction: `SolPoolAmm` derives mints, vaults, and orientation from parsed `PoolState` account data, so future SOL-quoted pools construct without SDK changes

## Pool Types

Dr. Fraudsworth exposes 6 Amm instances to Jupiter:

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
- **CRIME <-> FRAUD direct conversion is not supported on-chain.** Jupiter routes this via multi-hop (CRIME -> PROFIT -> FRAUD).

## Pool Discovery

Jupiter integrators call these factory functions at startup to register all Dr. Fraudsworth pools:

```rust
use drfraudsworth_jupiter_adapter::{known_instances, known_sol_pool_keys, all_pool_keys};

// SOL pools: returns 2 pool PDAs for SolPoolAmm (created via from_keyed_account)
let sol_keys: Vec<Pubkey> = known_sol_pool_keys();

// Vault instances: returns 4 pre-built (Pubkey, VaultAmm) pairs
let vault_instances: Vec<(Pubkey, VaultAmm)> = known_instances();

// All 6 pool keys in one call
let all_keys: Vec<Pubkey> = all_pool_keys();
```

- `known_sol_pool_keys()` -- Returns 2 SOL pool PDAs. Jupiter fetches account data and calls `SolPoolAmm::from_keyed_account()`.
- `known_instances()` -- Returns 4 pre-constructed `VaultAmm` instances (fixed-pool protocol, no `getProgramAccounts` needed).
- `all_pool_keys()` -- Convenience: all 6 instance keys combined.

### Automatic discovery of future pools

`SolPoolAmm::from_keyed_account` is safe to feed arbitrary accounts and constructs generically:

1. Rejects accounts not owned by the AMM program
2. Rejects data without the `PoolState` Anchor discriminator (`sha256("account:PoolState")[0..8]`)
3. Rejects pools that are not SOL-quoted or whose token side is not CRIME/FRAUD
4. Derives mints, vaults, reserves, and orientation entirely from the account bytes

This means the constructor also works with scan-based market discovery (e.g. `getProgramAccounts` on the AMM program filtered by the `PoolState` discriminator): a new SOL-quoted pool created by the protocol would construct with no SDK changes. Note that pool accounts are **owned by the AMM program** while `program_id()` returns the **Tax Program** (the swap entry point) — see Program IDs below.

## Fee Structure

### SOL Pools (CRIME/SOL, FRAUD/SOL)

SOL pool swaps have two fee components:

1. **LP fee:** 1% (100 BPS), fixed, deducted from swap amount
2. **Dynamic tax:** 1-4% (cheap side) or 11-14% (expensive side), VRF-randomized each epoch (~30 min). Tax is split across staking rewards (71%), Carnage Fund (24%), and treasury (5%)

**Buy (SOL -> token):** Tax deducted from SOL input BEFORE the AMM swap.
**Sell (token -> SOL):** Tax deducted from SOL output AFTER the AMM swap.

Tax rates change every epoch (~30 minutes). Jupiter's `update()` method refreshes EpochState to get current rates. Stale rates between quote and execution are handled by on-chain slippage protection (`minimum_output`).

### Vault Conversions

- **Zero fees**, fixed rate conversion
- CRIME/FRAUD -> PROFIT: divide by 100 (100 CRIME = 1 PROFIT)
- PROFIT -> CRIME/FRAUD: multiply by 100 (1 PROFIT = 100 CRIME)

## Epoch Dynamics

Each epoch (~30 minutes), VRF randomness determines:

1. **Which faction is cheap** — 75% chance of flipping each epoch
2. **Exact tax magnitudes** — independently randomized per token from discrete sets

| Side | Buy Tax | Sell Tax |
|------|---------|----------|
| Cheap | 1%, 2%, 3%, or 4% | 11%, 12%, 13%, or 14% |
| Expensive | 11%, 12%, 13%, or 14% | 1%, 2%, 3%, or 4% |

CRIME and FRAUD get **independent magnitude rolls** — e.g., CRIME cheap buy could be 2% while FRAUD expensive buy is 13%. No intermediate values exist (only the 8 discrete rates above).

This creates arbitrage opportunities between the two pools that Jupiter can route through.

The `EpochState` PDA is declared in `get_accounts_to_update()`, so Jupiter automatically refreshes it and passes the latest state to `update()`.

## Account Metas

Each instruction type requires a specific set of accounts. Pool-specific accounts (pool PDA, mints, vaults, orientation) are derived from the parsed `PoolState`; protocol singletons (authorities, staking, treasury, programs) are hardcoded mainnet addresses. Zero network calls.

### SwapSolBuy (SOL -> CRIME/FRAUD)

20 named accounts + 4 transfer hook accounts = **24 total**

Named accounts: user, epoch_state, swap_authority, tax_authority, pool, pool_vault_a, pool_vault_b, mint_a (WSOL), mint_b (token), user_token_a, user_token_b, stake_pool, staking_escrow, carnage_vault, treasury, amm_program, token_program_a (SPL Token), token_program_b (Token-2022), system_program, staking_program.

### SwapSolSell (CRIME/FRAUD -> SOL)

21 named accounts + 4 transfer hook accounts = **25 total**

Same as buy, plus `wsol_intermediary` PDA (account #16). The sell path routes SOL through an intermediary WSOL account before closing it back to the user.

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
    }).unwrap();
    println!("{}: {} -> {}", key, quote.in_amount, quote.out_amount);
}
```

See `examples/quote_example.rs` for a complete working example:

```bash
cargo run --example quote_example
```

## Interface Version

The crate declares `jupiter-amm-interface = "0.6"` and the committed `Cargo.lock` pins 0.6.0, matching [jup-ag/rust-amm-implementation](https://github.com/jup-ag/rust-amm-implementation). The library never constructs `QuoteParams`/`SwapParams`, so it compiles unchanged against 0.6.1 as well; note that a fresh resolve of 0.6.1 requires the solana 3.x crate generation (its `solana_account_decoder::encode_ui_account` import does not exist in the 2.x series).

## Program IDs

Jupiter needs to know which programs are called for each swap type:

| Program | Address | Called For |
|---------|---------|-----------|
| Tax Program | `43fZGRtmEsP7ExnJE1dbTbNjaP1ncvVmMPusSeksWGEj` | SOL pool swaps (CPI to AMM internally) |
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

- **Swap variant:** Uses `Swap::TokenSwap` as placeholder. Jupiter assigns the real variant during integration review.
- **Vault instance keying:** Synthetic PDAs derived from `[b"jup_vault", input_mint, output_mint]` via `Pubkey::find_program_address`. These are not real on-chain accounts -- they exist solely to give each VaultAmm instance a unique key.
- **`supports_exact_out`:** Returns `false` for all instances. Integer division in vault conversions loses information, and SOL pool exact-out would require iterative solving.
- **No network calls:** All methods (`quote`, `get_swap_and_account_metas`, `get_accounts_to_update`) operate on Jupiter-provided account snapshots and constants. Jupiter handles account fetching externally.
- **WSOL wrapping:** Jupiter handles SOL <-> WSOL wrapping/unwrapping. The SDK returns only the Tax Program swap instruction.
- **`unidirectional()`:** Returns `true` for VaultAmm, `false` (default) for SolPoolAmm. Jupiter uses this to avoid routing backwards through vault instances.
- **Mint-pair validation:** `quote()` and `get_swap_and_account_metas()` reject requests whose mints do not match the instance's pool.
- **Vault liquidity cap:** `VaultAmm::update()` reads the output-side vault token account balance, and quotes exceeding available liquidity are rejected rather than quoted-then-failed on-chain.
- **`program_dependencies()`:** returns the AMM, Staking, and Transfer Hook programs for SolPool swaps (Transfer Hook for vault conversions) so test harnesses know which dependent programs to load.
- **`underlying_liquidities()`:** the two `*->PROFIT` vault instances report the same underlying PROFIT vault account, exposing their shared liquidity to the routing engine.

## Testing

```bash
# Full standalone suite: unit tests + construction, edge gauntlet,
# instruction structure, mainnet-data validation, extended quoting
cargo test

# Run the quote example
cargo run --example quote_example
```

See [TESTING.md](./TESTING.md) for the full suite breakdown. CI runs the suite plus clippy on every push.

The mainnet-data validation suite parses real (hex-embedded) mainnet account snapshots and includes an equivalence proof that account lists built from parsed on-chain data are byte-identical to the constant-based builders.

Math-parity tests — proving SDK quote math equals on-chain program math with zero tolerance — live in the protocol repository (`sdk/jupiter-adapter/tests/parity_*.rs` there), because they compile against the on-chain program crates directly.

## License

MIT
