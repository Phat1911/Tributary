# M8 devnet soak — runbook + evidence (Solana program: escrow vault + claim registry)

Status: **RUN PENDING — one blocker: deploy key needs ≥4 SOL.** Devnet
airdrop is rate-limited and all public RPC faucets refuse `requestAirdrop`
(2026-10-30); the founder took the browser step ("i'll do it later"). Once
`4N2nTJ8WWwDKVoULt9bymaB6XsrB7HP973y8Ebw6tQhV` (deploy key) holds ≥4 SOL,
the whole soak is ONE command (below). Everything else is done and
verified: SBF ELF built (`target/deploy/pool.so`, program id
`9pEmyxftdRUZV2iw6ZzCNgZz9QygG53i7APNaWcEHvjX`), program-test suites
16/16 green against that exact ELF, soak client compiled clean, orchestrator
script updated to the real artifact paths and non-interactive faucet
polling (kickoff ruling at GO 2026-10-30: manual runbook + script;
scheduled CI deferred).

## What this proves (SPEC §14 V2 M8 row, done-when)

1. **Program deployed to devnet under multisig** — Squads V4 2-of-2
   (founder + teammate; GO ruling; SPEC §12's 2-of-3 stays the production
   posture) holds the upgrade authority; a single-key upgrade attempt is
   part of the proof and MUST fail.
2. **Deposit/withdraw round-trip** in devnet faucet USDC — with only
   operator keys signing (rule 10).
3. **Claim registry live with transfers locked** — Token-2022 mint with
   the `NonTransferable` extension; claim accounts are program-PDA-owned.
4. **Soak run documented** — this file + the archived log.
5. **No V1 change** — nothing outside `programs/pool`, `scripts/`,
   `deploy/`, docs and the workspace member list is touched.

## One-time machine setup (already done this session; reproducible)

All toolchains live INSIDE the repo tree (site-restoration friendly):

```bash
export RUSTUP_HOME=$PWD/target/rustup-home CARGO_HOME=$PWD/target/cargo-home
cargo install avm --git https://github.com/otter-sec/anchor --tag v1.2.2 --locked
AVM_HOME=$PWD/target/avm avm install 1.2.2 && AVM_HOME=$PWD/target/avm avm use 1.2.2
HOME=$PWD/target/solana-home sh -c "$(curl -sSfL https://release.anza.xyz/v4.1.2/install)"
HOME=$PWD/target/solana-home target/solana-home/.local/share/solana/install/active_release/bin/cargo-build-sbf --tools-version v1.58 --install-only
```

Keypairs (gitignored `target/m8-keys/`, pubkeys in `deploy/devnet-m8.json`):
`pool-program`, `deploy`, `founder`, `teammate`, `backend`, `depositor`,
`ms-create-key`, `rogue-upgrader`, `teammate-rotated`.

## Running the soak

```bash
bash scripts/devnet_soak_m8.sh 2>&1 | tee target/soak-logs/soak-<ts>.log
```

Post-D40 doctrine (2026-10-31): the vault never holds depositor
principal — contributions are claim RECORDS; USDC enters only as renter
prepay. The soak proves exactly that shape, live.

The script: builds the .so (SBF v3, skips if present) → funds the deploy
key (airdrop loop; if rate-limited it says exactly which pubkey to faucet
at faucet.solana.com and exits) → `solana program deploy` → Circle
devnet-USDC faucet for the custodial depositor (API attempt, 90 s balance
polling, browser fallback documented on timeout) → writes a fresh upgrade
buffer → runs the standalone soak client (`scripts/devnet-soak`,
`cargo run --release`) through steps 0–10 below → re-checks the on-chain
authority. Each step skips itself if its artifact already exists, so the
script is safely re-runnable after any interruption.

## Soak steps (each prints one EVIDENCE line)

| # | Step | Proves |
|---|------|--------|
| 0 | USDC mint shape check | 6 decimals = micro-USD seam holds on devnet |
| 1 | Squads multisig create (2-of-2) | devnet ruling implemented |
| 2 | Upgrade authority → multisig vault PDA | no single-key upgrade authority |
| 3 | Single-key upgrade attempt | REFUSED (this failing is the proof) |
| 4 | `initialize` with admin = vault PDA | admin-gated from the first block |
| 5 | `record_contribution` (25 USDC-worth) + `burn_claims` via the DIRECT door (depositor key only, backend absent) | D40 live: claims are records, vault stays at zero — and the escape hatch works with the operator absent |
| 6 | Renter prepay (10 USDC) → partial refund (assisted door) → full refund (DIRECT door) | the ONLY USDC path: vault == Σ renter prepaid, round-trip exact, both doors live |
| 7 | `pause` via 2-of-2 vote | admin gate is genuinely the multisig |
| 8 | `record_contribution` while paused | REFUSED with `PoolError::Paused` (all movement classes refuse; prepay/refund refusals pinned in the adversarial suite) |
| 9 | Program UPGRADE via 2-of-2 vote | vault-signed upgrade works |
| 10 | Teammate key rotation via 2-of-2 | multisig governance is operational |

## Executed run — evidence

*(filled after the run; sigs + pubkeys below)*

- Program id:
- Multisig / vault:
- Deploy sig:
- Round-trip sigs:
- Upgrade sigs:
- Rotation sig:
- Program authority after soak:
- Log archive:
