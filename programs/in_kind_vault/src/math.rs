use anchor_lang::prelude::*;

use crate::error::VaultError;

/// Shares to mint for `amount` of base when the vault holds `total_assets`
/// and `supply` shares exist. First deposit is 1:1. Rounds down so the
/// vault never hands out more than it got.
pub fn shares_for_deposit(amount: u64, total_assets: u64, supply: u64) -> Result<u64> {
    if supply == 0 || total_assets == 0 {
        return Ok(amount);
    }
    mul_div(amount, supply, total_assets)
}

/// `held * shares / supply`, rounded down. Used for both the plain redeem
/// (held = total assets) and per-asset in the in-kind redeem.
pub fn pro_rata(held: u64, shares: u64, supply: u64) -> Result<u64> {
    require!(supply > 0, VaultError::MathOverflow);
    mul_div(held, shares, supply)
}

fn mul_div(a: u64, b: u64, c: u64) -> Result<u64> {
    let out = (a as u128)
        .checked_mul(b as u128)
        .and_then(|x| x.checked_div(c as u128))
        .ok_or(VaultError::MathOverflow)?;
    u64::try_from(out).map_err(|_| VaultError::MathOverflow.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_deposit_is_one_to_one() {
        assert_eq!(shares_for_deposit(500, 0, 0).unwrap(), 500);
    }

    #[test]
    fn later_deposits_follow_share_price() {
        // 1000 assets backing 500 shares -> price 2
        assert_eq!(shares_for_deposit(100, 1000, 500).unwrap(), 50);
    }

    #[test]
    fn rounding_goes_against_the_user() {
        assert_eq!(shares_for_deposit(1, 3, 2).unwrap(), 0);
        assert_eq!(pro_rata(10, 1, 3).unwrap(), 3);
    }
}
