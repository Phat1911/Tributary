//! Tributary root pool — on-chain custody rails (SPEC §12, V2 milestone M8).
//!
//! This program is the PUBLIC TWIN of the internal double-entry ledger M7
//! shipped (`crates/ledger`). Per D38/D40 it does exactly two things and
//! nothing else: it RECORDS depositor contributions as claims in a
//! non-transferable Token-2022 registry, and it HOLDS renter prepaid USDC
//! in a program-controlled escrow vault. Depositor principal itself never
//! touches this chain as money — it is API credits on the operator's books
//! (rule 1, Path B); the claim mint merely records its micro-USD valuation.
//! USDC enters exclusively as renter prepayment (the M10 rail, shipped
//! here as `prepay_renter`) and leaves as refunds (`refund_renter`) or,
//! from M11, settle payouts.
//!
//! # M11 settlement — implemented (M11-B, 2026-11-27)
//!
//! The settlement layer (Merkle roots, claims, challenges, bonds — SPEC
//! §12) lives in `settlement.rs` and is dispatched from the six handlers at
//! the bottom of this file: `fund_bond`, `post_root`, `claim`, `challenge`,
//! `resolve_challenge`, `slash_bond`. Session A (doctrine-suite mode, step
//! 14) shipped the declarations and the twelve-test suite with `todo!()`
//! bodies; Session B implemented the handlers and the four Merkle helpers
//! (SHA-256 by the `sol_sha256` syscall — AUDIT A-063) to green. The twelve
//! doctrine tests in `tests/m11_doctrine.rs` all pass.
//!
//! # Why the vault never holds depositor principal (D40 teaching story)
//!
//! A deposit is credits: the depositor pays the operator OFF the protocol
//! and the books record Path-B credits (rule 1). If the program also moved
//! depositor USDC, the chain would hold a second, competing copy of "who
//! contributed what" — one that can drift from the books, invites the
//! escrow to be spent like operator cash, and recreates the bank shape
//! Tributary explicitly is not (D41: utility marketplace, never lending).
//! So `record_contribution` MINTS claims (a record) and `burn_claims`
//! BURNS them (exit of the record) — neither moves USDC. The only USDC
//! the vault ever holds is renter prepaid: a liability the operator owes
//! BACK, bounded per-renter by the renter's OWN on-chain record.
//!
//! # Seams — the mirror correspondence (reconciliation itself is M11)
//!
//! The internal books are kept from the operator's perspective
//! (`crates/ledger/src/chart.rs`); every on-chain record below answers a
//! query those books already ask, which is what keeps reconciliation cheap:
//!
//! | On-chain record | Internal account (M7 chart) |
//! |---|---|
//! | `vault` USDC balance | `cash.escrow` (asset — the rule-7 numerator) |
//! | per-renter prepaid record (`[prepaid, renter]` PDA) | `prepaid.renter` owner row (liability, INSIDE rule 7) |
//! | per-depositor claim balance (`[claim, depositor]` account) | `claims.contributed` owner row (liability, capacity-backed, deliberately OUTSIDE rule 7) |
//! | claim mint total supply | Σ `claims.contributed` |
//!
//! Two reconciliation invariants the journey suite asserts after every
//! movement: `vault == Σ prepaid` (renter money, USDC side) and
//! `claim_supply == Σ recorded contributions` (depositor money, record
//! side). Both mints carry 6 decimals, so 1 minor unit on-chain ==
//! 1 micro-USD == the ledger's `MicroUsd` integer unit; no conversion
//! exists anywhere. `payable.depositor` has NO on-chain twin yet —
//! settled payables arrive with M11, whose sweep/settle instructions
//! reuse the exact account shapes `refund_renter` establishes here.
//!
//! # Authority model (least privilege, devnet posture per kickoff)
//!
//! * deployment authority — the program's UPGRADE authority (the Squads
//!   multisig vault on devnet). Its signature gates `initialize`
//!   (A-041 fix, 2026-11-02): the pool cannot come into existence
//!   without a 2-of-2 vote, so the window between program deploy and
//!   initialize is not capturable by any funded interloper.
//! * `admin_authority` — the Squads multisig vault PDA (devnet: 2-of-2,
//!   founder + teammate; SPEC §12 keeps 2-of-3 as the production posture).
//!   Pause/resume and authority rotations require its signature, which only
//!   the Squads program can produce after a 2-of-2 vote. The program's
//!   upgrade authority is that same multisig — a single-key upgrade
//!   authority would be a finding against this milestone.
//! * `backend_authority` — the operator's hot key. Signs attestations
//!   (`record_contribution`, from the books) and the ASSISTED door of
//!   every exit, so members, renters and depositors never need a wallet
//!   (rule 10, SPEC §4.5).
//! * THE TWO DOORS (doctrine, 2026-10-31): every exit — `burn_claims` for
//!   depositors, `refund_renter` for renters — accepts the backend
//!   authority (assisted, custodial UX) OR the named identity's own key
//!   (direct, self-custody escape hatch). Assisted must never mean
//!   imprisoned: with the operator evil or offline, an identity holding
//!   their own key still exits without cooperation. Both doors share the
//!   same ceiling checks; neither door can skip one.
//!
//! # Rule 9 — claims are ledger claims, not a tradable token
//!
//! The claim mint carries the Token-2022 `NonTransferable` extension: the
//! token program ITSELF rejects any transfer between accounts, so no bug in
//! this program, no future upgrade short of a multisig vote, and no holder
//! key can ever make a claim tradable. Claims are created only by
//! `record_contribution` (mint, operator-attested from the books) and
//! destroyed only by `burn_claims` (burn, either door). The claim token
//! accounts are PDAs owned by the pool PDA — no depositor key can even
//! burn or close them directly.
//!
//! # One pool per deployment (D42) — no cross-pool drawing, ever
//!
//! Every PDA here (`pool`, `vault`, `claim_mint`, claim and prepaid rows)
//! derives from THIS program's id and fixed seeds, so a pool IS its
//! deployment: one operator, one multisig, one escrow. There is no shared
//! program authority, no protocol-level kill switch, and no instruction
//! that accepts a foreign pool/mint/vault pair — claims of pool A cannot
//! draw on pool B's escrow by construction (asserted by the adversarial
//! suite's forged-mint test). M12's registry indexes deployments — a
//! phone book, never a bank.

use anchor_lang::prelude::*;
// The BPF Upgradeable Loader's program id + state layout, reached through
// anchor's re-export of its own `solana_program` (the v3 cohort — A-033):
// `initialize` reads THIS deployment's upgrade authority out of the
// loader-owned ProgramData account (see the A-041 gate in `initialize`).
use anchor_lang::solana_program::bpf_loader_upgradeable;
use anchor_spl::token::{self, Token, TokenAccount};
use anchor_spl::token_2022::{self, InitializeMint2, InitializeNonTransferableMint, Token2022};
// The underlying Token-2022 crate, reached ONLY through anchor-spl's
// re-export: it follows whichever Solana cohort anchor-spl's features
// select, so this program never pins — or fights with — a second copy.
use anchor_spl::token_2022::spl_token_2022;
use anchor_spl::token_interface::{self, Mint, TokenAccount as InterfaceTokenAccount};

// `declare_id!` pins this program's on-chain address: every PDA in this
// program derives from it, and the deployed program must match (Anchor
// enforces the check at instruction time). Anchor.toml carries the same id.
// Minted by `solana-keygen` 2026-10-30 (keypair: target/m8-keys, gitignored).
declare_id!("9pEmyxftdRUZV2iw6ZzCNgZz9QygG53i7APNaWcEHvjX");

pub mod errors;
pub mod settlement;
pub mod state;

pub use errors::PoolError;
// GLOB re-export, deliberately: `#[derive(Accounts)]` defines the
// `__client_accounts_*` macros WHERE the struct lives, and `#[program]`
// imports them by CRATE-ROOT path — a named re-export carries the types
// but not the macros (the compiler says only "unresolved import `crate`").
pub use settlement::*;
pub use state::{
    DECIMALS, Pool, RenterPrepaid, SEED_CLAIM, SEED_CLAIM_MINT, SEED_POOL, SEED_PREPAID, SEED_VAULT,
};

/// THE M11 SEAM — exit ceiling check.
///
/// M8 ships custody rails only: there is no per-period exit ceiling yet,
/// so this function is an explicit, tested no-op. M11's rate-limited
/// exits (SPEC §11 "liquidity buffer parameter") slot in by replacing
/// this ONE body — the instruction signature, accounts and seam stay
/// fixed. It is called identically on BOTH doors of `burn_claims`, and
/// M11's settle instruction will call it with the same shape.
fn check_withdrawal_ceiling(_pool: &Account<Pool>, _identity: &Pubkey, _amount: u64) -> Result<()> {
    Ok(())
}

/// THE TWO-DOOR AUTHORITY CHECK (doctrine, 2026-10-31).
///
/// Rule 10 makes the backend the ASSISTED door (custodial users never
/// need a wallet); the doctrine's other half gives every named identity a
/// DIRECT door (self-custody escape hatch) so an evil or offline operator
/// can never imprison funds or records. Shared by `burn_claims` and
/// `refund_renter` — one policy, two instructions, zero divergence.
fn require_exit_authority(
    pool: &Account<Pool>,
    authority: &Signer,
    identity: &Pubkey,
) -> Result<()> {
    let signer = authority.key();
    require!(
        signer == pool.backend_authority || signer == *identity,
        PoolError::ExitAuthority
    );
    Ok(())
}

/// # Instructions
///
/// Nine implemented instructions: initialize, record_contribution,
/// burn_claims, prepay_renter, refund_renter, pause, resume, and two
/// authority rotations — plus six M11 settlement handlers that are
/// DECLARED with `todo!()` bodies (M11-A stubs, at the bottom of this
/// module). There is intentionally no `transfer_claim`, no `close_claim`,
/// no instruction that takes depositor USDC as principal — the absence of
/// an instruction IS a security property here (rules 1, 9 and 10).
#[program]
pub mod pool_program {
    use super::*;

    /// One-time pool bootstrap. Creates the config PDA, the Token-2022
    /// `NonTransferable` claim mint, and the USDC vault. Called by the
    /// deployer immediately after program deploy; the `admin_authority`
    /// passed in is the Squads multisig vault PDA (computed before this
    /// call from the multisig's create key).
    ///
    /// # Who may call it — the A-041 gate (deployment front-run)
    ///
    /// The FIRST caller no longer wins: this instruction requires the
    /// signature of THIS DEPLOYMENT'S UPGRADE AUTHORITY — the key or PDA
    /// the BPF loader recorded in the program's ProgramData account when
    /// the program was deployed. On devnet that authority is the Squads
    /// multisig vault (2-of-2), so the pool cannot come into existence
    /// without a 2-of-2 vote executing this instruction — the vault PDA's
    /// signature arrives exactly the way `AdminGate`'s `admin_authority`
    /// signatures do (Squads `execute_transaction` produces them
    /// cross-program). Between deploy and initialize the singleton cannot
    /// be captured: an attacker landing there has no multisig signature,
    /// their initialize refuses (atomically — nothing is created), and
    /// the operator's identical call succeeds whenever it lands. The dev
    /// deploy key pays rent and holds nothing. An immutable deployment
    /// (ProgramData authority `None`) can never birth a pool — refused
    /// with the same error.
    pub fn initialize(
        ctx: Context<Initialize>,
        admin_authority: Pubkey,
        backend_authority: Pubkey,
    ) -> Result<()> {
        // ---- A-041 gate: only this deployment's upgrade authority may
        // initialize. The ProgramData account is the loader-owned source
        // of truth for WHO controls this deployment; everything below is
        // defense against a substituted or hand-crafted stand-in:
        //   1. it must sit at the exact loader-derived PDA
        //      `find_program_address(&[this_program_id], loader)` — no
        //      other account can be parked at that address (only the
        //      loader program creates it, at deploy time), so an
        //      attacker cannot pass a ProgramData they control;
        //   2. its owner must be the loader itself;
        //   3. its bytes must be the ProgramData variant of
        //      UpgradeableLoaderState (u32 variant tag 3 — the layout is
        //      a stable public protocol constant: 4-byte tag, u64 slot,
        //      1-byte option tag at offset 12, 32-byte authority at
        //      13..45, ELF after — pinned the hard way by the live soak
        //      parser, AUDIT A-040);
        //   4. the declared authority must be `Some` and must BE the
        //      signer of this transaction (a PDA signer only the
        //      authority's owning program — Squads on devnet — can
        //      produce).
        let program_data_address =
            Pubkey::find_program_address(&[crate::ID.as_ref()], &bpf_loader_upgradeable::ID).0;
        require!(
            ctx.accounts.program_data.key() == program_data_address,
            PoolError::DeploymentAuthority
        );
        require!(
            ctx.accounts.program_data.owner == &bpf_loader_upgradeable::ID,
            PoolError::DeploymentAuthority
        );
        {
            let data = ctx.accounts.program_data.try_borrow_data()?;
            require!(data.len() >= 45, PoolError::DeploymentAuthority);
            // `UpgradeableLoaderState::ProgramData` — variant tag 3, LE.
            require!(data[0..4] == [3, 0, 0, 0], PoolError::DeploymentAuthority);
            // Option tag at offset 12: 1 = Some(authority), 0 = None
            // (immutable — nobody can sign, so no pool can be born).
            require!(data[12] == 1, PoolError::DeploymentAuthority);
            let authority =
                Pubkey::new_from_array(data[13..45].try_into().expect("45-13 = 32 bytes"));
            require!(
                ctx.accounts.upgrade_authority.key() == authority,
                PoolError::DeploymentAuthority
            );
        }

        let pool = &mut ctx.accounts.pool;
        pool.admin_authority = admin_authority;
        pool.backend_authority = backend_authority;
        pool.usdc_mint = ctx.accounts.usdc_mint.key();
        pool.claim_mint = ctx.accounts.claim_mint.key();
        pool.vault = ctx.accounts.vault.key();
        pool.paused = false;
        // `bump` is handed to us by the constraint on the `pool` account —
        // storing it lets later instructions re-derive the PDA signer
        // without recomputing (and without trusting a passed-in bump).
        pool.bump = ctx.bumps.pool;
        // Reserved bytes start zeroed. Still unused: M11-A keeps settlement
        // state in its own PDA family (`settlement.rs`), not in this field.
        pool.reserved = [0u8; 64];

        // ---- Claim mint bootstrap: Token-2022 + NonTransferable ----
        // anchor-spl 1.x has no `extensions::non_transferable` account
        // constraint, so the registry mint is built the CPI way, in the
        // canonical SPL order (allocate -> extension -> base state). This
        // is also MORE explicit about rule 9: the NonTransferable flag is
        // initialized before any supply can exist.
        use spl_token_2022::extension::ExtensionType;
        use spl_token_2022::state::Mint as MintState;

        let mint_bump = ctx.bumps.claim_mint;
        let mint_seeds = &[SEED_POOL, SEED_CLAIM_MINT, &[mint_bump]];

        // 1) Rent-exempt allocation at the claim-mint PDA, owned by the
        //    Token-2022 program. Space = base Mint + the TLV entry the
        //    NonTransferable extension needs — computed by the SPL crate,
        //    never hand-sized.
        let space = ExtensionType::try_calculate_account_len::<MintState>(&[
            ExtensionType::NonTransferable,
        ])?;
        let rent = &ctx.accounts.rent;
        anchor_lang::system_program::create_account(
            CpiContext::new_with_signer(
                *ctx.accounts.system_program.key,
                anchor_lang::system_program::CreateAccount {
                    from: ctx.accounts.authority.to_account_info(),
                    to: ctx.accounts.claim_mint.to_account_info(),
                },
                &[mint_seeds],
            ),
            rent.minimum_balance(space),
            space as u64,
            ctx.accounts.token_2022_program.key,
        )?;
        // 2) The NonTransferable extension: the token program now rejects
        //    ANY transfer of this mint's tokens, forever (rule 9).
        token_2022::initialize_non_transferable_mint(CpiContext::new(
            *ctx.accounts.token_2022_program.key,
            InitializeNonTransferableMint {
                mint: ctx.accounts.claim_mint.to_account_info(),
            },
        ))?;

        // 3) Base mint state: 6 decimals (micro-USD seam), mint authority
        //    = the pool PDA (only `deposit`/`withdraw` mint and burn), no
        //    freeze authority (NonTransferable makes one pointless).
        let pool_key = pool.key();
        token_2022::initialize_mint2(
            CpiContext::new(
                *ctx.accounts.token_2022_program.key,
                InitializeMint2 {
                    mint: ctx.accounts.claim_mint.to_account_info(),
                },
            ),
            DECIMALS,
            &pool_key,
            None,
        )?;

        emit!(PoolOpened {
            admin_authority,
            backend_authority,
            usdc_mint: pool.usdc_mint,
            claim_mint: pool.claim_mint,
            vault: pool.vault,
        });
        Ok(())
    }

    /// Record a depositor contribution: mint `amount` claims into the
    /// depositor's program-owned claim account. NO USDC moves (D40) —
    /// principal is Path-B credits on the operator's books (rule 1); the
    /// claim records the micro-USD valuation of what the books say the
    /// depositor contributed.
    ///
    /// Signer: `backend_authority` ONLY — this is an operator ATTESTATION
    /// from the books, not a user action. (A self-service mint would let
    /// anyone claim arbitrary contributions; the depositor's power is the
    /// OTHER door — `burn_claims`.) The known limitation: on-chain code
    /// cannot see the books, so a rogue backend could attest fake
    /// contributions — containment is the M11 challenge window + multisig
    /// pause/rotate (AUDIT A-039).
    pub fn record_contribution(
        ctx: Context<RecordContribution>,
        depositor: Pubkey,
        amount: u64,
    ) -> Result<()> {
        require!(!ctx.accounts.pool.paused, PoolError::Paused);
        require!(amount > 0, PoolError::ZeroAmount);

        let pool = &ctx.accounts.pool;
        let seeds = &[SEED_POOL, &[pool.bump]];

        // The registry row: mint `amount` claims to `[claim, depositor]`.
        // The pool PDA is the mint authority and signs via CPI seeds.
        // This is the ONLY mint in the program.
        token_interface::mint_to(
            CpiContext::new_with_signer(
                *ctx.accounts.token_2022_program.key,
                token_interface::MintTo {
                    mint: ctx.accounts.claim_mint.to_account_info(),
                    to: ctx.accounts.claim_account.to_account_info(),
                    authority: pool.to_account_info(),
                },
                &[seeds],
            ),
            amount,
        )?;

        // A-042: Anchor deserialized `claim_mint` ONCE at instruction
        // entry; the mint CPI above mutated the real account, not our
        // stale copy. Reload so the event reports POST-instruction truth
        // — these events are the M11 reconciler's source (A-039's
        // detection layer): a subscriber rebuilds `Σ claims.contributed`
        // from `supply_after`, so a value that is wrong by exactly one
        // instruction corrupts the drift alarm on EVERY event.
        ctx.accounts.claim_mint.reload()?;
        emit!(ContributionRecorded {
            depositor,
            amount,
            supply_after: ctx.accounts.claim_mint.supply,
        });
        Ok(())
    }

    /// Exit a depositor's record: burn `amount` claims from their claim
    /// account. NO USDC moves (D40) — principal was never on-chain money;
    /// depositor-bound USDC only ever flows OUT via M11 settle payouts,
    /// which reuse this instruction's account shapes.
    ///
    /// Signers — THE TWO DOORS: `backend_authority` (assisted; the
    /// operator submitting for a custodial depositor, rule 10) OR the
    /// depositor's own key (direct; the escape hatch that keeps exit
    /// possible when the operator is evil or offline). With neither
    /// signature the instruction refuses. The M11 exit ceiling attaches
    /// identically on both doors, inside `check_withdrawal_ceiling`.
    pub fn burn_claims(ctx: Context<BurnClaims>, depositor: Pubkey, amount: u64) -> Result<()> {
        require!(!ctx.accounts.pool.paused, PoolError::Paused);
        require!(amount > 0, PoolError::ZeroAmount);

        let pool = &ctx.accounts.pool;
        require_exit_authority(pool, &ctx.accounts.authority, &depositor)?;

        // M11 SEAM: rate-limit/liquidity-buffer check attaches here.
        check_withdrawal_ceiling(pool, &depositor, amount)?;

        // The claim account is keyed by the SAME `depositor` pubkey its
        // seeds derived from (anchor constraint below), so this balance is
        // by construction the depositor's registry row.
        require!(
            ctx.accounts.claim_account.amount >= amount,
            PoolError::InsufficientClaims
        );

        // Registry exit: burn the claims (pool PDA owns the account and
        // signs the Token-2022 burn via CPI seeds). No USDC leg exists.
        let seeds = &[SEED_POOL, &[pool.bump]];
        token_interface::burn(
            CpiContext::new_with_signer(
                *ctx.accounts.token_2022_program.key,
                token_interface::Burn {
                    mint: ctx.accounts.claim_mint.to_account_info(),
                    from: ctx.accounts.claim_account.to_account_info(),
                    authority: pool.to_account_info(),
                },
                &[seeds],
            ),
            amount,
        )?;

        // A-042 (same class as the record leg): reload the mint copy
        // deserialized at entry so `supply_after` is post-burn truth —
        // the M11 reconciler consumes this event symmetrically with
        // `ContributionRecorded`.
        ctx.accounts.claim_mint.reload()?;
        emit!(ClaimsBurned {
            depositor,
            amount,
            supply_after: ctx.accounts.claim_mint.supply,
        });
        Ok(())
    }

    /// Renter prepayment: move `amount` USDC from the renter's token
    /// account into the vault, and increment the renter's OWN prepaid
    /// record `[prepaid, renter]`. This is the ONLY way USDC enters the
    /// protocol (D40) — the M10 rail: metering, checkout and pro-rata
    /// consumption live OFF-chain (M9/M10); this pair is what they plug
    /// into.
    ///
    /// Signers: `backend_authority` (operator gate — uninvited money must
    /// not create uninvited liabilities) AND `renter_key` (the source
    /// account's owner: the renter's custodial key on devnet, or the
    /// renter themselves in self-custody). Prepay has no direct-door
    /// variant by design: money IN always needs the source owner's
    /// signature anyway — the token program enforces it.
    pub fn prepay_renter(ctx: Context<PrepayRenter>, renter: Pubkey, amount: u64) -> Result<()> {
        require!(!ctx.accounts.pool.paused, PoolError::Paused);
        require!(amount > 0, PoolError::ZeroAmount);

        // Renter money into escrow: USDC source -> vault (SPL Token v3
        // transfer; `renter_key` signed the transaction, which the token
        // program checks as the source account's owner). No PDA seeds
        // needed on this leg — the authority is the renter's own key.
        token::transfer(
            CpiContext::new(
                *ctx.accounts.token_program.key,
                token::Transfer {
                    from: ctx.accounts.source.to_account_info(),
                    to: ctx.accounts.vault.to_account_info(),
                    authority: ctx.accounts.renter_key.to_account_info(),
                },
            ),
            amount,
        )?;

        // The renter's own record — refunds are bounded by THIS number,
        // never by an operator attestation (refund_renter checks it).
        let prepaid = &mut ctx.accounts.prepaid;
        prepaid.amount = prepaid
            .amount
            .checked_add(amount)
            .ok_or(PoolError::Overflow)?;
        prepaid.bump = ctx.bumps.prepaid;

        // A-042: reload the vault copy (deserialized at entry, mutated
        // by the transfer CPI we just executed) so the event reports the
        // POST-instruction `cash.escrow` balance. Reload — not
        // pre-state + delta arithmetic — on purpose: the emitted value
        // must equal chain truth BY CONSTRUCTION (a future
        // fee-bearing USDC would make vault-delta ≠ amount and the
        // computed mirror would lie again, silently).
        ctx.accounts.vault.reload()?;
        emit!(PrepayRecorded {
            renter,
            amount,
            vault_balance_after: ctx.accounts.vault.amount,
            prepaid_after: prepaid.amount,
        });
        Ok(())
    }

    /// Renter refund: move `amount` USDC from the vault to the renter's
    /// destination account, bounded by the renter's OWN on-chain prepaid
    /// record (`amount ≤ prepaid.amount`), and decrement that record.
    /// With the vault holding only renter money (D40), this is the
    /// depositor-independent refund rail M10 composes with — separability
    /// is an on-chain fact here, not a promise.
    ///
    /// Signers — THE TWO DOORS (same policy as `burn_claims`):
    /// `backend_authority` (assisted) OR the renter's own key (direct
    /// exit without operator cooperation). Destination routing is pinned
    /// to the renter on both doors, so the backend can never route a
    /// refund to itself.
    pub fn refund_renter(ctx: Context<RefundRenter>, renter: Pubkey, amount: u64) -> Result<()> {
        require!(!ctx.accounts.pool.paused, PoolError::Paused);
        require!(amount > 0, PoolError::ZeroAmount);

        let pool = &ctx.accounts.pool;
        require_exit_authority(pool, &ctx.accounts.authority, &renter)?;

        // On-chain ceiling: refunds can never exceed what THIS renter
        // actually prepaid on-chain — the record, not an attestation.
        let prepaid = &mut ctx.accounts.prepaid;
        require!(amount <= prepaid.amount, PoolError::PrepaidInsufficient);
        prepaid.amount = prepaid
            .amount
            .checked_sub(amount)
            .ok_or(PoolError::Overflow)?;

        // Renter money out of escrow: vault -> destination, pool PDA signs.
        let seeds = &[SEED_POOL, &[pool.bump]];
        token::transfer(
            CpiContext::new_with_signer(
                *ctx.accounts.token_program.key,
                token::Transfer {
                    from: ctx.accounts.vault.to_account_info(),
                    to: ctx.accounts.destination.to_account_info(),
                    authority: pool.to_account_info(),
                },
                &[seeds],
            ),
            amount,
        )?;

        // A-042 (same reload as the prepay leg): the refund CPI changed
        // the vault; the event must mirror the resulting balance, or the
        // M11 reconciler's `cash.escrow` mirror drifts by one
        // instruction on every refund.
        ctx.accounts.vault.reload()?;
        emit!(PrepayRefunded {
            renter,
            amount,
            vault_balance_after: ctx.accounts.vault.amount,
            prepaid_after: prepaid.amount,
        });
        Ok(())
    }

    /// Admin circuit breaker — halt deposits and withdrawals. Signed by
    /// `admin_authority` only (the multisig vault PDA on devnet).
    pub fn pause(ctx: Context<AdminGate>) -> Result<()> {
        ctx.accounts.pool.paused = true;
        emit!(PauseToggled { paused: true });
        Ok(())
    }

    /// Admin circuit breaker release.
    pub fn resume(ctx: Context<AdminGate>) -> Result<()> {
        ctx.accounts.pool.paused = false;
        emit!(PauseToggled { paused: false });
        Ok(())
    }

    /// Rotate the operator backend hot key (compromise response,
    /// key rollover). Admin-only — the backend cannot re-key itself.
    pub fn set_backend_authority(ctx: Context<AdminGate>, new: Pubkey) -> Result<()> {
        let old = ctx.accounts.pool.backend_authority;
        ctx.accounts.pool.backend_authority = new;
        emit!(BackendAuthorityRotated { old, new });
        Ok(())
    }

    /// Rotate the admin authority itself (multisig migration). Admin-only:
    /// changing the admin requires the CURRENT admin's signature — on
    /// devnet that means a 2-of-2 vote.
    pub fn set_admin_authority(ctx: Context<AdminGate>, new: Pubkey) -> Result<()> {
        let old = ctx.accounts.pool.admin_authority;
        ctx.accounts.pool.admin_authority = new;
        emit!(AdminAuthorityRotated { old, new });
        Ok(())
    }

    // -----------------------------------------------------------------------
    // M11 SETTLEMENT — six instructions (doctrine-suite mode, Session B,
    // 2026-11-27; workflow step 14).
    //
    // Session A shipped these as `todo!()` stubs; Session B implemented them
    // to green. The account lists, constraints, events and the Merkle
    // contract live in `settlement.rs`; the doc comment on each handler
    // records the doctrine it enforces and the check ORDER the suite pins
    // (only the OUTCOME each test pins is fixed — the ordering is fixed
    // where a doc comment fixes it).
    // -----------------------------------------------------------------------

    /// Bond the operator: move `amount` USDC from the operator's OWN wallet
    /// into the bond vault and grow the bond record. Backend-signed (the
    /// operator bonds its own money); emits `BondFunded`.
    ///
    /// Session B owes: the transfer, the record increment, the event, and
    /// the usual refusals (paused pool, zero amount).
    pub fn fund_bond(ctx: Context<FundBond>, amount: u64) -> Result<()> {
        require!(!ctx.accounts.pool.paused, PoolError::Paused);
        require!(amount > 0, PoolError::ZeroAmount);

        // USDC from the operator's OWN wallet into the bond vault. The
        // source's owner is the backend key, which signed this transaction,
        // so the token program checks the signature directly — no PDA seeds
        // on this leg (unlike the vault/bond-vault spends, which the pool
        // PDA signs for).
        token::transfer(
            CpiContext::new(
                *ctx.accounts.token_program.key,
                token::Transfer {
                    from: ctx.accounts.source.to_account_info(),
                    to: ctx.accounts.bond_vault.to_account_info(),
                    authority: ctx.accounts.backend_authority.to_account_info(),
                },
            ),
            amount,
        )?;

        // The bond record is the on-chain twin of the bond vault's balance:
        // every funding grows it by exactly the transferred amount (checked
        // arithmetic — money never wraps silently).
        let bond = &mut ctx.accounts.bond;
        bond.amount = bond.amount.checked_add(amount).ok_or(PoolError::Overflow)?;
        bond.bump = ctx.bumps.bond;

        emit!(BondFunded {
            funder: ctx.accounts.backend_authority.key(),
            amount,
            bond_after: bond.amount,
        });
        Ok(())
    }

    /// Post a settlement root: record, for `window`, the Merkle root and
    /// the total it pays, backed by the operator's current bond and stamped
    /// with the slot (the challenge window starts here).
    ///
    /// Doctrine: invariant 1 — `total_payout > vault.amount` refuses with
    /// `RootExceedsVault` and NOTHING is created or moved; invariant 7 — a
    /// second root for the same window dies at account creation (the
    /// `SettlementRoot` PDA is plain `init`), so roots are append-only.
    /// A fresh root starts `Active` with `stands_ruled = false` and
    /// `paid_out = 0` (invariants 11 and 12 read both). Emits `RootPosted`.
    pub fn post_root(
        ctx: Context<PostRoot>,
        window: u64,
        merkle_root: [u8; 32],
        total_payout: u64,
    ) -> Result<()> {
        // Invariant 1 — fail CLOSED: the declared total may not exceed the
        // vault's USDC, checked BEFORE any field is written. Solana
        // instructions are atomic, so a refusal here also rolls back the
        // `init` above: no root account is born and no balance moves.
        // `<=` (not `<`): a root paying EXACTLY the vault's balance posts.
        require!(
            total_payout <= ctx.accounts.vault.amount,
            PoolError::RootExceedsVault
        );

        // The slot is where the challenge window starts; slots never
        // approach i64::MAX, but convert checked anyway (no silent wrap).
        let posted_at = i64::try_from(ctx.accounts.clock.slot).map_err(|_| PoolError::Overflow)?;
        // Snapshot the bond backing THIS root: a later ruling may not slash
        // more than this (invariant 6), even if other roots slash it down.
        let bond = ctx.accounts.bond.amount;

        let root = &mut ctx.accounts.settlement_root;
        root.pool = ctx.accounts.pool.key();
        root.window = window;
        root.merkle_root = merkle_root;
        root.total_payout = total_payout;
        root.status = RootStatus::Active;
        root.posted_at = posted_at;
        root.bond = bond;
        root.challenger = Pubkey::default();
        root.evidence = [0u8; 32];
        root.ruled_slash = 0;
        root.victim = Pubkey::default();
        root.stands_ruled = false;
        root.paid_out = 0;
        root.bump = ctx.bumps.settlement_root;

        emit!(RootPosted {
            window,
            merkle_root,
            total_payout,
            bond,
            posted_at,
        });
        Ok(())
    }

    /// Claim one leaf: verify the inclusion proof against the published
    /// root, pay `amount` USDC from the vault to the leaf's named
    /// `depositor`, write the leaf's claim marker, and add `amount` to the
    /// root's `paid_out`.
    ///
    /// Doctrine: invariant 2 — a proof that does not hash up to the root
    /// refuses with `InvalidProof`; invariant 3 — the marker is plain
    /// `init`, so a replayed leaf dies at account creation, and the proof
    /// is bound to its index; invariant 4 — THE TWO DOORS (the backend, or
    /// `depositor`'s own key — anything else is `ExitAuthority`) and the
    /// destination must be owned by `depositor`; invariant 5 — a root that
    /// is not `Active` refuses with `RootFrozen`; invariant 11 — a root
    /// whose challenge window is still open refuses with
    /// `ChallengeWindowOpen`, unless the multisig ruled it stands; invariant
    /// 12 — a claim that would push `paid_out` past `total_payout` refuses
    /// with `PayoutExceedsTotal`. Emits `ClaimPaid`.
    ///
    /// Session B owes the FIRST TWO of these IN THIS ORDER — the suite pins
    /// which refusal a shape gets: (1) the status, else `RootFrozen`;
    /// (2) the window, else `ChallengeWindowOpen`. A root challenged while
    /// still INSIDE its window is frozen, not merely delayed. After those:
    /// the proof, the door, the declared total, then the transfer, the
    /// marker and `paid_out += amount`.
    pub fn claim(
        ctx: Context<Claim>,
        window: u64,
        leaf_index: u32,
        depositor: Pubkey,
        amount: u64,
        proof: Vec<[u8; 32]>,
    ) -> Result<()> {
        // (1) Invariant 5 — the status gate FIRST: a root that is not
        //     `Active` (challenged, proven false, slashed) is FROZEN, not
        //     merely delayed — so this refusal precedes the window check.
        require!(
            ctx.accounts.settlement_root.status == RootStatus::Active,
            PoolError::RootFrozen
        );
        // (2) Invariant 11 — no payout before the challenge window has
        //     passed, unless the multisig ruled the root stands
        //     (`stands_ruled` waives the rest of the window).
        let window_end = ctx.accounts.settlement_root.posted_at + CHALLENGE_WINDOW_SLOTS;
        require!(
            ctx.accounts.settlement_root.stands_ruled
                || ctx.accounts.clock.slot
                    >= u64::try_from(window_end).map_err(|_| PoolError::Overflow)?,
            PoolError::ChallengeWindowOpen
        );
        // Invariant 2 — the inclusion proof, bound to the leaf's INDEX: the
        //     leaf hashes the named depositor and amount, so a tampered
        //     amount, a foreign depositor or another leaf's proof fails.
        require!(
            verify_proof(
                ctx.accounts.settlement_root.merkle_root,
                leaf_index,
                leaf_hash(&depositor, amount),
                &proof,
            ),
            PoolError::InvalidProof
        );
        // Invariant 4 — THE TWO DOORS: the backend (assisted) or the leaf's
        //     own depositor key (direct). Anything else opens neither door.
        require_exit_authority(&ctx.accounts.pool, &ctx.accounts.authority, &depositor)?;
        // Invariant 12 — never more than the DECLARED total, checked against
        //     the running `paid_out` before anything moves.
        let paid_out = ctx
            .accounts
            .settlement_root
            .paid_out
            .checked_add(amount)
            .ok_or(PoolError::Overflow)?;
        require!(
            paid_out <= ctx.accounts.settlement_root.total_payout,
            PoolError::PayoutExceedsTotal
        );

        // The payout: vault -> the leaf's depositor-owned destination (the
        // `destination.owner == depositor` account constraint already fired
        // above). The pool PDA is the vault's authority and signs via seeds.
        let seeds = &[SEED_POOL, &[ctx.accounts.pool.bump]];
        token::transfer(
            CpiContext::new_with_signer(
                *ctx.accounts.token_program.key,
                token::Transfer {
                    from: ctx.accounts.vault.to_account_info(),
                    to: ctx.accounts.destination.to_account_info(),
                    authority: ctx.accounts.pool.to_account_info(),
                },
                &[seeds],
            ),
            amount,
        )?;

        // The claim marker: born once, EVER (invariant 3's structural half —
        // a replay died at this account's `init` before the handler ran).
        let marker = &mut ctx.accounts.claim_marker;
        marker.root = ctx.accounts.settlement_root.key();
        marker.leaf_index = leaf_index;
        marker.depositor = depositor;
        marker.amount = amount;
        marker.bump = ctx.bumps.claim_marker;

        // Invariant 12's ledger: count the payout on the root.
        ctx.accounts.settlement_root.paid_out = paid_out;

        // A-042: reload the vault copy (deserialized at entry, mutated by the
        // transfer CPI) so the event reports the POST-transfer balance.
        ctx.accounts.vault.reload()?;
        emit!(ClaimPaid {
            window,
            leaf_index,
            depositor,
            amount,
            vault_balance_after: ctx.accounts.vault.amount,
        });
        Ok(())
    }

    /// Challenge a root: freeze its payouts. Permissionless — any key may
    /// challenge, inside the window (SPEC §10).
    ///
    /// Doctrine: invariant 5 — `clock.slot` must be inside
    /// `posted_at + CHALLENGE_WINDOW_SLOTS`, or the challenge refuses with
    /// `ChallengeWindowClosed`; the root turns `Challenged`, its claims
    /// freeze until the multisig rules, and `stands_ruled` is cleared (a new
    /// challenge outranks an earlier "stands"). Emits `RootChallenged`.
    pub fn challenge(ctx: Context<Challenge>, window: u64, evidence: [u8; 32]) -> Result<()> {
        let root = &mut ctx.accounts.settlement_root;
        // Only an `Active` root can be challenged: a challenged/false/slashed
        // root is already out of the payout game, so there is nothing new to
        // freeze. (The suite challenges only `Active` roots.)
        require!(root.status == RootStatus::Active, PoolError::RootFrozen);
        // Invariant 5 (complement) — inside the window ONLY. The window is
        // `clock.slot < posted_at + CHALLENGE_WINDOW_SLOTS`; at or past it a
        // challenge can no longer freeze the root.
        let window_end = root.posted_at + CHALLENGE_WINDOW_SLOTS;
        require!(
            ctx.accounts.clock.slot < u64::try_from(window_end).map_err(|_| PoolError::Overflow)?,
            PoolError::ChallengeWindowClosed
        );

        root.status = RootStatus::Challenged;
        root.challenger = ctx.accounts.challenger.key();
        root.evidence = evidence;
        // A new challenge outranks an earlier "stands": the multisig must
        // rule on the LATEST challenge, so the waiver is cleared.
        root.stands_ruled = false;

        emit!(RootChallenged {
            window,
            challenger: ctx.accounts.challenger.key(),
            evidence,
        });
        Ok(())
    }

    /// The multisig rules on a challenge. `Ruling::RootStands` returns the
    /// root to `Active` and sets `stands_ruled` — the ruling waives the rest
    /// of the challenge window (invariant 11); `Ruling::RootFalse {
    /// slash_amount, victim }` marks it `ProvenFalse` and records the exact
    /// slash and the victim.
    ///
    /// Doctrine: invariant 6 — a ruling may not exceed the bond that backs
    /// the root (`SlashExceedsBond`); a ruling that the root stands
    /// slashes nothing, ever. Emits `ChallengeResolved`.
    pub fn resolve_challenge(
        ctx: Context<ResolveChallenge>,
        window: u64,
        ruling: Ruling,
    ) -> Result<()> {
        let root = &mut ctx.accounts.settlement_root;
        // A ruling answers the LATEST challenge: only a challenged root is
        // awaiting one. Anything else has nothing to resolve. (The suite
        // resolves only challenged roots; the refusal keeps a stray ruling
        // from rewriting an unchallenged root's history.)
        require!(root.status == RootStatus::Challenged, PoolError::RootFrozen);

        match ruling {
            // The root STANDS: back to `Active`, and the ruling waives the
            // rest of the challenge window (invariant 11). A stand slashes
            // nothing and names no victim, so both are cleared — the event
            // subscriber mirrors exactly these values.
            Ruling::RootStands => {
                root.status = RootStatus::Active;
                root.stands_ruled = true;
                root.ruled_slash = 0;
                root.victim = Pubkey::default();
                emit!(ChallengeResolved {
                    window,
                    root_stands: true,
                    slash_amount: 0,
                    victim: Pubkey::default(),
                });
            }
            // The root is FALSE: record the EXACT slash and the victim. The
            // ruling may not exceed the bond that backs this root (the
            // posting-time snapshot, invariant 6) — and it only RECORDS; the
            // USDC moves when `slash_bond` cranks it.
            Ruling::RootFalse {
                slash_amount,
                victim,
            } => {
                require!(slash_amount <= root.bond, PoolError::SlashExceedsBond);
                root.status = RootStatus::ProvenFalse;
                root.ruled_slash = slash_amount;
                root.victim = victim;
                root.stands_ruled = false;
                emit!(ChallengeResolved {
                    window,
                    root_stands: false,
                    slash_amount,
                    victim,
                });
            }
        }
        Ok(())
    }

    /// Execute a ruled slash: move EXACTLY the recorded `ruled_slash` from
    /// the bond vault to the recorded victim. Permissionless to crank — the
    /// ruling was the multisig's act, and the amount and the destination
    /// come from the root account, never from arguments.
    ///
    /// Doctrine: invariant 6 — anything but a `ProvenFalse` root refuses
    /// with `RootNotProvenFalse` (never challenged, still challenged, ruled
    /// to stand, or already slashed); a second slash is therefore
    /// impossible.
    ///
    /// Session B owes these checks IN THIS ORDER, because the suite pins
    /// which refusal a shape gets: (1) `RootNotProvenFalse` unless the
    /// root is `ProvenFalse`; (2) `WrongDestinationOwner` unless
    /// `destination.owner == settlement_root.victim` (a handler check, not
    /// an account constraint — see `SlashBond`); (3) move EXACTLY
    /// `ruled_slash`, mark the root `Slashed`, emit `BondSlashed`.
    pub fn slash_bond(ctx: Context<SlashBond>, window: u64) -> Result<()> {
        let root = &mut ctx.accounts.settlement_root;
        // (1) Only a root PROVEN FALSE may be slashed — never challenged,
        //     still challenged, ruled to stand, or already slashed all
        //     refuse here, so a second slash is impossible by construction.
        require!(
            root.status == RootStatus::ProvenFalse,
            PoolError::RootNotProvenFalse
        );
        // (2) The destination must be the RECORDED victim. Checked in the
        //     handler AFTER the status check (deliberately NOT an account
        //     constraint): a misdirected crank on an un-ruled root must read
        //     as `RootNotProvenFalse`, not as a destination error.
        require!(
            ctx.accounts.destination.owner == root.victim,
            PoolError::WrongDestinationOwner
        );
        // Both the amount and the victim come from the root account — never
        // from arguments, so no caller can redirect or resize a ruling.
        let amount = root.ruled_slash;
        let victim = root.victim;

        // (3) Move EXACTLY the ruled amount: bond vault -> the recorded
        //     victim, the pool PDA signing as the bond vault's authority.
        let seeds = &[SEED_POOL, &[ctx.accounts.pool.bump]];
        token::transfer(
            CpiContext::new_with_signer(
                *ctx.accounts.token_program.key,
                token::Transfer {
                    from: ctx.accounts.bond_vault.to_account_info(),
                    to: ctx.accounts.destination.to_account_info(),
                    authority: ctx.accounts.pool.to_account_info(),
                },
                &[seeds],
            ),
            amount,
        )?;

        // The bond record mirrors the vault: it falls by exactly the slash.
        let bond = &mut ctx.accounts.bond;
        bond.amount = bond.amount.checked_sub(amount).ok_or(PoolError::Overflow)?;
        // Terminal state: the slash executed once (see (1)).
        root.status = RootStatus::Slashed;

        emit!(BondSlashed {
            window,
            amount,
            victim,
            bond_after: bond.amount,
        });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Account contexts. Each `#[derive(Accounts)]` struct is Anchor's typed
// account list: the macro expands each field's constraints into runtime
// checks, so a mismatched account fails BEFORE the handler runs.
// ---------------------------------------------------------------------------

/// Accounts for [`pool_program::initialize`].
#[derive(Accounts)]
pub struct Initialize<'info> {
    /// The singleton config PDA — `init` makes this instruction once-only
    /// (a second call finds the account occupied and aborts).
    #[account(
        init,
        payer = authority,
        space = 8 + Pool::INIT_SPACE,
        seeds = [SEED_POOL],
        bump
    )]
    pub pool: Account<'info, Pool>,

    /// The claim registry mint PDA — initialized MANUALLY in the handler
    /// (anchor-spl 1.x has no non_transferable account constraint): the
    /// Token-2022 NonTransferable extension is initialized before any
    /// supply exists, making transfers of this mint reject at the TOKEN
    /// PROGRAM level forever (rule 9 enforcement at the lowest layer).
    /// Mint authority is the pool PDA — only `record_contribution`
    /// mints, only `burn_claims` burns.
    #[account(mut, seeds = [SEED_POOL, SEED_CLAIM_MINT], bump)]
    /// CHECK: deliberately unchecked at derive time — this account is
    /// CREATED and initialized by the handler's CPIs in this same
    /// transaction (allocate -> NonTransferable -> mint state), so before
    /// the handler runs it is an empty system account. The seeds
    /// constraint above is the address check; the CPI chain is the
    /// ownership check (it can only land on the Token-2022 program).
    pub claim_mint: UncheckedAccount<'info>,

    /// Existing USDC mint (SPL Token v3 on devnet — Circle faucet USDC).
    /// The decimals constraint pins the micro-USD seam at bootstrap: a
    /// non-6-decimals "USDC" cannot be custodied by this pool.
    #[account(constraint = usdc_mint.decimals == DECIMALS @ PoolError::UsdcDecimals)]
    pub usdc_mint: Account<'info, token::Mint>,

    /// The escrow vault: a PDA token account, owner = pool PDA, so ONLY
    /// this program (deposit/withdraw handlers) can ever move custodial
    /// USDC. No human key, including the multisig's, signs token moves.
    #[account(
        init,
        payer = authority,
        seeds = [SEED_POOL, SEED_VAULT],
        bump,
        token::mint = usdc_mint,
        token::authority = pool
    )]
    pub vault: Account<'info, TokenAccount>,

    /// Deployer (payer for the PDAs and rent). On devnet this is the
    /// session deploy key; it receives NO standing authority — the admin
    /// and backend authorities arrive as instruction arguments.
    #[account(mut)]
    pub authority: Signer<'info>,

    /// The BPF loader's ProgramData account FOR THIS PROGRAM — the
    /// deployment-authority source the A-041 gate reads (see handler).
    /// Checked accounts: exact loader-derived address + loader owner +
    /// ProgramData variant, all verified there. Read-only.
    /// CHECK: validated field-by-field in `initialize` (address = the
    /// loader-derived ProgramData PDA of `crate::ID`, owner = the BPF
    /// Upgradeable Loader, bytes = ProgramData variant with a Some
    /// authority); the required signature is enforced on
    /// `upgrade_authority` below. UncheckedAccount (not AccountInfo)
    /// because anchor's derive must add nothing here — the handler IS
    /// the check.
    pub program_data: UncheckedAccount<'info>,

    /// THIS deployment's upgrade authority (devnet: the Squads multisig
    /// vault PDA, signing via a 2-of-2 `execute_transaction`). The
    /// `Signer` type makes the runtime verify the signature; the handler
    /// additionally requires the key to equal the ProgramData-declared
    /// authority. This is the A-041 gate: without it, initialize is
    /// refused and the singleton stays unborn.
    pub upgrade_authority: Signer<'info>,

    /// The rent sysvar, DECLARED even though on-chain code could fetch it
    /// via syscall: host-side test execution (program-test builtins) has no
    /// syscalls — sysvars arrive only as accounts — so declaring it keeps
    /// the program runnable in both worlds. Read-only, one account in the
    /// list, zero lamports.
    pub rent: Sysvar<'info, Rent>,

    pub token_program: Program<'info, Token>,
    pub token_2022_program: Program<'info, Token2022>,
    pub system_program: Program<'info, System>,
}

/// Accounts for [`pool_program::record_contribution`].
#[derive(Accounts)]
#[instruction(depositor: Pubkey)]
pub struct RecordContribution<'info> {
    /// `has_one` checks `pool.backend_authority == backend_authority.key()`
    /// — recording is an operator ATTESTATION from the books, so the
    /// backend gate is the whole signer story (see handler docs: the
    /// depositor's power is the OTHER door, `burn_claims`).
    #[account(
        mut,
        seeds = [SEED_POOL],
        bump = pool.bump,
        has_one = backend_authority
    )]
    pub pool: Account<'info, Pool>,

    /// Operator backend hot key; also pays the claim account's rent on
    /// first record (depositors never pay anything).
    #[account(mut)]
    pub backend_authority: Signer<'info>,

    /// The registry mint (seeds re-checked; typed so decimals/authority
    /// are constraint-verified on every record).
    #[account(
        mut,
        seeds = [SEED_POOL, SEED_CLAIM_MINT],
        bump,
        mint::decimals = DECIMALS,
        mint::authority = pool
    )]
    pub claim_mint: InterfaceAccount<'info, Mint>,

    /// The depositor's registry row: `[claim, depositor]`, owned by the
    /// POOL (not the depositor) — no holder key can burn or close it.
    /// `init_if_needed`: first record lazily creates the row; later
    /// records reuse it (the program pays rent via backend_authority).
    #[account(
        init_if_needed,
        payer = backend_authority,
        seeds = [SEED_CLAIM, depositor.as_ref()],
        bump,
        token::mint = claim_mint,
        token::authority = pool,
        token::token_program = token_2022_program
    )]
    pub claim_account: InterfaceAccount<'info, InterfaceTokenAccount>,

    pub token_2022_program: Program<'info, Token2022>,
    pub system_program: Program<'info, System>,
}

/// Accounts for [`pool_program::burn_claims`].
// `#[instruction(...)]` exposes instruction arguments to the constraint
// solver: `depositor` arrives as a Pubkey ARG, and the claim-account
// seeds below bind this exact pubkey, so only the named depositor's own
// row can burn. `authority` is the two-door signer — backend (assisted)
// or the depositor themselves (direct) — checked in the handler by
// `require_exit_authority`, NOT by `has_one`: either key must pass.
#[derive(Accounts)]
#[instruction(depositor: Pubkey)]
pub struct BurnClaims<'info> {
    #[account(seeds = [SEED_POOL], bump = pool.bump)]
    pub pool: Account<'info, Pool>,

    /// The one signer — WHICH door it opens is decided in the handler:
    /// backend_authority (assisted) or `depositor` (direct). No `mut`:
    /// burning pays no rent and moves no lamports.
    pub authority: Signer<'info>,

    #[account(
        mut,
        seeds = [SEED_POOL, SEED_CLAIM_MINT],
        bump,
        mint::decimals = DECIMALS,
        mint::authority = pool
    )]
    pub claim_mint: InterfaceAccount<'info, Mint>,

    /// The `depositor` argument is bound to the seeds here — anchor
    /// verifies this account IS `[claim, <that pubkey>]`, so the burned
    /// balance is provably the named depositor's registry row.
    #[account(
        mut,
        seeds = [SEED_CLAIM, depositor.as_ref()],
        bump,
        token::mint = claim_mint,
        token::authority = pool,
        token::token_program = token_2022_program
    )]
    pub claim_account: InterfaceAccount<'info, InterfaceTokenAccount>,

    pub token_2022_program: Program<'info, Token2022>,
}

/// Accounts for [`pool_program::prepay_renter`].
// `#[instruction(renter: Pubkey)]`: `renter` names WHOSE prepaid record
// increments; the record PDA seeds bind that exact pubkey. Two signers,
// deliberately: the backend gates (uninvited money must not create
// uninvited liabilities) and the source owner authorizes the USDC move
// (token program enforces source-owner signature on every transfer).
#[derive(Accounts)]
#[instruction(renter: Pubkey)]
pub struct PrepayRenter<'info> {
    #[account(
        seeds = [SEED_POOL],
        bump = pool.bump,
        has_one = backend_authority
    )]
    pub pool: Account<'info, Pool>,

    /// Operator gate signer; also pays the prepaid record's rent on
    /// first prepay (renters never pay chain costs).
    #[account(mut)]
    pub backend_authority: Signer<'info>,

    /// The renter identity key — owner of `source`. Custodial on devnet
    /// (the backend holds it) or self-custody in the direct future; the
    /// token program only cares that it owns the source account.
    pub renter_key: Signer<'info>,

    /// Where the prepayment comes from: must be owned by `renter_key` on
    /// the pool's USDC mint.
    #[account(
        mut,
        constraint = source.owner == renter_key.key() @ PoolError::WrongSourceOwner,
        constraint = source.mint == pool.usdc_mint @ PoolError::WrongSourceMint
    )]
    pub source: Account<'info, TokenAccount>,

    /// Escrow destination (seeds re-checked against the config).
    #[account(
        mut,
        seeds = [SEED_POOL, SEED_VAULT],
        bump,
        token::mint = pool.usdc_mint,
        token::authority = pool
    )]
    pub vault: Account<'info, TokenAccount>,

    /// The renter's OWN prepaid record: `[prepaid, renter]`, owned by the
    /// program. `init_if_needed`: first prepay lazily creates it; refunds
    /// are bounded by this record's `amount` — an on-chain fact.
    #[account(
        init_if_needed,
        payer = backend_authority,
        space = 8 + RenterPrepaid::INIT_SPACE,
        seeds = [SEED_PREPAID, renter.as_ref()],
        bump
    )]
    pub prepaid: Account<'info, RenterPrepaid>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

/// Accounts for [`pool_program::refund_renter`].
// Same two-door shape as `BurnClaims`: `authority` is backend (assisted)
// or the renter themselves (direct), decided by `require_exit_authority`
// in the handler. `destination.owner == renter` pins routing on BOTH
// doors — the backend can never route a refund to itself.
#[derive(Accounts)]
#[instruction(renter: Pubkey)]
pub struct RefundRenter<'info> {
    #[account(seeds = [SEED_POOL], bump = pool.bump)]
    pub pool: Account<'info, Pool>,

    /// The one signer — WHICH door it opens is decided in the handler.
    pub authority: Signer<'info>,

    /// Refund destination must belong to the renter being refunded.
    #[account(
        mut,
        constraint = destination.owner == renter @ PoolError::WrongDestinationOwner,
        constraint = destination.mint == pool.usdc_mint @ PoolError::WrongDestinationMint
    )]
    pub destination: Account<'info, TokenAccount>,

    /// Escrow source (seeds re-checked; pool-owned, so only this
    /// program's refund/settle logic can ever move it).
    #[account(
        mut,
        seeds = [SEED_POOL, SEED_VAULT],
        bump,
        token::mint = pool.usdc_mint,
        token::authority = pool
    )]
    pub vault: Account<'info, TokenAccount>,

    /// The renter's prepaid record — the refund ceiling. Its seeds bind
    /// the `renter` argument, so one renter can never spend another's
    /// record.
    #[account(mut, seeds = [SEED_PREPAID, renter.as_ref()], bump)]
    pub prepaid: Account<'info, RenterPrepaid>,

    pub token_program: Program<'info, Token>,
}

/// Accounts for the admin instructions (pause/resume/rotations). The
/// `has_one` on `admin_authority` is the multisig gate: passing anything
/// but the configured authority's signature aborts. On devnet the Squads
/// program produces that signature (vault PDA) only after a 2-of-2 vote.
#[derive(Accounts)]
pub struct AdminGate<'info> {
    #[account(mut, seeds = [SEED_POOL], bump = pool.bump, has_one = admin_authority)]
    pub pool: Account<'info, Pool>,

    /// The multisig vault PDA (devnet). A Signer constraint — the runtime
    /// verifies PDA signatures cross-program, so this accepts either a
    /// plain key (tests) or the Squads-executed vault signature (devnet).
    pub admin_authority: Signer<'info>,
}

// ---------------------------------------------------------------------------
// Events. Anchor serializes these into transaction logs (`emit!`), so M11's
// reconciler can rebuild the on-chain mirror by subscribing — no polling of
// every account needed. CONTRACT (A-042, fixed 2026-11-02): every *_after
// field is POST-instruction truth — the emitters reload the CPI-mutated
// account right before `emit!`, because Anchor otherwise holds the
// deserialized-at-entry (stale) copy. A subscriber must never have to
// wonder whether an event lags the state it claims to mirror.
// ---------------------------------------------------------------------------

/// Emitted once, at `initialize`.
#[event]
pub struct PoolOpened {
    pub admin_authority: Pubkey,
    pub backend_authority: Pubkey,
    pub usdc_mint: Pubkey,
    pub claim_mint: Pubkey,
    pub vault: Pubkey,
}

/// A contribution was recorded (mint side of the registry). NO USDC
/// moved — `amount` is the micro-USD valuation the books attest.
#[event]
pub struct ContributionRecorded {
    pub depositor: Pubkey,
    pub amount: u64,
    /// Claim-mint total supply after the mint — the Σ `claims.contributed`
    /// mirror. Deliberately NOT a vault balance: the vault is untouched.
    pub supply_after: u64,
}

/// A record was exited (burn side of the registry). NO USDC moved.
#[event]
pub struct ClaimsBurned {
    pub depositor: Pubkey,
    pub amount: u64,
    pub supply_after: u64,
}

/// Renter USDC entered escrow — the ONLY inflow (D40). Named for the
/// action (not `RenterPrepaid`, which is the STATE account's name).
#[event]
pub struct PrepayRecorded {
    pub renter: Pubkey,
    pub amount: u64,
    /// Vault USDC after the transfer — the `cash.escrow` mirror.
    pub vault_balance_after: u64,
    /// The renter's own prepaid record after the increment.
    pub prepaid_after: u64,
}

/// Renter USDC left escrow, bounded by their own on-chain record.
#[event]
pub struct PrepayRefunded {
    pub renter: Pubkey,
    pub amount: u64,
    pub vault_balance_after: u64,
    pub prepaid_after: u64,
}

/// Circuit breaker toggled by admin.
#[event]
pub struct PauseToggled {
    pub paused: bool,
}

/// Backend hot key rotated.
#[event]
pub struct BackendAuthorityRotated {
    pub old: Pubkey,
    pub new: Pubkey,
}

/// Admin authority rotated (multisig migration).
#[event]
pub struct AdminAuthorityRotated {
    pub old: Pubkey,
    pub new: Pubkey,
}
