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
//! from M11, settle payouts. It deliberately does NOT settle — Merkle
//! batches, ceilings, challenges and bonds are M11 (SPEC §14 V2).
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
pub mod state;

pub use errors::PoolError;
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
/// Nine instructions, no more: initialize, record_contribution,
/// burn_claims, prepay_renter, refund_renter, pause, resume, and two
/// authority rotations. There is intentionally no `transfer_claim`, no
/// `close_claim`, no instruction that moves depositor USDC — the absence
/// of an instruction IS a security property here (rules 1, 9 and 10).
#[program]
pub mod pool_program {
    use super::*;

    /// One-time pool bootstrap. Creates the config PDA, the Token-2022
    /// `NonTransferable` claim mint, and the USDC vault. Called by the
    /// deployer immediately after program deploy; the `admin_authority`
    /// passed in is the Squads multisig vault PDA (computed before this
    /// call from the multisig's create key), so from its first block the
    /// pool is admin-governed by 2-of-2 — there is no window where a
    /// single dev key administers the pool.
    pub fn initialize(
        ctx: Context<Initialize>,
        admin_authority: Pubkey,
        backend_authority: Pubkey,
    ) -> Result<()> {
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
        // Reserved bytes start zeroed; M11 settlement params land there.
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
// every account needed.
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
