//! On-chain state for the root-pool custody program.
//!
//! The layout is deliberately minimal: ONE singleton config PDA, ONE USDC
//! vault token account, ONE claim mint, and ONE claim token account per
//! depositor identity. Every record here mirrors a row the internal ledger
//! already keeps (see `lib.rs` module docs for the exact correspondence) —
//! reconciliation cheapness is a layout property, so nothing on-chain carries
//! a shape the books cannot answer with a single query.

use anchor_lang::prelude::*;

/// Seed for the singleton `Pool` config PDA. The root pool is singular
/// (SPEC §3.2), so one fixed seed is the whole namespace — a second pool
/// cannot exist under this program, by construction.
pub const SEED_POOL: &[u8] = b"pool";

/// Seed suffix for the USDC escrow vault token account (owned by the `Pool`
/// PDA, so only this program can move custodial cash).
pub const SEED_VAULT: &[u8] = b"vault";

/// Seed suffix for the Token-2022 claim mint PDA (the claim registry).
pub const SEED_CLAIM_MINT: &[u8] = b"claim_mint";

/// Seed prefix for per-depositor claim token accounts:
/// `[SEED_CLAIM, depositor_pubkey]` — one stable account per depositor
/// identity, which is exactly the key M11's Merkle leaves will use.
pub const SEED_CLAIM: &[u8] = b"claim";

/// Both mints use 6 decimals: USDC natively, the claim mint by our choice.
/// 1 claim unit therefore equals exactly 1 micro-USD of contributed
/// principal — the same integer micro-unit the internal ledger's `MicroUsd`
/// newtype uses (SPEC §6, M7). No conversion, no rounding, no floats.
pub const DECIMALS: u8 = 6;

/// The singleton configuration account. Authority model:
/// * `admin_authority` — the Squads multisig vault PDA (devnet: 2-of-2
///   founder + teammate). It signs admin instructions only through a
///   multisig-executed transaction, so every admin action is a 2-of-2 vote.
///   On-chain this is just a pubkey: whoever can produce its signature
///   passes, and on devnet only the Squads program can (PDA signature).
/// * `backend_authority` — the operator's hot key. It signs
///   deposit/withdraw so members, renters and depositors NEVER need a
///   wallet (rule 10): the backend submits on their behalf, custodially.
#[account]
#[derive(InitSpace)]
pub struct Pool {
    /// Multisig vault PDA — pause/unpause, authority rotations.
    pub admin_authority: Pubkey,
    /// Operator backend hot key — signs deposit/withdraw (rule 10).
    pub backend_authority: Pubkey,
    /// The USDC mint this pool custodies (devnet faucet USDC; SPL Token v3).
    pub usdc_mint: Pubkey,
    /// The claim mint (Token-2022, NonTransferable — the claim registry).
    pub claim_mint: Pubkey,
    /// Cached vault address for cheap reads/mirroring (derivable from
    /// seeds; stored so external tooling reads ONE account to know everything).
    pub vault: Pubkey,
    /// Circuit breaker: when true, deposits and withdrawals are refused.
    /// Admin-only toggle — an operator incident response lever, and the
    /// same lever M11's challenge window will use during disputes.
    pub paused: bool,
    /// Bump of the `Pool` PDA itself (needed to sign CPIs as the pool).
    pub bump: u8,
    /// Reserved, zero-filled today. M11 settlement parameters (challenge
    /// window length, operator bond size, active Merkle root) land in this
    /// space WITHOUT resizing the account — account layout is the part M11
    /// must not redesign.
    #[max_len(64)]
    pub reserved: [u8; 64],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_match_usdc_micro_units() {
        // The seam contract: on-chain minor units == ledger MicroUsd units.
        assert_eq!(DECIMALS, 6);
    }

    #[test]
    fn pool_space_fits_reserved_headroom() {
        // 8 = Anchor discriminator. If this stops holding, a serialized old
        // account would not fit a future realloc — fail loudly instead.
        // (`const { }` so clippy sees a compile-time assertion, not a
        // runtime one on constants.)
        const {
            assert!(8 + Pool::INIT_SPACE >= 8 + 32 * 5 + 1 + 1 + 64);
        }
    }

    #[test]
    fn seeds_are_distinct() {
        // Colliding seeds would alias PDAs; pin pairwise distinctness.
        let seeds = [SEED_POOL, SEED_VAULT, SEED_CLAIM_MINT, SEED_CLAIM];
        for (i, a) in seeds.iter().enumerate() {
            for (j, b) in seeds.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b);
                }
            }
        }
    }
}
