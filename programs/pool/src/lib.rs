//! Tributary root pool — on-chain custody rails (SPEC §12, V2 milestone M8).
//!
//! This program is the PUBLIC TWIN of the internal double-entry ledger M7
//! shipped (`crates/ledger`). It does exactly two custodial things and
//! nothing else: it holds depositor principal in a program-controlled USDC
//! escrow vault, and it keeps a per-depositor CLAIM REGISTRY as a
//! non-transferable Token-2022 mint. It deliberately does NOT settle —
//! Merkle batches, ceilings, challenges and bonds are M11 (SPEC §14 V2).
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
//! | per-depositor claim balance (`[claim, depositor]` account) | `claims.contributed` owner row (liability, capacity-backed, deliberately OUTSIDE rule 7) |
//! | claim mint total supply | Σ `claims.contributed` |
//!
//! Both mints carry 6 decimals, so 1 minor unit on-chain == 1 micro-USD ==
//! the ledger's `MicroUsd` integer unit. No conversion exists anywhere.
//! `prepaid.renter` and `payable.depositor` have NO on-chain twin yet —
//! renters and settled payables arrive with M10/M11; the rule-7 inequality
//! becomes an on-chain check exactly then, inside M11's reconciliation.
//!
//! # Authority model (least privilege, devnet posture per kickoff)
//!
//! * `admin_authority` — the Squads multisig vault PDA (devnet: 2-of-2,
//!   founder + teammate; SPEC §12 keeps 2-of-3 as the production posture).
//!   Pause/resume and authority rotations require its signature, which only
//!   the Squads program can produce after a 2-of-2 vote. The program's
//!   upgrade authority is that same multisig — a single-key upgrade
//!   authority would be a finding against this milestone.
//! * `backend_authority` — the operator's hot key. It signs deposits and
//!   withdrawals: members, renters and depositors NEVER need a wallet
//!   (rule 10, SPEC §4.5) — the backend submits their flows custodially.
//!
//! # Rule 9 — claims are ledger claims, not a tradable token
//!
//! The claim mint carries the Token-2022 `NonTransferable` extension: the
//! token program ITSELF rejects any transfer between accounts, so no bug in
//! this program, no future upgrade short of a multisig vote, and no holder
//! key can ever make a claim tradable. Claims are created only by `deposit`
//! (mint, 1:1 with principal) and destroyed only by `withdraw` (burn). The
//! claim token accounts are PDAs owned by the pool PDA — no depositor key
//! can even burn or close them directly.

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
pub use state::{DECIMALS, Pool, SEED_CLAIM, SEED_CLAIM_MINT, SEED_POOL, SEED_VAULT};

/// THE M11 SEAM — withdrawal ceiling check.
///
/// M8 ships custody rails only: there is no per-period withdrawal ceiling
/// yet, so this function is an explicit, tested no-op. M11's rate-limited
/// withdrawals (SPEC §11 "liquidity buffer parameter") slot in by replacing
/// this ONE body — the instruction signature, accounts and seam stay fixed.
fn check_withdrawal_ceiling(
    _pool: &Account<Pool>,
    _depositor: &Pubkey,
    _amount: u64,
) -> Result<()> {
    Ok(())
}

/// # Instructions
///
/// Seven instructions, no more: initialize, deposit, withdraw, pause,
/// resume, and two authority rotations. There is intentionally no
/// `transfer_claim`, no `close_claim`, no `sweep` — the absence of an
/// instruction IS a security property here (rules 9 and 10).
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

    /// Record a depositor principal contribution: move USDC from the
    /// depositor's (custodial) token account into the vault, and mint
    /// claims 1:1 into their program-owned claim account.
    ///
    /// Signers: `backend_authority` (the operator submitting for the
    /// depositor — rule 10) and `depositor` (their custodial key, which
    /// must authorize the token-level USDC move). On devnet the backend
    /// holds both; a depositor who later brings their own key changes
    /// nothing about this instruction's shape.
    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        require!(!ctx.accounts.pool.paused, PoolError::Paused);
        require!(amount > 0, PoolError::ZeroAmount);

        let pool = &ctx.accounts.pool;
        let depositor = ctx.accounts.depositor.key();
        let seeds = &[SEED_POOL, &[pool.bump]];

        // 1) Principal into escrow: USDC source -> vault (SPL Token v3
        //    transfer; `depositor` signed the transaction, which the token
        //    program checks as the source account's owner).
        token::transfer(
            CpiContext::new(
                *ctx.accounts.token_program.key,
                token::Transfer {
                    from: ctx.accounts.source.to_account_info(),
                    to: ctx.accounts.vault.to_account_info(),
                    authority: ctx.accounts.depositor.to_account_info(),
                },
            ),
            amount,
        )?;

        // 2) Registry entry: mint `amount` claims to `[claim, depositor]`.
        //    The pool PDA is the mint authority and signs via CPI seeds.
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

        emit!(Deposited {
            depositor,
            amount,
            vault_balance_after: ctx.accounts.vault.amount,
        });
        Ok(())
    }

    /// Pay out depositor principal: burn `amount` claims from their claim
    /// account and send the same `amount` of USDC from the vault to
    /// `destination` (a token account the depositor owns).
    ///
    /// Signer: `backend_authority` ONLY — the depositor does not sign
    /// (rule 10: assistable by the operator's backend; on devnet the
    /// backend also holds the destination key). M11's withdrawal ceiling
    /// and challenge window attach inside this handler, at
    /// `check_withdrawal_ceiling`, without changing this signature.
    pub fn withdraw(ctx: Context<Withdraw>, depositor: Pubkey, amount: u64) -> Result<()> {
        require!(!ctx.accounts.pool.paused, PoolError::Paused);
        require!(amount > 0, PoolError::ZeroAmount);

        let pool = &ctx.accounts.pool;
        let seeds = &[SEED_POOL, &[pool.bump]];

        // M11 SEAM: rate-limit/liquidity-buffer check attaches here.
        check_withdrawal_ceiling(pool, &depositor, amount)?;

        // The claim account is keyed by the SAME `depositor` pubkey its
        // seeds derived from (anchor constraint below), so this balance is
        // by construction the depositor's registry row.
        require!(
            ctx.accounts.claim_account.amount >= amount,
            PoolError::InsufficientClaims
        );

        // 1) Registry exit: burn the claims (pool PDA owns the account and
        //    signs the Token-2022 burn via CPI seeds).
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

        // 2) Principal out of escrow: vault -> destination, pool PDA signs.
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

        emit!(Withdrawn {
            depositor,
            amount,
            vault_balance_after: ctx.accounts.vault.amount,
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
    /// Mint authority is the pool PDA — only `deposit` mints, only
    /// `withdraw` burns.
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

/// Accounts for [`pool_program::deposit`].
#[derive(Accounts)]
pub struct Deposit<'info> {
    /// `has_one` checks `pool.backend_authority == backend_authority.key()`
    /// — the operator gate for every custodial movement (rule 10).
    #[account(
        mut,
        seeds = [SEED_POOL],
        bump = pool.bump,
        has_one = backend_authority
    )]
    pub pool: Account<'info, Pool>,

    /// Operator backend hot key; also pays the claim account's rent on
    /// first deposit (depositors never pay anything).
    #[account(mut)]
    pub backend_authority: Signer<'info>,

    /// The depositor identity. Signs the transaction because their
    /// (custodial) key owns `source`; on devnet the backend holds it.
    pub depositor: Signer<'info>,

    /// Where the principal comes from: must be the depositor's account on
    /// the pool's USDC mint.
    #[account(
        mut,
        constraint = source.owner == depositor.key() @ PoolError::WrongSourceOwner,
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

    /// The registry mint (seeds re-checked; typed so decimals/authority
    /// are constraint-verified on every deposit).
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
    /// `init_if_needed`: first deposit lazily creates the row; later
    /// deposits reuse it (the program pays rent via backend_authority).
    #[account(
        init_if_needed,
        payer = backend_authority,
        seeds = [SEED_CLAIM, depositor.key().as_ref()],
        bump,
        token::mint = claim_mint,
        token::authority = pool,
        token::token_program = token_2022_program
    )]
    pub claim_account: InterfaceAccount<'info, InterfaceTokenAccount>,

    pub token_program: Program<'info, Token>,
    pub token_2022_program: Program<'info, Token2022>,
    pub system_program: Program<'info, System>,
}

/// Accounts for [`pool_program::withdraw`].
// `#[instruction(...)]` exposes instruction arguments to the constraint
// solver: `depositor` arrives as a Pubkey ARG (the depositor signs
// nothing — rule 10), and the claim-account seeds below bind this exact
// pubkey, so a payout can only burn the named depositor's own row.
#[derive(Accounts)]
#[instruction(depositor: Pubkey)]
pub struct Withdraw<'info> {
    #[account(
        mut,
        seeds = [SEED_POOL],
        bump = pool.bump,
        has_one = backend_authority
    )]
    pub pool: Account<'info, Pool>,

    /// The ONLY signer: the operator backend submitting for the depositor.
    #[account(mut)]
    pub backend_authority: Signer<'info>,

    /// Withdrawal destination must belong to the depositor being paid —
    /// the backend cannot route a payout to itself.
    #[account(
        mut,
        constraint = destination.owner == depositor @ PoolError::WrongDestinationOwner,
        constraint = destination.mint == pool.usdc_mint @ PoolError::WrongDestinationMint
    )]
    pub destination: Account<'info, TokenAccount>,

    /// NOTE: `depositor` arrives as an instruction ARGUMENT (Pubkey), not
    /// an account — the depositor signs nothing to be paid out (rule 10).

    #[account(
        mut,
        seeds = [SEED_POOL, SEED_VAULT],
        bump,
        token::mint = pool.usdc_mint,
        token::authority = pool
    )]
    pub vault: Account<'info, TokenAccount>,

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

    pub token_program: Program<'info, Token>,
    pub token_2022_program: Program<'info, Token2022>,
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

/// A principal contribution was recorded (mint side of the registry).
#[event]
pub struct Deposited {
    pub depositor: Pubkey,
    pub amount: u64,
    /// Vault USDC balance after the transfer — the `cash.escrow` mirror.
    pub vault_balance_after: u64,
}

/// A principal payout was executed (burn side of the registry).
#[event]
pub struct Withdrawn {
    pub depositor: Pubkey,
    pub amount: u64,
    pub vault_balance_after: u64,
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
