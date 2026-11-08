pub mod constants;
pub mod error;
pub mod instructions;
pub mod math;
pub mod state;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("36jaqkMuBwvLwnADnHn1rTJ7GrwyCFgNCNLJgbXw2Kk1");

#[program]
pub mod in_kind_vault {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        ctx.accounts.initialize(&ctx.bumps)
    }

    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        ctx.accounts.deposit(amount)
    }

    pub fn allocate(ctx: Context<Rebalance>, amount: u64) -> Result<()> {
        ctx.accounts.allocate(amount)
    }

    pub fn deallocate(ctx: Context<Rebalance>, amount: u64) -> Result<()> {
        ctx.accounts.deallocate(amount)
    }

    pub fn redeem(ctx: Context<Redeem>, shares: u64) -> Result<()> {
        ctx.accounts.redeem(shares)
    }

    pub fn redeem_in_kind(ctx: Context<RedeemInKind>, shares: u64) -> Result<()> {
        ctx.accounts.redeem_in_kind(shares)
    }
}
