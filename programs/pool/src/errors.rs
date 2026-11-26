//! Program errors. Kept minimal and self-describing: each variant names the
//! guard that fired, so a failed transaction reads like the rule it broke.

use anchor_lang::prelude::*;

#[error_code]
pub enum PoolError {
    /// Pool is paused (admin circuit breaker) — every registry and custody
    /// movement is refused until the multisig resumes.
    #[msg("Pool is paused: all registry and custody movements are disabled by admin")]
    Paused,
    /// Amount must be strictly positive — zero-value legs are noise.
    #[msg("Amount must be greater than zero")]
    ZeroAmount,
    /// The escrowed mint must be USDC-shaped: 6 decimals, so 1 minor unit
    /// == 1 micro-USD == the internal ledger's integer unit (SPEC §6).
    #[msg("Escrow mint must have exactly 6 decimals (micro-USD seam)")]
    UsdcDecimals,
    /// Prepay source token account not owned by the renter's key.
    #[msg("Prepay source account is not owned by the renter's key")]
    WrongSourceOwner,
    /// Prepay source token account holds a different mint than the pool's.
    #[msg("Prepay source account holds a different mint than this pool custodies")]
    WrongSourceMint,
    /// Refund destination not owned by the named renter.
    #[msg("Refund destination is not owned by the renter")]
    WrongDestinationOwner,
    /// Refund destination holds a different mint.
    #[msg("Refund destination holds a different mint than this pool custodies")]
    WrongDestinationMint,
    /// Burn amount exceeds the depositor's recorded claim balance.
    /// (The Token-2022 burn would also fail, but with a generic error —
    /// this pre-check makes the on-chain record mismatch legible.)
    #[msg("Burn amount exceeds the depositor's claim balance")]
    InsufficientClaims,
    /// Refund amount exceeds the renter's OWN on-chain prepaid record —
    /// refunds are bounded by an on-chain fact, never an attestation.
    #[msg("Refund amount exceeds the renter's on-chain prepaid record")]
    PrepaidInsufficient,
    /// Exit door check (doctrine: assisted AND direct): the signer was
    /// neither the backend authority (assisted door) nor the named
    /// depositor/renter themselves (direct door). Neither door = refused.
    #[msg(
        "Signer is neither the backend authority nor the named identity — one of the two doors is required"
    )]
    ExitAuthority,
    /// Integer overflow in a checked money arithmetic step. Cannot happen
    /// with honest inputs before heat death of the chain; caught anyway
    /// because money code never wraps silently.
    #[msg("Checked money arithmetic overflowed")]
    Overflow,
    /// A-041: `initialize` is gated on the DEPLOYMENT authority — the
    /// program's own upgrade authority, read from the BPF loader's
    /// ProgramData account. Anything else (wrong signer, a substituted
    /// program-data account, an immutable deployment) refuses here, so
    /// the singleton cannot be front-run between deploy and initialize.
    /// Appended LAST on purpose: Anchor numbers variants from 6000 in
    /// declaration order, and the suites pin existing indices.
    #[msg("Initializer is not this deployment's upgrade authority")]
    DeploymentAuthority,
    // ---- M11 settlement (doctrine-suite Session A, 2026-11-26) ------------
    // Appended after DeploymentAuthority for the same reason it was appended
    // last: Anchor numbers variants from 6000 in declaration order, and
    // every suite pins codes by index. Each name below IS one doctrine
    // sentence; the M11 suite (`tests/m11_doctrine.rs`) pins each code.
    /// Invariant 1 — a root whose total payout exceeds the vault's USDC is
    /// refused at `post_root`, atomically: no root, no movement.
    #[msg("Root total payout exceeds the vault's USDC — refused, nothing moved")]
    RootExceedsVault,
    /// Invariant 2 — the claim's inclusion proof does not hash up to the
    /// published root (wrong sibling, tampered leaf, foreign tree).
    #[msg("Claim inclusion proof does not verify against the published root")]
    InvalidProof,
    /// Invariant 4 — the payout destination is not owned by the leaf's NAMED
    /// depositor (the operator, or anyone else, cannot route a payout away
    /// from the leaf's identity).
    #[msg("Payout destination is not owned by the leaf's named depositor")]
    WrongPayoutDestination,
    /// Invariant 5 — the root is not `Active` (challenged or proven false):
    /// its payouts are frozen until the multisig rules.
    #[msg("Root payouts are frozen — the root is not Active")]
    RootFrozen,
    /// Invariant 5 (complement) — the challenge window has closed; a late
    /// challenge cannot freeze a root.
    #[msg("Challenge window for this root has closed")]
    ChallengeWindowClosed,
    /// Invariant 6 — `slash_bond` on a root that is not `ProvenFalse`
    /// (never challenged, still challenged, ruling "stands", or already
    /// slashed): nothing is slashed.
    #[msg("Bond slash refused — the root is not proven false")]
    RootNotProvenFalse,
    /// Invariant 6 — the ruling's `slash_amount` exceeds the bond that backs
    /// the root. "Exactly the ruled amount" forbids both clamping and
    /// over-issuing, so the ruling itself refuses.
    #[msg("Ruled slash exceeds the bond backing this root")]
    SlashExceedsBond,
    // ---- added by the M11-A GO-gate ruling (2026-11-26) --------------------
    /// Invariant 11 — the root is `Active` but its challenge window is still
    /// open and the multisig has not ruled it stands: no payout leaves the
    /// vault yet.
    #[msg("Challenge window still open — no payout leaves the vault yet")]
    ChallengeWindowOpen,
    /// Invariant 12 — this claim would push the root's `paid_out` past its
    /// declared `total_payout`.
    #[msg("Claim would pay out more than the root's declared total")]
    PayoutExceedsTotal,
}
