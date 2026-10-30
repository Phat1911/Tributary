//! Program errors. Kept minimal and self-describing: each variant names the
//! guard that fired, so a failed transaction reads like the rule it broke.

use anchor_lang::prelude::*;

#[error_code]
pub enum PoolError {
    /// Pool is paused (admin circuit breaker) — deposits/withdrawals refused.
    #[msg("Pool is paused: deposits and withdrawals are disabled by admin")]
    Paused,
    /// Amount must be strictly positive — zero-value legs are noise.
    #[msg("Amount must be greater than zero")]
    ZeroAmount,
    /// The escrowed mint must be USDC-shaped: 6 decimals, so 1 minor unit
    /// == 1 micro-USD == the internal ledger's integer unit (SPEC §6).
    #[msg("Escrow mint must have exactly 6 decimals (micro-USD seam)")]
    UsdcDecimals,
    /// Deposit source token account not owned by the declared depositor.
    #[msg("Deposit source account is not owned by the depositor")]
    WrongSourceOwner,
    /// Deposit source token account holds a different mint than the pool's.
    #[msg("Deposit source account holds a different mint than this pool custodies")]
    WrongSourceMint,
    /// Withdrawal destination not owned by the declared depositor.
    #[msg("Withdrawal destination is not owned by the depositor")]
    WrongDestinationOwner,
    /// Withdrawal destination holds a different mint.
    #[msg("Withdrawal destination holds a different mint than this pool custodies")]
    WrongDestinationMint,
    /// Withdraw amount exceeds the depositor's recorded claim balance.
    /// (The Token-2022 burn would also fail, but with a generic error —
    /// this pre-check makes the on-chain record mismatch legible.)
    #[msg("Withdraw amount exceeds the depositor's claim balance")]
    InsufficientClaims,
}
