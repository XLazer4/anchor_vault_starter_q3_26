use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{
            instruction::{error::InstructionError, Instruction},
            system_program,
        },
        InstructionData, ToAccountMetas,
    },
    litesvm::{types::TransactionResult, LiteSVM},
    q3_26_vault::{STATE, VAULT_SEED},
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
    solana_transaction_error::TransactionError,
};

const DEPOSIT_LAMPORTS: u64 = 500_000_000;
const WITHDRAW_LAMPORTS: u64 = 100_000_000;

// anchor custom errors start at 6000, in the order they're declared in error.rs
const ERR_INVALID_AMOUNT: u32 = 6000;
const ERR_INSUFFICIENT_BALANCE: u32 = 6001;
// anchor's built-in ConstraintSeeds
const ERR_CONSTRAINT_SEEDS: u32 = 2006;

struct Ctx {
    svm: LiteSVM,
    user: Keypair,
    vault_state: Pubkey,
    vault: Pubkey,
}

fn pdas(user: &Pubkey) -> (Pubkey, Pubkey) {
    let program_id = q3_26_vault::id();
    let (vault_state, _) = Pubkey::find_program_address(&[STATE, user.as_ref()], &program_id);
    let (vault, _) = Pubkey::find_program_address(&[VAULT_SEED, user.as_ref()], &program_id);
    (vault_state, vault)
}

fn setup() -> Ctx {
    let mut svm = LiteSVM::new();
    let bytes = include_bytes!(concat!(
        env!("CARGO_TARGET_TMPDIR"),
        "/../deploy/q3_26_vault.so"
    ));
    svm.add_program(q3_26_vault::id(), bytes).unwrap();

    let user = Keypair::new();
    svm.airdrop(&user.pubkey(), 2_000_000_000).unwrap();
    let (vault_state, vault) = pdas(&user.pubkey());

    Ctx {
        svm,
        user,
        vault_state,
        vault,
    }
}

fn try_send(svm: &mut LiteSVM, payer: &Keypair, ix: Instruction) -> TransactionResult {
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(&[ix], Some(&payer.pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[payer]).unwrap();
    let res = svm.send_transaction(tx);
    // new blockhash so the same ix can be sent twice in a test without
    // tripping AlreadyProcessed
    svm.expire_blockhash();
    res
}

fn send(svm: &mut LiteSVM, payer: &Keypair, ix: Instruction) {
    try_send(svm, payer, ix).unwrap_or_else(|err| {
        panic!("tx failed: {err:?}\nlogs: {logs:#?}", logs = err.meta.logs);
    });
}

fn expect_custom_err(svm: &mut LiteSVM, payer: &Keypair, ix: Instruction, code: u32) {
    let err = try_send(svm, payer, ix).expect_err("tx should have failed");
    assert_eq!(
        err.err,
        TransactionError::InstructionError(0, InstructionError::Custom(code)),
        "logs: {:#?}",
        err.meta.logs
    );
}

fn initialize_ix(user: Pubkey, vault_state: Pubkey, vault: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        q3_26_vault::id(),
        &q3_26_vault::instruction::Initialize {}.data(),
        q3_26_vault::accounts::Initialize {
            user,
            vault_state,
            vault,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn deposit_ix(user: Pubkey, vault_state: Pubkey, vault: Pubkey, amount: u64) -> Instruction {
    Instruction::new_with_bytes(
        q3_26_vault::id(),
        &q3_26_vault::instruction::Deposit { amount }.data(),
        q3_26_vault::accounts::Deposit {
            user,
            vault_state,
            vault,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn withdraw_ix(user: Pubkey, vault_state: Pubkey, vault: Pubkey, amount: u64) -> Instruction {
    Instruction::new_with_bytes(
        q3_26_vault::id(),
        &q3_26_vault::instruction::Withdraw { amount }.data(),
        q3_26_vault::accounts::Withdraw {
            user,
            vault_state,
            vault,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn close_ix(user: Pubkey, vault_state: Pubkey, vault: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        q3_26_vault::id(),
        &q3_26_vault::instruction::Close {}.data(),
        q3_26_vault::accounts::Close {
            user,
            vault_state,
            vault,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

// initialize + deposit, used by most tests
fn open_vault(c: &mut Ctx, amount: u64) {
    let user = c.user.pubkey();
    send(
        &mut c.svm,
        &c.user,
        initialize_ix(user, c.vault_state, c.vault),
    );
    send(
        &mut c.svm,
        &c.user,
        deposit_ix(user, c.vault_state, c.vault, amount),
    );
}

#[test]
fn full_flow() {
    let mut c = setup();
    let user = c.user.pubkey();

    send(
        &mut c.svm,
        &c.user,
        initialize_ix(user, c.vault_state, c.vault),
    );

    let rent_exempt = c.svm.minimum_balance_for_rent_exemption(0);
    assert!(
        c.svm.get_account(&c.vault_state).is_some(),
        "vault_state should exist after initialize"
    );
    assert_eq!(
        c.svm.get_balance(&c.vault).unwrap(),
        rent_exempt,
        "vault should be rent-exempt after initialize"
    );

    send(
        &mut c.svm,
        &c.user,
        deposit_ix(user, c.vault_state, c.vault, DEPOSIT_LAMPORTS),
    );
    assert_eq!(
        c.svm.get_balance(&c.vault).unwrap(),
        rent_exempt + DEPOSIT_LAMPORTS,
        "vault should increase by the deposit"
    );

    send(
        &mut c.svm,
        &c.user,
        withdraw_ix(user, c.vault_state, c.vault, WITHDRAW_LAMPORTS),
    );
    assert_eq!(
        c.svm.get_balance(&c.vault).unwrap(),
        rent_exempt + DEPOSIT_LAMPORTS - WITHDRAW_LAMPORTS,
        "vault should decrease by the withdraw"
    );

    send(&mut c.svm, &c.user, close_ix(user, c.vault_state, c.vault));
    assert!(
        c.svm.get_account(&c.vault_state).is_none(),
        "vault_state should be closed"
    );
    assert_eq!(
        c.svm.get_balance(&c.vault).unwrap_or(0),
        0,
        "vault should be empty"
    );
}

#[test]
fn cannot_initialize_twice() {
    let mut c = setup();
    let user = c.user.pubkey();
    send(
        &mut c.svm,
        &c.user,
        initialize_ix(user, c.vault_state, c.vault),
    );

    let res = try_send(
        &mut c.svm,
        &c.user,
        initialize_ix(user, c.vault_state, c.vault),
    );
    assert!(res.is_err(), "second initialize should fail");
}

#[test]
fn deposit_zero_fails() {
    let mut c = setup();
    let user = c.user.pubkey();
    send(
        &mut c.svm,
        &c.user,
        initialize_ix(user, c.vault_state, c.vault),
    );

    expect_custom_err(
        &mut c.svm,
        &c.user,
        deposit_ix(user, c.vault_state, c.vault, 0),
        ERR_INVALID_AMOUNT,
    );
}

#[test]
fn withdraw_zero_fails() {
    let mut c = setup();
    open_vault(&mut c, DEPOSIT_LAMPORTS);
    let user = c.user.pubkey();

    expect_custom_err(
        &mut c.svm,
        &c.user,
        withdraw_ix(user, c.vault_state, c.vault, 0),
        ERR_INVALID_AMOUNT,
    );
}

#[test]
fn withdraw_more_than_deposited_fails() {
    let mut c = setup();
    open_vault(&mut c, DEPOSIT_LAMPORTS);
    let user = c.user.pubkey();

    expect_custom_err(
        &mut c.svm,
        &c.user,
        withdraw_ix(user, c.vault_state, c.vault, DEPOSIT_LAMPORTS + 1),
        ERR_INSUFFICIENT_BALANCE,
    );
}

#[test]
fn withdraw_cannot_touch_rent_reserve() {
    let mut c = setup();
    open_vault(&mut c, DEPOSIT_LAMPORTS);
    let user = c.user.pubkey();
    let rent_exempt = c.svm.minimum_balance_for_rent_exemption(0);

    // the whole balance including the rent reserve is not withdrawable
    expect_custom_err(
        &mut c.svm,
        &c.user,
        withdraw_ix(user, c.vault_state, c.vault, DEPOSIT_LAMPORTS + rent_exempt),
        ERR_INSUFFICIENT_BALANCE,
    );

    // but everything above it is
    send(
        &mut c.svm,
        &c.user,
        withdraw_ix(user, c.vault_state, c.vault, DEPOSIT_LAMPORTS),
    );
    assert_eq!(c.svm.get_balance(&c.vault).unwrap(), rent_exempt);
}

#[test]
fn withdraw_sends_lamports_to_user() {
    let mut c = setup();
    open_vault(&mut c, DEPOSIT_LAMPORTS);
    let user = c.user.pubkey();

    let before = c.svm.get_balance(&user).unwrap();
    send(
        &mut c.svm,
        &c.user,
        withdraw_ix(user, c.vault_state, c.vault, WITHDRAW_LAMPORTS),
    );
    let after = c.svm.get_balance(&user).unwrap();

    // litesvm charges 5000 lamports per signature
    assert_eq!(after, before + WITHDRAW_LAMPORTS - 5000);
}

#[test]
fn other_user_cannot_withdraw() {
    let mut c = setup();
    open_vault(&mut c, DEPOSIT_LAMPORTS);

    let thief = Keypair::new();
    c.svm.airdrop(&thief.pubkey(), 1_000_000_000).unwrap();

    // thief signs but points at the victim's vault accounts. the seeds are
    // derived from the signer so they won't match.
    expect_custom_err(
        &mut c.svm,
        &thief,
        withdraw_ix(thief.pubkey(), c.vault_state, c.vault, WITHDRAW_LAMPORTS),
        ERR_CONSTRAINT_SEEDS,
    );
}

#[test]
fn other_user_cannot_close() {
    let mut c = setup();
    open_vault(&mut c, DEPOSIT_LAMPORTS);

    let thief = Keypair::new();
    c.svm.airdrop(&thief.pubkey(), 1_000_000_000).unwrap();

    expect_custom_err(
        &mut c.svm,
        &thief,
        close_ix(thief.pubkey(), c.vault_state, c.vault),
        ERR_CONSTRAINT_SEEDS,
    );
    assert!(c.svm.get_account(&c.vault_state).is_some());
}

#[test]
fn close_returns_everything() {
    let mut c = setup();
    open_vault(&mut c, DEPOSIT_LAMPORTS);
    let user = c.user.pubkey();

    let vault_balance = c.svm.get_balance(&c.vault).unwrap();
    let state_rent = c.svm.get_balance(&c.vault_state).unwrap();
    let before = c.svm.get_balance(&user).unwrap();

    send(&mut c.svm, &c.user, close_ix(user, c.vault_state, c.vault));

    let after = c.svm.get_balance(&user).unwrap();
    assert_eq!(after, before + vault_balance + state_rent - 5000);
    assert!(c.svm.get_account(&c.vault_state).is_none());
    assert_eq!(c.svm.get_balance(&c.vault).unwrap_or(0), 0);
}

#[test]
fn can_reopen_after_close() {
    let mut c = setup();
    open_vault(&mut c, DEPOSIT_LAMPORTS);
    let user = c.user.pubkey();
    send(&mut c.svm, &c.user, close_ix(user, c.vault_state, c.vault));

    open_vault(&mut c, WITHDRAW_LAMPORTS);
    let rent_exempt = c.svm.minimum_balance_for_rent_exemption(0);
    assert_eq!(
        c.svm.get_balance(&c.vault).unwrap(),
        rent_exempt + WITHDRAW_LAMPORTS
    );
}
