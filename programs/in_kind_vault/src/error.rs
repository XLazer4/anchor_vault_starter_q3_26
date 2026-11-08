use anchor_lang::prelude::*;

#[error_code]
pub enum VaultError {
    #[msg("Amount must be greater than zero")]
    InvalidAmount,
    #[msg("Market is for a different base mint")]
    MarketMintMismatch,
    #[msg("Deposit is too small to mint any shares")]
    ZeroShares,
    #[msg("Math overflow")]
    MathOverflow,
    #[msg("Not enough idle funds for a plain redeem, use redeem_in_kind")]
    NotEnoughIdle,
    #[msg("Too few shares to get anything back")]
    NothingToRedeem,
}
