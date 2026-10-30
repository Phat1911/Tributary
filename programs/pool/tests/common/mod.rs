//! Shared program-test harness for the pool program suites.
//!
//! Everything goes through the PUBLIC instruction surface: instructions are
//! built by hand (Anchor discriminators + borsh args) and sent through a
//! `BanksClient`, exactly like an external integrator would. Nothing seeds
//! program state directly (workflow step 12) — the only non-program writes
//! are the USDC mint and its token accounts, which on devnet belong to
//! Circle's faucet, not to us.
//!
//! # Why the raw SPL instructions below?
//!
//! The harness rides the agave-4.1.2 granular-crate lattice (see the
//! dev-dependency notes in Cargo.toml), and the convenience wrapper crate
//! for SPL token instructions is typed against a different Pubkey cohort
//! than this lattice — mixing them does not compile. Hand-rolling the five
//! Token-v3 instructions we need (tags and layouts are stable public
//! protocol constants, verified against spl-token-interface 2.0.0's source)
//! keeps the harness dependency-light and teaches the actual wire format.

#![allow(dead_code)] // shared harness: suites use different subsets

use borsh::BorshSerialize;
use sha2::{Digest, Sha256};
use solana_commitment_config::CommitmentLevel;
use solana_instruction::{AccountMeta, Instruction};
use solana_instruction_error::InstructionError;
use solana_keypair::Keypair;
use solana_message::Message;
use solana_program_test::{BanksClient, BanksClientError, ProgramTest};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_system_interface::instruction::create_account as system_create_account;
use solana_transaction::Transaction;
use solana_transaction_error::TransactionError;
// The program's own world: anchor 1.2 keeps ITS Pubkey (3.x line) inside
// the program; `AccountInfo` and `ProgramResult` are single-version crates
// shared with the harness.
// The program's own world: anchor 1.2 keeps ITS Pubkey (3.x line) inside
// the program. Tests never touch those types — only raw bytes cross
// (`pool::ID.to_bytes()` below).
// Token-2022 state PARSING (extension-aware). Its internal Pubkey type
// never crosses our boundary: we read scalars and compare raw bytes.
use spl_token_2022_interface::extension::StateWithExtensions;
use spl_token_2022_interface::state as token22_state;

/// Classic SPL Token v3 program id (USDC's program on devnet). Bytes are
/// the decoded form of the well-known address — self-checked at start().
pub const TOKEN_V3: Pubkey = Pubkey::new_from_array([
    0x06, 0xdd, 0xf6, 0xe1, 0xd7, 0x65, 0xa1, 0x93, 0xd9, 0xcb, 0xe1, 0x46, 0xce, 0xeb, 0x79, 0xac,
    0x1c, 0xb4, 0x85, 0xed, 0x5f, 0x5b, 0x37, 0x91, 0x3a, 0x8c, 0xf5, 0x85, 0x7e, 0xff, 0x00, 0xa9,
]);

/// Token-2022 program id (the claim registry's program).
pub const TOKEN_2022: Pubkey = Pubkey::new_from_array([
    0x06, 0xdd, 0xf6, 0xe1, 0xee, 0x75, 0x8f, 0xde, 0x18, 0x42, 0x5d, 0xbc, 0xe4, 0x6c, 0xcd, 0xda,
    0xb6, 0x1a, 0xfc, 0x4d, 0x83, 0xb9, 0x0d, 0x27, 0xfe, 0xbd, 0xf9, 0x28, 0xd8, 0xa1, 0x8b, 0xfc,
]);

/// Rent sysvar address (Initialize declares it — see the account docs in
/// lib.rs; host-test execution has no syscalls).
pub const RENT_SYSVAR: Pubkey = Pubkey::new_from_array([
    0x06, 0xa7, 0xd5, 0x17, 0x19, 0x2c, 0x5c, 0x51, 0x21, 0x8c, 0xc9, 0x4c, 0x3d, 0x4a, 0xf1, 0x7f,
    0x58, 0xda, 0xee, 0x08, 0x9b, 0xa1, 0xfd, 0x44, 0xe3, 0xdb, 0xd9, 0x8a, 0x00, 0x00, 0x00, 0x00,
]);

/// Token v3 `Mint` account length: COption tag(4)+key(32), supply u64,
/// decimals u8, is_initialized u8, freeze COption(36) = 82.
pub const MINT_LEN: usize = 82;
/// Token v3 `Account` length (mint 32, owner 32, amount u64, three
/// authorities/options, flags) = 165.
pub const ACCOUNT_LEN: usize = 165;

/// Anchor custom-error offset: `#[error_code]` variants are numbered from
/// 6000 in DECLARATION ORDER (errors.rs) — the suites pin that order by
/// referencing variant indices through this helper.
pub const ANCHOR_ERROR_OFFSET: u32 = 6000;

// ---------------------------------------------------------------------------
// Program addresses (mirror of the seeds in state.rs). The program id
// crosses the anchor/harness cohort boundary as BYTES only.
// ---------------------------------------------------------------------------

/// The program id in the harness's Pubkey world.
pub fn program_id() -> Pubkey {
    Pubkey::new_from_array(pool::ID.to_bytes())
}

pub mod addrs {
    use super::*;

    /// The singleton config PDA.
    pub fn pool() -> Pubkey {
        Pubkey::find_program_address(&[pool::SEED_POOL], &program_id()).0
    }

    /// The USDC escrow vault token account PDA.
    pub fn vault() -> Pubkey {
        Pubkey::find_program_address(&[pool::SEED_POOL, pool::SEED_VAULT], &program_id()).0
    }

    /// The Token-2022 claim mint PDA.
    pub fn claim_mint() -> Pubkey {
        Pubkey::find_program_address(&[pool::SEED_POOL, pool::SEED_CLAIM_MINT], &program_id()).0
    }

    /// A depositor's claim token account PDA.
    pub fn claim(depositor: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(&[pool::SEED_CLAIM, depositor.as_ref()], &program_id()).0
    }
}

// ---------------------------------------------------------------------------
// Raw Token-v3 instruction builders (wire format from the SPL spec; tags
// verified against spl-token-interface 2.0.0 `instruction.rs`).
// ---------------------------------------------------------------------------

mod raw_token {
    use super::*;

    /// `InitializeMint2` (tag 20): decimals, mint authority, freeze
    /// authority COption (u32 tag + optional key).
    pub fn initialize_mint2(mint: &Pubkey, mint_authority: &Pubkey, decimals: u8) -> Instruction {
        let mut data = vec![20u8, decimals];
        data.extend_from_slice(&mint_authority.to_bytes());
        data.extend_from_slice(&[0u8; 4]); // COption::None (freeze authority)
        Instruction {
            program_id: super::TOKEN_V3,
            accounts: vec![AccountMeta::new(*mint, false)],
            data,
        }
    }

    /// `InitializeAccount3` (tag 18): owner pubkey; no rent sysvar needed.
    pub fn initialize_account3(account: &Pubkey, owner: &Pubkey, mint: &Pubkey) -> Instruction {
        let mut data = vec![18u8];
        data.extend_from_slice(&owner.to_bytes());
        Instruction {
            program_id: super::TOKEN_V3,
            accounts: vec![
                AccountMeta::new(*account, false),
                AccountMeta::new_readonly(*mint, false),
            ],
            data,
        }
    }

    /// `Transfer` (tag 3): LE u64 amount.
    pub fn transfer(
        source: &Pubkey,
        destination: &Pubkey,
        authority: &Pubkey,
        amount: u64,
    ) -> Instruction {
        let mut data = vec![3u8];
        data.extend_from_slice(&amount.to_le_bytes());
        Instruction {
            program_id: super::TOKEN_V3,
            accounts: vec![
                AccountMeta::new(*source, false),
                AccountMeta::new(*destination, false),
                AccountMeta::new_readonly(*authority, true),
            ],
            data,
        }
    }

    /// `MintTo` (tag 7): LE u64 amount.
    pub fn mint_to(
        mint: &Pubkey,
        destination: &Pubkey,
        authority: &Pubkey,
        amount: u64,
    ) -> Instruction {
        let mut data = vec![7u8];
        data.extend_from_slice(&amount.to_le_bytes());
        Instruction {
            program_id: super::TOKEN_V3,
            accounts: vec![
                AccountMeta::new(*mint, false),
                AccountMeta::new(*destination, false),
                AccountMeta::new_readonly(*authority, true),
            ],
            data,
        }
    }
}

// ---------------------------------------------------------------------------
// State parsing (raw offsets per the v3 layout — see MINT_LEN comment).
// ---------------------------------------------------------------------------

/// u64 amount of a Token v3 account (offset 64: mint 32 + owner 32).
pub fn v3_amount(data: &[u8]) -> u64 {
    u64::from_le_bytes(data[64..72].try_into().unwrap())
}

/// u64 supply of a Token v3 mint (offset 36: authority COption 36).
pub fn v3_supply(data: &[u8]) -> u64 {
    u64::from_le_bytes(data[36..44].try_into().unwrap())
}

/// Token-2022 claim account amount (base state via extension-aware unpack).
pub fn claim_amount(data: &[u8]) -> u64 {
    StateWithExtensions::<token22_state::Account>::unpack(data)
        .expect("token-2022 account parses")
        .base
        .amount
}

/// Token-2022 claim mint supply.
pub fn claim_supply(data: &[u8]) -> u64 {
    StateWithExtensions::<token22_state::Mint>::unpack(data)
        .expect("token-2022 mint parses")
        .base
        .supply
}

// ---------------------------------------------------------------------------
// Validator bootstrap.
// ---------------------------------------------------------------------------

/// Boots the in-process validator with the REAL compiled pool program
/// (`target/deploy/pool.so`) executing inside the SBPF VM — the same
/// artifact that ships to devnet.
///
/// Why the ELF and not a host-compiled builtin (the `processor!` route):
/// anchor 1.2 (solana-v3 cohort) routes host-side syscalls — rent fetch
/// for `init`, and every CPI — through `solana-sysvar` 3.1.1's
/// `program_stubs` slot, while program-test 4.1.2 wires ITS stub only into
/// ITS own solana-sysvar (4.x) copy: two crate copies, two statics. A host
/// builtin therefore dies on `UnsupportedSysvar`/no-op CPIs, and bridging
/// the slots would mean reimplementing program-test's invoke-context
/// plumbing. Loading the ELF sidesteps the entire cohort-skew class: the
/// program gets REAL syscalls, the REAL entry ABI, and the REAL allocator
/// — and the tests exercise the exact bytes that get deployed.
///
/// Consequence: run `anchor build` (or `cargo build-sbf`) before
/// `cargo test -p pool`. A missing ELF is a hard error, never a skip.
pub async fn start() -> (BanksClient, Keypair) {
    // Self-check the hardcoded program ids (decoded offline; verified here
    // against the canonical strings so a bad byte never slips through).
    assert_eq!(
        TOKEN_V3.to_string(),
        "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
    );
    assert_eq!(
        TOKEN_2022.to_string(),
        "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
    );

    // Locate the compiled program: explicit env override first, then the
    // anchor/`cargo build-sbf` output directory at the workspace root.
    // (CARGO_MANIFEST_DIR = programs/pool; workspace root is two levels up.)
    let elf = std::env::var_os("POOL_ELF")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"))
                .join("../../target/deploy/pool.so")
        });
    assert!(
        elf.exists(),
        "compiled program not found at {} — run `anchor build` (or `cargo build-sbf`) before `cargo test -p pool`; \
         or point POOL_ELF at a pool.so",
        elf.display()
    );
    // `add_upgradeable_program_to_genesis` locates `<name>.so` itself via
    // BPF_OUT_DIR, so we export the directory containing our ELF.
    // (`set_var` is unsafe in edition 2024 — it is not thread-safe in
    // general. SAFETY: this runs during single-threaded test bootstrap,
    // before any #[tokio::test] spawns work; no other thread reads the
    // environment concurrently.)
    if std::env::var_os("BPF_OUT_DIR").is_none() {
        unsafe {
            std::env::set_var("BPF_OUT_DIR", elf.parent().expect("elf has a parent"));
        }
    }

    let mut pt = ProgramTest::default();
    pt.add_upgradeable_program_to_genesis("pool", &program_id());
    let (banks, payer, _last_blockhash) = pt.start().await;
    (banks, payer)
}

/// Anchor instruction discriminator: sha256("global:<snake_name>")
/// truncated to 8 bytes. Anchor 1.2 namespaces EVERY instruction under the
/// literal "global" (anchor-syn `SIGHASH_GLOBAL_NAMESPACE` — the #[program]
/// module name is NOT part of the preimage; verified against
/// anchor-syn-1.2.2/src/codegen/program/instruction.rs after the harness
/// first hit `InstructionFallbackNotFound`).
pub fn disc(name: &str) -> Vec<u8> {
    let h = Sha256::digest(format!("global:{name}").as_bytes());
    h[..8].to_vec()
}

/// Builds a program instruction the way Anchor serializes it: discriminator
/// + borsh of the argument tuple.
pub fn ix(name: &str, args: impl BorshSerialize, accounts: Vec<AccountMeta>) -> Instruction {
    let mut data = disc(name);
    let mut args_b = borsh::to_vec(&args).expect("borsh args");
    data.append(&mut args_b);
    Instruction {
        program_id: program_id(),
        accounts,
        data,
    }
}

/// Sends instructions as one transaction signed by payer + extra signers.
/// Err carries the bank client error — adversarial suites match on the
/// transaction error inside it.
pub async fn send(
    banks: &BanksClient,
    payer: &Keypair,
    ixs: Vec<Instruction>,
    signers: &[&Keypair],
) -> Result<(), BanksClientError> {
    let blockhash = banks.get_latest_blockhash().await.unwrap();
    let mut all: Vec<&Keypair> = vec![payer];
    all.extend(signers.iter());
    let mut tx = Transaction::new_unsigned(Message::new(&ixs, Some(&payer.pubkey())));
    tx.sign(&all, blockhash);
    banks
        .process_transaction_with_commitment(tx, CommitmentLevel::Processed)
        .await
}

/// Extracts the `TransactionError::InstructionError(Custom(code))` chain
/// from a failed send, so suites can pin WHICH program error fired (never
/// just "it failed").
pub fn custom_code(err: &BanksClientError) -> Option<u32> {
    let BanksClientError::TransactionError(TransactionError::InstructionError(
        _,
        InstructionError::Custom(c),
    )) = err
    else {
        return None;
    };
    Some(*c)
}

/// Creates a USDC-shaped Token v3 mint (6 decimals — the micro-USD seam)
/// with `payer` as mint authority. Stands in for Circle's devnet USDC.
pub async fn create_usdc_mint(banks: &BanksClient, payer: &Keypair) -> Pubkey {
    let mint = Keypair::new();
    let rent = banks.get_rent().await.unwrap();
    let lamports = rent.minimum_balance(MINT_LEN);
    let create = system_create_account(
        &payer.pubkey(),
        &mint.pubkey(),
        lamports,
        MINT_LEN as u64,
        &TOKEN_V3,
    );
    let init = raw_token::initialize_mint2(&mint.pubkey(), &payer.pubkey(), pool::DECIMALS);
    send(banks, payer, vec![create, init], &[&mint])
        .await
        .unwrap();
    mint.pubkey()
}

/// Creates a plain Token v3 account for `owner` on `mint` (rent paid by
/// `payer`) and returns its address. Depositor source/destination accounts
/// on devnet are exactly this shape (custodial keys).
pub async fn create_token_account(
    banks: &BanksClient,
    payer: &Keypair,
    owner: &Pubkey,
    mint: &Pubkey,
) -> Pubkey {
    let acct = Keypair::new();
    let rent = banks.get_rent().await.unwrap();
    let lamports = rent.minimum_balance(ACCOUNT_LEN);
    let create = system_create_account(
        &payer.pubkey(),
        &acct.pubkey(),
        lamports,
        ACCOUNT_LEN as u64,
        &TOKEN_V3,
    );
    let init = raw_token::initialize_account3(&acct.pubkey(), owner, mint);
    send(banks, payer, vec![create, init], &[&acct])
        .await
        .unwrap();
    acct.pubkey()
}

/// Faucet: mints `amount` of USDC to `dest` (mint authority = payer).
pub async fn faucet_usdc(
    banks: &BanksClient,
    payer: &Keypair,
    mint: &Pubkey,
    dest: &Pubkey,
    amount: u64,
) {
    let mint_to = raw_token::mint_to(mint, dest, &payer.pubkey(), amount);
    send(banks, payer, vec![mint_to], &[]).await.unwrap();
}

/// Funds an auxiliary key with lamports from the test payer. The backend
/// key needs SOL to pay claim-account rent (and, on devnet, tx fees) —
/// exactly like the soak client funds its operator keys.
pub async fn fund(banks: &BanksClient, payer: &Keypair, dest: &Pubkey, lamports: u64) {
    use solana_system_interface::instruction::transfer;
    send(
        banks,
        payer,
        vec![transfer(&payer.pubkey(), dest, lamports)],
        &[],
    )
    .await
    .expect("funding transfer lands");
}

/// Reads a Token v3 account's amount.
pub async fn token_balance(banks: &BanksClient, acct: &Pubkey) -> u64 {
    let ai = banks
        .get_account(*acct)
        .await
        .unwrap()
        .expect("token account exists");
    v3_amount(&ai.data)
}

/// Reads a Token-2022 account's amount.
pub async fn token_2022_balance(banks: &BanksClient, acct: &Pubkey) -> u64 {
    let ai = banks
        .get_account(*acct)
        .await
        .unwrap()
        .expect("token account exists");
    claim_amount(&ai.data)
}

// ---------------------------------------------------------------------------
// Instruction builders — account lists mirror the Accounts structs in
// lib.rs field-for-field (order matters on the wire).
// ---------------------------------------------------------------------------

pub mod ixs {
    use super::*;

    /// `initialize(admin_authority, backend_authority)`
    pub fn initialize(
        payer: &Keypair,
        usdc_mint: &Pubkey,
        admin: &Pubkey,
        backend: &Pubkey,
    ) -> Instruction {
        ix(
            "initialize",
            (admin, backend),
            vec![
                AccountMeta::new(addrs::pool(), false),
                AccountMeta::new(addrs::claim_mint(), false),
                AccountMeta::new_readonly(*usdc_mint, false),
                AccountMeta::new(addrs::vault(), false),
                AccountMeta::new(payer.pubkey(), true),
                AccountMeta::new_readonly(RENT_SYSVAR, false),
                AccountMeta::new_readonly(TOKEN_V3, false),
                AccountMeta::new_readonly(TOKEN_2022, false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            ],
        )
    }

    /// `deposit(amount)` — signers: backend + depositor (custodial pair).
    #[allow(clippy::too_many_arguments)]
    pub fn deposit(
        pool: &Pubkey,
        backend: &Pubkey,
        depositor: &Pubkey,
        source: &Pubkey,
        vault: &Pubkey,
        claim_mint: &Pubkey,
        claim: &Pubkey,
        amount: u64,
    ) -> Instruction {
        ix(
            "deposit",
            (amount,),
            vec![
                AccountMeta::new(*pool, false),
                AccountMeta::new(*backend, true), // backend_authority (mut, signer)
                AccountMeta::new_readonly(*depositor, true),
                AccountMeta::new(*source, false),
                AccountMeta::new(*vault, false),
                AccountMeta::new(*claim_mint, false),
                AccountMeta::new(*claim, false),
                AccountMeta::new_readonly(TOKEN_V3, false),
                AccountMeta::new_readonly(TOKEN_2022, false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            ],
        )
    }

    /// `withdraw(depositor, amount)` — sole signer: the backend.
    #[allow(clippy::too_many_arguments)]
    pub fn withdraw(
        pool: &Pubkey,
        backend: &Pubkey,
        destination: &Pubkey,
        depositor: &Pubkey,
        vault: &Pubkey,
        claim_mint: &Pubkey,
        claim: &Pubkey,
        amount: u64,
    ) -> Instruction {
        ix(
            "withdraw",
            (depositor, amount),
            vec![
                AccountMeta::new(*pool, false),
                AccountMeta::new(*backend, true),
                AccountMeta::new(*destination, false),
                AccountMeta::new(*vault, false),
                AccountMeta::new(*claim_mint, false),
                AccountMeta::new(*claim, false),
                AccountMeta::new_readonly(TOKEN_V3, false),
                AccountMeta::new_readonly(TOKEN_2022, false),
            ],
        )
    }

    /// `pause()` / `resume()` — signer: admin authority.
    pub fn toggle(name: &str, pool: &Pubkey, admin: &Pubkey) -> Instruction {
        ix(
            name,
            (),
            vec![
                AccountMeta::new(*pool, false),
                AccountMeta::new_readonly(*admin, true),
            ],
        )
    }

    /// `set_backend_authority(new)` / `set_admin_authority(new)`.
    pub fn rotate(name: &str, pool: &Pubkey, admin: &Pubkey, new: &Pubkey) -> Instruction {
        ix(
            name,
            (new,),
            vec![
                AccountMeta::new(*pool, false),
                AccountMeta::new_readonly(*admin, true),
            ],
        )
    }
}
