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
}
