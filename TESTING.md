# Testing

Run everything in this repository with:

```bash
cargo test
```

CI runs the full suite plus `cargo clippy -- -D warnings` on every push and pull request.

## Suites in this repository (v0.1.5: 224 tests)

| Suite | Tests | What it proves |
|---|---|---|
| Unit tests (`src/`) | 127 | Quote math, tax math, vault math, state parsers (incl. discriminator validation and the transition-gate flag with an offset-pin test), account-meta builders (both mint orientations), factory functions, `from_keyed_account` shape validation, vault balance capping, gate-aware quote refusal |
| `tests/test_construction.rs` | 18 | Full Jupiter lifecycle: `from_keyed_account` → `update` → `quote` with mock account data, error cases |
| `tests/test_edge_gauntlet.rs` | 29 | Boundary amounts, extreme tax rates, dust, overflow guards |
| `tests/test_instruction_structure.rs` | 15 | Exact account ordering, writability, and signer flags for every instruction |
| `tests/test_mainnet_validation.rs` | 15 | Real mainnet account data (hex-embedded, no RPC): parsing, quoting with live reserves, discriminator match, a byte-identical equivalence proof between generic (parsed-data) and constant-based account building, and the pre-gate transition-flag zero proof |
| `tests/test_quoting_extended.rs` | 20 | Quoting properties: monotonicity, parity with reference values, speed |

Mainnet snapshots are hex-embedded at fixed fetch dates (see comments in
`test_mainnet_validation.rs`), keeping the suite deterministic and offline —
no network calls anywhere, matching Jupiter's integration requirements.

## Math-parity suite (protocol repository)

Zero-tolerance proofs that the SDK's quote math equals the on-chain program
math live in the protocol monorepo
([github.com/MetalLegBob/drfraudsworth](https://github.com/MetalLegBob/drfraudsworth),
`sdk/jupiter-adapter/tests/parity_*.rs`), because they compile against the
on-chain program crates directly (37 tests: SOL-pool swap parity and vault
conversion parity).
