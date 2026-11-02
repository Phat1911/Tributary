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
| 4 | `initialize` via 2-of-2 vote (vault PDA signs as the DEPLOYMENT authority) | A-041 fix live: the pool's birth itself needs the multisig — no front-run window |
| 5 | `record_contribution` (25 USDC-worth) + `burn_claims` via the DIRECT door (depositor key only, backend absent) | D40 live: claims are records, vault stays at zero — and the escape hatch works with the operator absent |
| 6 | Renter prepay (10 USDC) → partial refund (assisted door) → full refund (DIRECT door) | the ONLY USDC path: vault == Σ renter prepaid, round-trip exact, both doors live |
| 7 | `pause` via 2-of-2 vote | admin gate is genuinely the multisig |
| 8 | `record_contribution` while paused | REFUSED with `PoolError::Paused` (all movement classes refuse; prepay/refund refusals pinned in the adversarial suite) |
| 9 | Program UPGRADE via 2-of-2 vote | vault-signed upgrade works |
| 10 | Teammate key rotation via 2-of-2 | multisig governance is operational |

## Sync upgrade — 2026-11-02: devnet program → HEAD 460fd00 (A-041/A-042 fixes live)

Executed as an ops chore (2-of-2, existing Squads; nothing merged). Facts and
signatures (log archive: `target/soak-logs/sync-*.log`):

- **Preflight** (read-only): members = founder + `teammate-rotated` (the
  original teammate was rotated out on 2026-11-01 — approvals MUST come from
  the members that exist NOW), threshold 2; program authority = vault
  `BhLQqw…`; pool admin = vault, backend unchanged, arrived PAUSED
  (resume-aware, as documented); vault 0 USDC, renter wallet 20 USDC.
- **Buffer** `2ASdmWydX1p4iy1We2feHi8MRdgeLJorx5D4MNfigEJR` (HEAD ELF,
  288,397 B), authority handed to the vault.
- **ProgramData EXTEND +65,536 B first** (top-level, payer = deploy key,
  sig in log): the fix GREW the ELF (283,245 B allocated < 288,405 needed —
  every prior upgrade was same-size, so this was the first grow). The
  runtime REJECTS `ExtendProgram` from inner instructions (CPI allowlist is
  Upgrade/SetAuthority/Close only — "Program … not supported by inner
  instructions"), and it takes NO authority account, so the extend ran as a
  plain top-level tx with the vault authority untouched. Allocation now
  348,781 B — headroom for M11.
- **UPGRADE via 2-of-2** (vault-signed, tx index 9):
  [sig `576QrbHP…`](https://solscan.io/tx/576QrbHPxvDB3P63nHedFA5hDqVBXJchzkauEvrpAJzHUyyDCnfd6KhXaRw2gePS7RF2NQh2eM5qsgj6wwCdeaP5?cluster=devnet)
  — deployed slot 517758888 → 517759050; authority STAYS the vault.
- **Resume via 2-of-2**: sig `5NPX56zR…` (pool arrived paused; now ACTIVE).
- **State intact**: pool admin/backend/layout byte-identical across the
  upgrade (upgrade-safe diff confirmed: `state.rs` unchanged, error enum
  append-only, `initialize` interface change is dead code on a born pool).
- **Live verification on the NEW code**: `record_contribution` — emitted
  `supply_after` == 151,000,000 == on-chain supply; `prepay_renter` —
  emitted `vault_balance_after` == 1,000,000 == on-chain vault (**A-042
  live: the old code emitted the pre-instruction values**); direct-door
  `burn_claims` and the full prepay→refund round-trip exact (vault 0,
  record 0, wallet back at 20 USDC). `initialize` remains un-callable for
  ANY signer on this born singleton (occupied-PDA refusal); the A-041 gate
  protects every FUTURE deployment and is regression-pinned by the audit
  suite's `c1`.
- **Ops lessons (dated, for the next sync)**: (1) a GROWN ELF needs
  `extend_program` BEFORE the ms upgrade — top-level only; (2) the buffer
  authority layout is `[u32 tag=1][u8 option][32 B key]` — key at 5..37
  (the soak client's 4..36 parse is off by one; A-040 class); (3) stale
  approved-but-unexecuted Squads proposals (indexes 7, 8) remain on the
  multisig from the two refused execute attempts — unexecutable by
  construction, clearable with a `transaction_accounts_close` vote if the
  founder wants them gone.

## Executed run — evidence (final run 2026-11-01, ~11:55 ICT)

> **Post-run change (2026-11-02, A-041 fix):** this run executed against the
> pre-fix program, whose `initialize` was permissionless (AUDIT A-041 — the
> singleton was captured-risk in the deploy→initialize window; devnet was
> moot because the operator initialized first). The program now requires the
> deployment authority's signature — the upgrade authority in the BPF
> ProgramData, i.e. this multisig vault — and the soak client's step 4
> routes `initialize` through a 2-of-2 vote (vault PDA as
> `upgrade_authority` signer; deploy key only pays rent). The evidence below
> is the historical record of the 2026-11-01 run; a FUTURE deployment (new
> program id) will print its step-4 evidence line as an ms-executed
> transaction.

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
