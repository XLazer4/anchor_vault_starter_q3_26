//! in_kind_vault + mock_market on LiteSVM.
//!
//! The interesting one is `redeem_in_kind_works_when_market_has_no_liquidity`:
//! everything is lent out, the curator can't pull funds back, plain redeem
//! fails, and users can still leave with their slice of the market position.

use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{
            instruction::{error::InstructionError, Instruction},
            system_program,
        },
        InstructionData, ToAccountMetas,
    },
    anchor_spl::{
        associated_token::{self, get_associated_token_address},
        token,
    },
    in_kind_vault::{SHARES_SEED, VAULT_SEED},
    litesvm::{types::TransactionResult, LiteSVM},
    litesvm_token::{CreateAssociatedTokenAccount, CreateMint, MintTo, Transfer},
    mock_market::{MARKET_SEED, RECEIPT_SEED},
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
    solana_transaction_error::TransactionError,
};

const ERR_INVALID_AMOUNT: u32 = 6000;
const ERR_ZERO_SHARES: u32 = 6002;
const ERR_NOT_ENOUGH_IDLE: u32 = 6004;
const ERR_NOTHING_TO_REDEEM: u32 = 6005;
const ERR_HAS_ONE: u32 = 2001;
// mock_market's error, bubbles up through the CPI
const ERR_MARKET_NO_LIQUIDITY: u32 = 6001;

struct Market {
    base_mint: Pubkey,
    market: Pubkey,
    receipt_mint: Pubkey,
    liquidity: Pubkey,
}

struct Env {
    svm: LiteSVM,
    curator: Keypair,
    /// mint authority for the base token and authority of the market
    admin: Keypair,
    m: Market,
    vault: Pubkey,
    share_mint: Pubkey,
    idle: Pubkey,
    position: Pubkey,
}

// ---------- tx helpers ----------

fn try_send(svm: &mut LiteSVM, payer: &Keypair, ix: Instruction) -> TransactionResult {
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(&[ix], Some(&payer.pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[payer]).unwrap();
    let res = svm.send_transaction(tx);
    svm.expire_blockhash();
    res
}

fn send(svm: &mut LiteSVM, payer: &Keypair, ix: Instruction) {
    try_send(svm, payer, ix).unwrap_or_else(|err| {
        panic!("tx failed: {err:?}\nlogs: {logs:#?}", logs = err.meta.logs);
    });
}

fn expect_err(svm: &mut LiteSVM, payer: &Keypair, ix: Instruction, code: u32) {
    let err = try_send(svm, payer, ix).expect_err("tx should have failed");
    assert_eq!(
        err.err,
        TransactionError::InstructionError(0, InstructionError::Custom(code)),
        "logs: {:#?}",
        err.meta.logs
    );
}

fn balance(svm: &LiteSVM, token_account: &Pubkey) -> u64 {
    match svm.get_account(token_account) {
        Some(acc) if acc.data.len() >= 72 => {
            u64::from_le_bytes(acc.data[64..72].try_into().unwrap())
        }
        _ => 0,
    }
}

fn ata(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    get_associated_token_address(owner, mint)
}

// ---------- setup ----------

fn create_market(svm: &mut LiteSVM, admin: &Keypair) -> Market {
    let base_mint = CreateMint::new(svm, admin).decimals(6).send().unwrap();
    let market =
        Pubkey::find_program_address(&[MARKET_SEED, base_mint.as_ref()], &mock_market::id()).0;
    let receipt_mint =
        Pubkey::find_program_address(&[RECEIPT_SEED, market.as_ref()], &mock_market::id()).0;
    let liquidity = ata(&market, &base_mint);

    send(
        svm,
        admin,
        Instruction::new_with_bytes(
            mock_market::id(),
            &mock_market::instruction::Initialize {}.data(),
            mock_market::accounts::Initialize {
                authority: admin.pubkey(),
                base_mint,
                market,
                receipt_mint,
                liquidity,
                associated_token_program: associated_token::ID,
                token_program: token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        ),
    );

    Market {
        base_mint,
        market,
        receipt_mint,
        liquidity,
    }
}

fn setup() -> Env {
    let mut svm = LiteSVM::new();
    svm.add_program(
        mock_market::id(),
        include_bytes!(concat!(
            env!("CARGO_TARGET_TMPDIR"),
            "/../deploy/mock_market.so"
        )),
    )
    .unwrap();
    svm.add_program(
        in_kind_vault::id(),
        include_bytes!(concat!(
            env!("CARGO_TARGET_TMPDIR"),
            "/../deploy/in_kind_vault.so"
        )),
    )
    .unwrap();

    let curator = Keypair::new();
    let admin = Keypair::new();
    svm.airdrop(&curator.pubkey(), 10_000_000_000).unwrap();
    svm.airdrop(&admin.pubkey(), 10_000_000_000).unwrap();

    let m = create_market(&mut svm, &admin);

    let vault = Pubkey::find_program_address(
        &[VAULT_SEED, curator.pubkey().as_ref(), m.base_mint.as_ref()],
        &in_kind_vault::id(),
    )
    .0;
    let share_mint =
        Pubkey::find_program_address(&[SHARES_SEED, vault.as_ref()], &in_kind_vault::id()).0;
    let idle = ata(&vault, &m.base_mint);
    let position = ata(&vault, &m.receipt_mint);

    send(
        &mut svm,
        &curator,
        Instruction::new_with_bytes(
            in_kind_vault::id(),
            &in_kind_vault::instruction::Initialize {}.data(),
            in_kind_vault::accounts::Initialize {
                curator: curator.pubkey(),
                base_mint: m.base_mint,
                market: m.market,
                receipt_mint: m.receipt_mint,
                vault,
                share_mint,
                idle,
                position,
                associated_token_program: associated_token::ID,
                token_program: token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        ),
    );

    Env {
        svm,
        curator,
        admin,
        m,
        vault,
        share_mint,
        idle,
        position,
    }
}

impl Env {
    /// new wallet holding `amount` of the base token
    fn user(&mut self, amount: u64) -> Keypair {
        let kp = Keypair::new();
        self.svm.airdrop(&kp.pubkey(), 1_000_000_000).unwrap();
        let base_mint = self.m.base_mint;
        let user_ata = CreateAssociatedTokenAccount::new(&mut self.svm, &kp, &base_mint)
            .send()
            .unwrap();
        if amount > 0 {
            MintTo::new(&mut self.svm, &self.admin, &base_mint, &user_ata, amount)
                .send()
                .unwrap();
        }
        kp
    }

    fn base_of(&self, owner: &Pubkey) -> u64 {
        balance(&self.svm, &ata(owner, &self.m.base_mint))
    }

    fn receipts_of(&self, owner: &Pubkey) -> u64 {
        balance(&self.svm, &ata(owner, &self.m.receipt_mint))
    }

    fn shares_of(&self, owner: &Pubkey) -> u64 {
        balance(&self.svm, &ata(owner, &self.share_mint))
    }

    fn idle(&self) -> u64 {
        balance(&self.svm, &self.idle)
    }

    fn position(&self) -> u64 {
        balance(&self.svm, &self.position)
    }

    fn liquidity(&self) -> u64 {
        balance(&self.svm, &self.m.liquidity)
    }

    // ---- vault ixs ----

    fn deposit_ix(&self, user: &Pubkey, amount: u64) -> Instruction {
        Instruction::new_with_bytes(
            in_kind_vault::id(),
            &in_kind_vault::instruction::Deposit { amount }.data(),
            in_kind_vault::accounts::Deposit {
                user: *user,
                vault: self.vault,
                base_mint: self.m.base_mint,
                share_mint: self.share_mint,
                idle: self.idle,
                position: self.position,
                user_base: ata(user, &self.m.base_mint),
                user_shares: ata(user, &self.share_mint),
                associated_token_program: associated_token::ID,
                token_program: token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        )
    }

    fn rebalance_accounts(
        &self,
        signer: &Pubkey,
        market: &Market,
    ) -> Vec<anchor_lang::prelude::AccountMeta> {
        in_kind_vault::accounts::Rebalance {
            curator: *signer,
            vault: self.vault,
            market: market.market,
            base_mint: self.m.base_mint,
            receipt_mint: self.m.receipt_mint,
            liquidity: market.liquidity,
            idle: self.idle,
            position: self.position,
            market_program: mock_market::id(),
            token_program: token::ID,
        }
        .to_account_metas(None)
    }

    fn allocate_ix(&self, signer: &Pubkey, amount: u64) -> Instruction {
        Instruction::new_with_bytes(
            in_kind_vault::id(),
            &in_kind_vault::instruction::Allocate { amount }.data(),
            self.rebalance_accounts(signer, &self.m),
        )
    }

    fn deallocate_ix(&self, signer: &Pubkey, amount: u64) -> Instruction {
        Instruction::new_with_bytes(
            in_kind_vault::id(),
            &in_kind_vault::instruction::Deallocate { amount }.data(),
            self.rebalance_accounts(signer, &self.m),
        )
    }

    fn redeem_ix(&self, user: &Pubkey, shares: u64) -> Instruction {
        Instruction::new_with_bytes(
            in_kind_vault::id(),
            &in_kind_vault::instruction::Redeem { shares }.data(),
            in_kind_vault::accounts::Redeem {
                user: *user,
                vault: self.vault,
                base_mint: self.m.base_mint,
                share_mint: self.share_mint,
                idle: self.idle,
                position: self.position,
                user_shares: ata(user, &self.share_mint),
                user_base: ata(user, &self.m.base_mint),
                associated_token_program: associated_token::ID,
                token_program: token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        )
    }

    fn redeem_in_kind_ix(&self, user: &Pubkey, shares: u64) -> Instruction {
        Instruction::new_with_bytes(
            in_kind_vault::id(),
            &in_kind_vault::instruction::RedeemInKind { shares }.data(),
            in_kind_vault::accounts::RedeemInKind {
                user: *user,
                vault: self.vault,
                base_mint: self.m.base_mint,
                receipt_mint: self.m.receipt_mint,
                share_mint: self.share_mint,
                idle: self.idle,
                position: self.position,
                user_shares: ata(user, &self.share_mint),
                user_base: ata(user, &self.m.base_mint),
                user_receipt: ata(user, &self.m.receipt_mint),
                associated_token_program: associated_token::ID,
                token_program: token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        )
    }

    // ---- market ixs ----

    /// simulate borrowers taking `amount` out of the pool
    fn market_borrow(&mut self, amount: u64) {
        let sink = self.user(0);
        let ix = Instruction::new_with_bytes(
            mock_market::id(),
            &mock_market::instruction::Borrow { amount }.data(),
            mock_market::accounts::Borrow {
                authority: self.admin.pubkey(),
                market: self.m.market,
                base_mint: self.m.base_mint,
                liquidity: self.m.liquidity,
                destination: ata(&sink.pubkey(), &self.m.base_mint),
                token_program: token::ID,
            }
            .to_account_metas(None),
        );
        send(&mut self.svm, &self.admin, ix);
    }

    fn market_repay(&mut self, amount: u64) {
        let payer = self.user(amount);
        let ix = Instruction::new_with_bytes(
            mock_market::id(),
            &mock_market::instruction::Repay { amount }.data(),
            mock_market::accounts::Repay {
                payer: payer.pubkey(),
                market: self.m.market,
                base_mint: self.m.base_mint,
                liquidity: self.m.liquidity,
                payer_base: ata(&payer.pubkey(), &self.m.base_mint),
                token_program: token::ID,
            }
            .to_account_metas(None),
        );
        send(&mut self.svm, &payer, ix);
    }

    /// a user cashing receipts directly with the market
    fn market_withdraw_ix(&self, user: &Pubkey, amount: u64) -> Instruction {
        Instruction::new_with_bytes(
            mock_market::id(),
            &mock_market::instruction::Withdraw { amount }.data(),
            mock_market::accounts::Withdraw {
                user: *user,
                market: self.m.market,
                base_mint: self.m.base_mint,
                receipt_mint: self.m.receipt_mint,
                liquidity: self.m.liquidity,
                user_base: ata(user, &self.m.base_mint),
                user_receipt: ata(user, &self.m.receipt_mint),
                token_program: token::ID,
            }
            .to_account_metas(None),
        )
    }

    // ---- shortcuts ----

    fn deposit(&mut self, user: &Keypair, amount: u64) {
        let ix = self.deposit_ix(&user.pubkey(), amount);
        send(&mut self.svm, user, ix);
    }

    fn allocate(&mut self, amount: u64) {
        let ix = self.allocate_ix(&self.curator.pubkey(), amount);
        send(&mut self.svm, &self.curator, ix);
    }
}

// ---------- deposit ----------

#[test]
fn first_deposit_is_one_to_one() {
    let mut env = setup();
    let alice = env.user(1_000);
    env.deposit(&alice, 1_000);

    assert_eq!(env.shares_of(&alice.pubkey()), 1_000);
    assert_eq!(env.idle(), 1_000);
    assert_eq!(env.base_of(&alice.pubkey()), 0);
}

#[test]
fn deposit_after_donation_mints_fewer_shares() {
    let mut env = setup();
    let alice = env.user(1_000);
    let bob = env.user(1_000);
    env.deposit(&alice, 1_000);

    // someone sends base straight to the vault, share price doubles
    let donor = env.user(1_000);
    let base_mint = env.m.base_mint;
    let idle = env.idle;
    Transfer::new(&mut env.svm, &donor, &base_mint, &idle, 1_000)
        .send()
        .unwrap();

    env.deposit(&bob, 1_000);
    assert_eq!(env.shares_of(&bob.pubkey()), 500);
}

#[test]
fn deposit_zero_fails() {
    let mut env = setup();
    let alice = env.user(1_000);
    let ix = env.deposit_ix(&alice.pubkey(), 0);
    expect_err(&mut env.svm, &alice, ix, ERR_INVALID_AMOUNT);
}

#[test]
fn deposit_that_rounds_to_zero_shares_is_rejected() {
    // the classic first-depositor inflation setup: 1 share backed by a huge
    // donation. bob's small deposit would mint 0 shares and just gift his
    // tokens to alice, so it has to fail instead.
    let mut env = setup();
    let alice = env.user(1);
    let bob = env.user(1_000);
    env.deposit(&alice, 1);

    let donor = env.user(1_000_000);
    let base_mint = env.m.base_mint;
    let idle = env.idle;
    Transfer::new(&mut env.svm, &donor, &base_mint, &idle, 1_000_000)
        .send()
        .unwrap();

    let ix = env.deposit_ix(&bob.pubkey(), 1_000);
    expect_err(&mut env.svm, &bob, ix, ERR_ZERO_SHARES);
    assert_eq!(env.base_of(&bob.pubkey()), 1_000);
}

// ---------- allocate / deallocate ----------

#[test]
fn allocate_moves_idle_into_the_market() {
    let mut env = setup();
    let alice = env.user(1_000);
    env.deposit(&alice, 1_000);
    env.allocate(700);

    assert_eq!(env.idle(), 300);
    assert_eq!(env.position(), 700);
    assert_eq!(env.liquidity(), 700);
    // share count doesn't change, value is just spread out
    assert_eq!(env.shares_of(&alice.pubkey()), 1_000);
}

#[test]
fn only_curator_can_allocate() {
    let mut env = setup();
    let alice = env.user(1_000);
    env.deposit(&alice, 1_000);

    let ix = env.allocate_ix(&alice.pubkey(), 500);
    expect_err(&mut env.svm, &alice, ix, ERR_HAS_ONE);
    assert_eq!(env.idle(), 1_000);
}

#[test]
fn curator_cannot_point_allocate_at_another_market() {
    let mut env = setup();
    let alice = env.user(1_000);
    env.deposit(&alice, 1_000);

    // a second market the curator might control
    let admin2 = Keypair::new();
    env.svm.airdrop(&admin2.pubkey(), 1_000_000_000).unwrap();
    let other = create_market(&mut env.svm, &admin2);

    let ix = Instruction::new_with_bytes(
        in_kind_vault::id(),
        &in_kind_vault::instruction::Allocate { amount: 500 }.data(),
        env.rebalance_accounts(&env.curator.pubkey(), &other),
    );
    expect_err(&mut env.svm, &env.curator, ix, ERR_HAS_ONE);
}

#[test]
fn deallocate_brings_base_back_to_idle() {
    let mut env = setup();
    let alice = env.user(1_000);
    env.deposit(&alice, 1_000);
    env.allocate(1_000);

    let ix = env.deallocate_ix(&env.curator.pubkey(), 400);
    send(&mut env.svm, &env.curator, ix);

    assert_eq!(env.idle(), 400);
    assert_eq!(env.position(), 600);
}

#[test]
fn deallocate_fails_when_market_is_drained() {
    let mut env = setup();
    let alice = env.user(1_000);
    env.deposit(&alice, 1_000);
    env.allocate(1_000);
    env.market_borrow(1_000);

    let ix = env.deallocate_ix(&env.curator.pubkey(), 1);
    expect_err(&mut env.svm, &env.curator, ix, ERR_MARKET_NO_LIQUIDITY);
}

// ---------- redeem ----------

#[test]
fn redeem_pays_base_from_idle() {
    let mut env = setup();
    let alice = env.user(1_000);
    env.deposit(&alice, 1_000);

    let ix = env.redeem_ix(&alice.pubkey(), 250);
    send(&mut env.svm, &alice, ix);

    assert_eq!(env.base_of(&alice.pubkey()), 250);
    assert_eq!(env.shares_of(&alice.pubkey()), 750);
    assert_eq!(env.idle(), 750);
}

#[test]
fn redeem_fails_when_funds_are_in_the_market() {
    let mut env = setup();
    let alice = env.user(1_000);
    env.deposit(&alice, 1_000);
    env.allocate(800);

    // 500 shares are worth 500 but only 200 is idle
    let ix = env.redeem_ix(&alice.pubkey(), 500);
    expect_err(&mut env.svm, &alice, ix, ERR_NOT_ENOUGH_IDLE);

    // a redeem that fits in idle still works
    let ix = env.redeem_ix(&alice.pubkey(), 200);
    send(&mut env.svm, &alice, ix);
    assert_eq!(env.base_of(&alice.pubkey()), 200);
}

#[test]
fn cannot_redeem_more_shares_than_you_have() {
    let mut env = setup();
    let alice = env.user(1_000);
    let bob = env.user(1_000);
    env.deposit(&alice, 1_000);
    env.deposit(&bob, 1_000);

    let ix = env.redeem_ix(&alice.pubkey(), 1_001);
    assert!(try_send(&mut env.svm, &alice, ix).is_err());
    let ix = env.redeem_in_kind_ix(&alice.pubkey(), 1_001);
    assert!(try_send(&mut env.svm, &alice, ix).is_err());
}

// ---------- redeem in kind ----------

#[test]
fn redeem_in_kind_works_when_market_has_no_liquidity() {
    let mut env = setup();
    let alice = env.user(600);
    let bob = env.user(400);
    env.deposit(&alice, 600);
    env.deposit(&bob, 400);

    // everything goes to the market, then borrowers take all of it
    env.allocate(1_000);
    env.market_borrow(1_000);
    assert_eq!(env.idle(), 0);
    assert_eq!(env.liquidity(), 0);

    // curator can't unwind and a plain redeem has nothing to pay from
    let ix = env.deallocate_ix(&env.curator.pubkey(), 1);
    expect_err(&mut env.svm, &env.curator, ix, ERR_MARKET_NO_LIQUIDITY);
    let ix = env.redeem_ix(&alice.pubkey(), 600);
    expect_err(&mut env.svm, &alice, ix, ERR_NOT_ENOUGH_IDLE);

    // alice exits anyway, taking her slice of the market position
    let ix = env.redeem_in_kind_ix(&alice.pubkey(), 600);
    send(&mut env.svm, &alice, ix);

    assert_eq!(env.shares_of(&alice.pubkey()), 0);
    assert_eq!(env.receipts_of(&alice.pubkey()), 600);
    assert_eq!(env.base_of(&alice.pubkey()), 0);
    assert_eq!(env.position(), 400, "bob's part stays in the vault");

    // alice now holds the market claim herself. once borrowers repay she
    // cashes it out directly, no vault involved
    env.market_repay(1_000);
    let ix = env.market_withdraw_ix(&alice.pubkey(), 600);
    send(&mut env.svm, &alice, ix);
    assert_eq!(env.base_of(&alice.pubkey()), 600);
    assert_eq!(env.receipts_of(&alice.pubkey()), 0);
}

#[test]
fn redeem_in_kind_splits_idle_and_position() {
    let mut env = setup();
    let alice = env.user(1_000);
    env.deposit(&alice, 1_000);
    env.allocate(600);

    // half the shares -> half of idle (400) and half of position (600)
    let ix = env.redeem_in_kind_ix(&alice.pubkey(), 500);
    send(&mut env.svm, &alice, ix);

    assert_eq!(env.base_of(&alice.pubkey()), 200);
    assert_eq!(env.receipts_of(&alice.pubkey()), 300);
    assert_eq!(env.idle(), 200);
    assert_eq!(env.position(), 300);
    assert_eq!(env.shares_of(&alice.pubkey()), 500);
}

#[test]
fn in_kind_exit_does_not_hurt_whoever_stays() {
    let mut env = setup();
    let alice = env.user(500);
    let bob = env.user(500);
    env.deposit(&alice, 500);
    env.deposit(&bob, 500);
    env.allocate(700); // idle 300, position 700

    let ix = env.redeem_in_kind_ix(&alice.pubkey(), 500);
    send(&mut env.svm, &alice, ix);
    assert_eq!(env.base_of(&alice.pubkey()), 150);
    assert_eq!(env.receipts_of(&alice.pubkey()), 350);

    // bob's 500 shares are now all of the vault: still worth 500
    assert_eq!(env.idle() + env.position(), 500);
    let ix = env.redeem_in_kind_ix(&bob.pubkey(), 500);
    send(&mut env.svm, &bob, ix);
    assert_eq!(env.base_of(&bob.pubkey()), 150);
    assert_eq!(env.receipts_of(&bob.pubkey()), 350);
    assert_eq!(env.idle(), 0);
    assert_eq!(env.position(), 0);
}

#[test]
fn redeem_in_kind_with_dust_shares_fails() {
    let mut env = setup();
    let alice = env.user(3);
    env.deposit(&alice, 3);
    env.allocate(2); // idle 1, position 2, supply 3

    // 1 share -> 1/3 of 1 and 1/3 of 2, both round to 0
    let ix = env.redeem_in_kind_ix(&alice.pubkey(), 1);
    expect_err(&mut env.svm, &alice, ix, ERR_NOTHING_TO_REDEEM);
    assert_eq!(env.shares_of(&alice.pubkey()), 3);
}

#[test]
fn full_round_trip() {
    let mut env = setup();
    let alice = env.user(1_000);
    env.deposit(&alice, 1_000);
    env.allocate(1_000);

    let ix = env.deallocate_ix(&env.curator.pubkey(), 1_000);
    send(&mut env.svm, &env.curator, ix);

    let ix = env.redeem_ix(&alice.pubkey(), 1_000);
    send(&mut env.svm, &alice, ix);

    assert_eq!(env.base_of(&alice.pubkey()), 1_000);
    assert_eq!(env.shares_of(&alice.pubkey()), 0);
    assert_eq!(env.idle(), 0);
    assert_eq!(env.position(), 0);
}
