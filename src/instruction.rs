//! Stable Tax instruction encoding contract used by Jupiter's route encoder.

use crate::accounts::addresses::NATIVE_MINT;
use crate::constants::{
    SWAP_SOL_BUY_DISCRIMINATOR, SWAP_SOL_SELL_DISCRIMINATOR, SWAP_SPL_BUY_DISCRIMINATOR,
    SWAP_SPL_SELL_DISCRIMINATOR,
};
use solana_sdk::pubkey::Pubkey;

/// Tax entrypoint selected from quote type and swap direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaxSwapLane {
    SolBuy,
    SolSell,
    SplBuy,
    SplSell,
}

impl TaxSwapLane {
    /// Select a Tax entrypoint from the adapter's quote/faction identity and
    /// the requested ordered pair. Invalid or reversed-identity pairs return
    /// `None` rather than guessing from account count.
    pub fn for_mints(
        quote_mint: &Pubkey,
        faction_mint: &Pubkey,
        input_mint: &Pubkey,
        output_mint: &Pubkey,
    ) -> Option<Self> {
        let is_sol = *quote_mint == NATIVE_MINT;
        match (*input_mint == *quote_mint, *output_mint == *faction_mint) {
            (true, true) => Some(if is_sol { Self::SolBuy } else { Self::SplBuy }),
            (false, false) if *input_mint == *faction_mint && *output_mint == *quote_mint => {
                Some(if is_sol { Self::SolSell } else { Self::SplSell })
            }
            _ => None,
        }
    }

    pub const fn discriminator(self) -> [u8; 8] {
        match self {
            Self::SolBuy => SWAP_SOL_BUY_DISCRIMINATOR,
            Self::SolSell => SWAP_SOL_SELL_DISCRIMINATOR,
            Self::SplBuy => SWAP_SPL_BUY_DISCRIMINATOR,
            Self::SplSell => SWAP_SPL_SELL_DISCRIMINATOR,
        }
    }

    /// Encode the selected Anchor Tax instruction. SOL lanes include the
    /// `is_crime` argument; SPL lanes derive faction identity from PoolState.
    pub fn encode(self, amount_in: u64, minimum_output: u64, is_crime: bool) -> Vec<u8> {
        let mut data = Vec::with_capacity(if matches!(self, Self::SolBuy | Self::SolSell) {
            25
        } else {
            24
        });
        data.extend_from_slice(&self.discriminator());
        data.extend_from_slice(&amount_in.to_le_bytes());
        data.extend_from_slice(&minimum_output.to_le_bytes());
        if matches!(self, Self::SolBuy | Self::SolSell) {
            data.push(u8::from(is_crime));
        }
        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    const VECTORS: [(TaxSwapLane, &str, [u8; 8]); 4] = [
        (
            TaxSwapLane::SolBuy,
            "swap_sol_buy",
            [158, 213, 169, 65, 11, 116, 176, 25],
        ),
        (
            TaxSwapLane::SolSell,
            "swap_sol_sell",
            [136, 242, 218, 149, 17, 222, 250, 240],
        ),
        (
            TaxSwapLane::SplBuy,
            "swap_spl_buy",
            [145, 254, 202, 19, 150, 31, 185, 146],
        ),
        (
            TaxSwapLane::SplSell,
            "swap_spl_sell",
            [250, 17, 196, 68, 180, 202, 218, 155],
        ),
    ];

    #[test]
    fn discriminators_match_anchor_preimages() {
        for (lane, name, expected) in VECTORS {
            let hash = Sha256::digest(format!("global:{name}"));
            assert_eq!(lane.discriminator(), expected);
            assert_eq!(lane.discriminator(), hash[..8]);
        }
    }

    #[test]
    fn instruction_data_has_stable_argument_order() {
        let amount_in = 0x0102_0304_0506_0708;
        let minimum_output = 0x1112_1314_1516_1718;
        for (lane, _, discriminator) in VECTORS {
            let encoded = lane.encode(amount_in, minimum_output, true);
            assert_eq!(&encoded[..8], &discriminator);
            assert_eq!(&encoded[8..16], &amount_in.to_le_bytes());
            assert_eq!(&encoded[16..24], &minimum_output.to_le_bytes());
            if matches!(lane, TaxSwapLane::SolBuy | TaxSwapLane::SolSell) {
                assert_eq!(&encoded[24..], &[1]);
            } else {
                assert_eq!(encoded.len(), 24);
            }
        }
    }

    #[test]
    fn sol_lane_encodes_faction_boolean() {
        assert_eq!(TaxSwapLane::SolBuy.encode(1, 2, true)[24], 1);
        assert_eq!(TaxSwapLane::SolSell.encode(1, 2, false)[24], 0);
    }

    #[test]
    fn lane_selection_is_explicit_for_sol_spl_and_direction() {
        let faction = Pubkey::new_unique();
        let spl_quote = Pubkey::new_unique();
        assert_eq!(
            TaxSwapLane::for_mints(&NATIVE_MINT, &faction, &NATIVE_MINT, &faction),
            Some(TaxSwapLane::SolBuy)
        );
        assert_eq!(
            TaxSwapLane::for_mints(&NATIVE_MINT, &faction, &faction, &NATIVE_MINT),
            Some(TaxSwapLane::SolSell)
        );
        assert_eq!(
            TaxSwapLane::for_mints(&spl_quote, &faction, &spl_quote, &faction),
            Some(TaxSwapLane::SplBuy)
        );
        assert_eq!(
            TaxSwapLane::for_mints(&spl_quote, &faction, &faction, &spl_quote),
            Some(TaxSwapLane::SplSell)
        );
        assert_eq!(
            TaxSwapLane::for_mints(&spl_quote, &faction, &faction, &faction),
            None
        );
    }
}
