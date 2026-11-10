use {
    anchor_lang::{
        prelude::{msg, Clock},
        solana_program::instruction::Instruction,
        solana_program::program_pack::Pack,
        system_program::ID as SYSTEM_PROGRAM_ID,
        AccountDeserialize, InstructionData, ToAccountMetas,
    },
    anchor_spl::{
        associated_token::{self, ID as ASSOCIATED_TOKEN_PROGRAM_ID},
        token::spl_token,
    },
    litesvm::{
        types::{FailedTransactionMetadata, TransactionMetadata},
        LiteSVM,
    },
    litesvm_token::{
        spl_token::ID as TOKEN_PROGRAM_ID, CreateAssociatedTokenAccount, CreateMint, MintTo,
    },
    solana_keypair::Keypair,
    solana_message::Message,
    solana_pubkey::Pubkey,
    solana_signer::Signer,
    solana_transaction::Transaction,
};

use anchor_lang::error::ErrorCode as AnchorError;
use escrowq32026::error::ErrorCode as EscrowError;

// Setup function to initialize LiteSVM and create a payer keypair
fn setup() -> (LiteSVM, Keypair) {
    let program_id = escrowq32026::id();
    let payer = Keypair::new();
    let mut svm = LiteSVM::new();
    let bytes = include_bytes!(concat!(
        env!("CARGO_TARGET_TMPDIR"),
        "/../deploy/escrowq32026.so"
    ));
    svm.add_program(program_id, bytes).unwrap();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();

    // Return the LiteSVM instance and payer keypair
    (svm, payer)
}

// Tests:   make, then refund (this is the original test from the repo, unchanged).
// Against: the escrow and the vault are closed after the refund.
#[test]
fn test_make_and_refund() {
    // Setup the test environment by initializing LiteSVM and creating a payer keypair
    let (mut program, payer) = setup();

    // Get the maker's public key from the payer keypair
    let maker = payer.pubkey();

    // Create two mints (Mint A and Mint B) with 6 decimal places and the maker as the authority
    // This done using litesvm-token's CreateMint utility which creates the mint in the LiteSVM environment
    let mint_a = CreateMint::new(&mut program, &payer)
        .decimals(6)
        .authority(&maker)
        .send()
        .unwrap();
    msg!("Mint A: {}\n", mint_a);

    let mint_b = CreateMint::new(&mut program, &payer)
        .decimals(6)
        .authority(&maker)
        .send()
        .unwrap();
    msg!("Mint B: {}\n", mint_b);

    // Create the maker's associated token account for Mint A
    // This is done using litesvm-token's CreateAssociatedTokenAccount utility
    let maker_ata_a = CreateAssociatedTokenAccount::new(&mut program, &payer, &mint_a)
        .owner(&maker)
        .send()
        .unwrap();
    msg!("Maker ATA A: {}\n", maker_ata_a);

    // Derive the PDA for the escrow account using the maker's public key and a seed value
    let escrow = Pubkey::find_program_address(
        &[b"escrow", maker.as_ref(), &123u64.to_le_bytes()],
        &escrowq32026::id(),
    )
    .0;
    msg!("Escrow PDA: {}\n", escrow);

    // Derive the PDA for the vault associated token account using the escrow PDA and Mint A
    let vault = associated_token::get_associated_token_address(&escrow, &mint_a);
    msg!("Vault PDA: {}\n", vault);

    // Mint 1,000 tokens (with 6 decimal places) of Mint A to the maker's associated token account
    MintTo::new(&mut program, &payer, &mint_a, &maker_ata_a, 1000_000_000)
        .send()
        .unwrap();

    // Create the "Make" instruction to deposit tokens into the escrow
    let make_ix = Instruction {
        program_id: escrowq32026::id(),
        accounts: escrowq32026::accounts::Make {
            maker: maker,
            mint_a: mint_a,
            mint_b: mint_b,
            maker_ata_a: maker_ata_a,
            escrow: escrow,
            vault: vault,
            associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
            token_program: TOKEN_PROGRAM_ID,
            system_program: SYSTEM_PROGRAM_ID,
        }
        .to_account_metas(None),
        data: escrowq32026::instruction::Make {
            deposit: 10_000_000,
            seed: 123u64,
            receive: 10_000_000,
            expiration: 17780206209,
        }
        .data(),
    };

    // Create and send the transaction containing the "Make" instruction
    let message = Message::new(&[make_ix], Some(&payer.pubkey()));
    let recent_blockhash = program.latest_blockhash();

    let transaction = Transaction::new(&[&payer], message, recent_blockhash);

    // Send the transaction and capture the result
    let tx = program.send_transaction(transaction).unwrap();

    // Log transaction details
    msg!("\n\nMake transaction sucessfull");
    msg!("CUs Consumed: {}", tx.compute_units_consumed);
    msg!("Tx Signature: {}", tx.signature);

    // Verify the vault account and escrow account data after the "Make" instruction
    let vault_account = program.get_account(&vault).unwrap();
    let vault_data = spl_token::state::Account::unpack(&vault_account.data).unwrap();
    assert_eq!(vault_data.amount, 10_000_000);
    assert_eq!(vault_data.owner, escrow);
    assert_eq!(vault_data.mint, mint_a);

    let escrow_account = program.get_account(&escrow).unwrap();
    let escrow_data =
        escrowq32026::state::Escrow::try_deserialize(&mut escrow_account.data.as_ref()).unwrap();
    assert_eq!(escrow_data.seed, 123u64);
    assert_eq!(escrow_data.maker, maker);
    assert_eq!(escrow_data.mint_a, mint_a);
    assert_eq!(escrow_data.mint_b, mint_b);
    assert_eq!(escrow_data.receive, 10_000_000);

    // Create the "Refund" instruction to refund tokens back to the maker
    let refund_ix = Instruction {
        program_id: escrowq32026::id(),
        accounts: escrowq32026::accounts::Refund {
            maker: maker,
            mint_a: mint_a,
            maker_ata_a: maker_ata_a,
            escrow: escrow,
            vault: vault,
            token_program: TOKEN_PROGRAM_ID,
            system_program: SYSTEM_PROGRAM_ID,
        }
        .to_account_metas(None),
        data: escrowq32026::instruction::Refund {}.data(),
    };

    // Create and send the transaction containing the "Refund" instruction
    let message = Message::new(&[refund_ix], Some(&payer.pubkey()));
    let recent_blockhash = program.latest_blockhash();

    let transaction = Transaction::new(&[&payer], message, recent_blockhash);

    // Send the transaction and capture the result
    let tx = program.send_transaction(transaction).unwrap();

    // Log transaction details
    msg!("\n\nRefund transaction sucessful");
    msg!("CUs Consumed: {}", tx.compute_units_consumed);
    msg!("Tx Signature: {}", tx.signature);
    assert!(program.get_account(&escrow).is_none());
    assert!(program.get_account(&vault).is_none());
}

const SEED: u64 = 42;
const STARTING_BALANCE: u64 = 1_000_000_000; // 1,000 tokens with 6 decimals
const DEPOSIT: u64 = 10_000_000; // the maker locks 10 token A ...
const RECEIVE: u64 = 5_000_000; // ... and wants 5 token B in return
const NOW: i64 = 1_000_000; // the fake "current time" every test starts at
const ONE_HOUR: i64 = 3_600;

type TxResult = Result<TransactionMetadata, FailedTransactionMetadata>;

struct Fixture {
    svm: LiteSVM,
    maker: Keypair,
    taker: Keypair,
    mint_a: Pubkey,
    mint_b: Pubkey,
    maker_ata_a: Pubkey,
    taker_ata_b: Pubkey,
    escrow: Pubkey,
    vault: Pubkey,
}

// Boots LiteSVM + the program and creates the starting world described above
fn fixture() -> Fixture {
    // `setup()` loads the program and hands back a funded keypair: that is our maker
    let (mut svm, maker) = setup();
    let taker = Keypair::new();
    svm.airdrop(&taker.pubkey(), 1_000_000_000).unwrap();
    set_time(&mut svm, NOW);

    // Two mints, both controlled by the maker so they can mint test tokens
    let mint_a = CreateMint::new(&mut svm, &maker)
        .decimals(6)
        .authority(&maker.pubkey())
        .send()
        .unwrap();
    let mint_b = CreateMint::new(&mut svm, &maker)
        .decimals(6)
        .authority(&maker.pubkey())
        .send()
        .unwrap();

    // The maker holds token A, the taker holds token B
    let maker_ata_a = CreateAssociatedTokenAccount::new(&mut svm, &maker, &mint_a)
        .owner(&maker.pubkey())
        .send()
        .unwrap();
    let taker_ata_b = CreateAssociatedTokenAccount::new(&mut svm, &maker, &mint_b)
        .owner(&taker.pubkey())
        .send()
        .unwrap();
    MintTo::new(&mut svm, &maker, &mint_a, &maker_ata_a, STARTING_BALANCE)
        .send()
        .unwrap();
    MintTo::new(&mut svm, &maker, &mint_b, &taker_ata_b, STARTING_BALANCE)
        .send()
        .unwrap();

    let escrow = Pubkey::find_program_address(
        &[b"escrow", maker.pubkey().as_ref(), &SEED.to_le_bytes()],
        &escrowq32026::id(),
    )
    .0;
    let vault = ata(&escrow, &mint_a);

    Fixture {
        svm,
        maker,
        taker,
        mint_a,
        mint_b,
        maker_ata_a,
        taker_ata_b,
        escrow,
        vault,
    }
}

// The address of `owner`'s associated token account for `mint`
fn ata(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    associated_token::get_associated_token_address(owner, mint)
}

// Moves the on-chain clock (what `Clock::get()` returns inside the program)
fn set_time(svm: &mut LiteSVM, unix_timestamp: i64) {
    let mut clock = svm.get_sysvar::<Clock>();
    clock.unix_timestamp = unix_timestamp;
    svm.set_sysvar::<Clock>(&clock);
}

fn token_balance(svm: &LiteSVM, token_account: &Pubkey) -> u64 {
    let account = svm
        .get_account(token_account)
        .expect("token account should exist");
    spl_token::state::Account::unpack(&account.data)
        .unwrap()
        .amount
}

// Signs and sends one transaction
fn send(svm: &mut LiteSVM, payer: &Keypair, ixs: &[Instruction]) -> TxResult {
    // A fresh blockhash every time: LiteSVM rejects an identical transaction as `AlreadyProcessed`
    svm.expire_blockhash();
    let message = Message::new(ixs, Some(&payer.pubkey()));
    let transaction = Transaction::new(&[payer], message, svm.latest_blockhash());
    svm.send_transaction(transaction)
}

// Checks that a transaction failed with the given custom error code.
// Program errors (`ErrorCode`) are numbered from 6000, Anchor's own errors (constraints) from 2000,
// and the token program has its own small numbers.
fn assert_fails_with(result: TxResult, code: u32) {
    let failed = result.expect_err("the transaction should have failed");
    assert!(
        format!("{:?}", failed.err).contains(&format!("Custom({code})")),
        "expected custom error {code}, got {:?}\nlogs:\n{}",
        failed.err,
        failed.meta.logs.join("\n")
    );
}

// What the world must look like after a successful take where the taker paid `paid` token B
fn assert_taken(f: &Fixture, paid: u64) {
    let taker = f.taker.pubkey();
    let maker = f.maker.pubkey();
    // the taker got the whole deposit, the maker got paid
    assert_eq!(token_balance(&f.svm, &ata(&taker, &f.mint_a)), DEPOSIT);
    assert_eq!(token_balance(&f.svm, &ata(&maker, &f.mint_b)), paid);
    assert_eq!(
        token_balance(&f.svm, &f.taker_ata_b),
        STARTING_BALANCE - paid
    );
    assert_eq!(
        token_balance(&f.svm, &f.maker_ata_a),
        STARTING_BALANCE - DEPOSIT
    );
    // the offer is closed: no escrow, no vault
    assert!(f.svm.get_account(&f.escrow).is_none());
    assert!(f.svm.get_account(&f.vault).is_none());
}

// One method per instruction: it builds the instruction, signs it and sends it.
// They return the transaction result so a test can `.unwrap()` it or expect an error.
impl Fixture {
    fn escrow_state(&self) -> escrowq32026::state::Escrow {
        let account = self
            .svm
            .get_account(&self.escrow)
            .expect("escrow account should exist");
        escrowq32026::state::Escrow::try_deserialize(&mut account.data.as_ref()).unwrap()
    }

    // make: the maker locks `deposit` of token A and asks for `receive` of token B
    fn make_with(&mut self, deposit: u64, receive: u64, expiration: i64) -> TxResult {
        let ix = Instruction {
            program_id: escrowq32026::id(),
            accounts: escrowq32026::accounts::Make {
                maker: self.maker.pubkey(),
                mint_a: self.mint_a,
                mint_b: self.mint_b,
                maker_ata_a: self.maker_ata_a,
                escrow: self.escrow,
                vault: self.vault,
                associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
                token_program: TOKEN_PROGRAM_ID,
                system_program: SYSTEM_PROGRAM_ID,
            }
            .to_account_metas(None),
            data: escrowq32026::instruction::Make {
                seed: SEED,
                deposit,
                receive,
                expiration,
            }
            .data(),
        };
        send(&mut self.svm, &self.maker, &[ix])
    }

    // the standard offer: DEPOSIT of token A for RECEIVE of token B
    fn make(&mut self, expiration: i64) {
        self.make_with(DEPOSIT, RECEIVE, expiration).unwrap();
    }

    // take, where the taker pays with `mint_b` / `taker_ata_b` (lets a test try a wrong mint)
    fn take_paying_with(&mut self, mint_b: Pubkey, taker_ata_b: Pubkey) -> TxResult {
        let taker = self.taker.pubkey();
        let maker = self.maker.pubkey();
        let ix = Instruction {
            program_id: escrowq32026::id(),
            accounts: escrowq32026::accounts::Take {
                taker,
                maker,
                mint_a: self.mint_a,
                mint_b,
                taker_ata_a: ata(&taker, &self.mint_a),
                taker_ata_b,
                maker_ata_b: ata(&maker, &mint_b),
                escrow: self.escrow,
                vault: self.vault,
                associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
                token_program: TOKEN_PROGRAM_ID,
                system_program: SYSTEM_PROGRAM_ID,
            }
            .to_account_metas(None),
            data: escrowq32026::instruction::Take {}.data(),
        };
        send(&mut self.svm, &self.taker, &[ix])
    }

    fn take(&mut self) -> TxResult {
        self.take_paying_with(self.mint_b, self.taker_ata_b)
    }

    // refund, signed by `signer` who claims to be the maker and wants the tokens in `signer_ata_a`
    fn refund_signed_by(&mut self, who: Who, signer_ata_a: Pubkey) -> TxResult {
        let signer = self.keypair(who).insecure_clone();
        let ix = Instruction {
            program_id: escrowq32026::id(),
            accounts: escrowq32026::accounts::Refund {
                maker: signer.pubkey(),
                mint_a: self.mint_a,
                maker_ata_a: signer_ata_a,
                escrow: self.escrow,
                vault: self.vault,
                token_program: TOKEN_PROGRAM_ID,
                system_program: SYSTEM_PROGRAM_ID,
            }
            .to_account_metas(None),
            data: escrowq32026::instruction::Refund {}.data(),
        };
        send(&mut self.svm, &signer, &[ix])
    }

    fn refund(&mut self) -> TxResult {
        self.refund_signed_by(Who::Maker, self.maker_ata_a)
    }

    fn refund_as_taker(&mut self, taker_ata_a: Pubkey) -> TxResult {
        self.refund_signed_by(Who::Taker, taker_ata_a)
    }

    // update, signed by `signer` who claims to be the maker
    fn update_signed_by(
        &mut self,
        who: Who,
        receive: Option<u64>,
        expiration: Option<i64>,
    ) -> TxResult {
        let signer = self.keypair(who).insecure_clone();
        let ix = Instruction {
            program_id: escrowq32026::id(),
            accounts: escrowq32026::accounts::Update {
                maker: signer.pubkey(),
                escrow: self.escrow,
            }
            .to_account_metas(None),
            data: escrowq32026::instruction::Update {
                receive,
                expiration,
            }
            .data(),
        };
        send(&mut self.svm, &signer, &[ix])
    }

    fn update(&mut self, receive: Option<u64>, expiration: Option<i64>) -> TxResult {
        self.update_signed_by(Who::Maker, receive, expiration)
    }

    fn update_as_taker(&mut self, receive: Option<u64>, expiration: Option<i64>) -> TxResult {
        self.update_signed_by(Who::Taker, receive, expiration)
    }

    fn keypair(&self, who: Who) -> &Keypair {
        match who {
            Who::Maker => &self.maker,
            Who::Taker => &self.taker,
        }
    }
}

#[derive(Clone, Copy)]
enum Who {
    Maker,
    Taker,
}

// make + take

// Tests:   make, then take: the full happy path. The taker has never held token A and the maker
//          has never held token B, so `take` creates both token accounts (init_if_needed).
// Against: the taker ends with the deposit, the maker with the payment, escrow and vault are
//          closed, the escrow's rent goes back to the maker, and the offer can't be taken twice.
#[test]
fn test_make_and_take() {
    let mut f = fixture();
    f.make(NOW + ONE_HOUR);

    // Remember a few numbers so we can check the rent refund after the take
    let maker_lamports_before = f.svm.get_balance(&f.maker.pubkey()).unwrap();
    let escrow_rent = f.svm.get_balance(&f.escrow).unwrap();

    let tx = f.take().unwrap();
    msg!("Take CUs consumed: {}", tx.compute_units_consumed);

    assert_taken(&f, RECEIVE);

    // `close = maker` on the escrow sent its rent back to the maker
    assert_eq!(
        f.svm.get_balance(&f.maker.pubkey()).unwrap(),
        maker_lamports_before + escrow_rent
    );

    // The offer no longer exists, so a second take fails
    assert_fails_with(f.take(), AnchorError::AccountNotInitialized as u32);
}

// Tests:   take when both destination token accounts already exist.
// Against: the "already there" branch of init_if_needed works just like the "create it" branch.
#[test]
fn test_take_when_token_accounts_already_exist() {
    let mut f = fixture();
    CreateAssociatedTokenAccount::new(&mut f.svm, &f.taker, &f.mint_a)
        .send()
        .unwrap();
    CreateAssociatedTokenAccount::new(&mut f.svm, &f.maker, &f.mint_b)
        .send()
        .unwrap();
    f.make(NOW + ONE_HOUR);

    f.take().unwrap();

    assert_taken(&f, RECEIVE);
}

// Tests:   take one second before the expiration time.
// Against: an offer is still open up to (but not including) its expiration time.
#[test]
fn test_take_one_second_before_expiration() {
    let mut f = fixture();
    f.make(NOW + ONE_HOUR);

    set_time(&mut f.svm, NOW + ONE_HOUR - 1);
    f.take().unwrap();

    assert_taken(&f, RECEIVE);
}

// Tests:   take exactly at the expiration time.
// Against: EscrowExpired (the offer is closed to takers), and nothing moves.
#[test]
fn test_take_at_expiration_fails() {
    let mut f = fixture();
    f.make(NOW + ONE_HOUR);

    set_time(&mut f.svm, NOW + ONE_HOUR);
    assert_fails_with(f.take(), u32::from(EscrowError::EscrowExpired));

    // A failed transaction changes nothing: the deposit is still locked in the vault
    assert!(f.svm.get_account(&f.escrow).is_some());
    assert_eq!(token_balance(&f.svm, &f.vault), DEPOSIT);
    assert_eq!(token_balance(&f.svm, &f.taker_ata_b), STARTING_BALANCE);
}

// Tests:   take when the taker holds less token B than the maker asks for.
// Against: the token program's InsufficientFunds error (the escrow stays untouched).
#[test]
fn test_take_with_insufficient_token_b_fails() {
    let mut f = fixture();

    // The maker asks for 2,000 token B but the taker only holds 1,000
    f.make_with(DEPOSIT, 2 * STARTING_BALANCE, NOW + ONE_HOUR)
        .unwrap();

    assert_fails_with(
        f.take(),
        spl_token::error::TokenError::InsufficientFunds as u32,
    );

    assert!(f.svm.get_account(&f.escrow).is_some());
    assert_eq!(token_balance(&f.svm, &f.vault), DEPOSIT);
}

// Tests:   take where the taker pays with a different token than the one the maker asked for.
// Against: the classic fake-token attack: `has_one = mint_b` on the escrow rejects it.
#[test]
fn test_take_with_wrong_mint_b_fails() {
    let mut f = fixture();
    f.make(NOW + ONE_HOUR);

    // A worthless look-alike token the taker tries to pay with instead of token B
    let fake_mint = CreateMint::new(&mut f.svm, &f.taker)
        .decimals(6)
        .send()
        .unwrap();
    let fake_ata = CreateAssociatedTokenAccount::new(&mut f.svm, &f.taker, &fake_mint)
        .send()
        .unwrap();
    MintTo::new(
        &mut f.svm,
        &f.taker,
        &fake_mint,
        &fake_ata,
        STARTING_BALANCE,
    )
    .send()
    .unwrap();

    assert_fails_with(
        f.take_paying_with(fake_mint, fake_ata),
        AnchorError::ConstraintHasOne as u32,
    );

    assert!(f.svm.get_account(&f.escrow).is_some());
    assert_eq!(token_balance(&f.svm, &f.vault), DEPOSIT);
}

// refund

// Tests:   refund long after the offer expired.
// Against: the maker can reclaim everything at any time, and escrow + vault are closed.
#[test]
fn test_refund_returns_tokens_even_after_expiration() {
    let mut f = fixture();
    f.make(NOW + ONE_HOUR);
    assert_eq!(
        token_balance(&f.svm, &f.maker_ata_a),
        STARTING_BALANCE - DEPOSIT
    );

    set_time(&mut f.svm, NOW + 10 * ONE_HOUR);
    f.refund().unwrap();

    assert_eq!(token_balance(&f.svm, &f.maker_ata_a), STARTING_BALANCE);
    assert!(f.svm.get_account(&f.escrow).is_none());
    assert!(f.svm.get_account(&f.vault).is_none());
}

// Tests:   refund signed by someone who is not the maker (they send the vault to their own account).
// Against: ConstraintSeeds: the escrow's address is derived from the real maker's key, so it can't
//          match the attacker's.
#[test]
fn test_refund_by_non_maker_fails() {
    let mut f = fixture();
    f.make(NOW + ONE_HOUR);

    let attacker_ata_a = CreateAssociatedTokenAccount::new(&mut f.svm, &f.taker, &f.mint_a)
        .send()
        .unwrap();

    assert_fails_with(
        f.refund_as_taker(attacker_ata_a),
        AnchorError::ConstraintSeeds as u32,
    );

    // The deposit is still locked in the vault
    assert!(f.svm.get_account(&f.escrow).is_some());
    assert_eq!(token_balance(&f.svm, &f.vault), DEPOSIT);
    assert_eq!(token_balance(&f.svm, &attacker_ata_a), 0);
}

// update

// Tests:   make, then update the price only, then the deadline only, then take.
// Against: each update changes only the field passed (`None` = leave it), never the owner, the
//          mints or the locked tokens, and the next take pays the updated price.
#[test]
fn test_make_and_update() {
    let mut f = fixture();
    f.make(NOW + ONE_HOUR);

    // New price only: the expiration must stay as it was
    f.update(Some(7_000_000), None).unwrap();
    let escrow = f.escrow_state();
    assert_eq!(escrow.receive, 7_000_000);
    assert_eq!(escrow.expiration, NOW + ONE_HOUR);

    // New deadline only: the price from the previous step must stay
    f.update(None, Some(NOW + 5 * ONE_HOUR)).unwrap();
    let escrow = f.escrow_state();
    assert_eq!(escrow.receive, 7_000_000);
    assert_eq!(escrow.expiration, NOW + 5 * ONE_HOUR);

    // Update never touches who the offer belongs to, what it is about, or the locked tokens
    assert_eq!(escrow.seed, SEED);
    assert_eq!(escrow.maker, f.maker.pubkey());
    assert_eq!(escrow.mint_a, f.mint_a);
    assert_eq!(escrow.mint_b, f.mint_b);
    assert_eq!(token_balance(&f.svm, &f.vault), DEPOSIT);

    // The taker now has to pay the updated price
    f.take().unwrap();
    assert_taken(&f, 7_000_000);
}

// Tests:   update the deadline of an offer that has already expired.
// Against: an expired offer can be revived (instead of refunding and starting over).
#[test]
fn test_update_can_revive_an_expired_escrow() {
    let mut f = fixture();
    f.make(NOW + ONE_HOUR);

    // Two hours later the offer has expired and nobody can take it
    set_time(&mut f.svm, NOW + 2 * ONE_HOUR);
    assert_fails_with(f.take(), u32::from(EscrowError::EscrowExpired));

    // The maker pushes the deadline into the future
    f.update(None, Some(NOW + 5 * ONE_HOUR)).unwrap();

    f.take().unwrap();
    assert_taken(&f, RECEIVE);
}

// Tests:   update with nothing to change, a price of zero, and a deadline that is not in the future.
// Against: NothingToUpdate, InvalidAmount and InvalidExpiration, and the escrow stays unchanged.
#[test]
fn test_update_rejects_invalid_input() {
    let mut f = fixture();
    f.make(NOW + ONE_HOUR);

    assert_fails_with(
        f.update(None, None),
        u32::from(EscrowError::NothingToUpdate),
    );
    assert_fails_with(
        f.update(Some(0), None),
        u32::from(EscrowError::InvalidAmount),
    );
    // "right now" and "in the past" both count as not in the future
    for bad_expiration in [NOW, NOW - 1] {
        assert_fails_with(
            f.update(None, Some(bad_expiration)),
            u32::from(EscrowError::InvalidExpiration),
        );
    }

    let escrow = f.escrow_state();
    assert_eq!(escrow.receive, RECEIVE);
    assert_eq!(escrow.expiration, NOW + ONE_HOUR);
}

// Tests:   update signed by someone who is not the maker (they try to make the offer dirt cheap).
// Against: ConstraintSeeds: the escrow's address is derived from the real maker's key, so it can't
//          match the attacker's.
#[test]
fn test_update_by_non_maker_fails() {
    let mut f = fixture();
    f.make(NOW + ONE_HOUR);

    assert_fails_with(
        f.update_as_taker(Some(1), None),
        AnchorError::ConstraintSeeds as u32,
    );

    assert_eq!(f.escrow_state().receive, RECEIVE);
}
