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
use solana_account::Account;
use solana_commitment_config::CommitmentLevel;
use solana_instruction::{AccountMeta, Instruction};
use solana_instruction_error::InstructionError;
use solana_keypair::Keypair;
use solana_message::Message;
use solana_program_test::{BanksClient, BanksClientError, ProgramTest, ProgramTestContext};
use solana_pubkey::Pubkey;
use solana_rent::Rent;
use solana_signer::Signer;
use solana_system_interface::instruction::create_account as system_create_account;
use solana_transaction::Transaction;
// The simulate path takes the versioned shape; `From<Transaction>` converts
// the signed legacy transaction the harness builds.
use solana_transaction::versioned::VersionedTransaction;
// The subscriber's maps: ordered, so two mirrors compare deterministically.
use solana_transaction_error::TransactionError;
use std::collections::BTreeMap;
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

/// BPF Upgradeable Loader program id — owner of the ProgramData account
/// `initialize`'s A-041 gate reads (the deployment authority lives there).
/// Same convention as the token ids above: bytes decoded offline,
/// self-checked against the canonical string in `start()`.
pub const BPF_LOADER_UPGRADEABLE: Pubkey = Pubkey::new_from_array([
    0x02, 0xa8, 0xf6, 0x91, 0x4e, 0x88, 0xa1, 0xb0, 0xe2, 0x10, 0x15, 0x3e, 0xf7, 0x63, 0xae, 0x2b,
    0x00, 0xc2, 0xb9, 0x3d, 0x16, 0xc1, 0x24, 0xd2, 0xc0, 0x53, 0x7a, 0x10, 0x04, 0x80, 0x00, 0x00,
]);

/// ProgramData header size (the loader's own constant, hand-mirrored):
/// u32 variant tag + u64 slot + u8 option tag + Pubkey authority = 45.
pub const PROGRAMDATA_HEADER: usize = 45;

/// The rig's deployment authority (A-041): the upgrade authority the
/// genesis ProgramData override names. Random per PROCESS (never
/// committed, never logged) — every suite signs initialize with it via
/// [`deploy_authority`]. On devnet the same slot is the Squads multisig
/// vault PDA; here a plain keypair stands in, exactly like the fixture
/// admin stands in for the multisig on admin instructions.
static DEPLOY_AUTHORITY: std::sync::OnceLock<Keypair> = std::sync::OnceLock::new();

/// The deployment authority keypair (same instance for every `start()` in
/// this process). Callers pass `&[common::deploy_authority()]` as extra
/// signers on `initialize` transactions.
pub fn deploy_authority() -> &'static Keypair {
    DEPLOY_AUTHORITY.get_or_init(Keypair::new)
}

/// Rent sysvar address (Initialize declares it — see the account docs in
/// lib.rs; host-test execution has no syscalls).
pub const RENT_SYSVAR: Pubkey = Pubkey::new_from_array([
    0x06, 0xa7, 0xd5, 0x17, 0x19, 0x2c, 0x5c, 0x51, 0x21, 0x8c, 0xc9, 0x4c, 0x3d, 0x4a, 0xf1, 0x7f,
    0x58, 0xda, 0xee, 0x08, 0x9b, 0xa1, 0xfd, 0x44, 0xe3, 0xdb, 0xd9, 0x8a, 0x00, 0x00, 0x00, 0x00,
]);

/// Clock sysvar address. `post_root` and `challenge` DECLARE the clock the
/// same way `initialize` declares rent (see lib.rs): the program reads the
/// slot from the account the test passes, so host and SBF execution share
/// one source of time. Taken from the program crate's own re-export so the
/// two sides can never disagree, and self-checked against the canonical
/// string in `boot`.
pub const CLOCK_SYSVAR: Pubkey =
    Pubkey::new_from_array(anchor_lang::solana_program::clock::sysvar::ID.to_bytes());

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

    /// A renter's prepaid record PDA (the `prepaid.renter` mirror).
    pub fn prepaid(renter: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(&[pool::SEED_PREPAID, renter.as_ref()], &program_id()).0
    }

    /// The BPF-loader ProgramData PDA of the pool program — where the
    /// deployment (upgrade) authority lives on chain. Same derivation the
    /// program's A-041 gate performs: `find_program_address(&[program],
    /// &bpf_loader_upgradeable)`.
    pub fn program_data() -> Pubkey {
        Pubkey::find_program_address(&[program_id().as_ref()], &BPF_LOADER_UPGRADEABLE).0
    }

    // ---- M11 settlement (seeds mirrored from programs/pool/src/settlement.rs)

    /// A posted settlement root: `[settlement_root, window_le_u64]` — one
    /// address per window, EVER (invariant 7).
    pub fn root(window: u64) -> Pubkey {
        Pubkey::find_program_address(&[pool::SEED_ROOT, &window.to_le_bytes()], &program_id()).0
    }

    /// A claim marker: `[claim_marker, root_pubkey, leaf_index_le_u32]` —
    /// one address per leaf of a root, EVER (invariant 3).
    pub fn claim_marker(root: &Pubkey, leaf_index: u32) -> Pubkey {
        Pubkey::find_program_address(
            &[
                pool::SEED_CLAIM_MARKER,
                root.as_ref(),
                &leaf_index.to_le_bytes(),
            ],
            &program_id(),
        )
        .0
    }

    /// The operator bond record: `[pool, bond]`.
    pub fn bond() -> Pubkey {
        Pubkey::find_program_address(&[pool::SEED_POOL, pool::SEED_BOND], &program_id()).0
    }

    /// The operator bond VAULT token account: `[pool, bond_vault]`.
    pub fn bond_vault() -> Pubkey {
        Pubkey::find_program_address(&[pool::SEED_POOL, pool::SEED_BOND_VAULT], &program_id()).0
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
///
/// Returns the validator BEFORE it starts: [`start`] and
/// [`start_with_context`] both begin here, so every suite boots the
/// identical rig.
pub fn boot() -> ProgramTest {
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
    assert_eq!(
        BPF_LOADER_UPGRADEABLE.to_string(),
        "BPFLoaderUpgradeab1e11111111111111111111111"
    );
    assert_eq!(
        CLOCK_SYSVAR.to_string(),
        "SysvarC1ock11111111111111111111111111111111"
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

    // ---- A-041 rig: give the genesis ProgramData a SIGNABLE upgrade
    // authority -----------------------------------------------------------
    // `add_upgradeable_program_to_genesis` (solana-program-binaries
    // 4.1.2) pins the authority to `Some(Pubkey::default())` — the system
    // program id, which nobody can sign as, so `initialize`'s
    // deployment-authority gate would be untestable. ProgramTest's
    // post-genesis `add_account` path is the sanctioned override ("Over-
    // riding account at …", program-test lib.rs): we replace the
    // ProgramData bytes with an identically-shaped account whose declared
    // authority is this process's random [`deploy_authority`] keypair.
    //
    // Layout hand-mirrored from solana-loader-v3-interface 6.1.1
    // (`UpgradeableLoaderState::ProgramData`, bincode): u32 variant tag
    // 3, u64 slot, u8 option tag (1 = Some), 32-byte authority, then the
    // ELF. The same offsets were re-derived from LIVE devnet bytes during
    // the soak (AUDIT A-040) — never parse from memory.
    let elf_bytes = std::fs::read(&elf)
        .unwrap_or_else(|e| panic!("failed reading compiled program at {}: {e}", elf.display()));
    let authority = deploy_authority();
    let mut pd_data = Vec::with_capacity(PROGRAMDATA_HEADER + elf_bytes.len());
    pd_data.extend_from_slice(&3u32.to_le_bytes()); // ProgramData variant
    pd_data.extend_from_slice(&0u64.to_le_bytes()); // slot 0
    pd_data.push(1u8); // COption::Some — a signable authority exists
    pd_data.extend_from_slice(authority.pubkey().as_ref());
    pd_data.extend_from_slice(&elf_bytes);
    let pd_space = pd_data.len();
    pt.add_account(
        addrs::program_data(),
        Account {
            lamports: Rent::default().minimum_balance(pd_space),
            data: pd_data,
            owner: BPF_LOADER_UPGRADEABLE,
            executable: false,
            rent_epoch: u64::MAX,
        },
    );
    pt
}

/// Boots the rig and returns a client plus a funded payer — the entry point
/// every M8 suite uses (its behavior is unchanged by the M11-A split that
/// extracted [`boot`]: same program bytes, same A-041 override).
pub async fn start() -> (BanksClient, Keypair) {
    let (banks, payer, _last_blockhash) = boot().start().await;
    (banks, payer)
}

/// Boots the rig and keeps the [`ProgramTestContext`]. The M11 suite needs
/// it for ONE thing the bare client cannot do: `warp_to_slot`. The
/// challenge window is measured in slots, and only the context can move the
/// clock forward ("inside the window" has meaning only once a test can also
/// stand outside it).
pub async fn start_with_context() -> ProgramTestContext {
    boot().start_with_context().await
}

/// Moves the clock PAST the challenge window of a root posted in `posted_at`
/// (invariant 11: no payout leaves the vault before the window has passed) —
/// one slot past `posted_at + CHALLENGE_WINDOW_SLOTS`, inclusive. Returns
/// the slot warped to. A root posted AFTER this call starts its own window
/// at the warped slot, so it can still be challenged.
pub fn warp_past_window(ctx: &mut ProgramTestContext, posted_at: i64) -> u64 {
    let target = u64::try_from(posted_at + pool::CHALLENGE_WINDOW_SLOTS + 1)
        .expect("a slot past the challenge window is a positive number");
    ctx.warp_to_slot(target)
        .unwrap_or_else(|e| panic!("the clock must warp past the challenge window: {e}"));
    target
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

/// Reads a renter's on-chain prepaid record (the refund ceiling — an
/// on-chain FACT, never an operator attestation).
pub async fn prepaid_amount(banks: &BanksClient, renter: &Pubkey) -> u64 {
    let ai = banks
        .get_account(addrs::prepaid(renter))
        .await
        .unwrap()
        .expect("prepaid record exists (prepay first)");
    let rec: pool::RenterPrepaid =
        anchor_lang::AccountDeserialize::try_deserialize(&mut &ai.data[..])
            .expect("prepaid record deserializes");
    rec.amount
}

/// Reads a posted settlement root. `None` while the window has no root —
/// "nothing was created" is itself a pinned outcome (invariant 1).
pub async fn settlement_root(banks: &BanksClient, window: u64) -> Option<pool::SettlementRoot> {
    let ai = banks.get_account(addrs::root(window)).await.unwrap()?;
    Some(
        anchor_lang::AccountDeserialize::try_deserialize(&mut &ai.data[..])
            .expect("settlement root deserializes"),
    )
}

/// Reads a claim marker. `None` while the leaf is unpaid — the marker's
/// EXISTENCE is the one-claim fact (invariant 3).
pub async fn claim_marker(
    banks: &BanksClient,
    root: &Pubkey,
    leaf_index: u32,
) -> Option<pool::ClaimMarker> {
    let ai = banks
        .get_account(addrs::claim_marker(root, leaf_index))
        .await
        .unwrap()?;
    Some(
        anchor_lang::AccountDeserialize::try_deserialize(&mut &ai.data[..])
            .expect("claim marker deserializes"),
    )
}

/// Reads the operator bond record's amount (the on-chain twin of the bond
/// vault's USDC balance).
pub async fn bond_amount(banks: &BanksClient) -> u64 {
    let ai = banks
        .get_account(addrs::bond())
        .await
        .unwrap()
        .expect("bond record exists (fund_bond first)");
    let rec: pool::OperatorBond =
        anchor_lang::AccountDeserialize::try_deserialize(&mut &ai.data[..])
            .expect("bond record deserializes");
    rec.amount
}

/// Reads a Token v3 account's OWNER (offset 32: mint 32, then owner 32) —
/// "who can move this balance" is a byte-level fact.
pub fn v3_owner(data: &[u8]) -> Pubkey {
    Pubkey::new_from_array(data[32..64].try_into().expect("32 owner bytes"))
}

/// Runs the instructions in SIMULATION (no state commit) and returns the
/// program log lines — BanksClient's simulate path is the one surface that
/// exposes logs, and `emit!` events land there as
/// `Program data: <base64>`. (The M8 audit suite carries a private twin of
/// this helper; this one is shared so the settlement suite can decode
/// events the same way.)
pub async fn simulate_logs(
    banks: &BanksClient,
    payer: &Keypair,
    instructions: Vec<Instruction>,
    signers: &[&Keypair],
) -> Vec<String> {
    let blockhash = banks.get_latest_blockhash().await.unwrap();
    let mut all: Vec<&Keypair> = vec![payer];
    all.extend(signers.iter());
    let mut tx = Transaction::new_unsigned(Message::new(&instructions, Some(&payer.pubkey())));
    tx.sign(&all, blockhash);
    let res = banks
        .simulate_transaction(VersionedTransaction::from(tx))
        .await
        .expect("simulation runs");
    res.simulation_details
        .expect("simulation details (logs) present")
        .logs
}

// ---------------------------------------------------------------------------
// Test-side Merkle tree — the OFF-CHAIN operator's view (what the books
// build before posting a root). Deliberately independent of the program's
// verifier: two implementations of one published recipe that must agree.
// Recipe (programs/pool/src/settlement.rs, module docs): leaf = SHA-256(
// depositor ‖ amount_le_u64); node = SHA-256(left ‖ right), ORDER-SENSITIVE;
// an odd level pairs its last node with itself; proof = siblings bottom-up,
// verified by walking the bits of the leaf's index.
// ---------------------------------------------------------------------------
pub mod merkle {
    use super::*;
    use sha2::{Digest, Sha256};

    /// One payout leaf: WHO gets paid, and how much (micro-USDC).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Leaf {
        pub depositor: Pubkey,
        pub amount: u64,
    }

    /// SHA-256, with the harness's own `sha2` (the crate that already
    /// computes the instruction discriminators — no new dependency).
    pub fn sha256(data: &[u8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        out.copy_from_slice(&Sha256::digest(data));
        out
    }

    /// The leaf preimage: depositor pubkey, then the amount as LE u64.
    pub fn leaf_hash(leaf: &Leaf) -> [u8; 32] {
        let mut preimage = Vec::with_capacity(40);
        preimage.extend_from_slice(leaf.depositor.as_ref());
        preimage.extend_from_slice(&leaf.amount.to_le_bytes());
        sha256(&preimage)
    }

    /// An interior node: the LEFT child's hash, then the RIGHT child's.
    /// The preimage is 64 bytes where a leaf's is 40: the two layers
    /// cannot stand in for each other.
    pub fn node_hash(left: [u8; 32], right: [u8; 32]) -> [u8; 32] {
        let mut preimage = Vec::with_capacity(64);
        preimage.extend_from_slice(&left);
        preimage.extend_from_slice(&right);
        sha256(&preimage)
    }

    /// A payout tree: `levels[0]` is the leaf hashes, the last level is the
    /// single root. The leaves are kept too, so a test reads a leaf's
    /// amount and depositor off the tree it posted — never off a copy.
    pub struct Tree {
        leaves: Vec<Leaf>,
        levels: Vec<Vec<[u8; 32]>>,
    }

    impl Tree {
        /// Builds every level above the leaves.
        pub fn new(leaves: &[Leaf]) -> Self {
            assert!(!leaves.is_empty(), "a tree needs at least one leaf");
            let mut levels = vec![leaves.iter().map(leaf_hash).collect::<Vec<_>>()];
            while levels.last().expect("at least the leaf level").len() > 1 {
                let prev = levels.last().expect("at least the leaf level");
                let mut next = Vec::with_capacity(prev.len().div_ceil(2));
                for pair in prev.chunks(2) {
                    // An unpaired last node is its own sibling.
                    next.push(node_hash(pair[0], *pair.last().expect("non-empty chunk")));
                }
                levels.push(next);
            }
            Tree {
                leaves: leaves.to_vec(),
                levels,
            }
        }

        /// Leaf `index`, as it was built.
        pub fn leaf(&self, index: usize) -> Leaf {
            self.leaves[index]
        }

        /// How many leaves the tree was built from.
        pub fn leaf_count(&self) -> usize {
            self.leaves.len()
        }

        /// The sum of the leaf amounts — the `total_payout` a root over this
        /// tree attests to.
        pub fn total(&self) -> u64 {
            self.leaves.iter().map(|leaf| leaf.amount).sum()
        }

        /// The root to hand to `post_root`.
        pub fn root(&self) -> [u8; 32] {
            self.levels.last().expect("at least the leaf level")[0]
        }

        /// The inclusion proof for the leaf at `index`: one sibling per
        /// level, from the leaf's level upward.
        pub fn proof(&self, index: usize) -> Vec<[u8; 32]> {
            let top = self.levels.len() - 1;
            let mut proof = Vec::with_capacity(top);
            let mut at = index;
            for level in &self.levels[..top] {
                // The pair partner: 2k pairs with 2k+1 and vice versa.
                let partner = at ^ 1;
                // An odd level's last node pairs with itself.
                proof.push(level[partner.min(level.len() - 1)]);
                at /= 2;
            }
            proof
        }
    }

    /// Applies a proof the way the program must, walking the bits of
    /// `leaf_index`: an even index puts the running hash on the LEFT of its
    /// sibling, an odd one on the right. Used to build FORGERIES too (flip
    /// a sibling, swap leaves, pick another tree) and to check the tree
    /// builder against itself.
    pub fn apply_proof(leaf_index: u32, mut h: [u8; 32], proof: &[[u8; 32]]) -> [u8; 32] {
        let mut at = leaf_index;
        for sibling in proof {
            h = if at.is_multiple_of(2) {
                node_hash(h, *sibling)
            } else {
                node_hash(*sibling, h)
            };
            at /= 2;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// Event decoding — invariant 8's instrument. Anchor's `emit!` lands one
// `Program data: <base64>` line per event: an 8-byte discriminator
// (sha256("event:<StructName>")[..8]) followed by the borsh fields in
// declaration order.
// ---------------------------------------------------------------------------
pub mod events {
    use super::*;

    /// Every event this program declares — M8's eight and M11's six.
    pub const NAMES: &[&str] = &[
        "PoolOpened",
        "ContributionRecorded",
        "ClaimsBurned",
        "PrepayRecorded",
        "PrepayRefunded",
        "PauseToggled",
        "BackendAuthorityRotated",
        "AdminAuthorityRotated",
        "BondFunded",
        "RootPosted",
        "ClaimPaid",
        "RootChallenged",
        "ChallengeResolved",
        "BondSlashed",
    ];

    /// One emitted event: its declared name and its fields, verbatim.
    #[derive(Debug, PartialEq, Eq)]
    pub struct Decoded {
        pub name: &'static str,
        /// Everything after the 8-byte discriminator.
        pub body: Vec<u8>,
    }

    /// Standard-alphabet base64 decode (padding-aware). Anchor logs event
    /// data through `sol_log_data`, and the harness deliberately carries no
    /// base64 dependency (the M8 audit suite hand-rolls the same 20 lines).
    fn b64_decode(s: &str) -> Vec<u8> {
        fn val(c: u8) -> u32 {
            match c {
                b'A'..=b'Z' => u32::from(c - b'A'),
                b'a'..=b'z' => u32::from(c - b'a' + 26),
                b'0'..=b'9' => u32::from(c - b'0' + 52),
                b'+' => 62,
                b'/' => 63,
                _ => 0, // '=' padding contributes nothing
            }
        }
        let bytes: Vec<u8> = s.bytes().filter(|b| *b != b'=').collect();
        let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
        for chunk in bytes.chunks(4) {
            let b0 = val(chunk[0]) as u8;
            let b1 = val(chunk[1]) as u8;
            out.push((b0 << 2) | (b1 >> 4));
            if chunk.len() > 2 {
                let b2 = val(chunk[2]) as u8;
                out.push((b1 << 4) | (b2 >> 2));
                if chunk.len() > 3 {
                    let b3 = val(chunk[3]) as u8;
                    out.push((b2 << 6) | b3);
                }
            }
        }
        out
    }

    /// `sha256("event:<name>")[..8]` — the same recipe as `disc`, with the
    /// event namespace.
    pub fn event_disc(name: &str) -> [u8; 8] {
        let mut out = [0u8; 8];
        out.copy_from_slice(&Sha256::digest(format!("event:{name}").as_bytes())[..8]);
        out
    }

    /// Decodes EVERY `Program data:` line of a log into a named event. A
    /// line whose discriminator matches no declared event panics — an
    /// undeclared event is exactly the kind of state change a subscriber
    /// could not reconstruct (invariant 8), so it must stop the test.
    pub fn decode(logs: &[String]) -> Vec<Decoded> {
        let known: Vec<([u8; 8], &'static str)> =
            NAMES.iter().map(|name| (event_disc(name), *name)).collect();
        let mut out = Vec::new();
        for line in logs {
            if let Some(payload) = line.strip_prefix("Program data: ") {
                let bytes = b64_decode(payload);
                assert!(
                    bytes.len() >= 8,
                    "event shorter than a discriminator: {line}"
                );
                let (disc, body) = bytes.split_at(8);
                let name = known
                    .iter()
                    .find(|(d, _)| d.as_slice() == disc)
                    .unwrap_or_else(|| {
                        panic!("UNDECLARED event (discriminator {disc:?}) — invariant 8 needs every event named")
                    })
                    .1;
                out.push(Decoded {
                    name,
                    body: body.to_vec(),
                });
            }
        }
        out
    }

    // Field readers for borsh layouts — fixed widths, little-endian.

    /// A `u64` field at `offset` in the body.
    pub fn u64_at(body: &[u8], offset: usize) -> u64 {
        u64::from_le_bytes(body[offset..offset + 8].try_into().expect("u64 field fits"))
    }

    /// An `i64` field (slots and timestamps) at `offset` in the body.
    pub fn i64_at(body: &[u8], offset: usize) -> i64 {
        i64::from_le_bytes(body[offset..offset + 8].try_into().expect("i64 field fits"))
    }

    /// A `u32` field at `offset` in the body.
    pub fn u32_at(body: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(body[offset..offset + 4].try_into().expect("u32 field fits"))
    }

    /// A `Pubkey` field at `offset` in the body.
    pub fn pubkey_at(body: &[u8], offset: usize) -> Pubkey {
        Pubkey::new_from_array(
            body[offset..offset + 32]
                .try_into()
                .expect("32 pubkey bytes"),
        )
    }

    /// A `[u8; 32]` field at `offset` in the body.
    pub fn bytes32_at(body: &[u8], offset: usize) -> [u8; 32] {
        body[offset..offset + 32].try_into().expect("32 bytes")
    }

    /// A `bool` field at `offset` in the body.
    pub fn bool_at(body: &[u8], offset: usize) -> bool {
        body[offset] != 0
    }

    /// The one event of `name` in a decoded list — or a panic naming what
    /// WAS emitted instead (a missing event is a missing state change).
    pub fn one<'a>(decoded: &'a [Decoded], name: &str) -> &'a Decoded {
        decoded.iter().find(|d| d.name == name).unwrap_or_else(|| {
            panic!(
                "no {name} event — emitted: {:?}",
                decoded.iter().map(|d| d.name).collect::<Vec<_>>()
            )
        })
    }

    /// How a subscriber that reads ONLY events sees one settlement root.
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
    pub struct RootMirror {
        pub merkle_root: [u8; 32],
        pub total_payout: u64,
        pub posted_at: i64,
        pub bond: u64,
        pub challenger: Pubkey,
        pub evidence: [u8; 32],
        pub ruled_slash: u64,
        pub victim: Pubkey,
        /// True once the multisig ruled the latest challenge to stand — the
        /// ruling that waives the rest of the window (invariant 11).
        pub stands_ruled: bool,
        /// Micro-USDC this root has paid out: the running sum of its claims
        /// (invariant 12).
        pub paid_out: u64,
    }

    /// How a subscriber that reads ONLY events sees one paid leaf.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct MarkerMirror {
        pub depositor: Pubkey,
        pub amount: u64,
    }

    /// A SUBSCRIBER: the settlement state rebuilt from events alone — no
    /// account is ever read. Invariant 8 is exactly the claim that this
    /// mirror equals the chain once every instruction has run.
    #[derive(Debug, Default, PartialEq, Eq)]
    pub struct Subscriber {
        /// Vault USDC, rebuilt from the `*_after` mirrors (the A-042 contract).
        pub vault: u64,
        /// The operator bond, rebuilt from `BondFunded` / `BondSlashed`.
        pub bond: u64,
        /// Window → the root as the events describe it.
        pub roots: BTreeMap<u64, RootMirror>,
        /// Status by window, beside the immutable root fields.
        pub status: BTreeMap<u64, pool::RootStatus>,
        /// (window, leaf_index) → who was paid, how much.
        pub markers: BTreeMap<(u64, u32), MarkerMirror>,
    }

    impl Subscriber {
        /// Applies ONE decoded event. Offsets are the borsh layouts of the
        /// event structs in programs/pool/src/{lib,settlement}.rs.
        pub fn apply(&mut self, event: &Decoded) {
            let b = &event.body;
            match event.name {
                // ---- M8 events that move the vault mirror
                "PrepayRecorded" | "PrepayRefunded" => {
                    // renter(32) amount(8) vault_balance_after(8) prepaid_after(8)
                    self.vault = u64_at(b, 40);
                }
                // ---- M11 events
                "BondFunded" => {
                    // funder(32) amount(8) bond_after(8)
                    self.bond = u64_at(b, 40);
                }
                "BondSlashed" => {
                    // window(8) amount(8) victim(32) bond_after(8)
                    self.bond = u64_at(b, 48);
                    self.status.insert(u64_at(b, 0), pool::RootStatus::Slashed);
                }
                "RootPosted" => {
                    // window(8) merkle_root(32) total_payout(8) bond(8) posted_at(8)
                    let window = u64_at(b, 0);
                    self.roots.insert(
                        window,
                        RootMirror {
                            merkle_root: bytes32_at(b, 8),
                            total_payout: u64_at(b, 40),
                            bond: u64_at(b, 48),
                            posted_at: i64_at(b, 56),
                            ..RootMirror::default()
                        },
                    );
                    self.status.insert(window, pool::RootStatus::Active);
                }
                "ClaimPaid" => {
                    // window(8) leaf_index(4) depositor(32) amount(8) vault_balance_after(8)
                    let window = u64_at(b, 0);
                    let amount = u64_at(b, 44);
                    self.markers.insert(
                        (window, u32_at(b, 8)),
                        MarkerMirror {
                            depositor: pubkey_at(b, 12),
                            amount,
                        },
                    );
                    self.roots.entry(window).or_default().paid_out += amount;
                    self.vault = u64_at(b, 52);
                }
                "RootChallenged" => {
                    // window(8) challenger(32) evidence(32)
                    let window = u64_at(b, 0);
                    let root = self.roots.entry(window).or_default();
                    root.challenger = pubkey_at(b, 8);
                    root.evidence = bytes32_at(b, 40);
                    // A new challenge outranks an earlier "stands".
                    root.stands_ruled = false;
                    self.status.insert(window, pool::RootStatus::Challenged);
                }
                "ChallengeResolved" => {
                    // window(8) root_stands(1) slash_amount(8) victim(32)
                    let window = u64_at(b, 0);
                    let stands = bool_at(b, 8);
                    let root = self.roots.entry(window).or_default();
                    root.stands_ruled = stands;
                    root.ruled_slash = if stands { 0 } else { u64_at(b, 9) };
                    root.victim = if stands {
                        Pubkey::default()
                    } else {
                        pubkey_at(b, 17)
                    };
                    self.status.insert(
                        window,
                        if stands {
                            pool::RootStatus::Active
                        } else {
                            pool::RootStatus::ProvenFalse
                        },
                    );
                }
                // ---- M8 events that touch no settlement mirror
                "PoolOpened"
                | "ContributionRecorded"
                | "ClaimsBurned"
                | "PauseToggled"
                | "BackendAuthorityRotated"
                | "AdminAuthorityRotated" => {}
                // `decode` only yields names from NAMES, and every name is
                // handled above — a new event must be added HERE.
                _ => unreachable!("every declared event is applied: {}", event.name),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Instruction builders — account lists mirror the Accounts structs in
// lib.rs field-for-field (order matters on the wire).
// ---------------------------------------------------------------------------

pub mod ixs {
    use super::*;

    /// `initialize(admin_authority, backend_authority)` — signer story
    /// since A-041: `payer` (first arg) pays rent, and the DEPLOYMENT
    /// authority ([`deploy_authority`], standing in for the Squads
    /// multisig vault that holds the real upgrade authority on devnet)
    /// must co-sign or the instruction refuses. The two extra metas are
    /// injected here so every call site reads as the wire truth.
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
                // A-041 gate accounts (order mirrors the Initialize
                // struct): the loader's ProgramData (source of the
                // deployment authority) and its required signer.
                AccountMeta::new_readonly(addrs::program_data(), false),
                AccountMeta::new_readonly(deploy_authority().pubkey(), true),
                AccountMeta::new_readonly(RENT_SYSVAR, false),
                AccountMeta::new_readonly(TOKEN_V3, false),
                AccountMeta::new_readonly(TOKEN_2022, false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            ],
        )
    }

    /// `record_contribution(depositor, amount)` — sole signer: backend.
    /// Mints claims; touches NO token account of the depositor (D40).
    pub fn record_contribution(
        pool: &Pubkey,
        backend: &Pubkey,
        depositor: &Pubkey,
        claim_mint: &Pubkey,
        claim: &Pubkey,
        amount: u64,
    ) -> Instruction {
        ix(
            "record_contribution",
            (depositor, amount),
            vec![
                AccountMeta::new(*pool, false),
                AccountMeta::new(*backend, true), // backend_authority (mut, signer)
                AccountMeta::new(*claim_mint, false),
                AccountMeta::new(*claim, false),
                AccountMeta::new_readonly(TOKEN_2022, false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            ],
        )
    }

    /// `burn_claims(depositor, amount)` — two doors: `authority` is the
    /// backend (assisted) or the depositor themselves (direct). Burns
    /// claims; moves NO USDC (D40).
    pub fn burn_claims(
        pool: &Pubkey,
        authority: &Pubkey,
        depositor: &Pubkey,
        claim_mint: &Pubkey,
        claim: &Pubkey,
        amount: u64,
    ) -> Instruction {
        ix(
            "burn_claims",
            (depositor, amount),
            vec![
                AccountMeta::new_readonly(*pool, false),
                AccountMeta::new_readonly(*authority, true), // the one signer (either door)
                AccountMeta::new(*claim_mint, false),
                AccountMeta::new(*claim, false),
                AccountMeta::new_readonly(TOKEN_2022, false),
            ],
        )
    }

    /// `prepay_renter(renter, amount)` — signers: backend (gate) +
    /// renter_key (source owner; custodial on devnet). The ONLY USDC
    /// inflow (D40).
    #[allow(clippy::too_many_arguments)]
    pub fn prepay_renter(
        pool: &Pubkey,
        backend: &Pubkey,
        renter_key: &Pubkey,
        source: &Pubkey,
        vault: &Pubkey,
        prepaid: &Pubkey,
        renter: &Pubkey,
        amount: u64,
    ) -> Instruction {
        ix(
            "prepay_renter",
            (renter, amount),
            vec![
                AccountMeta::new_readonly(*pool, false),
                AccountMeta::new(*backend, true), // backend_authority (mut, signer, rent payer)
                AccountMeta::new_readonly(*renter_key, true), // source owner
                AccountMeta::new(*source, false),
                AccountMeta::new(*vault, false),
                AccountMeta::new(*prepaid, false),
                AccountMeta::new_readonly(TOKEN_V3, false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            ],
        )
    }

    /// `refund_renter(renter, amount)` — two doors: `authority` is the
    /// backend (assisted) or the renter themselves (direct). USDC out of
    /// the vault, bounded by the renter's OWN on-chain prepaid record.
    #[allow(clippy::too_many_arguments)]
    pub fn refund_renter(
        pool: &Pubkey,
        authority: &Pubkey,
        destination: &Pubkey,
        vault: &Pubkey,
        prepaid: &Pubkey,
        renter: &Pubkey,
        amount: u64,
    ) -> Instruction {
        ix(
            "refund_renter",
            (renter, amount),
            vec![
                AccountMeta::new_readonly(*pool, false),
                AccountMeta::new_readonly(*authority, true), // the one signer (either door)
                AccountMeta::new(*destination, false),
                AccountMeta::new(*vault, false),
                AccountMeta::new(*prepaid, false),
                AccountMeta::new_readonly(TOKEN_V3, false),
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

    // ---- M11 settlement. Each builder mirrors its Accounts struct in
    // programs/pool/src/settlement.rs field-for-field, in order.

    /// `fund_bond(amount)` — signer: backend (gate AND rent payer).
    pub fn fund_bond(
        pool: &Pubkey,
        backend: &Pubkey,
        source: &Pubkey,
        usdc_mint: &Pubkey,
        bond_vault: &Pubkey,
        bond: &Pubkey,
        amount: u64,
    ) -> Instruction {
        ix(
            "fund_bond",
            (amount,),
            vec![
                AccountMeta::new_readonly(*pool, false),
                AccountMeta::new(*backend, true), // backend_authority (mut, signer)
                AccountMeta::new(*source, false),
                AccountMeta::new_readonly(*usdc_mint, false),
                AccountMeta::new(*bond_vault, false),
                AccountMeta::new(*bond, false),
                AccountMeta::new_readonly(TOKEN_V3, false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            ],
        )
    }

    /// `post_root(window, merkle_root, total_payout)` — signer: backend.
    /// The vault is READ-ONLY in this list: posting a root cannot move it.
    // Eight parameters — one per account the instruction names, in wire
    // order (the harness convention: builders mirror their Accounts struct).
    #[allow(clippy::too_many_arguments)]
    pub fn post_root(
        pool: &Pubkey,
        backend: &Pubkey,
        settlement_root: &Pubkey,
        vault: &Pubkey,
        bond: &Pubkey,
        window: u64,
        merkle_root: [u8; 32],
        total_payout: u64,
    ) -> Instruction {
        ix(
            "post_root",
            (window, merkle_root, total_payout),
            vec![
                AccountMeta::new_readonly(*pool, false),
                AccountMeta::new(*backend, true), // backend_authority (mut, signer)
                AccountMeta::new(*settlement_root, false),
                AccountMeta::new_readonly(*vault, false),
                AccountMeta::new_readonly(*bond, false),
                AccountMeta::new_readonly(TOKEN_V3, false),
                AccountMeta::new_readonly(CLOCK_SYSVAR, false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            ],
        )
    }

    /// `claim(window, leaf_index, depositor, amount, proof)` — THE TWO
    /// DOORS: `authority` is the backend (assisted) or the leaf's depositor
    /// (direct); `payer` pays the marker's rent (any funded key); the clock
    /// is where the handler reads the slot for the challenge-window rule.
    #[allow(clippy::too_many_arguments)]
    pub fn claim(
        pool: &Pubkey,
        authority: &Pubkey,
        settlement_root: &Pubkey,
        destination: &Pubkey,
        vault: &Pubkey,
        claim_marker: &Pubkey,
        payer: &Pubkey,
        window: u64,
        leaf_index: u32,
        depositor: &Pubkey,
        amount: u64,
        proof: &[[u8; 32]],
    ) -> Instruction {
        ix(
            "claim",
            (window, leaf_index, depositor, amount, proof),
            vec![
                AccountMeta::new_readonly(*pool, false),
                AccountMeta::new_readonly(*authority, true), // the one signer (either door)
                AccountMeta::new_readonly(*settlement_root, false),
                AccountMeta::new(*destination, false),
                AccountMeta::new(*vault, false),
                AccountMeta::new(*claim_marker, false),
                AccountMeta::new(*payer, true), // rent payer
                AccountMeta::new_readonly(CLOCK_SYSVAR, false),
                AccountMeta::new_readonly(TOKEN_V3, false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            ],
        )
    }

    /// `challenge(window, evidence)` — signer: ANY key (SPEC §10).
    pub fn challenge(
        pool: &Pubkey,
        challenger: &Pubkey,
        settlement_root: &Pubkey,
        window: u64,
        evidence: [u8; 32],
    ) -> Instruction {
        ix(
            "challenge",
            (window, evidence),
            vec![
                AccountMeta::new_readonly(*pool, false),
                AccountMeta::new_readonly(*challenger, true),
                AccountMeta::new(*settlement_root, false),
                AccountMeta::new_readonly(CLOCK_SYSVAR, false),
            ],
        )
    }

    /// `resolve_challenge(window, ruling)` — signer: the admin (multisig).
    pub fn resolve_challenge(
        pool: &Pubkey,
        admin: &Pubkey,
        settlement_root: &Pubkey,
        bond: &Pubkey,
        window: u64,
        ruling: Ruling<'_>,
    ) -> Instruction {
        ix(
            "resolve_challenge",
            (window, ruling),
            vec![
                AccountMeta::new_readonly(*pool, false),
                AccountMeta::new_readonly(*admin, true), // admin_authority
                AccountMeta::new(*settlement_root, false),
                AccountMeta::new_readonly(*bond, false),
            ],
        )
    }

    /// `slash_bond(window)` — NO signer: it executes a ruling that already
    /// exists; amount and destination come from the root account.
    pub fn slash_bond(
        pool: &Pubkey,
        settlement_root: &Pubkey,
        bond_vault: &Pubkey,
        bond: &Pubkey,
        destination: &Pubkey,
        window: u64,
    ) -> Instruction {
        ix(
            "slash_bond",
            (window,),
            vec![
                AccountMeta::new_readonly(*pool, false),
                AccountMeta::new(*settlement_root, false),
                AccountMeta::new(*bond_vault, false),
                AccountMeta::new(*bond, false),
                AccountMeta::new(*destination, false),
                AccountMeta::new_readonly(TOKEN_V3, false),
            ],
        )
    }

    /// A RAW Token v3 transfer — what any key would try against the vault
    /// without going through the program (invariant 10's attack shape).
    pub fn raw_v3_transfer(
        source: &Pubkey,
        destination: &Pubkey,
        authority: &Pubkey,
        amount: u64,
    ) -> Instruction {
        super::raw_token::transfer(source, destination, authority, amount)
    }

    /// The multisig's ruling, as an external integrator hand-encodes it for
    /// `resolve_challenge`: borsh writes an enum as its variant byte
    /// followed by that variant's fields.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Ruling<'a> {
        /// The root stands — variant 0, no fields.
        Stands,
        /// The root is false — variant 1, then `slash_amount`, then `victim`.
        False {
            slash_amount: u64,
            victim: &'a Pubkey,
        },
    }

    // Borsh by hand (the harness's borsh is pinned without its `derive`
    // feature): variant byte, then the fields, in declaration order.
    impl BorshSerialize for Ruling<'_> {
        fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
            match self {
                Ruling::Stands => writer.write_all(&[0u8]),
                Ruling::False {
                    slash_amount,
                    victim,
                } => {
                    writer.write_all(&[1u8])?;
                    slash_amount.serialize(writer)?;
                    victim.serialize(writer)
                }
            }
        }
    }
}
