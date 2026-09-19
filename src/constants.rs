// Protocol constants for the Dr. Fraudsworth Jupiter adapter.

/// LP fee in basis points (1%).
pub const LP_FEE_BPS: u16 = 100;

/// Conversion rate for the vault (100:1 CRIME/FRAUD:PROFIT).
pub const CONVERSION_RATE: u64 = 100;

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

/// Absolute byte offsets of the upgraded EpochState pause fields.
///
/// Byte 106 is no longer a boolean transition flag: it is the first byte of
/// the little-endian `pause_end_slot: u64` field. Treating it as a boolean
/// makes route availability depend on the low byte of a slot number. Public
/// swaps are allowed only when `trading_paused == false` and the shared
/// Jupiter clock is at or beyond `pause_end_slot` (inclusive).
pub const PAUSE_END_SLOT_OFFSET: usize = 106;
pub const TRADING_PAUSED_OFFSET: usize = 114;
pub const EPOCH_INITIALIZED_OFFSET: usize = 170;

/// Anchor instruction discriminators for the Tax swap lanes.
/// The pinned Jupiter interface still returns `Swap::TokenSwap`; the paused
/// integration must map the selected lane to these bytes when serializing the
/// Tax instruction rather than guessing from account count.
pub const SWAP_SOL_BUY_DISCRIMINATOR: [u8; 8] = [158, 213, 169, 65, 11, 116, 176, 25];
pub const SWAP_SOL_SELL_DISCRIMINATOR: [u8; 8] = [136, 242, 218, 149, 17, 222, 250, 240];
pub const SWAP_SPL_BUY_DISCRIMINATOR: [u8; 8] = [145, 254, 202, 19, 150, 31, 185, 146];
pub const SWAP_SPL_SELL_DISCRIMINATOR: [u8; 8] = [250, 17, 196, 68, 180, 202, 218, 155];
