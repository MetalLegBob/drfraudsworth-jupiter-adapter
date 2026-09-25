# Testing

Run everything in this repository with:

```bash
cargo test --locked
```

CI runs locked tests, clippy with warnings denied, formatting, documentation,
and package verification on every push and pull request.

## Suites in this repository (v0.2.0)

| Suite | Tests | What it proves |
|---|---|---|
| Unit tests (`src/`) | 144 | Quote/tax/vault math, exact state parsing, dynamic PoolState discovery, instruction discriminator goldens, SOL/SPL account builders in both orientations, quote-mint transfer policy, pause boundaries, stale-state deactivation, factory functions, and vault balance capping |
| `tests/pool_discovery_properties.rs` | 2 | Arbitrary AMM-owned bytes never panic; every canonical faction-pool shape is discoverable for classic SPL Token and Token-2022 quotes |
| `tests/test_construction.rs` | 18 | Full Jupiter lifecycle: `from_keyed_account` → `update` → `quote` with mock account data, error cases |
| `tests/test_edge_gauntlet.rs` | 29 | Boundary amounts, extreme tax rates, dust, overflow guards |
| `tests/test_instruction_structure.rs` | 15 | Exact account ordering, writability, and signer flags for every instruction |
| `tests/test_mainnet_validation.rs` | 15 | Real mainnet account data (hex-embedded, no RPC): parsing, quoting with live reserves, discriminator match, pause fields, and byte-identical generic/constant SOL account building |
| `tests/live_spl_pool_snapshots.rs` | 1 | All four live SPL pilot PoolStates: generic discovery, both orientations and directions, exact snapshot quotes, Tax lane selection, and native account shapes |
| `tests/test_quoting_extended.rs` | 20 | Quoting properties: monotonicity, parity with reference values, speed |
| `tests/vault_mint_pair.rs` | 4 | Only the four on-chain conversion edges construct; mismatched legs fail while CRIME/FRAUD routes compose through PROFIT |

Mainnet snapshots are hex-embedded at fixed fetch dates (see comments in the
two snapshot suites), keeping the suite deterministic and offline. Pilot pool
addresses are test fixtures, not production admission logic.

## Math-parity suite (private protocol monorepo)

Zero-tolerance proofs that the SDK's quote math equals the on-chain program
math live in the private protocol monorepo, because they compile against the
on-chain program crates directly. The source of the live on-chain programs is
public in the
[protocol repository](https://github.com/MetalLegBob/fantastical-finance-factory);
the parity suite itself is not published, and its results can be provided
during integration review. It covers:

- 37 zero-tolerance SOL-pool and vault quote-math parity tests
- Direct SPL account-meta parity against Anchor-generated Tax account structs
  for both factions, both pool orientations, and both supported quote-token
  programs
- 64 real-SBF Tax -> AMM CPI tests for SPL buy and sell, including pause,
  transfer-hook, sweep, fee, decimal, and orientation coverage
