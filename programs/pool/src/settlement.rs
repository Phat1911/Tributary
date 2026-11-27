//! M11 SETTLEMENT — the settlement layer, implemented
//! (doctrine-suite mode: Session A, 2026-11-26, declarations; Session B,
//! 2026-11-27, implementation to green; workflow step 14).
//!
//! This module carries the settlement layer: seeds, state accounts, the
//! typed account lists, the events and the four Merkle helpers (`sha256`,
//! `leaf_hash`, `node_hash`, `verify_proof`). The six INSTRUCTION HANDLERS
//! live in `lib.rs`'s dispatcher (`#[program] mod pool_program` — Anchor 1.2
//! registers only handlers written directly inside that module). Session A
//! shipped these as `todo!()` stubs; Session B implemented them, and the
//! twelve-test doctrine suite (`tests/m11_doctrine.rs`) is green.
//!
//! # The doctrine this module encodes (twelve sentences — the test names)
//!
//! 1. A root whose total payout exceeds the vault's USDC fails closed and
//!    moves nothing.
//! 2. A claim whose inclusion proof does not verify against the published
//!    root is rejected.
//! 3. One leaf funds exactly one claim; a replayed claim is refused.
//! 4. A payout reaches only the leaf's named depositor, through either door.
//! 5. A challenge inside the window freezes that root's payouts until the
//!    multisig rules.
//! 6. The bond slashes by exactly the ruled amount for a root proven false,
//!    and by nothing otherwise.
//! 7. A second root for the same window is refused; roots are append-only.
//! 8. Every state change is reconstructible from emitted events alone.
//! 9. Settlement mints no transferable receipt.
//! 10. No single key and no upgrade authority can move vault funds; only a
//!     satisfied program instruction can.
//! 11. No payout leaves the vault before the challenge window has passed.
//! 12. A root never pays out more than its declared total.
//!
//! Sentences 11 and 12 come from the GO-gate ruling of 2026-11-26, and so
//! does the claim rule below.
//!
//! # The shape, in one paragraph
//!
//! The operator posts one Merkle root per settlement WINDOW. Every leaf names
//! one depositor and a micro-USDC amount. The vault (renter prepaid USDC —
//! the only on-chain money, D40) pays each leaf ONCE, to a wallet the leaf's
//! named depositor owns — but NOT YET: a root pays nothing until its
//! challenge window has passed, unless the multisig has ruled the root
//! stands (invariant 11), and it never pays more than the total it declared
//! (invariant 12). Anyone may challenge a root while its challenge window is
//! open; a challenge FREEZES that root's payouts until the multisig rules.
//! A ruling that the root stands unfreezes it and waives the rest of its
//! window; a ruling that the root is false records the exact slash and the
//! victim, and the slash then executes as that exact number — never more,
//! never less, never without such a ruling.
//!
//! # The claim rule, in one line (invariants 5 and 11)
//!
//! A claim succeeds iff the root is `Active` AND (the challenge window has
//! passed OR the multisig ruled the root stands). The handler therefore
//! checks, IN THIS ORDER: (1) the status, else `RootFrozen`; (2) the window,
//! else `ChallengeWindowOpen`. The order is load-bearing: a root challenged
//! while still INSIDE its window must refuse with `RootFrozen` (the suite
//! pins it) — a freeze is not a delay. `challenge` clears `stands_ruled`,
//! so "the multisig ruled it stands" always means the LATEST challenge, if
//! any.
//!
//! # Why each invariant is STRUCTURAL where it can be
//!
//! A rule enforced by an address cannot be bypassed by handler logic, so
//! the suite leans on PDAs before it leans on checks:
//!
//! | Rule | Structural enforcement | What is left to handler code |
//! |---|---|---|
//! | 3 (one leaf, one claim) | `ClaimMarker` is `[claim_marker, root, leaf_index]` with plain `init` — the account can be created once, EVER | nothing: a replay dies at account creation, before any handler runs |
//! | 7 (append-only roots) | `SettlementRoot` is `[settlement_root, window]` with plain `init` — one account per window | nothing: a second root for a window dies at account creation |
//! | 4 (named depositor only) | `destination.owner == depositor` as an account constraint | which door the signer opens |
//! | 10 (no key moves the vault) | the vault's token authority is the pool PDA (M8) — no key can sign for it | nothing |
//! | 6 (nothing without a ruling) | the ruled amount and victim come from the ROOT account — `slash_bond` takes neither as an argument | refuse unless `ProvenFalse`; check the destination's owner AFTER that (so the refusal names its reason) |
//! | 11 (no payout before the window passes) | a check, not an address | `claim`: refuse `ChallengeWindowOpen` unless the window has passed or the multisig ruled the root stands |
//! | 12 (never more than the declared total) | a check, not an address | `claim`: refuse `PayoutExceedsTotal` unless `paid_out + amount ≤ total_payout`; then add `amount` to `paid_out` |
//!
//! `init` (not `init_if_needed`) is a deliberate choice on both markers: a
//! refused replay surfaces as an account-creation failure rather than a
//! named program error, and that is the price of making double-payment
//! impossible regardless of what a handler does.
//!
//! # Addresses
//!
//! | Account | Seeds | One per |
//! |---|---|---|
//! | `SettlementRoot` | `[SEED_ROOT, window_le_u64]` | window |
//! | `ClaimMarker` | `[SEED_CLAIM_MARKER, root_pubkey, leaf_index_le_u32]` | root × leaf |
//! | `OperatorBond` | `[SEED_POOL, SEED_BOND]` | pool (one bond per deployment, D42) |
//! | bond vault (Token v3) | `[SEED_POOL, SEED_BOND_VAULT]`, authority = pool PDA | pool |
//!
//! # The Merkle contract (the test suite builds trees with this exact recipe)
//!
//! * `leaf_hash = SHA-256(depositor_pubkey[32] ‖ amount_le_u64[8])` — 40 bytes.
//! * `node_hash = SHA-256(left[32] ‖ right[32])` — ORDER-SENSITIVE, 64 bytes.
//! * Leaves sit at level 0 in INDEX ORDER; each level pairs its nodes left to
//!   right (2k with 2k+1); a level with an odd count pairs its LAST node
//!   with itself.
//! * A proof is the list of sibling hashes from the leaf's level upward.
//! * Verification walks the proof TOGETHER WITH the leaf's index:
//!   `h = leaf; idx = leaf_index; for sib in proof { h = if idx is even
//!   { node_hash(h, sib) } else { node_hash(sib, h) }; idx >>= 1 }`, and
//!   succeeds iff `h == root`.
//!
//! # Why the proof is bound to the leaf's INDEX
//!
//! The claim marker is one per (`root`, `leaf_index`) — so a proof that
//! merely proved "this leaf is somewhere in the tree" would let the SAME
//! leaf be claimed a second time under a different, still-unclaimed index,
//! using its own valid proof (a sorted-pair tree has exactly that shape).
//! Walking the index bits makes a proof assert "this leaf is at THIS
//! position", so the marker's index and the proof's position are one fact.
//! The node preimage (64 bytes) and the leaf preimage (40 bytes) differ in
//! length, so a leaf can never pose as an interior node.

use anchor_lang::prelude::*;
// The program's own error enum, the pool config type and the two pool-level
// seeds the settlement accounts derive from (same items the M8 handlers in
// `lib.rs` use).
use crate::{Pool, PoolError, SEED_POOL, SEED_VAULT};
// Token v3 types for the USDC legs (bond vault, payouts) — the escrow side
// of the program; the Token-2022 claim registry is deliberately NOT imported
// here: settlement never touches it (invariant 9).
use anchor_spl::token::{self, Token, TokenAccount};

// ---------------------------------------------------------------------------
// Seeds and parameters
// ---------------------------------------------------------------------------

/// Seed prefix for settlement roots: `[SEED_ROOT, window_le_u64]`. One root
/// per window, ever — the address IS the append-only guarantee (invariant 7).
pub const SEED_ROOT: &[u8] = b"settlement_root";

/// Seed prefix for claim markers: `[SEED_CLAIM_MARKER, root, leaf_index]`.
/// One marker per leaf, ever — the address IS the one-claim guarantee
/// (invariant 3).
pub const SEED_CLAIM_MARKER: &[u8] = b"claim_marker";

/// Seed suffix for the operator bond RECORD (`[SEED_POOL, SEED_BOND]`).
pub const SEED_BOND: &[u8] = b"bond";

/// Seed suffix for the operator bond VAULT token account
/// (`[SEED_POOL, SEED_BOND_VAULT]`, authority = pool PDA).
pub const SEED_BOND_VAULT: &[u8] = b"bond_vault";

/// How many slots a root stays challengeable, counted from the slot it was
/// posted in: challenge is allowed while `clock.slot < posted_at + WINDOW`.
///
/// The window is a CONST, not an argument: a caller-supplied window would
/// let a challenger stretch it. 100 slots is DEMO-SIZED (about 40 s on
/// devnet at ~400 ms per slot); the production posture is a director call
/// and a D-log entry, not a silent edit here.
pub const CHALLENGE_WINDOW_SLOTS: i64 = 100;

// ---------------------------------------------------------------------------
// Enumerations
// ---------------------------------------------------------------------------

/// `RootStatus` is borsh-encoded inside `SettlementRoot` as one byte (the
/// variant ordinal) — mirrors the way `Pool` stores `paused`.
#[derive(InitSpace, AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq, Debug)]
pub enum RootStatus {
    /// Posted, or challenged and then ruled to stand. Leaves pay once the
    /// challenge window has passed — or at once, if the multisig ruled the
    /// root stands (`stands_ruled`).
    Active,
    /// Challenged inside the window, awaiting the multisig: payouts frozen.
    Challenged,
    /// The multisig ruled the root false: payouts frozen for good, a slash
    /// is ruled and waiting to execute.
    ProvenFalse,
    /// The ruled slash has executed. Terminal.
    Slashed,
}

/// `Ruling` travels as instruction data (borsh: variant byte, then fields),
/// so the derive below is what puts it on the wire.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq, Debug)]
pub enum Ruling {
    /// The root stands: it returns to `Active`, nothing is slashed, and the
    /// ruling waives the rest of its challenge window (`stands_ruled`).
    RootStands,
    /// The root is proven false: EXACTLY `slash_amount` micro-USDC of the
    /// bond goes to `victim`. No amount, no victim, no ruling.
    RootFalse { slash_amount: u64, victim: Pubkey },
}

// ---------------------------------------------------------------------------
// State accounts
// ---------------------------------------------------------------------------

/// One posted root. Created once per window by `post_root` and never
/// rewritten: only `status`, `stands_ruled`, `challenger`, `evidence`,
/// `ruled_slash`, `victim` and `paid_out` ever change after birth — every
/// other field is write-once (invariant 7 — roots are append-only).
#[account] // generates the 8-byte discriminator + Owner impl (owner = this program)
#[derive(InitSpace)] // const INIT_SPACE: the exact borsh size, for `space = 8 + INIT_SPACE`
pub struct SettlementRoot {
    /// The pool this root belongs to (D42: no cross-pool drawing).
    pub pool: Pubkey,
    /// The settlement window — the right half of this account's seeds.
    pub window: u64,
    /// The SHA-256 Merkle root over the window's payout leaves.
    pub merkle_root: [u8; 32],
    /// Sum of the leaf amounts, as attested by the operator. Checked against
    /// the vault at posting (invariant 1).
    pub total_payout: u64,
    /// Lifecycle: Active → Challenged → Active | ProvenFalse → Slashed.
    pub status: RootStatus,
    /// Slot the root was posted in — where the challenge window starts.
    pub posted_at: i64,
    /// The operator bond backing THIS root, snapshotted at posting. A later
    /// ruling may not slash more than this (invariant 6).
    pub bond: u64,
    /// Who challenged the root (default pubkey while unchallenged).
    pub challenger: Pubkey,
    /// Commitment to the challenger's off-chain fraud proof — recorded, not
    /// verified: the chain cannot read the books, the multisig reads them.
    pub evidence: [u8; 32],
    /// The exact slash the multisig ruled (0 until a `RootFalse` ruling).
    pub ruled_slash: u64,
    /// Whom the ruled slash pays (default pubkey until ruled).
    pub victim: Pubkey,
    /// True once the multisig has ruled the LATEST challenge (if any) to
    /// stand: that ruling waives the rest of the challenge window for this
    /// root (invariant 11). `challenge` clears it.
    pub stands_ruled: bool,
    /// Micro-USDC this root has paid out so far. A claim requires
    /// `paid_out + amount <= total_payout` (invariant 12).
    pub paid_out: u64,
    /// Bump of this PDA.
    pub bump: u8,
}

/// Proof that leaf `leaf_index` of one root has been paid. Created by `claim`
/// and never updated: its EXISTENCE is the fact (invariant 3).
#[account]
#[derive(InitSpace)]
pub struct ClaimMarker {
    /// The root this marker belongs to.
    pub root: Pubkey,
    /// The leaf that was paid (its index in the window's tree).
    pub leaf_index: u32,
    /// The leaf's named depositor — the only identity the payout may reach.
    pub depositor: Pubkey,
    /// Micro-USDC paid out for this leaf.
    pub amount: u64,
    /// Bump of this PDA.
    pub bump: u8,
}

/// The operator's bond record — the on-chain twin of the bond vault's USDC
/// balance. One per pool: a bonded operator posts roots against it.
#[account]
#[derive(InitSpace)]
pub struct OperatorBond {
    /// Micro-USDC bonded.
    pub amount: u64,
    /// Bump of this PDA.
    pub bump: u8,
}

// ---------------------------------------------------------------------------
// Typed account lists — one per instruction, consumed POSITIONALLY on the wire
// (the test harness mirrors each field-for-field). Read-only accounts carry
// no `mut`: an instruction that cannot write an account cannot move it.
// ---------------------------------------------------------------------------

/// Accounts for `fund_bond`: USDC from the OPERATOR's own wallet into the
/// bond vault; the backend hot key signs and pays rent.
#[derive(Accounts)]
pub struct FundBond<'info> {
    #[account(seeds = [SEED_POOL], bump = pool.bump, has_one = backend_authority)]
    pub pool: Account<'info, Pool>,

    /// The operator's hot key — gate AND rent payer.
    #[account(mut)]
    pub backend_authority: Signer<'info>,

    /// Where the bond comes from: a wallet the BACKEND owns, on the pool's
    /// USDC mint (the operator bonds its own money).
    #[account(
        mut,
        constraint = source.owner == backend_authority.key() @ PoolError::WrongSourceOwner,
        constraint = source.mint == pool.usdc_mint @ PoolError::WrongSourceMint
    )]
    pub source: Account<'info, TokenAccount>,

    /// The pool's USDC mint, as a typed ACCOUNT: Anchor's token-account
    /// init can only bind `token::mint` to an account field, so the mint
    /// rides along here and is pinned to the pool's own below (a foreign
    /// mint cannot give birth to the bond vault).
    #[account(constraint = usdc_mint.key() == pool.usdc_mint @ PoolError::WrongSourceMint)]
    pub usdc_mint: Account<'info, token::Mint>,

    /// The bond vault: a pool-owned token account, so no key can move the
    /// bond directly — only `slash_bond` (a satisfied instruction) can.
    #[account(
        init_if_needed,
        payer = backend_authority,
        seeds = [SEED_POOL, SEED_BOND_VAULT],
        bump,
        token::mint = usdc_mint,
        token::authority = pool
    )]
    pub bond_vault: Account<'info, TokenAccount>,

    /// The bond record. `init_if_needed`: first funding creates it, later
    /// fundings top it up (every reuse re-verifies seeds and owner).
    #[account(
        init_if_needed,
        payer = backend_authority,
        space = 8 + OperatorBond::INIT_SPACE,
        seeds = [SEED_POOL, SEED_BOND],
        bump
    )]
    pub bond: Account<'info, OperatorBond>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

/// Accounts for `post_root`. The vault is READ-ONLY here on purpose: posting
/// a root never moves funds, so the instruction cannot even write the
/// account it measures (invariant 1 — "fails closed and moves nothing").
#[derive(Accounts)]
// `#[instruction(window: u64)]` exposes `window` to the seed expression below.
#[instruction(window: u64)]
pub struct PostRoot<'info> {
    #[account(seeds = [SEED_POOL], bump = pool.bump, has_one = backend_authority)]
    pub pool: Account<'info, Pool>,

    /// The operator posts roots: a root is the operator's ATTESTATION of
    /// what the books say the window owes (A-039's caveat, by design).
    #[account(mut)]
    pub backend_authority: Signer<'info>,

    /// Plain `init`: the account can be born ONCE per window. A second root
    /// for the same window fails HERE, in account creation (invariant 7).
    #[account(
        init,
        payer = backend_authority,
        space = 8 + SettlementRoot::INIT_SPACE,
        seeds = [SEED_ROOT, window.to_le_bytes().as_ref()],
        bump
    )]
    pub settlement_root: Account<'info, SettlementRoot>,

    /// The escrow the root will be paid from — read for the ceiling check.
    #[account(
        seeds = [SEED_POOL, SEED_VAULT],
        bump,
        token::mint = pool.usdc_mint,
        token::authority = pool
    )]
    pub vault: Account<'info, TokenAccount>,

    /// The bond backing the root. Not `init`: a root cannot be posted
    /// without an existing bond (SPEC §10 — BONDED operators).
    #[account(seeds = [SEED_POOL, SEED_BOND], bump)]
    pub bond: Account<'info, OperatorBond>,

    pub token_program: Program<'info, Token>,

    /// The clock, DECLARED like the rent sysvar in `initialize`: the suite
    /// passes it as an account so host and SBF execution read the same
    /// source of slots.
    pub clock: Sysvar<'info, Clock>,

    pub system_program: Program<'info, System>,
}

/// Accounts for `claim` — THE TWO DOORS (D41) for payouts. `authority` is
/// the backend (assisted) or the leaf's depositor (direct); the handler
/// decides which, exactly like `burn_claims` and `refund_renter`. The claim
/// rule itself (Active, and the window passed or ruled stands) is the
/// handler's — see "The claim rule, in one line" in the module docs.
#[derive(Accounts)]
// `#[instruction(...)]` lists the handler's arguments IN DECLARATION ORDER
// (Anchor type-checks the two against each other at compile time); the
// three below are the ones the seed expressions need. `depositor` names WHO
// the leaf pays: it also binds the destination's owner below.
#[instruction(window: u64, leaf_index: u32, depositor: Pubkey)]
pub struct Claim<'info> {
    #[account(seeds = [SEED_POOL], bump = pool.bump)]
    pub pool: Account<'info, Pool>,

    /// The one signer — WHICH door it opens is decided in the handler.
    pub authority: Signer<'info>,

    /// The root the proof must verify against. `mut`: the handler counts the
    /// payout on it (`paid_out`, invariant 12) — without `mut` Anchor never
    /// serializes the update back, so the write is silently dropped.
    #[account(mut, seeds = [SEED_ROOT, window.to_le_bytes().as_ref()], bump)]
    pub settlement_root: Account<'info, SettlementRoot>,

    /// Where the payout lands. `owner == depositor` is an ACCOUNT
    /// CONSTRAINT: neither door can route a payout anywhere else
    /// (invariant 4).
    #[account(
        mut,
        constraint = destination.owner == depositor @ PoolError::WrongPayoutDestination,
        constraint = destination.mint == pool.usdc_mint @ PoolError::WrongDestinationMint
    )]
    pub destination: Account<'info, TokenAccount>,

    /// Escrow source — pool-owned; only this instruction can draw on it.
    #[account(
        mut,
        seeds = [SEED_POOL, SEED_VAULT],
        bump,
        token::mint = pool.usdc_mint,
        token::authority = pool
    )]
    pub vault: Account<'info, TokenAccount>,

    /// Plain `init`: the marker can be born ONCE per leaf. A replay fails
    /// HERE, in account creation, before any handler logic runs
    /// (invariant 3).
    #[account(
        init,
        payer = payer,
        space = 8 + ClaimMarker::INIT_SPACE,
        seeds = [
            SEED_CLAIM_MARKER,
            settlement_root.key().as_ref(),
            leaf_index.to_le_bytes().as_ref()
        ],
        bump
    )]
    pub claim_marker: Account<'info, ClaimMarker>,

    /// Rent payer for the marker. A SEPARATE account from `authority` so the
    /// direct door needs nothing from the depositor but their signature: any
    /// funded key (the transaction's fee payer, a relayer) can pay.
    #[account(mut)]
    pub payer: Signer<'info>,

    /// The clock, DECLARED like in `post_root` and `challenge`: the handler
    /// reads the slot from it to apply the challenge-window rule
    /// (invariant 11).
    pub clock: Sysvar<'info, Clock>,

    /// Token v3 ONLY. No Token-2022 program and no mint account appear in
    /// this list: a claim moves USDC and cannot mint anything
    /// (invariant 9).
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

/// Accounts for `challenge` — permissionless (SPEC §10: "lets anyone submit
/// fraud proofs"). No `mut` on the challenger: challenging is free and
/// moves nothing; it only freezes.
#[derive(Accounts)]
#[instruction(window: u64)]
pub struct Challenge<'info> {
    #[account(seeds = [SEED_POOL], bump = pool.bump)]
    pub pool: Account<'info, Pool>,

    /// Any funded key.
    pub challenger: Signer<'info>,

    /// `has_one = pool`: a root can only be challenged through ITS pool.
    #[account(
        mut,
        seeds = [SEED_ROOT, window.to_le_bytes().as_ref()],
        bump,
        has_one = pool
    )]
    pub settlement_root: Account<'info, SettlementRoot>,

    pub clock: Sysvar<'info, Clock>,
}

/// Accounts for `resolve_challenge` — the MULTISIG RULES. `has_one =
/// admin_authority` is the gate: nothing but the configured admin's
/// signature (on devnet, a 2-of-2 Squads vote) can rule.
#[derive(Accounts)]
#[instruction(window: u64)]
pub struct ResolveChallenge<'info> {
    #[account(seeds = [SEED_POOL], bump = pool.bump, has_one = admin_authority)]
    pub pool: Account<'info, Pool>,

    /// The multisig vault PDA on devnet (a plain key in tests).
    pub admin_authority: Signer<'info>,

    #[account(
        mut,
        seeds = [SEED_ROOT, window.to_le_bytes().as_ref()],
        bump,
        has_one = pool
    )]
    pub settlement_root: Account<'info, SettlementRoot>,

    /// Read here so the handler can refuse a ruling larger than the bond.
    #[account(seeds = [SEED_POOL, SEED_BOND], bump)]
    pub bond: Account<'info, OperatorBond>,
}

/// Accounts for `slash_bond` — EXECUTES a ruling that already exists. No
/// signer on purpose: the ruling was the multisig's act; the execution is
/// anyone's crank, and it can only ever pay the recorded victim the
/// recorded amount (both come from the root account, not from arguments).
#[derive(Accounts)]
#[instruction(window: u64)]
pub struct SlashBond<'info> {
    #[account(seeds = [SEED_POOL], bump = pool.bump)]
    pub pool: Account<'info, Pool>,

    #[account(
        mut,
        seeds = [SEED_ROOT, window.to_le_bytes().as_ref()],
        bump,
        has_one = pool
    )]
    pub settlement_root: Account<'info, SettlementRoot>,

    #[account(
        mut,
        seeds = [SEED_POOL, SEED_BOND_VAULT],
        bump,
        token::mint = pool.usdc_mint,
        token::authority = pool
    )]
    pub bond_vault: Account<'info, TokenAccount>,

    #[account(mut, seeds = [SEED_POOL, SEED_BOND], bump)]
    pub bond: Account<'info, OperatorBond>,

    /// Where the slash lands. The OWNER check lives in the handler, AFTER
    /// the status check — on purpose: as an account constraint it would
    /// fire first for every root with no victim on record, and a slash of
    /// an un-ruled root would be reported as a destination error instead
    /// of `RootNotProvenFalse`. Only the mint check stays here (nothing but
    /// the pool's USDC is ever a payout leg).
    #[account(
        mut,
        constraint = destination.mint == pool.usdc_mint @ PoolError::WrongDestinationMint
    )]
    pub destination: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

// ---------------------------------------------------------------------------
// Events — invariant 8's whole point: a subscriber that reads ONLY these
// rebuilds every settlement account. Each carries its POST-instruction
// `*_after` value (the A-042 contract: reload the mutated account first).
// ---------------------------------------------------------------------------

/// Bond grew (or was born). `bond_after` mirrors `OperatorBond.amount`.
#[event]
pub struct BondFunded {
    pub funder: Pubkey,
    pub amount: u64,
    pub bond_after: u64,
}

/// A root was posted — every immutable field of `SettlementRoot`, plus the
/// bond snapshot and the slot (the window's origin).
#[event]
pub struct RootPosted {
    pub window: u64,
    pub merkle_root: [u8; 32],
    pub total_payout: u64,
    pub bond: u64,
    pub posted_at: i64,
}

/// One leaf was paid. A subscriber counts these to rebuild the markers and
/// the vault.
#[event]
pub struct ClaimPaid {
    pub window: u64,
    pub leaf_index: u32,
    pub depositor: Pubkey,
    pub amount: u64,
    /// Vault USDC AFTER the transfer — the `cash.escrow` mirror.
    pub vault_balance_after: u64,
}

/// The root froze.
#[event]
pub struct RootChallenged {
    pub window: u64,
    pub challenger: Pubkey,
    pub evidence: [u8; 32],
}

/// The multisig ruled: `root_stands` says which way; `slash_amount` and
/// `victim` are meaningful only when the root was ruled false.
#[event]
pub struct ChallengeResolved {
    pub window: u64,
    pub root_stands: bool,
    pub slash_amount: u64,
    pub victim: Pubkey,
}

/// A ruled slash executed — `amount` is EXACTLY the ruled amount.
#[event]
pub struct BondSlashed {
    pub window: u64,
    pub amount: u64,
    pub victim: Pubkey,
    pub bond_after: u64,
}

// ---------------------------------------------------------------------------
// Merkle helpers — implemented by Session B against the recipe fixed above.
// `sha256` is the `sol_sha256` syscall (GO-gate R1, road (b); AUDIT A-061);
// the other three build on it and match the harness's off-chain tree.
// ---------------------------------------------------------------------------

/// The MURMUR3_32 hash of a syscall name, computed at compile time. This is
/// the "static syscalls" calling convention the SBF toolchain uses: a
/// syscall is invoked by CALLING this hash as an address, because the VM
/// keeps its syscall registry keyed by exactly that hash — no `extern "C"`
/// symbol and no load-time relocation is involved. (`extern "C"` is the
/// WRONG road here: in a static-syscalls build the symbol is never bound,
/// so the call lands as a BPF-to-BPF jump to an unmapped address and the VM
/// dies with `exceeded max BPF to BPF call depth` — found the hard way.)
///
/// This is a verbatim, dependency-free copy of the const fn inside
/// `solana_define_syscall::define_syscall!` (the macro every on-chain crate
/// uses), re-declared because the kickoff forbids new dependencies and the
/// Anchor cohort re-exports no hashing primitive (AUDIT A-058).
#[cfg(target_os = "solana")]
const fn murmur3_32(buf: &[u8], seed: u32) -> u32 {
    const fn pre_mix(buf: [u8; 4]) -> u32 {
        u32::from_le_bytes(buf)
            .wrapping_mul(0xcc9e2d51)
            .rotate_left(15)
            .wrapping_mul(0x1b873593)
    }
    let mut hash = seed;
    let mut i = 0;
    while i < buf.len() / 4 {
        let buf = [buf[i * 4], buf[i * 4 + 1], buf[i * 4 + 2], buf[i * 4 + 3]];
        hash ^= pre_mix(buf);
        hash = hash.rotate_left(13);
        hash = hash.wrapping_mul(5).wrapping_add(0xe6546b64);
        i += 1;
    }
    match buf.len() % 4 {
        0 => {}
        1 => hash ^= pre_mix([buf[i * 4], 0, 0, 0]),
        2 => hash ^= pre_mix([buf[i * 4], buf[i * 4 + 1], 0, 0]),
        3 => hash ^= pre_mix([buf[i * 4], buf[i * 4 + 1], buf[i * 4 + 2], 0]),
        _ => {}
    }
    hash ^= buf.len() as u32;
    hash ^= hash.wrapping_shr(16);
    hash = hash.wrapping_mul(0x85ebca6b);
    hash ^= hash.wrapping_shr(13);
    hash = hash.wrapping_mul(0xc2b2ae35);
    hash ^= hash.wrapping_shr(16);
    hash
}

/// The address of the VM's SHA-256 syscall: `murmur3_32("sol_sha256")`.
#[cfg(target_os = "solana")]
const SOL_SHA256: usize = murmur3_32(b"sol_sha256", 0) as usize;

/// One (address, length) input slice for the SHA-256 syscall — the VM's
/// `SolBytes` record. `#[repr(C)]` pins the two fields to the exact 16-byte
/// layout the syscall reads (an 8-byte pointer, then an 8-byte length), so
/// no padding or reordering can corrupt what the VM sees.
#[cfg(target_os = "solana")]
#[repr(C)]
struct SolBytes {
    addr: *const u8,
    len: u64,
}

/// SHA-256, FIPS 180-4. The suite hashes on the host with the `sha2` crate,
/// so a correct implementation agrees with it bit for bit.
///
/// Implemented with the `sol_sha256` SYSCALL (GO-gate ruling R1, road (b)):
/// no dependency, and the on-chain hasher is the VM's own audited one. The
/// syscall ABI: it reads `vals_len` `SolBytes` records at `vals`, hashes
/// their concatenation, writes the 32-byte digest at `result`, returns 0.
/// The `unsafe` this needs is covered by AUDIT.md A-061; the soundness
/// argument is the SAFETY comment at the call site below.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    #[cfg(target_os = "solana")]
    {
        // One input slice covering all of `data`; the digest lands in `out`.
        let input = [SolBytes {
            addr: data.as_ptr(),
            len: data.len() as u64,
        }];
        let mut out = [0u8; 32];
        // SAFETY: `SOL_SHA256` is the murmur3 hash of "sol_sha256", which IS
        // the address of the VM's registered SHA-256 syscall (the registry is
        // keyed by that hash), so transmuting it to the documented 3-argument
        // function-pointer type yields a valid callee with the exact ABI the
        // syscall expects. For the duration of the call `input` is a live,
        // aligned one-element `SolBytes` array (16 bytes) and `data` outlives
        // it, so the VM reads only initialized memory we own; `out` is a
        // writable 32-byte stack buffer, exactly the digest size written;
        // `vals_len` is 1, matching the array length. No pointer escapes the
        // call, so nothing dangles afterwards.
        unsafe {
            let syscall: unsafe extern "C" fn(*const u8, u64, *mut u8) -> u64 =
                core::mem::transmute(SOL_SHA256);
            syscall(input.as_ptr() as *const u8, 1, out.as_mut_ptr());
        }
        out
    }
    #[cfg(not(target_os = "solana"))]
    {
        // Never executed: the helpers run only inside the compiled program
        // (the SBF .so), and the host-side unit tests pin wire shapes, not
        // hashes. The syscall has no host equivalent without a new
        // dependency, which the kickoff forbids.
        let _ = data;
        unimplemented!("sol_sha256 exists only inside the SBF VM; host builds never hash")
    }
}

/// `SHA-256(depositor ‖ amount_le_u64)` — a leaf of the payout tree. The
/// preimage is exactly 40 bytes (32 + 8) on the stack: no heap allocation
/// on-chain, and a length no interior node can share (64), so a leaf can
/// never pose as a node.
pub fn leaf_hash(depositor: &Pubkey, amount: u64) -> [u8; 32] {
    let mut preimage = [0u8; 40];
    preimage[..32].copy_from_slice(depositor.as_ref());
    preimage[32..].copy_from_slice(&amount.to_le_bytes());
    sha256(&preimage)
}

/// `SHA-256(left ‖ right)` — an interior node of the payout tree. Order
/// matters: this is NOT a sorted-pair tree (see "Why the proof is bound to
/// the leaf's INDEX" in the module docs). The 64-byte preimage is built in
/// one stack buffer.
pub fn node_hash(left: [u8; 32], right: [u8; 32]) -> [u8; 32] {
    let mut preimage = [0u8; 64];
    preimage[..32].copy_from_slice(&left);
    preimage[32..].copy_from_slice(&right);
    sha256(&preimage)
}

/// True iff `leaf`, hashed up through `proof` while walking the bits of
/// `leaf_index` (even index: the running hash goes on the LEFT of its
/// sibling), reaches `root`. Pure: reads nothing but its arguments. One
/// `node_hash` per sibling, exactly the off-chain builder's `apply_proof`.
pub fn verify_proof(root: [u8; 32], leaf_index: u32, leaf: [u8; 32], proof: &[[u8; 32]]) -> bool {
    let mut h = leaf;
    let mut at = leaf_index;
    for sibling in proof {
        h = if at.is_multiple_of(2) {
            node_hash(h, *sibling)
        } else {
            node_hash(*sibling, h)
        };
        at >>= 1;
    }
    h == root
}

// ---------------------------------------------------------------------------
// Contract pins — unit tests, green ON THIS BRANCH BY DESIGN. They need no
// SBF artifact (the kickoff's RED evidence is the integration suite's), and
// they are what Session A could verify locally: that the six handlers are
// wired into the dispatcher, and that the wire shapes the test harness
// hand-builds are the shapes Anchor generates.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    // `Event::data` and the `DISCRIMINATOR` constant the macros generated.
    use anchor_lang::{Discriminator, Event};
    // Host-side SHA-256 — the SAME `sha2` the test harness uses for
    // `common::disc`; a dev-dependency, so unit tests can reach it.
    use sha2::{Digest, Sha256};

    /// The first 8 bytes of `sha256("<namespace>:<name>")` — Anchor's
    /// instruction discriminator (`namespace = "global"`) and its event
    /// discriminator (`namespace = "event"`).
    fn sighash(namespace: &str, name: &str) -> [u8; 8] {
        let mut out = [0u8; 8];
        out.copy_from_slice(&Sha256::digest(format!("{namespace}:{name}").as_bytes())[..8]);
        out
    }

    /// Concatenates byte parts in order — how one builds a wire format by
    /// hand, the way the test harness (and any external integrator) does.
    fn wire(parts: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        for part in parts {
            out.extend_from_slice(part);
        }
        out
    }

    /// WIRING: the `#[program]` macro generated an instruction struct for
    /// each of the six settlement handlers, with the documented
    /// discriminator. A handler that was merely declared in THIS module but
    /// not written inside `pool_program` (lib.rs) would produce no such
    /// struct and would never dispatch.
    #[test]
    fn settlement_instructions_are_registered_in_the_dispatcher() {
        assert_eq!(
            crate::instruction::FundBond::DISCRIMINATOR,
            sighash("global", "fund_bond")
        );
        assert_eq!(
            crate::instruction::PostRoot::DISCRIMINATOR,
            sighash("global", "post_root")
        );
        assert_eq!(
            crate::instruction::Claim::DISCRIMINATOR,
            sighash("global", "claim")
        );
        assert_eq!(
            crate::instruction::Challenge::DISCRIMINATOR,
            sighash("global", "challenge")
        );
        assert_eq!(
            crate::instruction::ResolveChallenge::DISCRIMINATOR,
            sighash("global", "resolve_challenge")
        );
        assert_eq!(
            crate::instruction::SlashBond::DISCRIMINATOR,
            sighash("global", "slash_bond")
        );
    }

    /// WIRE SHAPE: the suite hand-builds instruction data the way an
    /// external integrator does (discriminator + borsh fields, in
    /// declaration order). This pins that Anchor's generated argument
    /// structs serialize to the very same bytes — including the `Ruling`
    /// enum, whose borsh form is a variant byte followed by its fields.
    #[test]
    fn instruction_arguments_match_the_hand_built_wire_format() {
        // `borsh` is this crate's dev-dependency — the same crate Anchor's
        // `AnchorSerialize` aliases, and the one the test harness uses.
        let depositor = Pubkey::new_from_array([7u8; 32]);
        let victim = Pubkey::new_from_array([9u8; 32]);
        let proof: Vec<[u8; 32]> = vec![[1u8; 32], [2u8; 32]];

        // claim(window, leaf_index, depositor, amount, proof) — declaration order IS the wire order
        let claim = crate::instruction::Claim {
            window: 5u64,
            leaf_index: 3u32,
            depositor,
            amount: 42u64,
            proof: proof.clone(),
        };
        let expected = wire(&[
            &5u64.to_le_bytes(),
            &3u32.to_le_bytes(),
            &depositor.to_bytes(),
            &42u64.to_le_bytes(),
            &(proof.len() as u32).to_le_bytes(), // Vec: 4-byte length, then items
            &proof[0],
            &proof[1],
        ]);
        assert_eq!(
            borsh::to_vec(&claim).expect("claim serializes"),
            expected,
            "claim arguments on the wire"
        );
        // And BACK again: the hand-built bytes deserialize into the
        // generated argument struct — the dispatcher's own first step.
        use anchor_lang::AnchorDeserialize;
        let back = crate::instruction::Claim::try_from_slice(&expected)
            .expect("hand-built claim data deserializes");
        assert_eq!(
            (
                back.window,
                back.leaf_index,
                back.depositor,
                back.amount,
                back.proof
            ),
            (5, 3, depositor, 42, proof),
            "hand-built claim data round-trips"
        );

        // resolve_challenge(window, ruling) — both variants.
        let stands = crate::instruction::ResolveChallenge {
            window: 11u64,
            ruling: Ruling::RootStands,
        };
        assert_eq!(
            borsh::to_vec(&stands).expect("ruling serializes"),
            wire(&[&11u64.to_le_bytes(), &[0u8]]),
            "Ruling::RootStands on the wire: variant byte 0, no fields"
        );
        let false_ruling = crate::instruction::ResolveChallenge {
            window: 11u64,
            ruling: Ruling::RootFalse {
                slash_amount: 1_000_000u64,
                victim,
            },
        };
        assert_eq!(
            borsh::to_vec(&false_ruling).expect("ruling serializes"),
            wire(&[
                &11u64.to_le_bytes(),
                &[1u8], // second variant
                &1_000_000u64.to_le_bytes(),
                &victim.to_bytes(),
            ]),
            "Ruling::RootFalse on the wire: variant byte 1, then amount, then victim"
        );
        let hand_false = wire(&[
            &11u64.to_le_bytes(),
            &[1u8],
            &1_000_000u64.to_le_bytes(),
            &victim.to_bytes(),
        ]);
        let back = crate::instruction::ResolveChallenge::try_from_slice(&hand_false)
            .expect("hand-built ruling data deserializes");
        assert_eq!(
            (back.window, back.ruling),
            (
                11,
                Ruling::RootFalse {
                    slash_amount: 1_000_000,
                    victim
                }
            ),
            "hand-built ruling data round-trips"
        );
    }

    /// EVENT SHAPE: the suite decodes `Program data:` logs by discriminator
    /// and reads fields at fixed offsets. This pins both, for all six
    /// settlement events — the discriminator recipe AND the field layout.
    #[test]
    fn settlement_events_match_the_harness_decoder_layout() {
        let depositor = Pubkey::new_from_array([7u8; 32]);

        macro_rules! pinned {
            ($ty:ty, $name:literal, $event:expr, $parts:expr) => {{
                assert_eq!(
                    <$ty as Discriminator>::DISCRIMINATOR,
                    sighash("event", $name),
                    "event discriminator recipe for {}",
                    $name
                );
                let expected = {
                    let mut expected = sighash("event", $name).to_vec();
                    expected.extend_from_slice(&wire(&$parts));
                    expected
                };
                assert_eq!($event.data(), expected, "wire layout of {}", $name);
            }};
        }

        pinned!(
            BondFunded,
            "BondFunded",
            BondFunded {
                funder: depositor,
                amount: 1u64,
                bond_after: 2u64,
            },
            [
                &depositor.to_bytes(),
                &1u64.to_le_bytes(),
                &2u64.to_le_bytes()
            ]
        );
        pinned!(
            RootPosted,
            "RootPosted",
            RootPosted {
                window: 3u64,
                merkle_root: [4u8; 32],
                total_payout: 5u64,
                bond: 6u64,
                posted_at: 7i64,
            },
            [
                &3u64.to_le_bytes(),
                &[4u8; 32],
                &5u64.to_le_bytes(),
                &6u64.to_le_bytes(),
                &7i64.to_le_bytes(),
            ]
        );
        pinned!(
            ClaimPaid,
            "ClaimPaid",
            ClaimPaid {
                window: 3u64,
                leaf_index: 1u32,
                depositor,
                amount: 5u64,
                vault_balance_after: 6u64,
            },
            [
                &3u64.to_le_bytes(),
                &1u32.to_le_bytes(),
                &depositor.to_bytes(),
                &5u64.to_le_bytes(),
                &6u64.to_le_bytes(),
            ]
        );
        pinned!(
            RootChallenged,
            "RootChallenged",
            RootChallenged {
                window: 3u64,
                challenger: depositor,
                evidence: [8u8; 32],
            },
            [&3u64.to_le_bytes(), &depositor.to_bytes(), &[8u8; 32]]
        );
        pinned!(
            ChallengeResolved,
            "ChallengeResolved",
            ChallengeResolved {
                window: 3u64,
                root_stands: true,
                slash_amount: 5u64,
                victim: depositor,
            },
            [
                &3u64.to_le_bytes(),
                &[1u8], // bool on the wire: 1 = true
                &5u64.to_le_bytes(),
                &depositor.to_bytes(),
            ]
        );
        pinned!(
            BondSlashed,
            "BondSlashed",
            BondSlashed {
                window: 3u64,
                amount: 5u64,
                victim: depositor,
                bond_after: 6u64,
            },
            [
                &3u64.to_le_bytes(),
                &5u64.to_le_bytes(),
                &depositor.to_bytes(),
                &6u64.to_le_bytes(),
            ]
        );
    }

    /// The four new seeds are distinct from each other AND from the five M8
    /// seeds: a collision would alias PDAs across layers.
    #[test]
    fn settlement_seeds_are_distinct_from_every_other_seed() {
        let seeds = [
            SEED_POOL,
            SEED_VAULT,
            crate::SEED_CLAIM_MINT,
            crate::SEED_CLAIM,
            crate::SEED_PREPAID,
            SEED_ROOT,
            SEED_CLAIM_MARKER,
            SEED_BOND,
            SEED_BOND_VAULT,
        ];
        for (i, a) in seeds.iter().enumerate() {
            for (j, b) in seeds.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "seeds at {i} and {j} alias");
                }
            }
        }
    }

    /// Tight layout for the bond record (8 = discriminator + amount + bump):
    /// a surprise field addition shows up here, not as silent rent drift.
    #[test]
    fn operator_bond_space_is_tight() {
        assert_eq!(8 + OperatorBond::INIT_SPACE, 8 + 8 + 1);
    }
}
