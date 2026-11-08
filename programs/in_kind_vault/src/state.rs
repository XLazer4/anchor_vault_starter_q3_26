use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]
pub struct Vault {
    /// can move idle funds into the market and back. can't send them
    /// anywhere else and can't stop anyone from redeeming.
    pub curator: Pubkey,
    pub base_mint: Pubkey,
    /// the one market this vault is allowed to supply into, fixed at init
    pub market: Pubkey,
    /// receipt token the market hands out for supplied liquidity
    pub receipt_mint: Pubkey,
    pub share_mint: Pubkey,
    /// vault's base token account (liquid)
    pub idle: Pubkey,
    /// vault's receipt token account (the market position)
    pub position: Pubkey,
    pub bump: u8,
    pub share_mint_bump: u8,
}
