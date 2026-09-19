//! Jupiter AMM adapter SDK for the Dr. Fraudsworth DEX protocol.
//!
//! This crate provides the `jupiter-amm-interface::Amm` trait implementation
//! that enables Jupiter to route swaps through Dr. Fraudsworth's on-chain
//! programs (AMM, Tax, Conversion Vault).
//!
//! Architecture:
//! - `math` - Pure swap/tax/vault math functions (exact copies of on-chain logic)
//! - `state` - Raw byte parsers for on-chain account state (no anchor-lang dep)
//! - `accounts` - Protocol addresses and generic SOL/SPL account-meta builders
//! - `constants` - Protocol constants (fees, rates, decimals)

pub mod accounts;
pub mod constants;
pub mod instruction;
pub mod math;
pub mod sol_pool_amm;
pub mod state;
pub mod vault_amm;

// Re-export primary types and factory functions for convenient access by Jupiter integrators.
pub use sol_pool_amm::SolPoolAmm;
/// Compatibility-neutral name for the generic faction/quote pool adapter.
pub type FactionPoolAmm = SolPoolAmm;
pub use vault_amm::{all_pool_keys, known_instances, known_sol_pool_keys, VaultAmm};
