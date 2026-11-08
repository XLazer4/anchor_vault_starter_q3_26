use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{Mint, TokenAccount, TokenInterface},
};
use mock_market::Market;

use crate::{
    constants::{SHARES_SEED, VAULT_SEED},
    error::VaultError,
    state::Vault,
};

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub curator: Signer<'info>,

    #[account(mint::token_program = token_program)]
    pub base_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(constraint = market.base_mint == base_mint.key() @ VaultError::MarketMintMismatch)]
    pub market: Box<Account<'info, Market>>,

    #[account(address = market.receipt_mint)]
    pub receipt_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        init,
        payer = curator,
        space = Vault::DISCRIMINATOR.len() + Vault::INIT_SPACE,
        seeds = [VAULT_SEED, curator.key().as_ref(), base_mint.key().as_ref()],
        bump
    )]
    pub vault: Box<Account<'info, Vault>>,

    #[account(
        init,
        payer = curator,
        seeds = [SHARES_SEED, vault.key().as_ref()],
        bump,
        mint::decimals = base_mint.decimals,
        mint::authority = vault,
        mint::token_program = token_program
    )]
    pub share_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        init,
        payer = curator,
        associated_token::mint = base_mint,
        associated_token::authority = vault,
        associated_token::token_program = token_program
    )]
    pub idle: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        init,
        payer = curator,
        associated_token::mint = receipt_mint,
        associated_token::authority = vault,
        associated_token::token_program = token_program
    )]
    pub position: Box<InterfaceAccount<'info, TokenAccount>>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> Initialize<'info> {
    pub fn initialize(&mut self, bumps: &InitializeBumps) -> Result<()> {
        self.vault.set_inner(Vault {
            curator: self.curator.key(),
            base_mint: self.base_mint.key(),
            market: self.market.key(),
            receipt_mint: self.receipt_mint.key(),
            share_mint: self.share_mint.key(),
            idle: self.idle.key(),
            position: self.position.key(),
            bump: bumps.vault,
            share_mint_bump: bumps.share_mint,
        });
        Ok(())
    }
}
