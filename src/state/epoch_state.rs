// Raw byte parser for EpochState account data.
//
// Extracts tax rates from on-chain EpochState without anchor-lang dependency.
// Byte offsets verified against programs/tax-program/src/state/epoch_state_reader.rs.
//
// EpochState byte layout (172 bytes total with 8-byte discriminator):
//   [0..8]     Anchor discriminator (sha256("account:EpochState")[0..8])
//   [8..16]    genesis_slot (u64)
//   [16..20]   current_epoch (u32)
//   [20..28]   epoch_start_slot (u64)
//   [28]       cheap_side (u8)
//   [29..31]   low_tax_bps (u16)
//   [31..33]   high_tax_bps (u16)
//   [33..35]   crime_buy_tax_bps (u16)
//   [35..37]   crime_sell_tax_bps (u16)
//   [37..39]   fraud_buy_tax_bps (u16)
//   [39..41]   fraud_sell_tax_bps (u16)
//   ... remaining fields not needed for quoting

use anyhow::{anyhow, Result};
use sha2::{Sha256, Digest};

use crate::constants::{EPOCH_STATE_DISCRIMINATOR, TRANSITION_IN_PROGRESS_OFFSET};

/// Minimum account data length for EpochState (8 discriminator + 164 data).
const MIN_LEN: usize = 172;

/// Parsed EpochState -- only the fields needed for Jupiter quoting.
#[derive(Debug, Clone, Copy)]
pub struct ParsedEpochState {
    pub crime_buy_tax_bps: u16,
    pub crime_sell_tax_bps: u16,
    pub fraud_buy_tax_bps: u16,
    pub fraud_sell_tax_bps: u16,
    /// Whether an epoch transition window is open (byte 106). While true, the
    /// AMM's Layer-3 gate reverts public swaps with TransitionInProgress
    /// (6019); quotes should be refused so the router never routes into a
    /// known-closed window. False on pre-gate deployments (reserved padding).
    pub transition_in_progress: bool,
}

impl ParsedEpochState {
    /// Parse EpochState from raw account data bytes.
    ///
    /// Validates:
    /// - Minimum length (172 bytes)
    /// - Anchor discriminator (sha256("account:EpochState")[0..8])
    ///
    /// Extracts u16 LE fields at proven offsets.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < MIN_LEN {
            return Err(anyhow!(
                "EpochState data too short: {} bytes (need {})",
                data.len(),
                MIN_LEN
            ));
        }

        // Validate Anchor discriminator
        let disc = &data[0..8];
        if disc != EPOCH_STATE_DISCRIMINATOR {
            return Err(anyhow!(
                "EpochState discriminator mismatch: expected {:?}, got {:?}",
                EPOCH_STATE_DISCRIMINATOR,
                disc
            ));
        }

        Ok(Self {
            crime_buy_tax_bps: u16::from_le_bytes([data[33], data[34]]),
            crime_sell_tax_bps: u16::from_le_bytes([data[35], data[36]]),
            fraud_buy_tax_bps: u16::from_le_bytes([data[37], data[38]]),
            fraud_sell_tax_bps: u16::from_le_bytes([data[39], data[40]]),
            transition_in_progress: data[TRANSITION_IN_PROGRESS_OFFSET] != 0,
        })
    }

    /// Get the appropriate tax rate for a swap operation.
    ///
    /// # Arguments
    /// * `is_crime` - true for CRIME token, false for FRAUD token
    /// * `is_buy` - true for buy direction, false for sell direction
    pub fn get_tax_bps(&self, is_crime: bool, is_buy: bool) -> u16 {
        match (is_crime, is_buy) {
            (true, true) => self.crime_buy_tax_bps,
            (true, false) => self.crime_sell_tax_bps,
            (false, true) => self.fraud_buy_tax_bps,
            (false, false) => self.fraud_sell_tax_bps,
        }
    }
}

/// Compute the Anchor discriminator for EpochState at runtime.
/// This is used to verify the constant EPOCH_STATE_DISCRIMINATOR is correct.
pub fn compute_epoch_state_discriminator() -> [u8; 8] {
    let mut hasher = Sha256::new();
    hasher.update(b"account:EpochState");
    let hash = hasher.finalize();
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&hash[..8]);
    disc
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a mock EpochState byte array with known tax values.
    fn mock_epoch_state(
        crime_buy: u16,
        crime_sell: u16,
        fraud_buy: u16,
        fraud_sell: u16,
    ) -> Vec<u8> {
        let mut data = vec![0u8; MIN_LEN];

        // Set discriminator
        data[0..8].copy_from_slice(&EPOCH_STATE_DISCRIMINATOR);

        // Set tax rates at their offsets
        data[33..35].copy_from_slice(&crime_buy.to_le_bytes());
        data[35..37].copy_from_slice(&crime_sell.to_le_bytes());
        data[37..39].copy_from_slice(&fraud_buy.to_le_bytes());
        data[39..41].copy_from_slice(&fraud_sell.to_le_bytes());

        data
    }

    #[test]
    fn discriminator_matches_computed() {
        let computed = compute_epoch_state_discriminator();
        assert_eq!(computed, EPOCH_STATE_DISCRIMINATOR);
    }

    #[test]
    fn parse_known_tax_rates() {
        let data = mock_epoch_state(400, 1400, 1400, 400);
        let parsed = ParsedEpochState::from_bytes(&data).unwrap();

        assert_eq!(parsed.crime_buy_tax_bps, 400);
        assert_eq!(parsed.crime_sell_tax_bps, 1400);
        assert_eq!(parsed.fraud_buy_tax_bps, 1400);
        assert_eq!(parsed.fraud_sell_tax_bps, 400);
    }

    #[test]
    fn get_tax_bps_all_directions() {
        let data = mock_epoch_state(300, 1200, 1500, 500);
        let parsed = ParsedEpochState::from_bytes(&data).unwrap();

        assert_eq!(parsed.get_tax_bps(true, true), 300);   // CRIME buy
        assert_eq!(parsed.get_tax_bps(true, false), 1200);  // CRIME sell
        assert_eq!(parsed.get_tax_bps(false, true), 1500);  // FRAUD buy
        assert_eq!(parsed.get_tax_bps(false, false), 500);  // FRAUD sell
    }

    #[test]
    fn reject_too_short() {
        let data = vec![0u8; 100];
        assert!(ParsedEpochState::from_bytes(&data).is_err());
    }

    #[test]
    fn transition_flag_reads_exactly_byte_106() {
        // Offset-pin: only byte 106 controls the flag. Adjacent bytes set
        // with 106 clear must read false; 106 set must read true.
        let mut data = mock_epoch_state(100, 1100, 1100, 100);
        data[105] = 1;
        data[107] = 1;
        assert!(!ParsedEpochState::from_bytes(&data).unwrap().transition_in_progress);

        data[106] = 1;
        assert!(ParsedEpochState::from_bytes(&data).unwrap().transition_in_progress);

        // Any non-zero value counts as open (mirrors the on-chain `!= 0`).
        data[106] = 0xFF;
        assert!(ParsedEpochState::from_bytes(&data).unwrap().transition_in_progress);
    }

    #[test]
    fn parse_gate_active_devnet_snapshot() {
        // Real EpochState from the gate-ACTIVE devnet deployment
        // (9cUnCnKEMgfvVK1dqhVDDwriurM9ULVtkfiEAX855WVi, fetched 2026-07-18,
        // epoch 8507): proves the tax offsets survived the transition-state
        // upgrade (fields were carved from reserved padding; account size
        // unchanged at 172) and the flag parses from live gate-era data.
        const DEVNET_HEX: &str = "bf3f8bed900cdfd2f1fe0f1c000000003b210000cb59711c00000000002c01b0042c01b004b0042c01185a711c000000000001c0ae648ed3c682e04aed6ee79d5b7b262611dd74cadccf929cd9425e0d33ff93000100093e711c000000000f3d711c00000000312100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001ff";
        let data: Vec<u8> = (0..DEVNET_HEX.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&DEVNET_HEX[i..i + 2], 16).unwrap())
            .collect();
        assert_eq!(data.len(), 172);

        let parsed = ParsedEpochState::from_bytes(&data).unwrap();
        assert_eq!(parsed.crime_buy_tax_bps, 300);
        assert_eq!(parsed.crime_sell_tax_bps, 1200);
        assert_eq!(parsed.fraud_buy_tax_bps, 1200);
        assert_eq!(parsed.fraud_sell_tax_bps, 300);
        assert!(!parsed.transition_in_progress, "snapshot taken between transitions");
    }

    #[test]
    fn reject_bad_discriminator() {
        let mut data = vec![0u8; MIN_LEN];
        data[0..8].copy_from_slice(&[0xFF; 8]);
        assert!(ParsedEpochState::from_bytes(&data).is_err());
    }
}
