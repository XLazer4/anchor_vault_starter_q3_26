use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{
        burn, transfer_checked, Burn, Mint, TokenAccount, TokenInterface, TransferChecked,
    },
};

use crate::{constants::VAULT_SEED, error::VaultError, math::pro_rata, state::Vault};

/// Plain redeem: shares -> base, paid out of idle. Only works while the
/// vault has enough idle funds.
#[derive(Accounts)]
pub struct Redeem<'info> {
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
        token::mint = share_mint,
        token::authority = user,
        token::token_program = token_program
    )]
    pub user_shares: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = base_mint,
        associated_token::authority = user,
        associated_token::token_program = token_program
    )]
    pub user_base: Box<InterfaceAccount<'info, TokenAccount>>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> Redeem<'info> {
    pub fn redeem(&mut self, shares: u64) -> Result<()> {
        require!(shares > 0, VaultError::InvalidAmount);

        let total_assets = self
            .idle
            .amount
            .checked_add(self.position.amount)
            .ok_or(VaultError::MathOverflow)?;
        let out = pro_rata(total_assets, shares, self.share_mint.supply)?;
        require!(out > 0, VaultError::NothingToRedeem);
        require!(out <= self.idle.amount, VaultError::NotEnoughIdle);

        burn(
            CpiContext::new(
                self.token_program.key(),
                Burn {
                    mint: self.share_mint.to_account_info(),
                    from: self.user_shares.to_account_info(),
                    authority: self.user.to_account_info(),
                },
            ),
            shares,
        )?;

        let curator = self.vault.curator;
        let base_mint = self.vault.base_mint;
        let seeds: &[&[&[u8]]] = &[&[
            VAULT_SEED,
            curator.as_ref(),
            base_mint.as_ref(),
            &[self.vault.bump],
        ]];
        transfer_checked(
            CpiContext::new_with_signer(
                self.token_program.key(),
                TransferChecked {
                    from: self.idle.to_account_info(),
                    mint: self.base_mint.to_account_info(),
                    to: self.user_base.to_account_info(),
                    authority: self.vault.to_account_info(),
                },
                seeds,
            ),
            out,
            self.base_mint.decimals,
        )
    }
}

/// In-kind redeem: shares -> a slice of everything the vault holds. The user
/// gets `shares / supply` of the idle base and the same fraction of the
/// market receipts. Doesn't touch the market at all, so it works even when
/// the market has zero liquidity, and nobody (curator included) can block it.
#[derive(Accounts)]
pub struct RedeemInKind<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(has_one = base_mint, has_one = share_mint, has_one = receipt_mint)]
    pub vault: Box<Account<'info, Vault>>,

    pub base_mint: Box<InterfaceAccount<'info, Mint>>,

    pub receipt_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut)]
    pub share_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut, address = vault.idle)]
    pub idle: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut, address = vault.position)]
    pub position: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = share_mint,
        token::authority = user,
        token::token_program = token_program
    )]
    pub user_shares: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = base_mint,
        associated_token::authority = user,
        associated_token::token_program = token_program
    )]
    pub user_base: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = receipt_mint,
        associated_token::authority = user,
        associated_token::token_program = token_program
    )]
    pub user_receipt: Box<InterfaceAccount<'info, TokenAccount>>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> RedeemInKind<'info> {
    pub fn redeem_in_kind(&mut self, shares: u64) -> Result<()> {
        require!(shares > 0, VaultError::InvalidAmount);

        // both computed against the supply before the burn
        let supply = self.share_mint.supply;
        let base_out = pro_rata(self.idle.amount, shares, supply)?;
        let receipt_out = pro_rata(self.position.amount, shares, supply)?;
        require!(base_out > 0 || receipt_out > 0, VaultError::NothingToRedeem);

        burn(
            CpiContext::new(
                self.token_program.key(),
                Burn {
                    mint: self.share_mint.to_account_info(),
                    from: self.user_shares.to_account_info(),
                    authority: self.user.to_account_info(),
                },
            ),
            shares,
        )?;

        let curator = self.vault.curator;
        let base_mint = self.vault.base_mint;
        let seeds: &[&[&[u8]]] = &[&[
            VAULT_SEED,
            curator.as_ref(),
            base_mint.as_ref(),
            &[self.vault.bump],
        ]];

        if base_out > 0 {
            transfer_checked(
                CpiContext::new_with_signer(
                    self.token_program.key(),
                    TransferChecked {
                        from: self.idle.to_account_info(),
                        mint: self.base_mint.to_account_info(),
                        to: self.user_base.to_account_info(),
                        authority: self.vault.to_account_info(),
                    },
                    seeds,
                ),
                base_out,
                self.base_mint.decimals,
            )?;
        }

        if receipt_out > 0 {
            transfer_checked(
                CpiContext::new_with_signer(
                    self.token_program.key(),
                    TransferChecked {
                        from: self.position.to_account_info(),
                        mint: self.receipt_mint.to_account_info(),
                        to: self.user_receipt.to_account_info(),
                        authority: self.vault.to_account_info(),
                    },
                    seeds,
                ),
                receipt_out,
                self.receipt_mint.decimals,
            )?;
        }

        Ok(())
    }
}
