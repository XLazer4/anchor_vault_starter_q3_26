//! A deliberately dumb lending market so the in-kind vault has something
//! real to hold a position in.
//!
//! - `supply` takes base tokens and mints receipt tokens 1:1
//! - `withdraw` burns receipts and pays base back, but only if the pool
//!   actually has the liquidity
//! - `borrow` lets the market authority pull liquidity out. this stands in
//!   for borrowers draining the pool, which is the situation the vault's
//!   in-kind redemption is built for
//! - `repay` puts liquidity back
//!
//! No interest, no collateral, no health checks. Don't use it for anything.

use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{
        burn, mint_to, transfer_checked, Burn, Mint, MintTo, TokenAccount, TokenInterface,
        TransferChecked,
    },
};

declare_id!("Gvp82d7hxx5ddToKNAQWbdRa3KUsUk3tReTSHziWdRaQ");

pub const MARKET_SEED: &[u8] = b"market";
pub const RECEIPT_SEED: &[u8] = b"receipt";

#[program]
pub mod mock_market {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        ctx.accounts.market.set_inner(Market {
            authority: ctx.accounts.authority.key(),
            base_mint: ctx.accounts.base_mint.key(),
            receipt_mint: ctx.accounts.receipt_mint.key(),
            bump: ctx.bumps.market,
        });
        Ok(())
    }

    pub fn supply(ctx: Context<Supply>, amount: u64) -> Result<()> {
        require!(amount > 0, MarketError::InvalidAmount);
        let a = &ctx.accounts;

        transfer_checked(
            CpiContext::new(
                a.token_program.key(),
                TransferChecked {
                    from: a.user_base.to_account_info(),
                    mint: a.base_mint.to_account_info(),
                    to: a.liquidity.to_account_info(),
                    authority: a.user.to_account_info(),
                },
            ),
            amount,
            a.base_mint.decimals,
        )?;

        let base_mint = a.base_mint.key();
        let seeds: &[&[&[u8]]] = &[&[MARKET_SEED, base_mint.as_ref(), &[a.market.bump]]];
        mint_to(
            CpiContext::new_with_signer(
                a.token_program.key(),
                MintTo {
                    mint: a.receipt_mint.to_account_info(),
                    to: a.user_receipt.to_account_info(),
                    authority: a.market.to_account_info(),
                },
                seeds,
            ),
            amount,
        )
    }

    pub fn withdraw(ctx: Context<Withdraw>, amount: u64) -> Result<()> {
        require!(amount > 0, MarketError::InvalidAmount);
        let a = &ctx.accounts;
        require!(
            a.liquidity.amount >= amount,
            MarketError::InsufficientLiquidity
        );

        burn(
            CpiContext::new(
                a.token_program.key(),
                Burn {
                    mint: a.receipt_mint.to_account_info(),
                    from: a.user_receipt.to_account_info(),
                    authority: a.user.to_account_info(),
                },
            ),
            amount,
        )?;

        let base_mint = a.base_mint.key();
        let seeds: &[&[&[u8]]] = &[&[MARKET_SEED, base_mint.as_ref(), &[a.market.bump]]];
        transfer_checked(
            CpiContext::new_with_signer(
                a.token_program.key(),
                TransferChecked {
                    from: a.liquidity.to_account_info(),
                    mint: a.base_mint.to_account_info(),
                    to: a.user_base.to_account_info(),
                    authority: a.market.to_account_info(),
                },
                seeds,
            ),
            amount,
            a.base_mint.decimals,
        )
    }

    pub fn borrow(ctx: Context<Borrow>, amount: u64) -> Result<()> {
        let a = &ctx.accounts;
        let base_mint = a.base_mint.key();
        let seeds: &[&[&[u8]]] = &[&[MARKET_SEED, base_mint.as_ref(), &[a.market.bump]]];
        transfer_checked(
            CpiContext::new_with_signer(
                a.token_program.key(),
                TransferChecked {
                    from: a.liquidity.to_account_info(),
                    mint: a.base_mint.to_account_info(),
                    to: a.destination.to_account_info(),
                    authority: a.market.to_account_info(),
                },
                seeds,
            ),
            amount,
            a.base_mint.decimals,
        )
    }

    pub fn repay(ctx: Context<Repay>, amount: u64) -> Result<()> {
        let a = &ctx.accounts;
        transfer_checked(
            CpiContext::new(
                a.token_program.key(),
                TransferChecked {
                    from: a.payer_base.to_account_info(),
                    mint: a.base_mint.to_account_info(),
                    to: a.liquidity.to_account_info(),
                    authority: a.payer.to_account_info(),
                },
            ),
            amount,
            a.base_mint.decimals,
        )
    }
}

#[account]
#[derive(InitSpace)]
pub struct Market {
    pub authority: Pubkey,
    pub base_mint: Pubkey,
    pub receipt_mint: Pubkey,
    pub bump: u8,
}

#[error_code]
pub enum MarketError {
    #[msg("Amount must be greater than zero")]
    InvalidAmount,
    #[msg("Not enough liquidity in the market")]
    InsufficientLiquidity,
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(mint::token_program = token_program)]
    pub base_mint: InterfaceAccount<'info, Mint>,

    #[account(
        init,
        payer = authority,
        space = Market::DISCRIMINATOR.len() + Market::INIT_SPACE,
        seeds = [MARKET_SEED, base_mint.key().as_ref()],
        bump
    )]
    pub market: Account<'info, Market>,

    #[account(
        init,
        payer = authority,
        seeds = [RECEIPT_SEED, market.key().as_ref()],
        bump,
        mint::decimals = base_mint.decimals,
        mint::authority = market,
        mint::token_program = token_program
    )]
    pub receipt_mint: InterfaceAccount<'info, Mint>,

    #[account(
        init,
        payer = authority,
        associated_token::mint = base_mint,
        associated_token::authority = market,
        associated_token::token_program = token_program
    )]
    pub liquidity: InterfaceAccount<'info, TokenAccount>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Supply<'info> {
    pub user: Signer<'info>,

    #[account(
        seeds = [MARKET_SEED, base_mint.key().as_ref()],
        bump = market.bump,
        has_one = base_mint,
        has_one = receipt_mint
    )]
    pub market: Account<'info, Market>,

    pub base_mint: InterfaceAccount<'info, Mint>,

    #[account(mut)]
    pub receipt_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        associated_token::mint = base_mint,
        associated_token::authority = market,
        associated_token::token_program = token_program
    )]
    pub liquidity: InterfaceAccount<'info, TokenAccount>,

    #[account(mut, token::mint = base_mint, token::authority = user)]
    pub user_base: InterfaceAccount<'info, TokenAccount>,

    #[account(mut, token::mint = receipt_mint)]
    pub user_receipt: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    pub user: Signer<'info>,

    #[account(
        seeds = [MARKET_SEED, base_mint.key().as_ref()],
        bump = market.bump,
        has_one = base_mint,
        has_one = receipt_mint
    )]
    pub market: Account<'info, Market>,

    pub base_mint: InterfaceAccount<'info, Mint>,

    #[account(mut)]
    pub receipt_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        associated_token::mint = base_mint,
        associated_token::authority = market,
        associated_token::token_program = token_program
    )]
    pub liquidity: InterfaceAccount<'info, TokenAccount>,

    #[account(mut, token::mint = base_mint)]
    pub user_base: InterfaceAccount<'info, TokenAccount>,

    #[account(mut, token::mint = receipt_mint, token::authority = user)]
    pub user_receipt: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
pub struct Borrow<'info> {
    pub authority: Signer<'info>,

    #[account(
        seeds = [MARKET_SEED, base_mint.key().as_ref()],
        bump = market.bump,
        has_one = authority,
        has_one = base_mint
    )]
    pub market: Account<'info, Market>,

    pub base_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        associated_token::mint = base_mint,
        associated_token::authority = market,
        associated_token::token_program = token_program
    )]
    pub liquidity: InterfaceAccount<'info, TokenAccount>,

    #[account(mut, token::mint = base_mint)]
    pub destination: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
pub struct Repay<'info> {
    pub payer: Signer<'info>,

    #[account(
        seeds = [MARKET_SEED, base_mint.key().as_ref()],
        bump = market.bump,
        has_one = base_mint
    )]
    pub market: Account<'info, Market>,

    pub base_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        associated_token::mint = base_mint,
        associated_token::authority = market,
        associated_token::token_program = token_program
    )]
    pub liquidity: InterfaceAccount<'info, TokenAccount>,

    #[account(mut, token::mint = base_mint, token::authority = payer)]
    pub payer_base: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Interface<'info, TokenInterface>,
}
