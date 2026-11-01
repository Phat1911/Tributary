# M8 devnet soak — runbook + evidence (Solana program: escrow vault + claim registry)

Status: **RUN COMPLETE (2026-11-01).** All 10 steps green on devnet;
evidence below. Program `9pEmyx…` deployed, upgrade authority = Squads
vault `BhLQqw…` (2-of-2 founder+teammate), renter round-trip proven
through BOTH doors, the D40 invariant (vault untouched by contributions)
verified live, and the multisig proved it can pause, resume, upgrade the
program, and rotate a member — each via a 2-of-2 vote.

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

## Executed run — evidence (final run 2026-11-01, ~11:55 ICT)

- Program id: `9pEmyxftdRUZV2iw6ZzCNgZz9QygG53i7APNaWcEHvjX`
  (deploy sig `4YEMbVJPzwdaPDzmv1tkYHvUcQXw5sQg2J2uYyQ8BZK84xciSwjRfPVBto7EZgt5Zc7ruiZSjTqSfm64PSFLuzfU`)
- Multisig / vault: `GuSeFfFkdKtjkubeLUEeBHW5pfHSJctGnz8AFk7xrCk5` /
  vault `BhLQqw5rkr3otQoXvJSRjiaNHTgdcgXVkqXv8mwEhhJq` (create sig
  `2Y3SYgMzCcmoG4uEY9ZpRpKPQzx1nqstRx1padu7Kbu658eH6JzR2Ra7efRaWRNQ3Ue2wYHaYWqKq5SokgVQxWPj`)
- Upgrade authority → vault sig: `5QtwnHeEVt41kYh71wE6qi5YkT9kCXnz1DZBJiHtEzd2ikLS2XqwHjzniqHtWhW6zb9nTjUsd8n238iMDQDPUi7N`
- Single-key upgrade attempt: REFUSED (the refusal is the proof)
- initialize (admin = vault): sig `3xwvPyRMkNRubFV6RJwb1LCFeQzoDZEcujfPnbSkx6PH2BivZ3tg7RnoU3iaYgqHQcNzY3tzVMmUhPjLK5puxWMs`
- D40 proof (records, not money): record_contribution 25 USDC-worth →
  claims minted, **vault = 0** — sig (final run)
  `24EFHhDdkMci3C1gbyEVrvaj5qzSA8cmaZtrfMBePWcpomwSgWqAVGFELzeyVgdUqSeunWqQMpHqun58HSN5bh2w`
- Escape hatch (DIRECT door, backend absent): burn_claims 5 USDC-worth sig
  `2jpogJN2z4YJtX87dugFJu1LKybKH1wBt8oMsxLSbL3weoWdnJabcX6THcLYb7L91WKcXVV3kpUXbn1yr2ACzpeK`
- Renter round-trip: prepay 10 USDC (vault == record == 10,000,000) sig
  `2DKut6qqqCaZSJkCK6KRJxHete449THZhV3yWFJuw3nPzHzpKMzbHvPHYq12DioZwv3LnFqoSn9MCyfY51xL3gHD`;
  refund ASSISTED sig `nzQkmTGGb8d5p5iLfNWyC6be3nNkSf6XSPeMX4nyVYecPcqQZyrzkUT6qh6GfUYw1DfFvjmZNDG9AqAC9dsUS68`
  + refund DIRECT sig `A6akR6xmV9srkT6j5m6tBrk6bLGKUaKD7QYGGVKugHBoXhF3gLPt5mL8tQ4KnTXPtZ2VV6JERPLGxuAA8nh7ok1`
  → vault = 0, record = 0, wallet back at 20 USDC
- pause via 2-of-2 (tx index 4): sig `5MwfY4KLYraqdzYo8se5F5KZMmBYyd3ppt5iTB4U1bvrQ7VQaxZYVZGxk9y6GaDcpmsHWcoyNUS8dLhbF2jgmHm2`
- paused refusal: `record_contribution` → custom error 0x1770 = `PoolError::Paused` (6000)
- Program UPGRADE via 2-of-2 (tx index 5; buffer
  `J18XUcLaSSLb93aMN1o4PtQTxTH6M4GaFVoC7jzEBjhs`, buffer authority handed
  to the vault first, sig
  `2XdawasEEqpkm9wYQu2KAuH5vuTHyHPWt2YNEetWzGEExJdDPJBREDXNBMgMFpD9pHQ8vUAo1PJnVXojQqGjW8v8`):
  execute sig `46fSAVhMo4D5qcfECfX9VNN27SxPf5YMG3iZpbCvbrWde2bMTDVtvuVsVNtwVz5tcyfvu7UdnmtKFhiU1FHMRBAS`
- Member rotation (teammate → `2ys6xNTANVqoNar7RQHyQvtdDhCwYbsE3XEP4LAjpzbq`) via 2-of-2:
  sig `2LqGT5hJXR2V3Q5w5qCqtkB9aHmsdHvrFR4Sw97Vidc5ZeuxNJ7JUTSQ1qwSVrd8DXDJvq4VmsJa3KfRKXz2KeX5`
- Program authority after soak: `BhLQqw5rkr3otQoXvJSRjiaNHTgdcgXVkqXv8mwEhhJq` (the Squads vault)
- Log archive: `target/soak-logs/soak-20261101-*.log` (gitignored by design; the sigs above are the durable record)
