use drfraudsworth_jupiter_adapter::{
    accounts::addresses::{CONVERSION_VAULT_PROGRAM_ID, CRIME_MINT, FRAUD_MINT, PROFIT_MINT},
    vault_amm::VaultAmm,
};
use jupiter_amm_interface::{
    Amm, AmmContext, FeeMode, KeyedAccount, QuoteParams, SwapMode, SwapParams,
};
use serde_json::json;
use solana_sdk::{account::Account, pubkey::Pubkey};

fn keyed_vault(input_mint: Pubkey, output_mint: Pubkey) -> KeyedAccount {
    KeyedAccount {
        key: Pubkey::new_unique(),
        account: Account {
            lamports: 0,
            data: Vec::new(),
            owner: CONVERSION_VAULT_PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        },
        params: Some(json!({
            "input_mint": input_mint.to_string(),
            "output_mint": output_mint.to_string(),
        })),
    }
}

#[test]
fn params_accept_only_on_chain_conversion_edges() {
    for (input_mint, output_mint) in [
        (CRIME_MINT, PROFIT_MINT),
        (FRAUD_MINT, PROFIT_MINT),
        (PROFIT_MINT, CRIME_MINT),
        (PROFIT_MINT, FRAUD_MINT),
    ] {
        let keyed = keyed_vault(input_mint, output_mint);
        assert!(VaultAmm::from_keyed_account(&keyed, &AmmContext::default()).is_ok());
    }

    for (input_mint, output_mint) in [
        (CRIME_MINT, FRAUD_MINT),
        (FRAUD_MINT, CRIME_MINT),
        (CRIME_MINT, CRIME_MINT),
        (PROFIT_MINT, PROFIT_MINT),
    ] {
        let keyed = keyed_vault(input_mint, output_mint);
        assert!(VaultAmm::from_keyed_account(&keyed, &AmmContext::default()).is_err());
    }
}

#[test]
fn quote_rejects_wrong_output_mint() {
    let amm = VaultAmm::new_for_testing(CRIME_MINT, PROFIT_MINT);
    let result = amm.quote(&QuoteParams {
        amount: 10_000,
        input_mint: CRIME_MINT,
        output_mint: FRAUD_MINT,
        swap_mode: SwapMode::ExactIn,
        fee_mode: FeeMode::Normal,
    });

    assert!(result.is_err());
}

#[test]
fn account_builder_rejects_wrong_destination_mint() {
    let amm = VaultAmm::new_for_testing(CRIME_MINT, PROFIT_MINT);
    let authority = Pubkey::new_unique();
    let jupiter_program_id = Pubkey::new_unique();
    let result = amm.get_swap_and_account_metas(&SwapParams {
        swap_mode: SwapMode::ExactIn,
        in_amount: 10_000,
        out_amount: 100,
        source_mint: CRIME_MINT,
        destination_mint: FRAUD_MINT,
        source_token_account: Pubkey::new_unique(),
        destination_token_account: Pubkey::new_unique(),
        token_transfer_authority: authority,
        user: authority,
        payer: authority,
        quote_mint_to_referrer: None,
        jupiter_program_id: &jupiter_program_id,
        missing_dynamic_accounts_as_default: false,
    });

    assert!(result.is_err());
}
