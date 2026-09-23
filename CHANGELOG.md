# Changelog

All notable changes to this crate are recorded here.

## [Unreleased] - 0.2.0

### Added

- Generic discovery and quoting for AMM-owned SOL and SPL faction pools.
- SPL Tax buy/sell account builders for either PoolState orientation.
- Pause-aware routing and amount-preserving Token-2022 quote-mint validation.
- Stable Tax lane discriminator and argument encoding helpers.
- Exact and delta-mode Conversion Vault `convert_v2` encoding helpers.
- Deterministic coverage of all four live SPL pilot PoolStates in both swap
  directions and both pool orientations.
- Six mainnet IDLs synchronized byte-for-byte with the reviewed protocol
  release manifest.

### Changed

- Updated `jupiter-amm-interface` from 0.6.0 to 0.6.1.
- Renamed public descriptions from SOL-specific to quote-neutral terminology;
  `SolPoolAmm` remains the compatibility type name.
- Clarified the public protocol-source, release-provenance and independent-audit
  status without overstating the available security evidence.

### Fixed

- Reject unsupported or mismatched directed conversion-vault mint pairs during
  construction, quoting and account-meta generation. Cross-faction routing is
  unchanged: CRIME/FRAUD routes compose through PROFIT as two valid vault legs.
- Fail closed for non-transferable, default-frozen or currently paused
  Token-2022 quote mints.
- Encode the required trailing `is_crime` argument for SOL Tax lanes while
  preserving the two-argument SPL Tax layout.

## [0.1.5] - 2026-07-18

- Added transition-gate-aware quoting for the original SOL pools.
