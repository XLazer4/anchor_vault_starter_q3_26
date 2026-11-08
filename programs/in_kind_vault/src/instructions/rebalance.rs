use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};
use mock_market::{program::MockMarket, Market};

use crate::{constants::VAULT_SEED, error::VaultError, state::Vault};

/// Shared by allocate and deallocate. The curator can only move funds
/// between the vault's own idle account and its own position in the one
/// market fixed at init, so there's no path for them to take anything.
#[derive(Accounts)]
pub struct Rebalance<'info> {
    pub curator: Signer<'info>,

    #[account(
        has_one = curator,
        has_one = market,
        has_one = base_mint,
        has_one = receipt_mint
    )]
    pub vault: Box<Account<'info, Vault>>,

    pub market: Box<Account<'info, Market>>,

    pub base_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut)]
    pub receipt_mint: Box<InterfaceAccount<'info, Mint>>,

    /// CHECK: the market program checks this is its liquidity account
    #[account(mut)]
    pub liquidity: UncheckedAccount<'info>,

    #[account(mut, address = vault.idle)]
    pub idle: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut, address = vault.position)]
    pub position: Box<InterfaceAccount<'info, TokenAccount>>,

    pub market_program: Program<'info, MockMarket>,
    pub token_program: Interface<'info, TokenInterface>,
}

impl<'info> Rebalance<'info> {
    /// idle base -> market, vault gets receipts
    pub fn allocate(&mut self, amount: u64) -> Result<()> {
        require!(amount > 0, VaultError::InvalidAmount);

        let curator = self.vault.curator;
        let base_mint = self.vault.base_mint;
        let seeds: &[&[&[u8]]] = &[&[
            VAULT_SEED,
            curator.as_ref(),
            base_mint.as_ref(),
            &[self.vault.bump],
        ]];

        let accounts = mock_market::cpi::accounts::Supply {
            user: self.vault.to_account_info(),
            market: self.market.to_account_info(),
            base_mint: self.base_mint.to_account_info(),
            receipt_mint: self.receipt_mint.to_account_info(),
            liquidity: self.liquidity.to_account_info(),
            user_base: self.idle.to_account_info(),
            user_receipt: self.position.to_account_info(),
            token_program: self.token_program.to_account_info(),
        };
        mock_market::cpi::supply(
            CpiContext::new_with_signer(self.market_program.key(), accounts, seeds),
            amount,
        )
    }

    /// receipts -> market, vault gets base back. fails if the market is
    /// out of liquidity, which is exactly when users need redeem_in_kind.
    pub fn deallocate(&mut self, amount: u64) -> Result<()> {
        require!(amount > 0, VaultError::InvalidAmount);

        let curator = self.vault.curator;
        let base_mint = self.vault.base_mint;
        let seeds: &[&[&[u8]]] = &[&[
            VAULT_SEED,
            curator.as_ref(),
            base_mint.as_ref(),
            &[self.vault.bump],
        ]];

        let accounts = mock_market::cpi::accounts::Withdraw {
            user: self.vault.to_account_info(),
            market: self.market.to_account_info(),
            base_mint: self.base_mint.to_account_info(),
            receipt_mint: self.receipt_mint.to_account_info(),
            liquidity: self.liquidity.to_account_info(),
            user_base: self.idle.to_account_info(),
            user_receipt: self.position.to_account_info(),
            token_program: self.token_program.to_account_info(),
        };
        mock_market::cpi::withdraw(
            CpiContext::new_with_signer(self.market_program.key(), accounts, seeds),
            amount,
        )
    }
}
