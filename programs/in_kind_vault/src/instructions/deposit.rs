use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{
        mint_to, transfer_checked, Mint, MintTo, TokenAccount, TokenInterface, TransferChecked,
    },
};

use crate::{constants::VAULT_SEED, error::VaultError, math::shares_for_deposit, state::Vault};

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(has_one = base_mint, has_one = share_mint)]
    pub vault: Box<Account<'info, Vault>>,

    pub base_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut)]
    pub share_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut, address = vault.idle)]
    pub idle: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(address = vault.position)]
    pub position: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = base_mint,
        token::authority = user,
        token::token_program = token_program
    )]
    pub user_base: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = share_mint,
        associated_token::authority = user,
        associated_token::token_program = token_program
    )]
    pub user_shares: Box<InterfaceAccount<'info, TokenAccount>>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> Deposit<'info> {
    pub fn deposit(&mut self, amount: u64) -> Result<()> {
        require!(amount > 0, VaultError::InvalidAmount);

        // receipts are 1:1 with base in the mock market, so the position is
        // worth exactly its token amount
        let total_assets = self
            .idle
            .amount
            .checked_add(self.position.amount)
            .ok_or(VaultError::MathOverflow)?;
        let shares = shares_for_deposit(amount, total_assets, self.share_mint.supply)?;
        require!(shares > 0, VaultError::ZeroShares);

        transfer_checked(
            CpiContext::new(
                self.token_program.key(),
                TransferChecked {
                    from: self.user_base.to_account_info(),
                    mint: self.base_mint.to_account_info(),
                    to: self.idle.to_account_info(),
                    authority: self.user.to_account_info(),
                },
            ),
            amount,
            self.base_mint.decimals,
        )?;

        let curator = self.vault.curator;
        let base_mint = self.vault.base_mint;
        let seeds: &[&[&[u8]]] = &[&[
            VAULT_SEED,
            curator.as_ref(),
            base_mint.as_ref(),
            &[self.vault.bump],
        ]];
        mint_to(
            CpiContext::new_with_signer(
                self.token_program.key(),
                MintTo {
                    mint: self.share_mint.to_account_info(),
                    to: self.user_shares.to_account_info(),
                    authority: self.vault.to_account_info(),
                },
                seeds,
            ),
            shares,
        )
    }
}
