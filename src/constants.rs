// Protocol constants for the Dr. Fraudsworth Jupiter adapter.

/// LP fee in basis points (1%).
pub const LP_FEE_BPS: u16 = 100;

/// Conversion rate for the vault (100:1 CRIME/FRAUD:PROFIT).
pub const CONVERSION_RATE: u64 = 100;

/// Token decimals for all Dr. Fraudsworth tokens (CRIME, FRAUD, PROFIT).
pub const TOKEN_DECIMALS: u8 = 6;

/// SOL decimals (native mint).
pub const SOL_DECIMALS: u8 = 9;

/// Anchor discriminator for EpochState account.
///
/// Computed as: sha256("account:EpochState")[0..8]
///
/// Verified at compile time by state::epoch_state::compute_epoch_state_discriminator()
/// test. If the on-chain struct name changes, this must be updated.
///
/// Known value (hex): bf 3f 8b ed 90 0c df d2
pub const EPOCH_STATE_DISCRIMINATOR: [u8; 8] = [0xbf, 0x3f, 0x8b, 0xed, 0x90, 0x0c, 0xdf, 0xd2];

/// Anchor discriminator for PoolState account.
///
/// Computed as: sha256("account:PoolState")[0..8]
///
/// Verified by state::pool_state::pool_state_discriminator_matches_sha256()
/// test and against embedded mainnet account data. If the on-chain struct
/// name changes, this must be updated.
///
/// Known value (hex): f7 ed e3 f5 d7 c3 de 46
pub const POOL_STATE_DISCRIMINATOR: [u8; 8] = [0xf7, 0xed, 0xe3, 0xf5, 0xd7, 0xc3, 0xde, 0x46];

/// Absolute byte offset of `EpochState.transition_in_progress` (8-byte Anchor
/// discriminator + 98 bytes of preceding fields).
///
/// Mirrors the AMM's Layer-3 transition gate (`transition_gate.rs`
/// `TRANSITION_OFFSET = 106` on-chain), which reverts reserve-mutating swaps
/// with `TransitionInProgress` (6019) while this byte is non-zero — the
/// window in which the protocol's internal arb executes an epoch flip.
///
/// On deployments without the gate feature this byte sits inside zeroed
/// reserved padding, so the flag reads false and gate-aware quoting is a
/// no-op. Verified against live devnet (gate-active) and mainnet (pre-gate)
/// account data on 2026-07-18; the EpochState account size (172 bytes) is
/// identical on both.
pub const TRANSITION_IN_PROGRESS_OFFSET: usize = 106;
